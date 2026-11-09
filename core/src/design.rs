//! The file's design as a tree of scopes and signals, each path also in canonical form: `signals`
//! searches it and errors name its nearest names (spec §15). Built again when the computed vars
//! come or go.

use crate::trace::{Var, canon, join, parts};
use std::collections::HashMap;

/// A scope, or a var of the file: a signal, a struct, record or array, or a computed one.
pub struct Node {
    /// As the row shows it: the last part (`data[7:4]` for a computed slice, `{a,b}` for a
    /// concatenation).
    pub name: String,
    /// As the file writes it.
    pub path: String,
    /// In canonical form (`canon`): what searches match.
    pub canon: String,
    /// None for a scope.
    pub var: Option<usize>,
    /// The scope it is in, or the struct or array it is part of.
    pub parent: Option<usize>,
    /// How many scopes it is in, or for a scope how many it is and is in (`top.cpu`: 2).
    pub depth: usize,
    /// The signals of the file in it; a file signal: 1, a computed one: 0. What `signals`
    /// reports.
    pub signals: usize,
    /// The var of its first signal: nodes listed by it are in the file's order.
    pub first: usize,
    /// What is in it, in the panel's order (spec §5): a struct's or array's fields in their
    /// declaration order, else scopes and composites first, then signals, each by `first`.
    pub children: Vec<usize>,
    /// A var of the file, or a scope holding one: rows, search and errors are the file's.
    pub file: bool,
}

pub struct Design {
    pub nodes: Vec<Node>,
    /// The nodes no other node holds, in the panel's order.
    pub roots: Vec<usize>,
}

impl Design {
    /// The tree of all vars, `vars` (composites included); those from `fixed` on are computed.
    pub fn of(vars: &[Var], fixed: usize) -> Self {
        let mut nodes: Vec<Node> = Vec::new();
        let mut scopes: HashMap<String, usize> = HashMap::new();
        let mut of_var: Vec<usize> = Vec::with_capacity(vars.len());
        for (v, var) in vars.iter().enumerate() {
            // a concatenation is one part at the top; a computed var splits on `.` only, so a
            // slice is a leaf in its scope (`data[7:4]`); the file's as `parts` finds them
            let p: Vec<&str> = match () {
                _ if var.path.starts_with('{') => vec![&var.path],
                _ if v >= fixed => var.path.split('.').collect(),
                _ => parts(&var.path),
            };
            let n = p.len().saturating_sub(var.decl.max(1));
            // the scopes it is in, each made once
            let mut parent = None;
            for k in 1..=n {
                let path = join(p[..k].iter().copied());
                parent = Some(match scopes.get(&path) {
                    Some(&s) => s,
                    None => {
                        let name = p[k - 1].to_string();
                        nodes.push(Node { name, canon: canon(&path), path: path.clone(), var: None, parent, depth: k, signals: 0, first: v, children: Vec::new(), file: false });
                        scopes.insert(path, nodes.len() - 1);
                        nodes.len() - 1
                    }
                });
                nodes[parent.unwrap()].file |= v < fixed;
            }
            let name = p.last().copied().unwrap_or_default().to_string();
            nodes.push(Node { name, path: var.path.clone(), canon: canon(&var.path), var: Some(v), parent, depth: n, signals: 0, first: v, children: Vec::new(), file: v < fixed });
            of_var.push(nodes.len() - 1);
        }
        // a struct's or array's fields are in it rather than in its scope
        for (v, var) in vars.iter().enumerate() {
            for &c in &var.children {
                nodes[of_var[c]].parent = Some(of_var[v]);
            }
        }
        // each file signal counts in every node it is in; a node's first is its first leaf's
        for v in (0..vars.len()).filter(|&v| vars[v].children.is_empty()) {
            let mut at = Some(of_var[v]);
            while let Some(i) = at {
                if v < fixed {
                    nodes[i].signals += 1;
                }
                nodes[i].first = nodes[i].first.min(v);
                at = nodes[i].parent;
            }
        }
        // what is in each node, in the panel's order (spec §5)
        for i in 0..nodes.len() {
            if let Some(p) = nodes[i].parent {
                nodes[p].children.push(i);
            }
        }
        for i in 0..nodes.len() {
            let Some(v) = nodes[i].var else {
                let mut children = std::mem::take(&mut nodes[i].children);
                children.sort_by_key(|&c| (nodes[c].children.is_empty(), nodes[c].first));
                nodes[i].children = children;
                continue;
            };
            if !vars[v].children.is_empty() {
                nodes[i].children = vars[v].children.iter().map(|&c| of_var[c]).collect();
            }
        }
        let mut roots: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].parent.is_none()).collect();
        roots.sort_by_key(|&i| (!nodes[i].children.is_empty(), nodes[i].first));
        Design { nodes, roots }
    }

    /// The nodes with children whose canonical parts end with `name`'s, in node order: what a
    /// `:add` name or an error's name can stand for.
    pub fn ending(&self, name: &str) -> Vec<usize> {
        let name = canon(name);
        let key = parts(&name);
        (0..self.nodes.len()).filter(|&i| !self.nodes[i].children.is_empty() && parts(&self.nodes[i].canon).ends_with(&key)).collect()
    }

    /// The leaf vars under `nodes` and the nodes themselves, each once, in var order.
    pub fn leaves(&self, nodes: &[usize]) -> Vec<usize> {
        let mut leaves = Vec::new();
        let mut todo: Vec<usize> = nodes.to_vec();
        while let Some(k) = todo.pop() {
            if self.nodes[k].children.is_empty() {
                if let Some(v) = self.nodes[k].var {
                    leaves.push(v);
                }
            } else {
                todo.extend(&self.nodes[k].children);
            }
        }
        leaves.sort_unstable();
        leaves.dedup();
        leaves
    }

    /// The nodes whose canonical path `hit` takes, in no order.
    pub fn matching(&self, hit: impl Fn(&str) -> bool) -> Vec<usize> {
        (0..self.nodes.len()).filter(|&i| hit(&self.nodes[i].canon)).collect()
    }

    /// The last part of a node's canonical path, without its index (`gen` for `top.gen[3]`).
    fn end(&self, i: usize) -> &str {
        let c = &self.nodes[i].canon;
        let last = parts(c).last().copied().unwrap_or("");
        last.split('[').next().unwrap_or("")
    }

    /// The scopes and signals nearest to a name or pattern that matched nothing, best first, at
    /// most `n`: those whose last part holds its last word of two letters or more (`clk_i` for
    /// `clk`), else whose last part is that word with letters left out or added, half of it at
    /// least (`busy` for `bsy`, `clk` for `clock`), else with few letters changed (one in five,
    /// plus one); ranked by how many of its other words the path holds, then by the closest last
    /// part, then the shortest path. An error lists them, so the next try is a pick, not another
    /// guess (spec §15).
    pub fn nearest(&self, name: &str, n: usize) -> Vec<&str> {
        let name = canon(name);
        let words: Vec<&str> = name.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty() && !w.chars().all(|c| c.is_ascii_digit())).collect();
        let Some((&last, others)) = words.split_last() else { return Vec::new() };
        // an element of an array is the array's name again: the array stands for them
        let named = (0..self.nodes.len()).filter(|&i| self.nodes[i].file && !self.nodes[i].canon.ends_with(']'));
        let tiers: [&dyn Fn(&str) -> bool; 3] = [
            &|end| last.len() >= 2 && end.contains(last),
            &|end| {
                let (short, long) = if last.len() <= end.len() { (last, end) } else { (end, last) };
                short.len() >= 3 && 2 * short.len() >= long.len() && subsequence(short, long)
            },
            &|end| last.len() >= 4 && distance(last, end) <= last.len() / 5 + 1,
        ];
        let Some(mut found) = tiers.iter().map(|hit| named.clone().filter(|&i| hit(self.end(i))).collect::<Vec<_>>()).find(|f| !f.is_empty()) else {
            return Vec::new();
        };
        let node = &self.nodes;
        found.sort_by_key(|&i| (std::cmp::Reverse(others.iter().filter(|w| node[i].canon.contains(**w)).count()), self.end(i).len().abs_diff(last.len()), node[i].path.len()));
        found.into_iter().take(n).map(|i| node[i].path.as_str()).collect()
    }
}

/// `short`'s letters appear in `long` in order (`bsy` in `busy`).
fn subsequence(short: &str, long: &str) -> bool {
    let mut it = long.chars();
    short.chars().all(|c| it.any(|l| l == c))
}

/// The edit distance between two names.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let next = (prev + usize::from(ca != cb)).min(row[j] + 1).min(row[j + 1] + 1);
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}
