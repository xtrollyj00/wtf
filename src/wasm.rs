//! Protocols loaded from a file (spec §16): a WebAssembly component's `wit/protocol.wit` world, run
//! in wasmtime. The core's `Protocol` trait is the same shape, so a wasm protocol decodes as a
//! built-in does.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use wasmtime::component::{Component, Linker, bindgen};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wtf_core::protocol::{self, Desc, Factory, Given, Out, Params, Roles, Severity, StreamDesc};
use wtf_core::stream::Field;
use wtf_core::time::Tick;
use wtf_core::trace::Val;

bindgen!({ path: "wit/protocol.wit", world: "protocol" });

use exports::wtf::protocol::api::{
    Card, DefaultValue, Out as WasmOut, Severity as WasmSeverity, Value,
};

/// What one call may spend and one instance may hold: a decode that asks for more is a violation, not
/// a hung window. A `probe` feed spends 6,571 fuel, its `finish` 63.
const FUEL: u64 = 1_000_000;
const MEMORY: usize = 32 << 20;
/// The rule a trap or a cap hit reports under.
const HOST: &str = "host";

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        config.consume_fuel(true);
        Engine::new(&config).expect("a wasm engine")
    })
}

/// A store of its own per instance (a trapped decode leaves the others alive), with fuel refilled
/// before each call. It starts full too: instantiating spends fuel on some targets (16,385 on
/// aarch64, none on x86_64).
fn store() -> Store<StoreLimits> {
    let mut store = Store::new(
        engine(),
        StoreLimitsBuilder::new()
            .memory_size(MEMORY)
            .instances(4)
            .build(),
    );
    store.limiter(|limits| limits);
    store.set_fuel(FUEL).expect("the engine consumes fuel");
    store
}

/// A wasmtime error as the one line an error reads: its cause, not the wrapper's context.
fn say(e: wasmtime::Error) -> String {
    e.root_cause().to_string()
}

/// A card's list, in the one table a loaded protocol's strings live in.
fn list<T>(items: impl IntoIterator<Item = T>) -> &'static [T] {
    Box::leak(items.into_iter().collect::<Vec<T>>().into_boxed_slice())
}

/// The protocol a component file holds, named after the file: `probe.wasm` is `probe`. Errors read
/// `<path>: <reason>`.
pub fn load_one(path: &Path) -> Result<Arc<protocol::Loaded>, String> {
    let said = |e: String| format!("{}: {e}", path.display());
    if !path.is_file() {
        return Err(said("no such file".into()));
    }
    let component =
        Arc::new(Component::from_file(engine(), path).map_err(|e| said(e.to_string()))?);
    let card = describe(&component).map_err(said)?;
    let roles: Vec<protocol::RoleDef> = card
        .roles
        .iter()
        .map(|r| protocol::RoleDef {
            name: protocol::intern(&r.name),
            required: r.required,
            default: r.default.as_ref().map(default),
            about: protocol::intern(&r.about),
        })
        .collect();
    let def = protocol::Def {
        about: protocol::intern(&card.about),
        example: protocol::intern(&card.example),
        clocked: card.clocked,
        sides: list(
            card.sides
                .iter()
                .map(|side| list(side.iter().map(|r| protocol::intern(r)))),
        ),
        roles: list(roles),
        params: list(card.params.iter().map(|p| protocol::intern(p))),
        make: protocol::NO_MAKE,
    };
    let name = protocol::intern(
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("protocol"),
    );
    let make: Factory = Arc::new(move |given, params, _| {
        Ok(Box::new(Instance::new(&component, given, params)?) as Box<dyn protocol::Protocol>)
    });
    Ok(Arc::new(protocol::Loaded { name, def, make }))
}

/// The session's `[[protocol]] wasm=` hook (spec §11): the one file, loaded now. The engine keeps
/// the compiled component, so two sessions naming a file compile it once.
pub fn loader() -> protocol::Loader {
    std::sync::Arc::new(|path: &str| load_one(std::path::Path::new(path)))
}

/// The protocols a directory holds: the config dir's `protocols/` (spec §19), a `.wasm` a protocol,
/// sorted by file name, and what didn't load for the status line. A missing directory is no error:
/// most machines have none.
pub fn load_dir(dir: &Path) -> (Vec<Arc<protocol::Loaded>>, Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "wasm"))
        .collect();
    files.sort();
    let (mut loaded, mut failed) = (Vec::new(), Vec::new());
    // file names in one directory differ, so the protocols named after them do too
    for file in files {
        match load_one(&file) {
            Ok(l) => loaded.push(l),
            // load_one's own words, path first: the status line names the file that didn't load
            Err(e) => failed.push(e),
        }
    }
    (loaded, failed)
}

/// `describe`, once, with no trace: what `protocols <name>` shows.
fn describe(component: &Component) -> Result<Card, String> {
    let mut store = store();
    let world =
        Protocol::instantiate(&mut store, component, &Linker::new(engine())).map_err(say)?;
    store.set_fuel(FUEL).map_err(say)?;
    world
        .wtf_protocol_api()
        .call_describe(&mut store)
        .map_err(say)?
}

/// One decode's component instance: `create` once, then a `feed` a cycle and a `finish`.
struct Instance {
    store: Store<StoreLimits>,
    world: Protocol,
    desc: Desc,
    /// The roles in the order `create` was given them: `feed` passes values in that order.
    roles: Vec<&'static str>,
    /// The cycle being fed, which a trap reports on.
    at: (Tick, Tick),
    /// A trapped instance stays dead: the cycle is cut short and `:e` makes a new one.
    dead: bool,
}

impl Instance {
    fn new(component: &Component, given: &Given<'_>, params: &Params) -> Result<Self, String> {
        let mut store = store();
        let world =
            Protocol::instantiate(&mut store, component, &Linker::new(engine())).map_err(say)?;
        let roles: Vec<(String, Option<u32>)> = given
            .iter()
            .map(|(role, width)| ((*role).to_string(), *width))
            .collect();
        store.set_fuel(FUEL).map_err(say)?;
        let streams = world
            .wtf_protocol_api()
            .call_create(&mut store, &roles, params)
            .map_err(say)??;
        let desc = Desc {
            streams: streams
                .iter()
                .map(|s| StreamDesc {
                    name: protocol::intern(&s.name),
                    fields: s
                        .fields
                        .iter()
                        .map(|f| Field {
                            name: f.name.clone(),
                            width: f.width,
                            time: f.time,
                            signed: f.signed,
                        })
                        .collect(),
                    violations: s.violations,
                    about: s.about.clone(),
                    from: &[],
                })
                .collect(),
        };
        let roles = roles
            .iter()
            .map(|(role, _)| protocol::intern(role))
            .collect();
        Ok(Instance {
            store,
            world,
            desc,
            roles,
            at: (0, 0),
            dead: false,
        })
    }

    /// A trap, or a cap hit: one violation at the cycle being fed, and the instance is dead.
    fn trap(&mut self, detail: String) -> Vec<Out> {
        self.dead = true;
        vec![Out::Violation {
            rule: HOST,
            start: self.at.0,
            end: self.at.1,
            detail,
            severity: Severity::Error,
        }]
    }

    /// A component's out, as the core's: a record's stream resolved, an unknown name dropped.
    fn out(&self, out: WasmOut) -> Option<Out> {
        match out {
            WasmOut::Found(f) => {
                let stream = self
                    .desc
                    .streams
                    .iter()
                    .find(|s| s.name == f.stream_name)?
                    .name;
                Some(Out::Record {
                    stream,
                    start: f.start,
                    end: f.end,
                    fields: f.fields.into_iter().map(|v| v.map(val)).collect(),
                })
            }
            WasmOut::Violation(v) => Some(Out::Violation {
                rule: protocol::intern(&v.rule),
                start: v.start,
                end: v.end,
                detail: v.detail,
                severity: severity(v.severity),
            }),
        }
    }
}

impl protocol::Protocol for Instance {
    fn desc(&self) -> &Desc {
        &self.desc
    }

    fn feed(&mut self, start: Tick, end: Tick, roles: &Roles<'_>) -> Vec<Out> {
        self.at = (start, end);
        if self.dead {
            return Vec::new();
        }
        let values: Vec<Option<Value>> = self
            .roles
            .iter()
            .map(|role| roles.get(role).map(value))
            .collect();
        if let Err(e) = self.store.set_fuel(FUEL) {
            return self.trap(say(e));
        }
        match self
            .world
            .wtf_protocol_api()
            .call_feed(&mut self.store, start, end, &values)
        {
            Ok(out) => out.into_iter().filter_map(|o| self.out(o)).collect(),
            Err(e) => self.trap(say(e)),
        }
    }

    fn finish(&mut self) -> Vec<Out> {
        if self.dead {
            return Vec::new();
        }
        if let Err(e) = self.store.set_fuel(FUEL) {
            return self.trap(say(e));
        }
        match self.world.wtf_protocol_api().call_finish(&mut self.store) {
            Ok(out) => out.into_iter().filter_map(|o| self.out(o)).collect(),
            Err(e) => self.trap(say(e)),
        }
    }
}

fn value(v: &Val) -> Value {
    match v {
        Val::Bits(b) => Value::Bits(b.clone()),
        Val::Text(t) => Value::Text(t.clone()),
    }
}

fn val(v: Value) -> Val {
    match v {
        Value::Bits(b) => Val::Bits(b),
        Value::Text(t) => Val::Text(t),
    }
}

fn severity(s: WasmSeverity) -> Severity {
    match s {
        WasmSeverity::Error => Severity::Error,
        WasmSeverity::Warning => Severity::Warning,
    }
}

fn default(d: &DefaultValue) -> protocol::Default {
    match d {
        DefaultValue::Zero => protocol::Default::Zero,
        DefaultValue::Ones => protocol::Default::Ones,
        DefaultValue::Bytes(s) => protocol::Default::Bytes(protocol::intern(s)),
        DefaultValue::Same(s) => protocol::Default::Same(protocol::intern(s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use wtf_core::command;
    use wtf_core::stream::Stream;
    use wtf_core::workspace::Workspace;

    /// `wit/example`, built by `cargo component build --release --target wasm32-unknown-unknown`.
    fn probe() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/wasm/probe.wasm"
        ))
    }

    fn load() -> Arc<protocol::Loaded> {
        load_one(&probe()).unwrap()
    }

    /// `simple.vcd` (clk rising at 5, 15, ... 75 ns: seven cycles) with the probe loaded as `p`.
    fn ws() -> Workspace {
        let mut ws = Workspace::open(&PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/simple.vcd"
        )))
        .unwrap();
        ws.push_loaded(load());
        ws
    }

    fn stream<'a>(ws: &'a Workspace, name: &str) -> &'a Stream {
        ws.streams()
            .iter()
            .find(|s| s.name == name)
            .expect("a stream of that name")
    }

    fn text(s: &str) -> Option<Val> {
        Some(Val::Text(s.into()))
    }

    /// What the component says it is, before a trace: what `protocols probe` shows.
    #[test]
    fn the_card() {
        let loaded = load();
        assert_eq!(loaded.name, "probe");
        assert_eq!(
            loaded.def.about,
            "one beat a cycle, from a component: the example a wasm protocol is written from"
        );
        assert_eq!(loaded.def.example, "decode p probe clock=aclk");
        assert!(loaded.def.clocked);
        assert!(loaded.def.sides.is_empty());
        assert_eq!(loaded.def.params, ["spin", "hog"]);
        let role = &loaded.def.roles[0];
        assert_eq!((role.name, role.required), ("wide", false));
        assert_eq!(role.about, "a one-bit role: probe refuses a wider one");
    }

    /// A beat a cycle from `feed`, in the streams the card named.
    #[test]
    fn seven_beats() {
        let mut ws = ws();
        command::run(&mut ws, "decode p probe clock=top.clk").unwrap();
        let beat = stream(&ws, "p.beat");
        assert_eq!(beat.records.len(), 7);
        assert_eq!(
            beat.records[0].fields,
            vec![Some(Val::Bits("0".repeat(64)))]
        );
        assert_eq!(
            beat.records[1].fields,
            vec![Some(Val::Bits(format!("{:064b}", 1)))]
        );
        assert_eq!((beat.records[6].start, beat.records[6].end), (65, 75));
        assert_eq!(
            stream(&ws, "p.errors").records.len(),
            0,
            "a decode that fits its caps breaks no rule"
        );
    }

    /// `spin` outlasts the fuel one call may spend: one violation under `host` at that cycle, and the
    /// instance is dead, so the cycles after it make no beats.
    #[test]
    fn fuel_is_a_violation() {
        let mut ws = ws();
        command::run(&mut ws, "decode p probe clock=top.clk spin=10000000").unwrap();
        let errors = stream(&ws, "p.errors");
        assert_eq!(errors.records.len(), 1);
        assert_eq!((errors.records[0].start, errors.records[0].end), (5, 15));
        assert_eq!(
            errors.records[0].fields,
            vec![
                text("host"),
                text("wasm trap: all fuel consumed by WebAssembly"),
                text("error")
            ]
        );
        assert_eq!(stream(&ws, "p.beat").records.len(), 0);
    }

    /// `hog` holds 6 MB a cycle: the sixth passes the 32 MB an instance may hold.
    #[test]
    fn memory_is_a_violation() {
        let mut ws = ws();
        command::run(&mut ws, "decode p probe clock=top.clk hog=6").unwrap();
        let errors = stream(&ws, "p.errors");
        assert_eq!(errors.records.len(), 1);
        assert_eq!((errors.records[0].start, errors.records[0].end), (55, 65));
        assert_eq!(
            errors.records[0].fields,
            vec![
                text("host"),
                text("wasm trap: wasm `unreachable` instruction executed"),
                text("error")
            ]
        );
        assert_eq!(
            stream(&ws, "p.beat").records.len(),
            5,
            "the cycles before it made beats"
        );
    }

    /// `:e` makes the instance again, so a decode that trapped is usable after it.
    #[test]
    fn reload_remakes_the_instance() {
        let mut ws = ws();
        command::run(&mut ws, "decode p probe clock=top.clk spin=10000000").unwrap();
        ws.reload().unwrap();
        let errors = stream(&ws, "p.errors");
        assert_eq!(errors.records.len(), 1);
        assert_eq!(
            errors.records[0].fields[1],
            text("wasm trap: all fuel consumed by WebAssembly")
        );
    }

    /// `wit/axis` as a component decodes what the built-in `axis` does, violations and their
    /// severity, text fields and role defaults (`tkeep`, `tstrb` absent) included.
    #[test]
    fn axis_wasm_is_axis() {
        for trace in ["axis.vcd", "axis_packets.vcd"] {
            let mut ws = Workspace::open(&PathBuf::from(format!(
                "{}/tests/data/{trace}",
                env!("CARGO_MANIFEST_DIR")
            )))
            .unwrap();
            ws.push_loaded(
                load_one(&PathBuf::from(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/data/wasm/axis-wasm.wasm"
                )))
                .unwrap(),
            );
            let roles =
                "clock=clk tvalid=tvalid tready=tready tdata=tdata tlast=tlast aresetn=aresetn";
            command::run(&mut ws, &format!("decode r axis {roles}")).unwrap();
            command::run(&mut ws, &format!("decode w axis-wasm {roles}")).unwrap();
            let records = |prefix: &str| {
                let mut all: Vec<_> = ws
                    .streams()
                    .iter()
                    .filter_map(|s| Some((s.name.strip_prefix(prefix)?, s)))
                    .map(|(name, s)| {
                        let rs: Vec<_> = s
                            .records
                            .iter()
                            .map(|r| (r.start, r.end, r.fields.clone()))
                            .collect();
                        (name.to_string(), rs)
                    })
                    .collect();
                all.sort_by(|a, b| a.0.cmp(&b.0));
                all
            };
            let rust = records("r.");
            assert!(
                rust.iter().any(|(_, rs)| !rs.is_empty()),
                "{trace} decodes nothing"
            );
            assert_eq!(records("w."), rust, "{trace}");
        }
    }

    /// A path that holds no component, or nothing at all: the error says which file and why.
    #[test]
    fn a_file_that_is_not_a_component_says_so() {
        let Err(missing) = load_one(Path::new("/nonexistent/probe.wasm")) else {
            panic!("a missing file loaded")
        };
        assert_eq!(missing, "/nonexistent/probe.wasm: no such file");
        let path = std::env::temp_dir().join(format!("wtf-wasm-{}.wasm", std::process::id()));
        std::fs::write(&path, b"not a component").unwrap();
        let garbage = load_one(&path);
        std::fs::remove_file(&path).unwrap();
        let Err(garbage) = garbage else {
            panic!("a file that is no component loaded")
        };
        assert!(
            garbage.ends_with(": failed to parse WebAssembly module"),
            "{garbage}"
        );
    }

    /// `create` refuses a role it cannot read (`wide` is the card's optional one): the decode fails
    /// with the component's own words, not a trap.
    #[test]
    fn a_role_the_component_refuses_is_the_decode_error() {
        let mut ws = ws();
        let e =
            command::run(&mut ws, "decode p probe clock=top.clk wide=top.cpu.data").unwrap_err();
        assert_eq!(e, "p: probe reads one-bit roles; wide is 8 bits");
    }

    /// A session keeps a protocol from a file as its name and that file, and reading that back decodes
    /// again: `:protocol probe wasm=<file>`, `:decode p probe clock=top.clk spin=3`, `p.beat` (§11).
    #[test]
    fn session_wasm_entry() {
        let trace = std::path::Path::new("tests/data/simple.vcd");
        let file = "tests/data/wasm/probe.wasm";
        let mut ws = Workspace::open(trace).unwrap();
        ws.loader = Some(loader());
        wtf_core::command::run(&mut ws, &format!("protocol probe wasm={file}")).unwrap();
        wtf_core::command::run(&mut ws, "decode p probe clock=top.clk spin=3").unwrap();
        assert!(ws.streams().iter().any(|s| s.name == "p.beat"));
        let text = wtf_core::session::to_toml(&ws);
        assert!(
            text.contains(&format!(
                "[[protocol]]\nname = \"probe\"\nwasm = \"{file}\"\n"
            )),
            "{text}"
        );
        // reading it back: the binary had loaded the file before applying, as it does for the paths
        // the session kept from the last time (`:e` asks no file)
        let mut back = Workspace::open(trace).unwrap();
        back.push_loaded(load_one(std::path::Path::new(file)).unwrap());
        assert_eq!(wtf_core::session::apply(&mut back, &text), Ok(vec![]));
        assert!(back.streams().iter().any(|s| s.name == "p.beat"));
    }
}
