//! Shared by the core's tests: the fixture trace and short ways to look at a workspace.
#![allow(dead_code)] // each test file uses some of these

use std::path::PathBuf;
use wtf_core::rows::{self, Row};
use wtf_core::workspace::Workspace;

/// `tests/data/simple.<ext>`: clk (period 10ns, rising at 5, 15 ...), rst, cpu.state, cpu.data,
/// cpu.busy (high 20ns - 40ns), dma.state; 0 - 80ns at 1ns ticks.
pub fn fixture(ext: &str) -> PathBuf {
    PathBuf::from(format!("{}/../tests/data/simple.{ext}", env!("CARGO_MANIFEST_DIR")))
}

pub fn open() -> Workspace {
    Workspace::open(&fixture("vcd")).unwrap()
}

/// A workspace with these rows, added in order by `:add`.
pub fn with(rows: &[&str]) -> Workspace {
    let mut ws = open();
    for r in rows {
        wtf_core::command::run(&mut ws, &format!("add {r}")).unwrap();
    }
    ws
}

/// The rows as wtf names them, groups marked `▾` / `▸`.
pub fn names(ws: &Workspace) -> Vec<String> {
    rows::display_names(&ws.trace, &ws.rows)
}

pub fn var(ws: &Workspace, path: &str) -> usize {
    ws.trace.by_path[path]
}

/// The selected row's name.
pub fn selected(ws: &Workspace) -> String {
    names(ws)[ws.sel].trim().to_string()
}

pub fn radix_of(ws: &Workspace, path: &str) -> &'static str {
    let v = var(ws, path);
    match ws.rows.iter().find_map(|r| r.sig().filter(|s| s.var == v)) {
        Some(s) => wtf_core::radix::RADIXES[s.radix].name,
        None => panic!("{path} isn't shown"),
    }
}

pub fn is_group(r: &Row) -> bool {
    r.is_group()
}

/// A scratch directory of the test's own, removed when dropped.
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("wtf-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
