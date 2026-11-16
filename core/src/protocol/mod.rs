//! Protocols written in Rust (spec §16): the built-in ones, each a `Protocol` reading its roles a
//! cycle at a time (`axis`, `axilite`, `axi4`, `spi`, `uart`).

use crate::stream::{Field, Sampler};
use crate::time::Tick;
use crate::trace::Val;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub mod axi;
pub mod axi4;
pub mod axilite;
pub mod axis;
pub mod spi;
pub mod uart;

/// The roles an instance gives, with their signals' widths.
pub type Given<'a> = [(&'a str, Option<u32>)];

/// An instance's params, (name, value).
pub type Params = [(String, String)];

/// The protocols written in Rust.
pub const BUILTINS: &[&str] = &["axis", "axilite", "axi4", "spi", "uart"];

/// What a role's signal is when the instance doesn't give it.
#[derive(Clone, Copy, Debug)]
pub enum Default {
    /// All zeros.
    Zero,
    /// All ones.
    Ones,
    /// One bit per whole byte of another given role (AXI4-Stream `tkeep`, spec §3.1.2).
    Bytes(&'static str),
    /// The same value as another role.
    Same(&'static str),
}

impl Default {
    /// How `protocols <name>` says it.
    pub fn text(&self) -> String {
        match self {
            Default::Zero => "0".into(),
            Default::Ones => "1".into(),
            Default::Bytes(src) => format!("bytes of {src}"),
            Default::Same(src) => format!("same as {src}"),
        }
    }
}

/// How sure a protocol is that a violation is a bug: the spec forbids it, or only says not to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    /// The word the tool takes.
    pub fn text(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }

    /// The severity a record's `severity` field names; `None` for any other text.
    pub fn of(text: &str) -> Option<Severity> {
        match text {
            "error" => Some(Severity::Error),
            "warning" => Some(Severity::Warning),
            _ => None,
        }
    }
}

/// A rule a protocol checks: one violation record each time it holds.
#[derive(Clone, Copy)]
pub struct Rule {
    pub name: &'static str,
    pub severity: Severity,
    /// The clause of the protocol's spec it is from (`2.2.1`).
    pub spec: &'static str,
    pub about: &'static str,
}

/// A role of a `Protocol`, and what it is when the instance's signals don't give it.
pub struct RoleDef {
    pub name: &'static str,
    pub required: bool,
    pub default: Option<Default>,
    pub about: &'static str,
}

/// One record a protocol makes at a cycle, before the host turns it into a var.
pub enum Out {
    /// A record of one of `Desc`'s streams.
    Record {
        stream: &'static str,
        start: Tick,
        end: Tick,
        fields: Vec<Option<Val>>,
    },
    /// A record of the stream `violations` marks: which rule, and what it saw.
    Violation {
        rule: &'static str,
        start: Tick,
        end: Tick,
        detail: String,
        severity: Severity,
    },
}

/// A cycle's role values, and the roles they are.
pub struct Roles<'a> {
    values: &'a [Option<Val>],
    roles: &'a [RoleDef],
}

impl<'a> Roles<'a> {
    /// The role's value; None when the instance has no signal for it and no default.
    pub(crate) fn new(values: &'a [Option<Val>], roles: &'a [RoleDef]) -> Self {
        Roles { values, roles }
    }

    pub fn get(&self, role: &str) -> Option<&Val> {
        self.values[self.roles.iter().position(|r| r.name == role)?].as_ref()
    }

    /// The role's value as a flag: None for x or z, or no value.
    pub fn bit(&self, role: &str) -> Option<bool> {
        match self.get(role)? {
            Val::Bits(b) => bit_of(b, 0),
            Val::Text(_) => None,
        }
    }

    /// These fields' values, in their order.
    pub fn pick(&self, fields: &[Field]) -> Vec<Option<Val>> {
        fields.iter().map(|f| self.get(&f.name).cloned()).collect()
    }
}

/// Bit `k` of a bit string, from the lowest: None for x or z.
pub(crate) fn bit_of(b: &str, k: usize) -> Option<bool> {
    match b.len().checked_sub(k + 1).map(|i| b.as_bytes()[i]) {
        Some(b'1' | b'h') => Some(true),
        Some(b'0' | b'l') => Some(false),
        _ => None,
    }
}

/// One stream a `Protocol` makes.
pub struct StreamDesc {
    pub name: &'static str,
    pub fields: Vec<Field>,
    /// Violations: a record per rule that holds, its fields `rule`, `detail` and `severity`.
    pub violations: bool,
    /// One line each on what the stream is.
    pub about: Vec<String>,
    /// The instance's streams it is made from, by name (a frame's beats): those are intermediate,
    /// it a result (spec §16 "Results"). Empty for a stream made from the trace alone.
    pub from: &'static [&'static str],
}

/// What a protocol makes, as `protocols <name>` says it.
pub struct Desc {
    pub streams: Vec<StreamDesc>,
}

/// A protocol written in Rust: its name, what it needs, and the function that makes it.
#[derive(Clone, Copy)]
pub struct Def {
    pub about: &'static str,
    pub example: &'static str,
    pub clocked: bool,
    pub sides: &'static [&'static [&'static str]],
    pub roles: &'static [RoleDef],
    pub params: &'static [&'static str],
    pub make: Make,
}

/// A protocol's constructor: the roles its instance has signals for, its params, and the file's
/// femtoseconds per tick (a param in time needs it).
pub type Make = fn(&Given, &Params, Option<u128>) -> Result<Box<dyn Protocol>, String>;

/// A loaded protocol's constructor (spec §16): its own, made from the roles and params its
/// instance is given, as a built-in's `Make` is. Send + Sync: the registry is shared.
pub type Factory =
    Arc<dyn Fn(&Given, &Params, Option<u128>) -> Result<Box<dyn Protocol>, String> + Send + Sync>;

/// The `make` of a loaded protocol's card: `Any::make` asks the protocol's `Factory` instead.
pub const NO_MAKE: Make = |_, _, _| Err("a loaded protocol is made by its factory".into());

/// A protocol that reads its roles cycle by cycle, a record per cycle it decides on.
pub trait Protocol {
    /// The streams it makes, their fields, and what they are.
    fn desc(&self) -> &Desc;

    /// The records this cycle makes: the cycle runs `start` to `end`.
    fn feed(&mut self, start: Tick, end: Tick, roles: &Roles<'_>) -> Vec<Out>;

    /// The records left when the trace ends: a packet no `tlast` closed.
    fn finish(&mut self) -> Vec<Out>;
}

/// A built-in protocol by name.
pub fn get(name: &str) -> Option<Def> {
    match name {
        "axis" => Some(axis::DEF),
        "axilite" => Some(axilite::DEF),
        "axi4" => Some(axi4::DEF),
        "spi" => Some(spi::DEF),
        "uart" => Some(uart::DEF),
        _ => None,
    }
}

impl Def {
    /// One line on what it makes, and a `command` line using it, for `protocols`.
    pub fn about(&self) -> &'static str {
        self.about
    }

    pub fn example(&self) -> &'static str {
        self.example
    }

    /// Sampled at the instance's clock; false: by time, or at a clock among its roles.
    pub fn clocked(&self) -> bool {
        self.clocked
    }

    /// Sides of the required roles that may be left out whole, if another is given.
    pub fn sides(&self) -> &'static [&'static [&'static str]] {
        self.sides
    }

    /// Params besides `prefix`.
    pub fn params(&self) -> &'static [&'static str] {
        self.params
    }

    /// The roles, required then optional.
    pub fn roles(&self) -> (Vec<&'static str>, Vec<&'static str>) {
        let (required, optional): (Vec<_>, Vec<_>) = self.roles.iter().partition(|r| r.required);
        (
            required.into_iter().map(|r| r.name).collect(),
            optional.into_iter().map(|r| r.name).collect(),
        )
    }

    /// The roles' values when their signals aren't given, one line each: `tkeep bytes of tdata`.
    pub fn defaults(&self) -> Vec<String> {
        self.roles
            .iter()
            .filter_map(|r| r.default.map(|v| format!("{} {}", r.name, v.text())))
            .collect()
    }

    /// The required roles of an instance where `has` tells the roles there are: a side with none
    /// of its roles there is left out, unless every side is.
    pub fn needed(&self, has: impl FnMut(&str) -> bool) -> Vec<&'static str> {
        let mut has = has;
        let gone: Vec<&[&str]> = self
            .sides
            .iter()
            .copied()
            .filter(|s| !s.iter().any(|r| has(r)))
            .collect();
        let gone = if gone.len() == self.sides.len() {
            vec![]
        } else {
            gone
        };
        self.roles
            .iter()
            .filter(|r| r.required)
            .map(|r| r.name)
            .filter(|r| !gone.iter().any(|s| s.contains(r)))
            .collect()
    }

    /// Its streams, worked out with every role given.
    pub fn streams(&self) -> Vec<About<String>> {
        made_streams(self, self.make)
    }
}

/// A protocol loaded from a file (spec §16): its card, which `Sources::get` answers for, and its
/// own constructor. `def.make` is `NO_MAKE`.
pub struct Loaded {
    /// The name a `:decode` line takes.
    pub name: &'static str,
    pub def: Def,
    pub make: Factory,
}

/// Where a `[[protocol]] wasm=` entry gets its protocol (spec §16 "A protocol from a file"): the
/// binary has the host and the filesystem, core has neither, so it asks. The file's stem is the
/// protocol's name; the path is the one the session spells.
pub type Loader = Arc<dyn Fn(&str) -> Result<Arc<Loaded>, String> + Send + Sync>;

/// A protocol to make, and where it came from: a built-in, or one loaded from a file.
#[derive(Clone)]
pub enum Any {
    Builtin(Def),
    Loaded(Arc<Loaded>),
}

impl Any {
    /// Its card: what it needs, and what it makes.
    pub fn def(&self) -> &Def {
        match self {
            Any::Builtin(def) => def,
            Any::Loaded(l) => &l.def,
        }
    }

    /// One instance, from the roles the `:decode` line gives and its params.
    pub fn make(
        &self,
        given: &Given,
        params: &Params,
        fs_per_tick: Option<u128>,
    ) -> Result<Box<dyn Protocol>, String> {
        match self {
            Any::Builtin(def) => (def.make)(given, params, fs_per_tick),
            Any::Loaded(l) => (l.make)(given, params, fs_per_tick),
        }
    }

    /// Its streams, worked out with every role given.
    pub fn streams(&self) -> Vec<About<String>> {
        made_streams(self.def(), |given, params, fpt| {
            self.make(given, params, fpt)
        })
    }
}

/// A protocol's streams, worked out with every role given, by `make`: a `Def`'s own, or a loaded
/// protocol's factory.
fn made_streams(
    def: &Def,
    make: impl Fn(&Given, &Params, Option<u128>) -> Result<Box<dyn Protocol>, String>,
) -> Vec<About<String>> {
    let (required, optional) = def.roles();
    let given: Vec<(&str, Option<u32>)> = required
        .into_iter()
        .chain(optional)
        .map(|r| (r, Some(32)))
        .collect();
    // the example's params (`reset_active=high`), so one that needs a param can be made
    let params: Vec<(String, String)> = def
        .example()
        .split_whitespace()
        .filter_map(|w| w.split_once('='))
        .filter(|(k, _)| def.params().contains(k))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    match make(&given, &params, Some(1)) {
        Ok(p) => p
            .desc()
            .streams
            .iter()
            .map(|s| {
                (
                    s.name.to_string(),
                    s.fields.iter().map(|f| f.name.clone()).collect(),
                    s.violations.then(String::new),
                    s.about.clone(),
                )
            })
            .collect(),
        Err(e) => vec![(
            "?".into(),
            Vec::new(),
            None,
            vec![format!("cannot be made with every role given: {e}")],
        )],
    }
}

/// Where a decode's protocol may come from (spec §16): a built-in, one loaded from a file, or the
/// session's `[[protocol]]`, which is sampled rather than made.
#[derive(Clone, Copy, Default)]
pub struct Sources<'a> {
    pub session: &'a [Sampler],
    pub loaded: &'a [Arc<Loaded>],
}

impl<'a> Sources<'a> {
    /// A protocol to make, by name: a built-in first, so a loaded file cannot take its name, then
    /// one loaded from a file. The session's are sampled (`decode_user`), never made here.
    pub fn get(&self, name: &str) -> Option<Any> {
        get(name).map(Any::Builtin).or_else(|| {
            self.loaded
                .iter()
                .find(|l| l.name == name)
                .map(|l| Any::Loaded(l.clone()))
        })
    }

    /// Every protocol a decode can take: the built-ins, the loaded, the session's. `frame`, `pair`
    /// and `check` work on streams and rules, not signals, and are names of their own.
    pub fn names(&self) -> Vec<&'a str> {
        BUILTINS
            .iter()
            .copied()
            .chain(self.loaded.iter().map(|l| l.name))
            .chain(self.session.iter().map(|p| p.name.as_str()))
            .collect()
    }
}

/// A card string from a file, kept as long as the process runs: the same `&'static str` for the
/// same text, so loading a protocol again copies no text (`Def` holds `&'static str`).
pub fn intern(s: &str) -> &'static str {
    static TABLE: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut table = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let table = table.get_or_insert_with(HashSet::new);
    match table.get(s) {
        Some(&t) => t,
        None => {
            let t: &'static str = Box::leak(s.to_string().into_boxed_str());
            table.insert(t);
            t
        }
    }
}

/// Names `[[decode]]` takes that aren't user protocols: the ones above, and the generic protocols
/// put on streams (`Workspace::add_decode`).
pub fn reserved(name: &str) -> bool {
    get(name).is_some() || matches!(name, "frame" | "pair" | "check")
}

/// The generic protocols on streams: name, one line, and the rest of `protocols <name>`.
pub const ON_STREAMS: &[(&str, &str, &str)] = &[
    (
        "frame",
        "records of a stream grouped into frames, by a last flag, a gap or a select",
        "roles: input (a stream)
params: last=<field> (a frame ends at a record where it is 1), gap=<time> (before a longer gap),
  select=<field> (when it changes), key=<field> (a frame per value of it, side by side),
  bytes=<field|n> (len: a field's 1 bits, or n per record), data=<field> (data: the bytes as hex text)
fields: beats, the select field, the key field, len (with bytes), data (with data), complete (0: the input ended first)
example: decode f frame input=rx.beat last=tlast bytes=tkeep data=tdata",
    ),
    (
        "pair",
        "each response with its request, by order or by an id field, with the delay",
        "roles: request, response (streams)
params: id=<field> (the same in both), or req_id=<field> resp_id=<field>; none: by order
fields: delay (from the request to its response), req_<each request field>, resp_<each response field>
also <name>.unmatched (violations): kind = no request | no response
example: decode p pair request=cmd response=rsp id=tag",
    ),
    (
        "check",
        "a violation record per clock cycle where a rule holds",
        "roles: none; needs clock=
params: <rule>=<expression>, any number; with rose() fell() stable() past()
fields: rule (the name of the rule that held); empty: no violations
example: decode stall check clock=clk waiting=valid && !ready dropped=fell(valid) && !past(ready)",
    ),
];

/// `protocols`: what can be decoded, one line each, with the protocols loaded from files and the
/// session's own.
pub fn list(sources: Sources<'_>) -> String {
    let mut out = vec!["built in, on signals: decode <name> <protocol> clock=<clk> prefix=<prefix> (or <role>=<signal> each)".to_string()];
    out.extend(
        BUILTINS
            .iter()
            .filter_map(|n| get(n).map(|p| format!("  {n:<8} {}", p.about()))),
    );
    out.push("on streams: decode <name> <protocol> ...".into());
    out.extend(
        ON_STREAMS
            .iter()
            .map(|(n, about, _)| format!("  {n:<8} {about}")),
    );
    if !sources.loaded.is_empty() {
        out.push("loaded from files: decode <name> <protocol> ...".into());
        out.extend(
            sources
                .loaded
                .iter()
                .map(|l| format!("  {:<8} {}", l.name, l.def.about())),
        );
    }
    // a `wasm=` entry is its file's, listed under `loaded from files:` once the file is
    let session: Vec<&Sampler> = sources
        .session
        .iter()
        .filter(|p| p.wasm.is_none())
        .collect();
    if !session.is_empty() {
        out.push("in this session (protocol <name> ...):".into());
        out.extend(
            session
                .iter()
                .map(|p| format!("  {:<8} {}", p.name, sampler_line(p))),
        );
    }
    out.push("protocols <name> describes one: its roles, params, streams and fields.".into());
    out.join("\n")
}

fn sampler_line(p: &Sampler) -> String {
    format!(
        "roles {}: a record where {} at the instance's clock",
        p.roles.join(", "),
        p.when
    )
}

/// `protocols <name>`: its roles, params, and the streams it makes with their fields.
pub fn describe(name: &str, sources: Sources<'_>) -> Result<String, String> {
    if let Some(p) = sources.get(name) {
        return Ok(describe_any(name, &p));
    }
    if let Some((_, about, rest)) = ON_STREAMS.iter().find(|g| g.0 == name) {
        return Ok(format!("{name}: {about}\n{rest}"));
    }
    if let Some(p) = sources.session.iter().find(|p| p.name == name) {
        let fields: Vec<String> = p
            .fields
            .iter()
            .map(|(f, sig)| format!("{f}={sig}"))
            .collect();
        let mut out = vec![
            format!("{name}: in this session, {}", sampler_line(p)),
            format!("fields: {}", fields.join(" ")),
        ];
        out.push(format!(
            "example: decode x {name} clock=<clk> {}",
            p.roles
                .iter()
                .map(|r| format!("{r}=<signal>"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        return Ok(out.join("\n"));
    }
    Err(unknown(name, sources))
}

/// The card of a protocol to make: a built-in's and a loaded file's alike.
fn describe_any(name: &str, p: &Any) -> String {
    let def = p.def();
    let (required, optional) = def.roles();
    let mut out = vec![
        format!("{name}: {}", def.about()),
        format!("roles: {} (required)", required.join(" ")),
    ];
    if !optional.is_empty() {
        out.push(format!("  {} (optional)", optional.join(" ")));
    }
    let defaults = def.defaults();
    if !defaults.is_empty() {
        out.push(format!("  when not given: {}", defaults.join(", ")));
    }
    if !def.sides().is_empty() {
        out.push("  a side may be left out whole: a write-only or read-only port".into());
    }
    out.push(
        "params: prefix=<prefix> plays each role not given by <prefix><role> (prefix=s_axi_)"
            .into(),
    );
    if !def.params().is_empty() {
        out.push(format!("  {}", def.params().join(" ")));
    }
    out.push(
        "streams, <name> being the instance's (those of optional roles only when they are given):"
            .into(),
    );
    for (stream, fields, violations, about) in p.streams() {
        let names: Vec<&str> = fields.iter().map(|f| f.as_str()).collect();
        let severity = violations.is_some() && fields.iter().any(|f| f == "severity");
        out.push(format!(
            "  <name>.{stream}: {}{}",
            names.join(" "),
            violations.map_or(String::new(), |v| format!(" (violations{v})"))
        ));
        if severity {
            out.push(format!("      severity: error or warning; records <name>.{stream} where severity == warning keeps the warnings"));
        }
        out.extend(about.into_iter().map(|l| format!("      {l}")));
    }
    out.push(format!("example: {}", def.example()));
    out.join("\n")
}

/// The error for a protocol that isn't one, naming those there are.
pub fn unknown(name: &str, sources: Sources<'_>) -> String {
    let names: Vec<&str> = BUILTINS
        .iter()
        .copied()
        .chain(ON_STREAMS.iter().map(|g| g.0))
        .chain(sources.loaded.iter().map(|l| l.name))
        .chain(sources.session.iter().map(|p| p.name.as_str()))
        .collect();
    format!(
        "unknown protocol: {name} (protocols: {}; `protocols <name>` describes one)",
        names.join(", ")
    )
}

/// A short card on a protocol, for the command line's docs (spec §4): its line, roles and
/// streams, a line each.
pub fn brief(name: &str, sources: Sources<'_>) -> Option<String> {
    if let Some(p) = sources.get(name) {
        let def = p.def();
        let clock = if def.clocked() { " clock=<clk>" } else { "" };
        let (required, optional) = def.roles();
        let mut roles = required.join(" ");
        if !optional.is_empty() {
            roles += &format!(" ({} optional)", optional.join(" "));
        }
        let streams: Vec<String> = p.streams().into_iter().map(|s| s.0).collect();
        let params = if def.params().is_empty() {
            String::new()
        } else {
            format!("\nParams: {}", def.params().join(" "))
        };
        return Some(format!(
            "decode <name> {name}{clock} prefix=<prefix>\n\n{}\nRoles: {roles}{params}\nStreams: {}",
            def.about(),
            streams.join(" ")
        ));
    }
    if let Some((_, about, _)) = ON_STREAMS.iter().find(|g| g.0 == name) {
        return Some(format!("decode <name> {name} ...\n\n{about}"));
    }
    let p = sources.session.iter().find(|p| p.name == name)?;
    let roles: Vec<String> = p.roles.iter().map(|r| format!("{r}=<signal>")).collect();
    let fields: Vec<&str> = p.fields.iter().map(|f| f.0.as_str()).collect();
    Some(format!(
        "decode <name> {name} clock=<clk> {}\n\nA record where {}\nFields: {}",
        roles.join(" "),
        p.when,
        fields.join(" ")
    ))
}

/// A stream of a built-in, for `protocols <name>`: its name, fields, for violations what they
/// are, and lines on what it is.
type About<F> = (String, Vec<F>, Option<String>, Vec<String>);

/// An interface in a trace that a built-in protocol decodes: its signals' common prefix
/// (`top.s_axi_`, playing each role as `<prefix><role>`), the protocol, and its clock if one
/// stands out.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub prefix: String,
    pub protocol: &'static str,
    pub clock: Option<String>,
}

impl Found {
    /// The name `decode_line` gives its streams: the prefix's last part (`s_axi` for `top.s_axi_`).
    pub fn name(&self) -> &str {
        let leaf = self
            .prefix
            .rsplit('.')
            .next()
            .unwrap_or("")
            .trim_matches('_');
        if leaf.is_empty() { self.protocol } else { leaf }
    }

    /// The `decode` line that puts the protocol on it, named after the prefix.
    pub fn decode_line(&self) -> String {
        let name = self.name();
        let clock = match (&self.clock, get(self.protocol).is_some_and(|p| p.clocked())) {
            (_, false) => String::new(),
            (Some(c), true) => format!(" clock={c}"),
            (None, true) => " clock=<clk>".into(),
        };
        format!(
            "decode {name} {}{clock} prefix={}",
            self.protocol, self.prefix
        )
    }
}

/// The interfaces of the file's signals that built-in protocols decode: wherever every role the
/// instance needs is a signal `<prefix><role>`, and at least two of the protocol's roles are
/// there. A prefix goes to the protocol needing most roles (AXI4's wlast / rlast over AXI-Lite).
/// A protocol with one role is never found (UART's `line`). Worked out once per trace: the file's
/// signals don't change.
pub fn detect(trace: &crate::trace::Trace) -> Vec<Found> {
    trace.found.get_or_init(|| find(trace)).clone()
}

/// A prefix is found when every role the instance needs is there under it and at least two of the
/// protocol's roles are: one role alone is a stray signal, not an interface.
fn find(trace: &crate::trace::Trace) -> Vec<Found> {
    let mut protocols: Vec<(&'static str, Def)> = BUILTINS
        .iter()
        .filter_map(|&n| Some((n, get(n)?)))
        .collect();
    protocols.sort_by_key(|(_, p)| std::cmp::Reverse(p.roles().0.len()));
    let file = |v: &usize| !trace.is_computed(*v);
    let mut found: Vec<Found> = Vec::new();
    for (name, p) in protocols {
        // a port is found by its first role, or a side's (a read-only one by arvalid)
        let (required, optional) = p.roles();
        let firsts: Vec<&str> = std::iter::once(required[0])
            .chain(p.sides().iter().map(|s| s[0]))
            .collect();
        let mut prefixes: Vec<&str> = trace
            .vars
            .iter()
            .enumerate()
            .filter(|(i, _)| file(i))
            .flat_map(|(_, v)| firsts.iter().filter_map(|f| v.path.strip_suffix(f)))
            .collect();
        prefixes.sort();
        prefixes.dedup();
        for prefix in prefixes {
            let there = |r: &str| trace.by_path.get(&format!("{prefix}{r}")).is_some_and(file);
            let present = required
                .iter()
                .copied()
                .chain(optional.iter().copied())
                .filter(|&r| there(r))
                .count();
            let all = p.needed(there).iter().all(|r| there(r));
            if present >= 2 && all && !found.iter().any(|f| f.prefix == prefix) {
                found.push(Found {
                    prefix: prefix.into(),
                    protocol: name,
                    clock: clock_of(trace, prefix),
                });
            }
        }
    }
    found.sort_by(|a, b| a.prefix.cmp(&b.prefix));
    found
}

/// The clock an interface likely runs on: `<prefix>aclk` or `<prefix>clk`, else the one 1-bit
/// signal in its scope named like a clock.
fn clock_of(trace: &crate::trace::Trace, prefix: &str) -> Option<String> {
    for own in ["aclk", "clk"] {
        let p = format!("{prefix}{own}");
        if trace.by_path.contains_key(&p) {
            return Some(p);
        }
    }
    let scope = prefix.rsplit_once('.').map_or("", |s| s.0);
    let mut clocks = trace
        .vars
        .iter()
        .filter(|v| v.width == Some(1))
        .filter(|v| {
            v.path.rsplit_once('.').is_some_and(|(s, leaf)| {
                s == scope && (leaf.contains("clk") || leaf.contains("clock"))
            })
        });
    match (clocks.next(), clocks.next()) {
        (Some(c), None) => Some(c.path.clone()),
        _ => None,
    }
}
