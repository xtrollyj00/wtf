//! Protocols written in Rust (spec §16): steps on an instance's signals, each making a
//! stream `<instance>.<step>`.

use crate::stream::Sampler;

/// One stream of a protocol, from its roles (names standing for the instance's signals).
pub enum Step {
    /// A record per clock cycle where `when` holds, with fields (field, signal).
    Sample { name: &'static str, when: String, fields: Vec<(String, String)> },
    /// Violations: a record per clock cycle where a rule (name, expression) holds, field `rule`.
    Check { name: &'static str, rules: Vec<(String, String)> },
    /// A decoder (`decode::make`) on an earlier step's stream.
    Decode { name: &'static str, input: &'static str, decoder: &'static str, params: Vec<(String, String)> },
    /// A record per change of `signal` (an expression of the roles, e.g. `{cs, sclk, mosi}`), with
    /// its value as field `value`, from it to the next change: the input of a decoder that samples by
    /// time (SPI, UART). Not a stream of the instance's own.
    Changes { name: &'static str, signal: String },
    /// Transactions (`decode::join`) of earlier steps' streams: requests answered by a response,
    /// in order, or with `id` (request step, its field, the response's field) by id.
    Join { name: &'static str, requests: Vec<&'static str>, response: &'static str, id: Option<(&'static str, &'static str, &'static str)> },
}

/// The roles an instance gives, with their signals' widths.
pub type Given<'a> = [(&'a str, Option<u32>)];

/// An instance's params, (name, value).
pub type Params = [(String, String)];

pub struct Builtin {
    /// One line on what it makes, and a `command` line using it, for `protocols`.
    pub about: &'static str,
    pub example: &'static str,
    pub required: &'static [&'static str],
    pub optional: &'static [&'static str],
    /// Params besides `prefix`, given to the steps (`mode`, `baud`).
    pub params: &'static [&'static str],
    /// Sampled at the instance's clock; false: by time, or at a clock among its roles.
    pub clocked: bool,
    /// The steps, from the roles given with their widths, and the params.
    pub steps: fn(&Given, &Params) -> Vec<Step>,
    /// What a stream is, for `protocols <name>`, where its steps don't say it: (stream, line).
    /// Sampled streams and rules describe themselves.
    pub notes: &'static [(&'static str, &'static str)],
}

const AXIS: &[(&str, &str)] = &[
    ("frame", "beats up to tlast; len in bytes (tkeep's 1 bits, or all of tdata per beat), data those bytes, complete 0 if the trace ended first"),
];
const AXILITE: &[(&str, &str)] = &[
    ("wr", "writes: AW and W in either order, then B; delay from the later of AW and W to B"),
    ("wr.unmatched", "a B without a write (no request), a write never answered (no response), a W without an AW (no aw) or the reverse (no w)"),
    ("rd", "reads: AR, then R; delay from AR to R"),
    ("rd.unmatched", "an R without an AR (no request), an AR never answered (no response)"),
];
const AXI4: &[(&str, &str)] = &[
    ("wburst", "the W beats of a burst up to wlast; data the bytes wstrb marks"),
    ("rburst", "the R beats of a burst up to rlast, each ID's apart (bursts of different IDs interleave)"),
    ("wr", "writes: AW and its W burst in order, then the B with its ID; delay from the later of AW and the burst's first beat to B"),
    ("wr.unmatched", "a B without a write (no request), a write never answered (no response), a W burst without an AW (no aw) or the reverse (no wburst)"),
    ("rd", "reads: AR, then the R burst with its ID; delay (latency) from AR to the first R beat"),
    ("rd.unmatched", "an R burst without an AR (no request), an AR never answered (no response)"),
];
const SPI: &[(&str, &str)] = &[
    ("bit", "a bit of mosi (and miso) at each sampling edge of sclk while cs is low (mode 0 and 3: rising, 1 and 2: falling)"),
    ("word", "`bits` bits (8), msb or lsb first by `order`"),
    ("transfer", "cs low to high, its words as hex text"),
];
const UART: &[(&str, &str)] = &[
    ("byte", "each frame's data bits, sampled in the middle of each bit at `baud`"),
    ("errors", "a stop bit that isn't 1 (framing), a wrong parity bit (parity)"),
];

/// The protocols written in Rust.
pub const BUILTINS: &[&str] = &["axis", "axilite", "axi4", "spi", "uart"];

/// The protocols written in Rust, by name.
pub fn builtin(name: &str) -> Option<Builtin> {
    match name {
        "axis" => Some(Builtin { about: "AXI4-Stream: beats, frames by tlast with their bytes, handshake errors", example: "decode rx axis clock=aclk prefix=s_axis_", required: &["tvalid", "tready", "tdata"], optional: &["tlast", "tkeep"], params: &[], clocked: true, steps: axis, notes: AXIS }),
        "axilite" => Some(Builtin {
            about: "AXI4-Lite: write and read transactions with their delay, handshake errors, error responses",
            example: "decode bus axilite clock=aclk prefix=s_axi_",
            required: &[
                "awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "bvalid", "bready", "arvalid", "arready", "araddr", "rvalid", "rready", "rdata",
            ],
            optional: &["awprot", "wstrb", "bresp", "arprot", "rresp"],
            params: &[],
            clocked: true,
            steps: axilite,
            notes: AXILITE,
        }),
        "axi4" => Some(Builtin {
            about: "AXI4: write and read bursts as transactions, paired by ID, with their latency; handshake errors, error responses",
            example: "decode mem axi4 clock=aclk prefix=m_axi_",
            required: &[
                "awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "wlast", "bvalid", "bready", "arvalid", "arready", "araddr", "rvalid", "rready", "rdata", "rlast",
            ],
            optional: &["awid", "awlen", "awsize", "awburst", "awprot", "wstrb", "bid", "bresp", "arid", "arlen", "arsize", "arburst", "arprot", "rid", "rresp"],
            params: &[],
            clocked: true,
            steps: axi4,
            notes: AXI4,
        }),
        "spi" => Some(Builtin {
            about: "SPI: bits at sclk's sampling edge, words, transfers framed by cs (active low)",
            example: "decode flash spi sclk=spi_clk cs=spi_cs_n mosi=spi_mosi miso=spi_miso mode=0",
            required: &["sclk", "cs", "mosi"],
            optional: &["miso"],
            params: &["mode", "bits", "order"],
            clocked: false,
            steps: spi,
            notes: SPI,
        }),
        "uart" => Some(Builtin {
            about: "UART: bytes from an idle-high line at a baud rate, framing and parity errors",
            example: "decode console uart line=uart_tx baud=115200",
            required: &["line"],
            optional: &[],
            params: &["baud", "bits", "parity", "stop"],
            clocked: false,
            steps: uart,
            notes: UART,
        }),
        _ => None,
    }
}

/// Names `[[decode]]` takes that aren't user protocols: the ones above, and the generic protocols
/// put on streams (`Workspace::add_decode`).
pub fn reserved(name: &str) -> bool {
    builtin(name).is_some() || matches!(name, "frame" | "pair" | "check")
}

/// AXI4-Stream: `beat`, a transfer (tvalid and tready); `frame`, beats up to tlast, with `len` in
/// bytes (tkeep's 1 bits, or the whole of tdata per beat) and `data`, those bytes; `errors`, a valid dropped before its
/// ready and the payload changed while stalled.
fn axis(given: &Given, _: &Params) -> Vec<Step> {
    let has = |r: &str| given.iter().find(|g| g.0 == r);
    let payload: Vec<&str> = ["tdata", "tlast", "tkeep"].into_iter().filter(|r| has(r).is_some()).collect();
    let fields = payload.iter().map(|r| (r.to_string(), r.to_string())).collect();
    let stable = payload.iter().map(|r| format!("stable({r})")).collect::<Vec<_>>().join(" && ");
    let rules = vec![
        ("dropped".into(), "fell(tvalid) && !past(tready)".into()),
        ("changed".into(), format!("tvalid && past(tvalid) && !past(tready) && !({stable})")),
    ];
    let mut steps = vec![Step::Sample { name: "beat", when: "tvalid && tready".into(), fields }, Step::Check { name: "errors", rules }];
    if has("tlast").is_some() {
        let bytes = match has("tkeep") {
            Some(_) => "tkeep".to_string(),
            None => has("tdata").and_then(|d| d.1).map_or(0, |w| w.div_ceil(8)).to_string(),
        };
        steps.push(Step::Decode { name: "frame", input: "beat", decoder: "frame", params: vec![("last".into(), "tlast".into()), ("bytes".into(), bytes), ("data".into(), "tdata".into())] });
    }
    steps
}

/// AXI4-Lite: a stream per channel (`aw`, `w`, `b`, `ar`, `r`: each handshake with its payload);
/// `wr`, writes (AW and W in either order, then B) with `delay` from the later of AW and W to B;
/// `rd`, reads (AR, then R) with `delay`; `wr.unmatched` / `rd.unmatched`, a response without a
/// request or the reverse; `errors`, a valid dropped before its ready, a payload changed while
/// stalled, and a non-OKAY response (`b_resp`, `r_resp`).
fn axilite(given: &Given, _: &Params) -> Vec<Step> {
    let (mut steps, rules) = channels(
        given,
        [
            ("aw", &[("addr", "awaddr"), ("prot", "awprot")]),
            ("w", &[("data", "wdata"), ("strb", "wstrb")]),
            ("b", &[("resp", "bresp")]),
            ("ar", &[("addr", "araddr"), ("prot", "arprot")]),
            ("r", &[("data", "rdata"), ("resp", "rresp")]),
        ],
    );
    steps.push(Step::Check { name: "errors", rules });
    steps.push(Step::Join { name: "wr", requests: vec!["aw", "w"], response: "b", id: None });
    steps.push(Step::Join { name: "rd", requests: vec!["ar"], response: "r", id: None });
    steps
}

/// AXI4: a stream per channel, as AXI-Lite's with the burst fields; `wburst`, the W beats of a
/// burst up to wlast (`data`, the bytes wstrb marks); `rburst`, the R beats of a burst up to rlast,
/// each ID's apart (bursts of different IDs interleave); `wr`, writes (AW and its W burst in order,
/// then the B with its ID) with `delay` from the later of AW and the burst's first beat to B; `rd`,
/// reads (AR, then the R burst with its ID) with `delay` (latency) to the first R beat; the
/// unmatched ones, and `errors` as AXI-Lite's.
fn axi4(given: &Given, _: &Params) -> Vec<Step> {
    let has = |r: &str| given.iter().any(|g| g.0 == r);
    let (mut steps, rules) = channels(
        given,
        [
            ("aw", &[("addr", "awaddr"), ("id", "awid"), ("len", "awlen"), ("size", "awsize"), ("burst", "awburst"), ("prot", "awprot")]),
            ("w", &[("data", "wdata"), ("strb", "wstrb"), ("last", "wlast")]),
            ("b", &[("id", "bid"), ("resp", "bresp")]),
            ("ar", &[("addr", "araddr"), ("id", "arid"), ("len", "arlen"), ("size", "arsize"), ("burst", "arburst"), ("prot", "arprot")]),
            ("r", &[("data", "rdata"), ("id", "rid"), ("resp", "rresp"), ("last", "rlast")]),
        ],
    );
    steps.push(Step::Check { name: "errors", rules });
    let mut w = vec![("last".to_string(), "last".to_string()), ("data".into(), "data".into())];
    if has("wstrb") {
        w.push(("bytes".into(), "strb".into()));
    }
    steps.push(Step::Decode { name: "wburst", input: "w", decoder: "frame", params: w });
    let mut r = vec![("last".to_string(), "last".to_string()), ("data".into(), "data".into())];
    if has("rid") {
        r.push(("key".into(), "id".into()));
    }
    steps.push(Step::Decode { name: "rburst", input: "r", decoder: "frame", params: r });
    let by_id = |a: &str, b: &str, req: &'static str| (has(a) && has(b)).then_some((req, "id", "id"));
    steps.push(Step::Join { name: "wr", requests: vec!["aw", "wburst"], response: "b", id: by_id("awid", "bid", "aw") });
    steps.push(Step::Join { name: "rd", requests: vec!["ar"], response: "rburst", id: by_id("arid", "rid", "ar") });
    steps
}

/// SPI: `bit`, a bit of mosi (and miso) at each sampling edge of sclk while cs is low (`mode` 0 - 3:
/// rising for 0 and 3, falling for 1 and 2); `word`, `bits` of them (8; `order` msb or lsb first);
/// `transfer`, cs low to high, its words as hex text. Sampled by the pins' own changes, so no clock.
fn spi(given: &Given, params: &Params) -> Vec<Step> {
    let miso = if given.iter().any(|g| g.0 == "miso") { ", miso" } else { "" };
    let mut steps = vec![Step::Changes { name: "pins", signal: format!("{{cs, sclk, mosi{miso}}}") }];
    for out in ["bit", "word", "transfer"] {
        let mut p = params.to_vec();
        p.push(("out".into(), out.into()));
        steps.push(Step::Decode { name: out, input: "pins", decoder: "spi", params: p });
    }
    steps
}

/// UART: `byte`, each frame's data, sampled in the middle of its bits at `baud`; `errors`, a stop
/// bit that isn't 1 (`framing`) or a wrong `parity` bit.
fn uart(_: &Given, params: &Params) -> Vec<Step> {
    let mut steps = vec![Step::Changes { name: "levels", signal: "line".into() }];
    for out in ["byte", "errors"] {
        let mut p = params.to_vec();
        p.push(("out".into(), out.into()));
        steps.push(Step::Decode { name: out, input: "levels", decoder: "uart", params: p });
    }
    steps
}

/// A stream per AXI channel (`aw` ...: each handshake with the payload given, as (field, role)),
/// and the rules of its handshake errors: a valid dropped before its ready, the payload changed
/// while stalled, a response not OKAY.
fn channels<const N: usize>(given: &Given, channels: [(&'static str, &[(&str, &str)]); N]) -> (Vec<Step>, Vec<(String, String)>) {
    let has = |r: &str| given.iter().any(|g| g.0 == r);
    let (mut steps, mut rules) = (Vec::new(), Vec::new());
    for (c, payload) in channels {
        let fields: Vec<(String, String)> = payload.iter().filter(|p| has(p.1)).map(|&(f, sig)| (f.into(), sig.into())).collect();
        steps.push(Step::Sample { name: c, when: format!("{c}valid && {c}ready"), fields: fields.clone() });
        rules.push((format!("{c}_dropped"), format!("fell({c}valid) && !past({c}ready)")));
        if !fields.is_empty() {
            let stable = fields.iter().map(|f| format!("stable({})", f.1)).collect::<Vec<_>>().join(" && ");
            rules.push((format!("{c}_changed"), format!("{c}valid && past({c}valid) && !past({c}ready) && !({stable})")));
        }
        if has(&format!("{c}resp")) {
            rules.push((format!("{c}_resp"), format!("{c}valid && {c}ready && {c}resp != 0")));
        }
    }
    (steps, rules)
}

/// The generic protocols on streams: name, one line, and the rest of `protocols <name>`.
const ON_STREAMS: &[(&str, &str, &str)] = &[
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

/// `protocols`: what can be decoded, one line each, with the session's own protocols.
pub fn list(session: &[Sampler]) -> String {
    let mut out = vec!["built in, on signals: decode <name> <protocol> clock=<clk> prefix=<prefix> (or <role>=<signal> each)".to_string()];
    out.extend(BUILTINS.iter().filter_map(|n| builtin(n).map(|b| format!("  {n:<8} {}", b.about))));
    out.push("on streams: decode <name> <protocol> ...".into());
    out.extend(ON_STREAMS.iter().map(|(n, about, _)| format!("  {n:<8} {about}")));
    if !session.is_empty() {
        out.push("in this session (protocol <name> ...):".into());
        out.extend(session.iter().map(|p| format!("  {:<8} {}", p.name, sampler_line(p))));
    }
    out.push("protocols <name> describes one: its roles, params, streams and fields.".into());
    out.join("\n")
}

fn sampler_line(p: &Sampler) -> String {
    let roles = if p.roles.is_empty() { String::new() } else { format!("roles {}: ", p.roles.join(", ")) };
    format!("{roles}a record where {} at {}", p.when, if p.clock.is_empty() { "the instance's clock" } else { &p.clock })
}

/// `protocols <name>`: its roles, params, and the streams it makes with their fields.
pub fn describe(name: &str, session: &[Sampler]) -> Result<String, String> {
    if let Some(b) = builtin(name) {
        let mut out = vec![format!("{name}: {}", b.about), format!("roles: {} (required)", b.required.join(" "))];
        if !b.optional.is_empty() {
            out.push(format!("  {} (optional)", b.optional.join(" ")));
        }
        out.push("params: prefix=<prefix> plays each role not given by <prefix><role> (prefix=s_axi_)".into());
        if !b.params.is_empty() {
            out.push(format!("  {}", b.params.join(" ")));
        }
        out.push("streams, <name> being the instance's (those of optional roles only when they are given):".into());
        for (stream, fields, violations, about) in streams_of(&b) {
            let names: Vec<&str> = fields.iter().map(|f| f.as_str()).collect();
            out.push(format!("  <name>.{stream}: {}{}", names.join(" "), violations.map_or(String::new(), |v| format!(" (violations{v})"))));
            out.extend(about.into_iter().map(|l| format!("      {l}")));
        }
        out.push(format!("example: {}", b.example));
        return Ok(out.join("\n"));
    }
    if let Some((_, about, rest)) = ON_STREAMS.iter().find(|g| g.0 == name) {
        return Ok(format!("{name}: {about}\n{rest}"));
    }
    if let Some(p) = session.iter().find(|p| p.name == name) {
        let fields: Vec<String> = p.fields.iter().map(|(f, sig)| format!("{f}={sig}")).collect();
        let mut out = vec![format!("{name}: in this session, {}", sampler_line(p)), format!("fields: {}", fields.join(" "))];
        out.push(match p.roles.is_empty() {
            true => format!("the stream {name} (a protocol without roles samples its own signals)"),
            false => format!("example: decode x {name} clock=<clk> {}", p.roles.iter().map(|r| format!("{r}=<signal>")).collect::<Vec<_>>().join(" ")),
        });
        return Ok(out.join("\n"));
    }
    Err(unknown(name, session))
}

/// The error for a protocol that isn't one, naming those there are.
pub fn unknown(name: &str, session: &[Sampler]) -> String {
    let names: Vec<&str> = BUILTINS.iter().copied().chain(ON_STREAMS.iter().map(|g| g.0)).chain(session.iter().map(|p| p.name.as_str())).collect();
    format!("unknown protocol: {name} (protocols: {}; `protocols <name>` describes one)", names.join(", "))
}

/// A stream of a built-in, for `protocols <name>`: its name, fields, for violations what they
/// are, and lines on what it is.
type About<F> = (String, Vec<F>, Option<String>, Vec<String>);

/// A built-in's streams, worked out from its steps with every role given: (stream, fields, for
/// violations what they are, what it is). A note says what a stream is; otherwise its step does:
/// what a sampled stream samples, a check's rules with their expressions.
fn streams_of(b: &Builtin) -> Vec<About<String>> {
    use crate::stream::Field;
    let given: Vec<(&str, Option<u32>)> = b.required.iter().chain(b.optional).map(|r| (*r, Some(32))).collect();
    let mut schemas: Vec<About<Field>> = Vec::new();
    let find = |schemas: &[About<Field>], n: &str| schemas.iter().find(|s| s.0 == n).map(|s| s.1.clone()).unwrap_or_default();
    // the example's params (`baud=115200`), so a decoder that needs one can tell its fields
    let params: Vec<(String, String)> = b
        .example
        .split_whitespace()
        .filter_map(|w| w.split_once('='))
        .filter(|(k, _)| b.params.contains(k))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let mut hidden = Vec::new();
    for step in (b.steps)(&given, &params) {
        match step {
            Step::Changes { name, .. } => {
                hidden.push(name.to_string());
                schemas.push((name.into(), vec![Field { name: "value".into(), width: Some(4), time: false }], None, vec![]));
            }
            Step::Sample { name, when, fields } => {
                let from: Vec<String> = fields.iter().filter(|(f, sig)| f != sig).map(|(f, sig)| format!("{f} = {sig}")).collect();
                let about = format!("a record per cycle where {when}{}", if from.is_empty() { String::new() } else { format!("; {}", from.join(", ")) });
                schemas.push((name.into(), fields.into_iter().map(|(f, _)| Field { name: f, width: None, time: false }).collect(), None, vec![about]));
            }
            Step::Check { name, rules } => {
                let mut about = vec!["a record per cycle where a rule holds, rule naming it:".to_string()];
                about.extend(rules.iter().map(|(r, e)| format!("  {r}: {e}")));
                schemas.push((name.into(), vec![Field { name: "rule".into(), width: None, time: false }], Some(String::new()), about));
            }
            Step::Decode { name, input, decoder, params } => {
                // any timescale: only the fields are wanted
                let d = crate::decode::make(decoder, &find(&schemas, input), &params, Some(1)).ok();
                let violations = d.as_ref().is_some_and(|d| d.violations()).then(String::new);
                schemas.push((name.into(), d.map(|d| d.schema()).unwrap_or_default(), violations, vec![]));
            }
            Step::Join { name, requests, response, .. } => {
                let reqs: Vec<(&str, Vec<Field>)> = requests.iter().map(|&r| (r, find(&schemas, r))).collect();
                let reqs: Vec<(&str, &[Field])> = reqs.iter().map(|(r, f)| (*r, &f[..])).collect();
                let schema = crate::decode::join_schema(&reqs, &find(&schemas, response));
                schemas.push((name.into(), schema, None, vec![]));
                // a part missing needs two parts
                let parts: String = requests.iter().filter(|_| requests.len() > 1).map(|r| format!(" | no {r}")).collect();
                let kinds = format!(": kind = no request | no response{parts}");
                schemas.push((format!("{name}.unmatched"), vec![Field { name: "kind".into(), width: None, time: false }], Some(kinds), vec![]));
            }
        }
    }
    for (stream, note) in b.notes {
        if let Some(s) = schemas.iter_mut().find(|s| s.0 == *stream) {
            s.3.insert(0, note.to_string());
        }
    }
    schemas.into_iter().filter(|s| !hidden.contains(&s.0)).map(|(n, f, v, a)| (n, f.into_iter().map(|f| f.name).collect(), v, a)).collect()
}

/// An interface in a trace that a built-in protocol decodes: its signals' common prefix
/// (`top.s_axi_`, playing each role as `<prefix><role>`), the protocol, and its clock if one
/// stands out.
#[derive(Debug, PartialEq)]
pub struct Found {
    pub prefix: String,
    pub protocol: &'static str,
    pub clock: Option<String>,
}

impl Found {
    /// The name `decode_line` gives its streams: the prefix's last part (`s_axi` for `top.s_axi_`).
    pub fn name(&self) -> &str {
        let leaf = self.prefix.rsplit('.').next().unwrap_or("").trim_matches('_');
        if leaf.is_empty() { self.protocol } else { leaf }
    }

    /// The `decode` line that puts the protocol on it, named after the prefix.
    pub fn decode_line(&self) -> String {
        let name = self.name();
        let clock = match (&self.clock, builtin(self.protocol).is_some_and(|b| b.clocked)) {
            (_, false) => String::new(),
            (Some(c), true) => format!(" clock={c}"),
            (None, true) => " clock=<clk>".into(),
        };
        format!("decode {name} {}{clock} prefix={}", self.protocol, self.prefix)
    }
}

/// The interfaces of the file's signals that built-in protocols decode: wherever every required
/// role is a signal `<prefix><role>`. A prefix goes to the protocol needing most roles (AXI4's
/// wlast / rlast over AXI-Lite). Protocols whose roles are too generic to tell (UART's `line`) are
/// left out.
pub fn detect(trace: &crate::trace::Trace) -> Vec<Found> {
    let mut protocols: Vec<(&'static str, Builtin)> = BUILTINS.iter().filter_map(|&n| Some((n, builtin(n)?))).filter(|(_, b)| b.required.len() > 1).collect();
    protocols.sort_by_key(|(_, b)| std::cmp::Reverse(b.required.len()));
    let file = |v: &usize| !trace.is_computed(*v);
    let mut found: Vec<Found> = Vec::new();
    for (name, b) in protocols {
        let first = b.required[0];
        let mut prefixes: Vec<&str> = trace.vars.iter().enumerate().filter(|(i, _)| file(i)).filter_map(|(_, v)| v.path.strip_suffix(first)).collect();
        prefixes.sort();
        for prefix in prefixes {
            let all = b.required.iter().all(|r| trace.by_path.get(&format!("{prefix}{r}")).is_some_and(file));
            if all && !found.iter().any(|f| f.prefix == prefix) {
                found.push(Found { prefix: prefix.into(), protocol: name, clock: clock_of(trace, prefix) });
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
    let mut clocks = trace.vars.iter().filter(|v| v.width == Some(1)).filter(|v| v.path.rsplit_once('.').is_some_and(|(s, leaf)| s == scope && (leaf.contains("clk") || leaf.contains("clock"))));
    match (clocks.next(), clocks.next()) {
        (Some(c), None) => Some(c.path.clone()),
        _ => None,
    }
}
