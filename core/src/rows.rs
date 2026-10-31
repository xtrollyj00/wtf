//! The rows (spec §6): signals and one level of groups, kept as one flat list. A group is its
//! header followed by the signals marked `grouped`, so moving, removing and undo work on one Vec.

use crate::radix::{self, RADIXES};
use crate::trace::{Trace, Val, Var};

#[derive(Clone, PartialEq, Debug)]
pub enum Row {
    Signal(Sig),
    Group { name: String, collapsed: bool },
}

#[derive(Clone, PartialEq, Debug)]
pub struct Sig {
    pub var: usize,
    /// How the row's own line shows its value.
    pub look: Look,
    /// Belongs to the group whose header is above it.
    pub grouped: bool,
    /// The vars opened under this row, the row's own among them: a bus shows its bits, a stream
    /// its fields, a composite its children (`Workspace::subs`); empty when folded.
    pub open: Vec<usize>,
    /// The lines under the row with a look of their own: a field, an element, a stream's field
    /// (`Workspace::look` gives every line's).
    pub looks: Vec<(usize, Look)>,
}

/// How a line shows its value (spec §10, §14 "Colours"): its radix, the fraction bits of a
/// fixed-point one (`q4.12`, 0 otherwise), and its colour, an index into `COLORS` (the theme's
/// when None; a line under a row takes the row's).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    pub radix: usize,
    pub frac: u32,
    pub color: Option<usize>,
}

impl Look {
    /// The default radix for `v`, no colour of its own.
    pub fn of(v: &Var) -> Look {
        Look { radix: radix::default_for(v.names.is_some(), v.signed), frac: 0, color: None }
    }
}

impl Sig {
    /// A row for `var` in its default look, folded and ungrouped.
    pub fn of(var: usize, v: &Var) -> Sig {
        Sig { var, look: Look::of(v), grouped: false, open: Vec::new(), looks: Vec::new() }
    }
}

/// The colours a row can have. The interfaces give each its shade.
pub const COLORS: [&str; 8] = ["red", "orange", "yellow", "green", "cyan", "blue", "magenta", "gray"];

/// A colour by name, or an error listing them.
pub fn color_named(name: &str) -> Result<usize, String> {
    COLORS.iter().position(|c| *c == name).ok_or_else(|| format!("unknown colour: {name} (colours: {})", COLORS.join(", ")))
}

impl Row {
    pub fn sig(&self) -> Option<&Sig> {
        match self {
            Row::Signal(s) => Some(s),
            Row::Group { .. } => None,
        }
    }

    pub fn sig_mut(&mut self) -> Option<&mut Sig> {
        match self {
            Row::Signal(s) => Some(s),
            Row::Group { .. } => None,
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self, Row::Group { .. })
    }

    fn grouped(&self) -> bool {
        self.sig().is_some_and(|s| s.grouped)
    }
}

/// End (exclusive) of the group whose header is at `h`.
pub fn group_end(rows: &[Row], h: usize) -> usize {
    h + 1 + rows[h + 1..].iter().take_while(|r| r.grouped()).count()
}

/// Header of the group row `i` belongs to; a header belongs to itself.
pub fn header_of(rows: &[Row], i: usize) -> Option<usize> {
    match &rows[i] {
        Row::Group { .. } => Some(i),
        Row::Signal(s) if s.grouped => rows[..i].iter().rposition(Row::is_group),
        Row::Signal(_) => None,
    }
}

/// Vars of the signals in the group at `h`.
pub fn members(rows: &[Row], h: usize) -> Vec<usize> {
    rows[h + 1..group_end(rows, h)].iter().filter_map(Row::sig).map(|s| s.var).collect()
}

/// Rows on screen, as indices: the signals of collapsed groups are hidden.
pub fn visible(rows: &[Row]) -> Vec<usize> {
    let mut hide = false;
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        match r {
            Row::Group { collapsed, .. } => hide = *collapsed,
            Row::Signal(s) if s.grouped && hide => continue,
            Row::Signal(_) => {}
        }
        out.push(i);
    }
    out
}

/// Drop group headers left without signals.
pub fn tidy(rows: &mut Vec<Row>) {
    let mut i = 0;
    while i < rows.len() {
        if rows[i].is_group() && group_end(rows, i) == i + 1 {
            rows.remove(i);
        } else {
            i += 1;
        }
    }
}

/// The rows `a..=b` (any order) as a range, widened to whole groups at the ends.
pub fn range(rows: &[Row], a: usize, b: usize) -> std::ops::Range<usize> {
    let (a, b) = (a.min(b), a.max(b));
    let end = if rows[b].is_group() { group_end(rows, b) } else { b + 1 };
    a..end
}

/// The rows `a..=b` as a block to move: widened to the end of every group whose header is in it,
/// so a group moves with its rows.
pub fn block(rows: &[Row], a: usize, b: usize) -> std::ops::Range<usize> {
    let (a, mut end) = (a.min(b), a.max(b) + 1);
    let mut i = a;
    while i < end {
        if rows[i].is_group() {
            end = end.max(group_end(rows, i));
        }
        i += 1;
    }
    a..end
}

/// Move the rows `from` (a `block`) to stand before row `before` (an index before the move;
/// `rows.len()` for the end). Its signals before any header in it become `grouped` or not there.
/// The caller keeps groups whole: no ungrouped signal between a header and its rows, no group
/// inside another. Returns the block's new start.
pub fn place(rows: &mut Vec<Row>, from: std::ops::Range<usize>, before: usize, grouped: bool) -> usize {
    let len = from.len();
    let at = match before {
        _ if (from.start..=from.end).contains(&before) => from.start,
        _ => {
            let block: Vec<Row> = rows.drain(from.clone()).collect();
            let at = if before > from.start { before - len } else { before };
            rows.splice(at..at, block);
            at
        }
    };
    for r in &mut rows[at..at + len] {
        match r {
            Row::Signal(s) => s.grouped = grouped,
            Row::Group { .. } => break,
        }
    }
    at
}

/// Move the rows `block` above or `below` row `r` (spec §2 "Drags"): signals join a group when
/// they land next to its rows (above or below a grouped row, below an expanded header); a block
/// with a header never lands in a group, but above or below the whole group, the way it moves.
/// Returns the block's new place, or None when `r` is in it.
pub fn next_to(rows: &mut Vec<Row>, block: std::ops::Range<usize>, r: usize, below: bool) -> Option<std::ops::Range<usize>> {
    if block.contains(&r) {
        return None;
    }
    let (before, grouped) = match (&rows[r], rows[block.clone()].iter().any(Row::is_group)) {
        (_, true) => match header_of(rows, r) {
            Some(h) if rows[r].is_group() && !below => (h, false),
            Some(h) if h < block.start => (h, false),
            Some(h) => (group_end(rows, h), false),
            None => (r + below as usize, false),
        },
        (Row::Group { .. }, false) if !below => (r, false),
        (Row::Group { collapsed: true, .. }, false) => (group_end(rows, r), false),
        (Row::Group { .. }, false) => (r + 1, true),
        (Row::Signal(s), false) => (r + below as usize, s.grouped),
    };
    let len = block.len();
    let at = place(rows, block, before, grouped);
    Some(at..at + len)
}

/// Move the rows `block` past the next row on screen, down or up, as `next_to` does (`J` / `K`
/// on a `V` range). Returns its new place, or None at the end.
pub fn step(rows: &mut Vec<Row>, block: std::ops::Range<usize>, down: bool) -> Option<std::ops::Range<usize>> {
    let vis = visible(rows);
    let r = match down {
        true => vis.into_iter().find(|&v| v >= block.end),
        false => vis.into_iter().rev().find(|&v| v < block.start),
    }?;
    next_to(rows, block, r, down)
}

/// Move the row at `i` (a whole group for a header) one place down or up. A signal next to a
/// group boundary moves into or out of the group; a collapsed group is passed over in one step.
/// Returns the row's new index.
pub fn move_row(rows: &mut [Row], i: usize, down: bool) -> usize {
    let grouped = rows[i].grouped();
    let set = |r: &mut Row, g: bool| {
        if let Row::Signal(s) = r {
            s.grouped = g;
        }
    };
    let collapsed = |r: &Row| matches!(r, Row::Group { collapsed: true, .. });
    if rows[i].is_group() {
        let end = group_end(rows, i);
        if down {
            if end == rows.len() {
                return i;
            }
            let next = if rows[end].is_group() { group_end(rows, end) } else { end + 1 };
            rows[i..next].rotate_left(end - i);
            return i + next - end;
        }
        if i == 0 {
            return i;
        }
        let prev = header_of(rows, i - 1).unwrap_or(i - 1);
        rows[prev..end].rotate_right(end - i);
        return prev;
    }
    match (down, grouped) {
        // inside a group: swap with a neighbour, or step out of it at its edge
        (true, true) if rows.get(i + 1).is_some_and(Row::grouped) => {
            rows.swap(i, i + 1);
            i + 1
        }
        (true, true) => {
            set(&mut rows[i], false);
            i
        }
        (false, true) if rows[i - 1].is_group() => {
            rows.swap(i - 1, i);
            set(&mut rows[i - 1], false);
            i - 1
        }
        (false, true) => {
            rows.swap(i - 1, i);
            i - 1
        }
        // outside: swap with a signal, step into an expanded group, pass a collapsed one
        (true, false) if i + 1 == rows.len() => i,
        (true, false) if collapsed(&rows[i + 1]) => {
            let end = group_end(rows, i + 1);
            rows[i..end].rotate_left(1);
            end - 1
        }
        (true, false) if rows[i + 1].is_group() => {
            rows.swap(i, i + 1);
            set(&mut rows[i + 1], true);
            i + 1
        }
        (true, false) => {
            rows.swap(i, i + 1);
            i + 1
        }
        (false, false) if i == 0 => i,
        (false, false) => match header_of(rows, i - 1) {
            Some(h) if collapsed(&rows[h]) => {
                rows[h..=i].rotate_right(1);
                h
            }
            Some(_) => {
                set(&mut rows[i], true);
                i
            }
            None => {
                rows.swap(i - 1, i);
                i - 1
            }
        },
    }
}

/// Put the signals in `r` into a new group. Groups don't nest yet (spec §6).
pub fn group(rows: &mut Vec<Row>, r: std::ops::Range<usize>, name: &str) -> Result<(), String> {
    if rows[r.clone()].iter().any(|x| x.is_group() || x.grouped()) {
        return Err("groups can't be nested yet".into());
    }
    rows[r.clone()].iter_mut().for_each(|x| {
        if let Row::Signal(s) = x {
            s.grouped = true;
        }
    });
    rows.insert(r.start, Row::Group { name: name.into(), collapsed: false });
    Ok(())
}

/// Equal apart from which groups are collapsed and which buses are expanded.
pub fn same_layout(a: &[Row], b: &[Row]) -> bool {
    let norm = |r: &Row| match r {
        Row::Group { name, .. } => Row::Group { name: name.clone(), collapsed: false },
        Row::Signal(s) => Row::Signal(Sig { open: Vec::new(), ..s.clone() }),
    };
    a.iter().map(norm).eq(b.iter().map(norm))
}

/// A composite's child as its line names it (spec §6 "Composite rows"): its field (`valid`), or
/// its array and index (`pkts[0]`, `grid[1][2]`).
pub fn child_name(path: &str) -> String {
    let p = crate::trace::parts(path);
    let start = p.iter().rposition(|x| !x.starts_with('[')).unwrap_or(0);
    crate::trace::join(p[start..].iter().copied())
}

/// Row names: leaf names with enough scope added to tell equal leaves apart, group signals
/// indented under their header, and a header's signal count when it is collapsed.
pub fn display_names(trace: &Trace, rows: &[Row]) -> Vec<String> {
    let vars: Vec<usize> = rows.iter().filter_map(Row::sig).map(|s| s.var).collect();
    let mut leaves = unique_leaves(trace, &vars).into_iter();
    rows.iter()
        .enumerate()
        .map(|(i, r)| match r {
            Row::Group { name, collapsed: false } => format!("▾ {name}"),
            Row::Group { name, collapsed: true } => format!("▸ {name} ({})", members(rows, i).len()),
            Row::Signal(s) => format!("{}{}", if s.grouped { "  " } else { "" }, leaves.next().unwrap_or_default()),
        })
        .collect()
}

/// A var's name ends: at least its HDL name (`pkts[1].pair.lo`, not `lo`), more as needed.
fn unique_leaves(trace: &Trace, vars: &[usize]) -> Vec<String> {
    // a concatenation `{top.a,top.b}` and an expression `top.a+1` are shown whole; other computed
    // names (`top.x[7:4]`) split at the dots only, so a slice stays with its signal
    let whole = |p: &str| p.starts_with('{') || matches!(crate::computed::form(p), Some(crate::computed::Form::Expr(_)));
    let parts: Vec<Vec<&str>> = vars
        .iter()
        .map(|&v| (v, trace.vars[v].path.as_str()))
        .map(|(v, p)| if whole(p) { vec![p] } else if trace.is_computed(v) && !trace.is_composite(v) { p.split('.').collect() } else { crate::trace::parts(p) })
        .collect();
    let mut depth: Vec<usize> = vars.iter().zip(&parts).map(|(&v, p)| trace.vars[v].decl.min(p.len())).collect();
    // a name never starts at an index: `gen[0].p.hi`, not `[0].p.hi`
    let name = |i: usize, d: usize| {
        let p = &parts[i];
        let mut start = p.len().saturating_sub(d);
        while start > 0 && p[start].starts_with('[') {
            start -= 1;
        }
        crate::trace::join(p[start..].iter().copied())
    };
    loop {
        let names: Vec<String> = (0..vars.len()).map(|i| name(i, depth[i])).collect();
        let mut changed = false;
        for i in 0..vars.len() {
            if depth[i] < parts[i].len() && names.iter().enumerate().any(|(j, n)| j != i && *n == names[i]) {
                depth[i] += 1;
                changed = true;
            }
        }
        if !changed {
            return names;
        }
    }
}

/// A value of `var` as text in the line's radix; 1-bit values (and bits of a bus) as the bit itself.
pub fn label(var: &Var, look: &Look, v: &Val) -> String {
    match v {
        Val::Bits(b) if b.len() == 1 => b.to_uppercase(),
        Val::Bits(b) => match (var.time, u64::from_str_radix(b, 2), var.name_of(b).filter(|_| look.radix == radix::ENUM)) {
            (Some(fpt), Ok(t), _) => crate::time::format(t, Some(fpt)),
            (_, _, Some(name)) => name.into(),
            _ => (RADIXES[look.radix].format)(b, look.frac),
        },
        Val::Text(t) => t.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(var: usize, grouped: bool) -> Row {
        Row::Signal(Sig { var, look: Look { radix: 0, frac: 0, color: None }, grouped, open: Vec::new(), looks: Vec::new() })
    }

    #[test]
    fn groups() {
        let g = |collapsed| Row::Group { name: "g".into(), collapsed };
        let mut rows = vec![sig(0, false), g(true), sig(1, true), sig(2, true), sig(3, false), g(false)];
        assert_eq!(group_end(&rows, 1), 4);
        assert_eq!(header_of(&rows, 3), Some(1));
        assert_eq!(header_of(&rows, 4), None);
        assert_eq!(members(&rows, 1), [1, 2]);
        assert_eq!(visible(&rows), [0, 1, 4, 5]);
        tidy(&mut rows);
        assert_eq!(rows.len(), 5);
    }

    fn vars(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| match r {
                Row::Group { .. } => "[".into(),
                Row::Signal(s) => format!("{}{}", if s.grouped { "g" } else { "" }, s.var),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn moves() {
        let g = Row::Group { name: "g".into(), collapsed: false };
        let mut rows = vec![sig(0, false), g, sig(1, true), sig(2, true), sig(3, false)];
        // 0 walks down into the group, through it and out again
        let mut i = 0;
        let mut seen = Vec::new();
        for _ in 0..5 {
            i = move_row(&mut rows, i, true);
            seen.push(vars(&rows));
        }
        assert_eq!(seen, ["[ g0 g1 g2 3", "[ g1 g0 g2 3", "[ g1 g2 g0 3", "[ g1 g2 0 3", "[ g1 g2 3 0"]);
        // and back up
        for _ in 0..5 {
            i = move_row(&mut rows, i, false);
        }
        assert_eq!((vars(&rows), i), ("0 [ g1 g2 3".into(), 0));
        // a group moves as a whole
        assert_eq!(move_row(&mut rows, 1, true), 2);
        assert_eq!(vars(&rows), "0 3 [ g1 g2");
        assert_eq!(move_row(&mut rows, 2, false), 1);
        assert_eq!(move_row(&mut rows, 1, false), 0);
        assert_eq!(vars(&rows), "[ g1 g2 0 3");
        // a collapsed group is passed over
        rows[0] = Row::Group { name: "g".into(), collapsed: true };
        assert_eq!(move_row(&mut rows, 3, false), 0);
        assert_eq!(vars(&rows), "0 [ g1 g2 3");
        assert_eq!(move_row(&mut rows, 0, true), 3);
    }

    #[test]
    fn places() {
        let g = Row::Group { name: "g".into(), collapsed: false };
        let mut rows = vec![sig(0, false), g, sig(1, true), sig(2, true), sig(3, false)];
        // a header takes its whole group into the block
        assert_eq!(block(&rows, 1, 1), 1..4);
        assert_eq!(block(&rows, 0, 2), 0..4);
        // into the group, between its rows; then out, at the end
        assert_eq!(place(&mut rows, 0..1, 3, true), 2);
        assert_eq!(vars(&rows), "[ g1 g0 g2 3");
        assert_eq!(place(&mut rows, 2..3, 5, false), 4);
        assert_eq!(vars(&rows), "[ g1 g2 3 0");
        // in place, it only changes its group: below the group's last row, out of it
        assert_eq!(place(&mut rows, 2..3, 3, false), 2);
        assert_eq!(vars(&rows), "[ g1 2 3 0");
        // a group moves whole
        assert_eq!(place(&mut rows, 0..2, 5, false), 3);
        assert_eq!(vars(&rows), "2 3 0 [ g1");
        assert_eq!(place(&mut rows, 3..5, 0, false), 0);
        assert_eq!(vars(&rows), "[ g1 2 3 0");
        // several rows: the signals before a header take the place's group, the group keeps its own
        assert_eq!(place(&mut rows, 2..4, 1, true), 1);
        assert_eq!(vars(&rows), "[ g2 g3 g1 0");
        let mut rows = vec![sig(0, false), sig(1, false), Row::Group { name: "h".into(), collapsed: false }, sig(2, true), sig(3, false)];
        assert_eq!(place(&mut rows, 1..4, 5, false), 2);
        assert_eq!(vars(&rows), "0 3 1 [ g2");
    }

    #[test]
    fn steps() {
        // a range of two walks down into a group, through it and out, as a drag would move it
        let g = Row::Group { name: "g".into(), collapsed: false };
        let mut rows = vec![sig(0, false), sig(1, false), g, sig(2, true), sig(3, false)];
        let mut b = 0..2;
        let mut seen = Vec::new();
        while let Some(n) = step(&mut rows, b.clone(), true) {
            b = n;
            seen.push(vars(&rows));
        }
        assert_eq!(seen, ["[ g0 g1 g2 3", "[ g2 g0 g1 3", "[ g2 3 0 1"]);
        // and back up; a block with a header passes a whole group
        while let Some(n) = step(&mut rows, b.clone(), false) {
            b = n;
        }
        assert_eq!((vars(&rows), b), ("0 1 [ g2 3".into(), 0..2));
        assert_eq!(step(&mut rows, 2..5, false), Some(1..4));
        assert_eq!(vars(&rows), "0 [ g2 3 1");
        let mut rows = vec![Row::Group { name: "h".into(), collapsed: false }, sig(0, true), sig(1, false), Row::Group { name: "g".into(), collapsed: false }, sig(2, true)];
        assert_eq!(step(&mut rows, 0..3, true), Some(2..5));
        assert_eq!(vars(&rows), "[ g2 [ g0 1");
    }

    #[test]
    fn grouping() {
        let mut rows = vec![sig(0, false), sig(1, false), sig(2, false)];
        assert_eq!(range(&rows, 2, 1), 1..3);
        group(&mut rows, 1..3, "g").unwrap();
        assert_eq!(vars(&rows), "0 [ g1 g2");
        assert_eq!(range(&rows, 0, 1), 0..4);
        assert!(group(&mut rows, 0..2, "h").is_err());
    }
}
