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
}

impl Panel {
    /// The tree is built from the signals' full paths, so scopes without signals don't appear.
    pub fn new(trace: &Trace) -> Self {
        let mut root = Scope::default();
        for (i, v) in trace.vars.iter().enumerate() {
            // a concatenation `{top.a,top.b}` is a signal at the top
            let mut parts: Vec<&str> = if v.path.starts_with('{') { vec![&v.path] } else { v.path.split('.').collect() };
            let leaf = parts.pop().unwrap_or_default();
            let mut s = &mut root;
            for p in parts {
                let k = *s.index.entry(p).or_insert_with(|| {
                    s.scopes.push((p, Scope::default()));
                    s.scopes.len() - 1
                });
                s = &mut s.scopes[k].1;
            }
            s.vars.push((leaf, i));
        }
        let mut nodes = Vec::new();
        flatten(&root, "", 0, &mut nodes);
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

    /// The signals of a node: itself, or those directly in a scope.
    pub fn vars_of(&self, i: usize) -> Vec<usize> {
        let n = &self.nodes[i];
        match n.var {
            Some(v) => vec![v],
            None => self.nodes[i + 1..n.end].iter().filter(|c| c.depth == n.depth + 1).filter_map(|c| c.var).collect(),
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
    for (name, sub) in &s.scopes {
        let path = if prefix.is_empty() { name.to_string() } else { format!("{prefix}.{name}") };
        let at = out.len();
        // top-level scopes start open, so the panel doesn't open on a single closed line
        out.push(Node { name: name.to_string(), path: path.clone(), depth, var: None, end: 0, open: depth == 0 });
        flatten(sub, &path, depth + 1, out);
        out[at].end = out.len();
    }
    for &(name, var) in &s.vars {
        let path = if prefix.is_empty() { name.to_string() } else { format!("{prefix}.{name}") };
        out.push(Node { name: name.to_string(), path, depth, var: Some(var), end: out.len() + 1, open: false });
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
