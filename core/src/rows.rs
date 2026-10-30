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
    pub radix: usize,
    /// Fraction bits for a fixed-point radix (`q4.12`), 0 otherwise.
    pub frac: u32,
    /// Belongs to the group whose header is above it.
    pub grouped: bool,
    /// A bus expanded into its bits shows this many bit lines under it (MSB first); 0 when folded.
    pub bits: u32,
    /// Its own colour, an index into `COLORS`; the theme's when None (spec §14 "Colours").
    pub color: Option<usize>,
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

/// Screen lines: each visible row, followed by the bit lines of an expanded bus.
pub fn lines(rows: &[Row]) -> Vec<(usize, Option<u32>)> {
    let mut out = Vec::new();
    for i in visible(rows) {
        out.push((i, None));
        if let Row::Signal(s) = &rows[i] {
            out.extend((0..s.bits).rev().map(|b| (i, Some(b))));
        }
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
        Row::Signal(s) => Row::Signal(Sig { bits: 0, ..s.clone() }),
    };
    a.iter().map(norm).eq(b.iter().map(norm))
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
        .map(|(v, p)| if whole(p) { vec![p] } else if trace.is_computed(v) { p.split('.').collect() } else { crate::trace::parts(p) })
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

/// A value of `var` as text in the row's radix; 1-bit values (and bits of a bus) as the bit itself.
pub fn label(var: &Var, row: &Sig, v: &Val) -> String {
    match v {
        Val::Bits(b) if b.len() == 1 => b.to_uppercase(),
        Val::Bits(b) => match (var.time, u64::from_str_radix(b, 2), var.name_of(b).filter(|_| row.radix == radix::ENUM)) {
            (Some(fpt), Ok(t), _) => crate::time::format(t, Some(fpt)),
            (_, _, Some(name)) => name.into(),
            _ => (RADIXES[row.radix].format)(b, row.frac),
        },
        Val::Text(t) => t.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(var: usize, grouped: bool) -> Row {
        Row::Signal(Sig { var, radix: 0, frac: 0, grouped, bits: 0, color: None })
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
        if let Row::Signal(s) = &mut rows[4] {
            s.bits = 2;
        }
        assert_eq!(lines(&rows), [(0, None), (1, None), (4, None), (4, Some(1)), (4, Some(0)), (5, None)]);
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
    fn grouping() {
        let mut rows = vec![sig(0, false), sig(1, false), sig(2, false)];
        assert_eq!(range(&rows, 2, 1), 1..3);
        group(&mut rows, 1..3, "g").unwrap();
        assert_eq!(vars(&rows), "0 [ g1 g2");
        assert_eq!(range(&rows, 0, 1), 0..4);
        assert!(group(&mut rows, 0..2, "h").is_err());
    }
}
