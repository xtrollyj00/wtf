//! The signal panel of Edit mode (spec §5): the file's scope tree, searched as in vim.

use std::collections::HashMap;
use wtf_core::trace::Trace;

pub struct Node {
    pub name: String,
    pub path: String,
    pub depth: usize,
    /// None for a scope.
    pub var: Option<usize>,
    /// End (exclusive) of this node's subtree in `nodes`.
    pub end: usize,
    pub open: bool,
    /// The signal a struct, record or array is (spec §6 "Composite rows"), added as one row.
    pub whole: Option<usize>,
}

pub struct Panel {
    /// The tree in display order: each scope is followed by its subscopes, then its signals.
    pub nodes: Vec<Node>,
    /// Highlighted node in the tree.
    pub sel: usize,
    /// The search being typed.
    pub query: Option<String>,
    /// The last search, and the nodes it matches in tree order.
    pub pattern: String,
    pub matches: Vec<usize>,
    /// Where the search being typed started; Esc goes back there.
    origin: usize,
    /// First line on screen.
    pub top: usize,
    /// How many vars of the trace the tree holds; computed ones come later (`draw` rebuilds it).
    pub vars: usize,
    /// Signals picked with Space, in the order they were picked.
    pub marked: Vec<usize>,
    /// The other end of a `V` range.
    pub anchor: Option<usize>,
}

#[derive(Default)]
struct Scope<'a> {
    scopes: Vec<(&'a str, Scope<'a>)>,
    /// Where each subscope is in `scopes`: a scope can have tens of thousands.
    index: HashMap<&'a str, usize>,
    vars: Vec<(&'a str, usize)>,
    /// Part of a signal's HDL name (a struct, record or array): listed in the file's order, its
    /// fields as declared, not subscopes first.
    composite: bool,
    /// Subscopes (true) and signals in the file's order, as indices into `scopes` / `vars`.
    order: Vec<(bool, usize)>,
}

impl Panel {
    /// The tree is built from the signals' full paths, so scopes without signals don't appear.
    pub fn new(trace: &Trace) -> Self {
        let mut root = Scope::default();
        // a composite is its scope's node, not a signal of its own in the tree
        for (i, v) in trace.vars.iter().enumerate().filter(|(i, _)| !trace.is_composite(*i)) {
            // a concatenation `{top.a,top.b}` is a signal at the top
            // an array's elements nest under it (`mem` > `[0]`); a computed slice `top.x[7:4]` stays whole
            let mut parts: Vec<&str> = match () {
                _ if v.path.starts_with('{') => vec![&v.path],
                _ if trace.is_computed(i) => v.path.split('.').collect(),
                _ => wtf_core::trace::parts(&v.path),
            };
            let leaf = parts.pop().unwrap_or_default();
            // the scopes from here on are inside the signal's HDL name
            let inside = (parts.len() + 1).saturating_sub(v.decl);
            let mut s = &mut root;
            for (depth, p) in parts.into_iter().enumerate() {
                let k = *s.index.entry(p).or_insert_with(|| {
                    s.scopes.push((p, Scope::default()));
                    s.order.push((true, s.scopes.len() - 1));
                    s.scopes.len() - 1
                });
                s = &mut s.scopes[k].1;
                s.composite |= depth >= inside;
            }
            s.order.push((false, s.vars.len()));
            s.vars.push((leaf, i));
        }
        let mut nodes = Vec::new();
        flatten(&root, "", 0, &mut nodes);
        for n in nodes.iter_mut().filter(|n| n.var.is_none()) {
            n.whole = trace.by_path.get(&n.path).copied().filter(|&v| trace.is_composite(v));
        }
        Panel { nodes, sel: 0, query: None, pattern: String::new(), matches: Vec::new(), origin: 0, top: 0, vars: trace.vars.len(), marked: Vec::new(), anchor: None }
    }

    /// Tree nodes on screen: the contents of closed scopes are hidden.
    pub fn visible(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.nodes.len() {
            out.push(i);
            i = if self.nodes[i].open || self.nodes[i].var.is_some() { i + 1 } else { self.nodes[i].end };
        }
        out
    }

    /// Move the highlight by `d` lines.
    pub fn step(&mut self, d: isize) {
        let vis = self.visible();
        let p = vis.iter().position(|&i| i == self.sel).unwrap_or(0);
        self.sel = vis[p.saturating_add_signed(d).min(vis.len().saturating_sub(1))];
    }

    /// `l`: open a scope. `h`: close it, or go to the parent scope.
    pub fn open(&mut self, open: bool) {
        let Some(n) = self.nodes.get_mut(self.sel) else { return };
        if n.var.is_none() && n.open != open {
            n.open = open;
        } else if !open {
            let depth = n.depth;
            if let Some(p) = self.nodes[..self.sel].iter().rposition(|x| x.depth < depth) {
                self.sel = p;
            }
        }
    }

    pub fn start_search(&mut self) {
        self.origin = self.sel;
        self.query = Some(String::new());
    }

    /// While typing, the highlight jumps to the first match from where the search started.
    pub fn search(&mut self) {
        self.matches = self.find(self.query.as_deref().unwrap_or(""));
        self.sel = self.origin;
        if !self.matches.is_empty() {
            let k = self.matches.partition_point(|&i| i < self.origin);
            self.jump(k % self.matches.len());
        }
    }

    /// Enter: the typed search becomes the one `n` / `N` repeat. Returns whether it matched.
    pub fn confirm(&mut self) -> bool {
        self.pattern = self.query.take().unwrap_or_default();
        !self.matches.is_empty()
    }

    /// Esc while typing: back to where the search started, with the previous search.
    pub fn cancel_search(&mut self) {
        self.query = None;
        self.sel = self.origin;
        self.matches = self.find(&self.pattern);
    }

    /// Case-insensitive substring of the full path, scopes included (`cpu.st` matches `top.cpu.state`).
    fn find(&self, q: &str) -> Vec<usize> {
        if q.is_empty() {
            return Vec::new();
        }
        let q = q.to_lowercase();
        self.nodes.iter().enumerate().filter(|(_, n)| n.path.to_lowercase().contains(&q)).map(|(i, _)| i).collect()
    }

    /// `n` / `N`: the `count`-th next / previous match in tree order, wrapping around.
    pub fn next(&mut self, forward: bool, count: usize) {
        let m = &self.matches;
        if m.is_empty() {
            return;
        }
        let k = if forward {
            m.partition_point(|&i| i <= self.sel) as i64 + count as i64 - 1
        } else {
            m.partition_point(|&i| i < self.sel) as i64 - count as i64
        };
        self.jump(k.rem_euclid(m.len() as i64) as usize);
    }

    /// Highlight a match, opening every scope around it.
    fn jump(&mut self, k: usize) {
        let i = self.matches[k];
        for j in 0..i {
            if self.nodes[j].var.is_none() && self.nodes[j].end > i {
                self.nodes[j].open = true;
            }
        }
        self.sel = i;
    }

    /// Tree nodes in the `V` range, in tree order. A range whose start was hidden by closing
    /// its scope is dropped.
    pub fn range(&mut self) -> Vec<usize> {
        let Some(a) = self.anchor else { return Vec::new() };
        let vis = self.visible();
        let (Some(pa), Some(ps)) = (vis.iter().position(|&i| i == a), vis.iter().position(|&i| i == self.sel)) else {
            self.anchor = None;
            return Vec::new();
        };
        vis[pa.min(ps)..=pa.max(ps)].to_vec()
    }

    /// The signals of a node: itself, a struct or array as one, or those directly in a scope.
    pub fn vars_of(&self, i: usize) -> Vec<usize> {
        let n = &self.nodes[i];
        match n.var.or(n.whole) {
            Some(v) => vec![v],
            None => self.nodes[i + 1..n.end].iter().filter(|c| c.depth == n.depth + 1).filter_map(|c| c.var.or(c.whole)).collect(),
        }
    }

    /// The signals in the `V` range; a closed scope in it brings the signals directly in it.
    pub fn range_vars(&mut self) -> Vec<usize> {
        let r = self.range();
        r.iter().filter(|&&i| self.nodes[i].var.is_some() || !self.nodes[i].open).flat_map(|&i| self.vars_of(i)).collect()
    }

    /// The signals of the highlighted node: itself, or those directly in a scope.
    pub fn vars_here(&self) -> Vec<usize> {
        if self.sel >= self.nodes.len() {
            return Vec::new();
        }
        self.vars_of(self.sel)
    }

    /// Position of the highlight among the matches (1-based), if it is on one.
    pub fn match_pos(&self) -> Option<usize> {
        self.matches.binary_search(&self.sel).ok().map(|k| k + 1)
    }
}

fn flatten(s: &Scope, prefix: &str, depth: usize, out: &mut Vec<Node>) {
    let order: Vec<(bool, usize)> = match s.composite {
        true => s.order.clone(),
        false => (0..s.scopes.len()).map(|k| (true, k)).chain((0..s.vars.len()).map(|k| (false, k))).collect(),
    };
    for (scope, k) in order {
        if scope {
            let (name, sub) = &s.scopes[k];
            let path = wtf_core::trace::join([prefix, name].into_iter().filter(|p| !p.is_empty()));
            let at = out.len();
            // top-level scopes start open, so the panel doesn't open on a single closed line
            out.push(Node { name: name.to_string(), path: path.clone(), depth, var: None, end: 0, open: depth == 0, whole: None });
            flatten(sub, &path, depth + 1, out);
            out[at].end = out.len();
        } else {
            let (name, var) = s.vars[k];
            let path = wtf_core::trace::join([prefix, name].into_iter().filter(|p| !p.is_empty()));
            out.push(Node { name: name.to_string(), path, depth, var: Some(var), end: out.len() + 1, open: false, whole: None });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> Panel {
        Panel::new(&Trace::open("tests/data/simple.vcd".as_ref()).unwrap())
    }

    fn tree(p: &Panel) -> Vec<String> {
        p.visible().iter().map(|&i| format!("{}{}", "  ".repeat(p.nodes[i].depth), p.nodes[i].name)).collect()
    }

    /// An array's elements nest under it, element by element (`grid` > `[1]` > `[2]`); fields
    /// under their struct (tests/data/types-*, core/tests/types.rs).
    #[test]
    fn composites_nest() {
        let open = |file: &str| {
            let mut p = Panel::new(&Trace::open(format!("tests/data/{file}").as_ref()).unwrap());
            p.nodes.iter_mut().for_each(|n| n.open = true);
            tree(&p).join("|")
        };
        // shortcut: wellen names grid's diagonal grid[i].grid[i]., so they nest as grid > [i] > grid
        // > [i] > an empty leaf, fixed by a later piece
        assert_eq!(
            open("types-verilator-structs.fst"),
            "types|  pkt|    valid|    state|    pair|      hi|      lo|  loose|    a|    b|  packed2|    [3]|    [2]|    [1]|    [0]|\
             \x20 mem|    [0]|    [1]|    [2]|    [3]|  grid|    [0]|      grid|        [0]|          |      [1]|      [2]|\
             \x20   [1]|      [0]|      grid|        [1]|          |      [2]|  pkts|    [0]|      valid|      state|      pair|\
             \x20       hi|        lo|    [1]|      valid|      state|      pair|        hi|        lo|  word|    byte_|    pair|\
             \x20     hi|      lo|  gen|    [0]|      p|        hi|        lo|    [1]|      p|        hi|        lo|  clk"
        );
        assert_eq!(
            open("types-nvc.fst"),
            "types|  pkt|    valid|    state|    pair|      hi|      lo|  pkts|    [0]|      valid|      state|      pair|        hi|\
             \x20       lo|    [1]|      valid|      state|      pair|        hi|        lo|  rows|    [0]|      [0]|      [1]|      [2]|\
             \x20   [1]|      [0]|      [1]|      [2]|  link|    addr|    data|      [0]|      [1]|      [2]|  gen|    [0]|      p|\
             \x20       hi|        lo|    [1]|      p|        hi|        lo|  clk"
        );
    }

    #[test]
    fn tree_and_search() {
        let mut p = panel();
        assert_eq!(tree(&p), ["top", "  cpu", "  dma", "  clk", "  rst"]);
        p.step(1);
        p.open(true);
        assert_eq!(tree(&p), ["top", "  cpu", "    state", "    data", "    busy", "  dma", "  clk", "  rst"]);
        p.step(2);
        p.open(false);
        assert_eq!(p.nodes[p.sel].path, "top.cpu");
        p.open(false);
        assert_eq!(tree(&p), ["top", "  cpu", "  dma", "  clk", "  rst"]);
        p.start_search();
        p.query = Some("STATE".into());
        p.search();
        assert_eq!(p.nodes[p.sel].path, "top.cpu.state");
        assert!(p.confirm());
        p.next(true, 1);
        assert_eq!(p.nodes[p.sel].path, "top.dma.state");
        p.next(true, 1);
        assert_eq!(p.nodes[p.sel].path, "top.cpu.state");
        p.next(false, 1);
        assert_eq!(p.nodes[p.sel].path, "top.dma.state");
        assert_eq!(p.match_pos(), Some(2));
    }
}
