//! Shared by the core's tests: the fixture trace and short ways to look at a workspace.
#![allow(dead_code)] // each test file uses some of these

use std::path::PathBuf;
use wtf_core::rows::{self, Row};
use wtf_core::workspace::Workspace;

/// `tests/data/simple.<ext>`: clk (period 10ns, rising at 5, 15 ...), rst, cpu.state, cpu.data,
/// cpu.busy (high 20ns - 40ns), dma.state; 0 - 80ns at 1ns ticks.
pub fn fixture(ext: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../tests/data/simple.{ext}",
        env!("CARGO_MANIFEST_DIR")
    ))
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
        Some(s) => wtf_core::radix::RADIXES[s.look.radix].name,
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

/// A protocol from a file, as the binary makes one: a card, and a factory for an instance that
/// makes one stream and no records of it (spec §16 "A protocol from a file").
pub fn loaded(
    name: &'static str,
    params: &'static [&'static str],
) -> std::sync::Arc<wtf_core::protocol::Loaded> {
    use wtf_core::protocol::{Def, Desc, Loaded, NO_MAKE, Out, Protocol, Roles, StreamDesc};
    struct Nothing {
        desc: Desc,
    }
    impl Protocol for Nothing {
        fn desc(&self) -> &Desc {
            &self.desc
        }
        fn feed(
            &mut self,
            _start: wtf_core::time::Tick,
            _end: wtf_core::time::Tick,
            _roles: &Roles<'_>,
        ) -> Vec<Out> {
            Vec::new()
        }
        fn finish(&mut self) -> Vec<Out> {
            Vec::new()
        }
    }
    std::sync::Arc::new(Loaded {
        name,
        def: Def {
            about: "a test's protocol",
            example: "decode p test",
            clocked: true,
            sides: &[],
            roles: &[],
            params,
            make: NO_MAKE,
        },
        make: std::sync::Arc::new(|_, _, _| {
            let streams = vec![StreamDesc {
                name: "beat",
                fields: Vec::new(),
                violations: false,
                about: Vec::new(),
                from: &[],
            }];
            Ok(Box::new(Nothing {
                desc: Desc { streams },
            }) as Box<dyn Protocol>)
        }),
    })
}
