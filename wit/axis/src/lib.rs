//! `axis-wasm`: AXI4-Stream as a WebAssembly component (wit/protocol.wit), the transcription of
//! `core/src/protocol/axis.rs` that the bench times against it. Both read Arm's spec (IHI 0051B).
//!
//! The host passes the roles the trace has, in the order `create` took them, and not the defaults:
//! a component fills the roles its trace has no signal for itself, as the Rust one does.

#[allow(warnings)]
mod bindings;

use std::cell::RefCell;

use bindings::exports::wtf::protocol::api::{
    Card, DefaultValue, Field, Found, Guest, Out, Role, Severity, StreamDesc, Value, Violation,
};

struct Component;

thread_local! {
    static STATE: RefCell<Axis> = RefCell::new(Axis::default());
}

/// How a role is filled in when the trace has no signal for it.
enum Fill {
    /// All zeros.
    Zero,
    /// All ones.
    Ones,
    /// One bit per whole byte of another role (spec §3.1.2).
    Bytes(&'static str),
    /// The same value as another role.
    Same(&'static str),
}

/// The roles of the built-in's `DEF`, with what fills one the trace has no signal for.
struct RoleDef {
    name: &'static str,
    required: bool,
    fill: Option<Fill>,
    about: &'static str,
}

const ROLES: &[RoleDef] = &[
    RoleDef {
        name: "tvalid",
        required: true,
        fill: None,
        about: "the transmitter has a beat",
    },
    RoleDef {
        name: "tready",
        required: false,
        fill: Some(Fill::Ones),
        about: "the receiver takes a beat (not given: always)",
    },
    RoleDef {
        name: "tdata",
        required: false,
        fill: Some(Fill::Zero),
        about: "the beat's bytes (not given: 0)",
    },
    RoleDef {
        name: "tkeep",
        required: false,
        fill: Some(Fill::Bytes("tdata")),
        about: "which bytes are data, one bit each",
    },
    RoleDef {
        name: "tstrb",
        required: false,
        fill: Some(Fill::Same("tkeep")),
        about: "which bytes of tdata hold information",
    },
    RoleDef {
        name: "tlast",
        required: false,
        fill: Some(Fill::Ones),
        about: "the packet's last beat",
    },
    RoleDef {
        name: "tid",
        required: false,
        fill: Some(Fill::Zero),
        about: "the packet's stream",
    },
    RoleDef {
        name: "tdest",
        required: false,
        fill: Some(Fill::Zero),
        about: "the packet's destination",
    },
    RoleDef {
        name: "tuser",
        required: false,
        fill: Some(Fill::Zero),
        about: "the beat's sideband",
    },
    RoleDef {
        name: "twakeup",
        required: false,
        fill: None,
        about: "activity, to wake the clock",
    },
    RoleDef {
        name: "aresetn",
        required: false,
        fill: None,
        about: "the reset, active low (reset_active=high: active high)",
    },
];

/// The rules a violation's `rule` names: (name, severity, spec, what the spec says of it).
const RULES: &[(&str, Severity, &str, &str)] = &[
    (
        "dropped",
        Severity::Error,
        "2.2",
        "tvalid fell before tready: it must stay until the handshake",
    ),
    (
        "changed",
        Severity::Error,
        "2.2.1",
        "a payload signal changed while tvalid was stalled",
    ),
    (
        "unknown",
        Severity::Warning,
        "2.2",
        "tvalid or tready is x or z out of reset",
    ),
    (
        "payload_unknown",
        Severity::Warning,
        "2.4",
        "a payload signal is x or z while tvalid",
    ),
    (
        "keep",
        Severity::Error,
        "2.5.3",
        "tkeep is 0 where tstrb is 1",
    ),
    (
        "reset",
        Severity::Error,
        "2.8.2",
        "tvalid while the interface is in reset, or in its first cycle out",
    ),
    (
        "wakeup",
        Severity::Error,
        "2.3",
        "twakeup fell while a transfer was stalled",
    ),
    (
        "wakeup_early",
        Severity::Warning,
        "2.3",
        "twakeup in the same cycle as tvalid",
    ),
];

/// The beat's fields, in the spec's order; those of roles not given are left out.
const BEAT: &[&str] = &["tdata", "tkeep", "tstrb", "tlast", "tid", "tdest", "tuser"];

/// A role's value, as a row spells it: the host's strings, kept apart by kind.
#[derive(Clone, PartialEq)]
enum Val {
    Bits(String),
    Text(String),
}

impl Val {
    fn of(v: Option<&Value>) -> Option<Val> {
        v.map(|v| match v {
            Value::Bits(b) => Val::Bits(b.clone()),
            Value::Text(t) => Val::Text(t.clone()),
        })
    }

    fn to(&self) -> Value {
        match self {
            Val::Bits(b) => Value::Bits(b.clone()),
            Val::Text(t) => Value::Text(t.clone()),
        }
    }

    fn bits(&self) -> Option<&str> {
        match self {
            Val::Bits(b) => Some(b),
            Val::Text(_) => None,
        }
    }
}

/// A packet of one (tid, tdest), from its first beat until tlast or the end of the trace.
struct Packet {
    key: (Option<Val>, Option<Val>),
    start: u64,
    end: u64,
    beats: u64,
    len: u64,
    pos: u64,
    data: Vec<String>,
}

/// The last cycle's handshake, for the twakeup rules.
#[derive(Clone, Copy, Default)]
struct Prev {
    valid: Option<bool>,
    ready: bool,
    wakeup: Option<bool>,
}

/// An instance: the roles its trace has, and what it has seen so far.
struct Axis {
    given: Vec<String>,
    widths: Vec<Option<u32>>,
    /// Reset is active low (`reset_active=low`), the spec's default.
    reset_low: bool,
    in_reset: bool,
    prev: Prev,
    /// The payload held by a stalled transfer, in `beats`' order.
    stall: Option<Vec<Option<Val>>>,
    packets: Vec<Packet>,
    /// The `beat` stream's fields: those of the roles it has.
    beats: Vec<String>,
}

impl Default for Axis {
    fn default() -> Self {
        Axis {
            given: Vec::new(),
            widths: Vec::new(),
            reset_low: true,
            in_reset: false,
            prev: Prev::default(),
            stall: None,
            packets: Vec::new(),
            beats: Vec::new(),
        }
    }
}

impl Axis {
    /// The width the trace gave a role: none for a text signal or a role it has no signal for.
    fn width(&self, role: &str) -> Option<u32> {
        self.given
            .iter()
            .position(|g| g == role)
            .and_then(|i| self.widths[i])
    }

    /// Whether the instance has a signal for the role, rather than a default.
    fn has(&self, role: &str) -> bool {
        self.given.iter().any(|g| g == role)
    }

    /// The role's value this cycle: what the host passed, or the default that fills a role the
    /// trace has no signal for (`Fill::Same` takes the role it names).
    fn value(&self, values: &[Option<Val>], role: &str) -> Option<Val> {
        if let Some(i) = self.given.iter().position(|g| g == role) {
            return values.get(i).cloned().flatten();
        }
        match ROLES.iter().find(|r| r.name == role)?.fill.as_ref()? {
            Fill::Zero => Some(Val::Bits(
                "0".repeat(self.width(role).unwrap_or(1) as usize),
            )),
            Fill::Ones => Some(Val::Bits(
                "1".repeat(self.width(role).unwrap_or(1) as usize),
            )),
            Fill::Bytes(src) => Some(Val::Bits("1".repeat(self.width(src)?.div_ceil(8) as usize))),
            Fill::Same(src) => self.value(values, src),
        }
    }

    /// The role's value as a flag: None for x or z, or no value.
    fn bit(&self, values: &[Option<Val>], role: &str) -> Option<bool> {
        match self.value(values, role)? {
            Val::Bits(b) => bit_of(&b, 0),
            Val::Text(_) => None,
        }
    }

    /// A `frame` record of a packet.
    fn frame(&self, p: Packet, complete: bool) -> Out {
        let mut fields = vec![num(p.beats, 32)];
        if self.has("tid") {
            fields.push(p.key.0.map(|v| v.to()));
        }
        if self.has("tdest") {
            fields.push(p.key.1.map(|v| v.to()));
        }
        fields.push(num(p.len, 32));
        if self.has("tstrb") {
            fields.push(num(p.pos, 32));
        }
        if self.has("tdata") {
            fields.push(Some(Value::Text(p.data.join(" "))));
        }
        fields.push(num(complete as u64, 1));
        Out::Found(Found {
            stream_name: "frame".to_string(),
            start: p.start,
            end: p.end,
            fields,
        })
    }

    fn feed(&mut self, start: u64, end: u64, values: &[Option<Val>]) -> Vec<Out> {
        let mut out = Vec::new();
        let valid = self.bit(values, "tvalid");
        let ready = self.bit(values, "tready").unwrap_or(true);
        let (wake, has_wake) = (self.bit(values, "twakeup"), self.has("twakeup"));
        // the spec's reset is active low (2.8.2)
        let in_reset = self
            .bit(values, "aresetn")
            .is_some_and(|high| high != self.reset_low);
        if in_reset {
            if valid != Some(false) {
                out.push(violation("reset", start, end, "tvalid in reset"));
            }
            self.stall = None;
            self.packets.clear();
            self.in_reset = true;
            self.prev = Prev {
                valid,
                ready,
                wakeup: wake,
            };
            return out;
        }
        if std::mem::replace(&mut self.in_reset, false) {
            // it may rise a cycle after the reset does, no sooner (2.8.2)
            if valid == Some(true) {
                out.push(violation(
                    "reset",
                    start,
                    end,
                    "tvalid in the cycle reset is released",
                ));
            }
            self.prev = Prev {
                valid,
                ready,
                wakeup: wake,
            };
            return out;
        }
        for role in ["tvalid", "tready"] {
            if self.value(values, role).as_ref().is_some_and(unknown) {
                out.push(violation(
                    "unknown",
                    start,
                    end,
                    format!("{role} is x or z out of reset"),
                ));
            }
        }
        if let (Some(true), Some(role)) = (
            valid,
            self.beats
                .iter()
                .find(|f| self.value(values, f.as_str()).as_ref().is_some_and(unknown)),
        ) {
            out.push(violation(
                "payload_unknown",
                start,
                end,
                format!("{role} is x or z while tvalid"),
            ));
        }
        if has_wake {
            // it must stay asserted until tready (2.3)
            if valid == Some(true) && wake == Some(true) && self.prev.wakeup != Some(true) {
                out.push(violation(
                    "wakeup_early",
                    start,
                    end,
                    "twakeup in the same cycle as tvalid",
                ));
            }
            if self.prev.valid == Some(true)
                && !self.prev.ready
                && self.prev.wakeup == Some(true)
                && wake != Some(true)
            {
                out.push(violation(
                    "wakeup",
                    start,
                    end,
                    "twakeup fell while the transfer stalled",
                ));
            }
        }
        // the cycle after a stall: a valid that fell, or a payload that moved (2.2.1)
        if let Some(held) = self.stall.take() {
            let changed = held
                .iter()
                .zip(self.beats.iter())
                .find(|(was, role)| self.value(values, role.as_str()) != **was);
            if valid != Some(true) {
                out.push(violation(
                    "dropped",
                    start,
                    end,
                    "tvalid fell before tready",
                ));
            } else if let Some((_, role)) = changed {
                out.push(violation(
                    "changed",
                    start,
                    end,
                    format!("{role} changed while tvalid stalled"),
                ));
            }
        }
        if valid == Some(true) && !ready {
            self.stall = Some(
                self.beats
                    .iter()
                    .map(|f| self.value(values, f.as_str()))
                    .collect(),
            );
        }
        if valid == Some(true) && ready {
            let (keep, strb) = (self.value(values, "tkeep"), self.value(values, "tstrb"));
            // tkeep is one bit per byte, tstrb says which of those hold information (2.5.3)
            if let (Some(k), Some(s)) = (
                keep.as_ref().and_then(Val::bits),
                strb.as_ref().and_then(Val::bits),
            ) {
                for b in (0..k.len().min(s.len()))
                    .filter(|&b| bit_of(k, b) == Some(false) && bit_of(s, b) == Some(true))
                {
                    out.push(violation(
                        "keep",
                        start,
                        end,
                        format!("tkeep 0, tstrb 1 on byte {b}"),
                    ));
                }
            }
            let fields = self
                .beats
                .iter()
                .map(|f| self.value(values, f.as_str()).map(|v| v.to()))
                .collect();
            out.push(Out::Found(Found {
                stream_name: "beat".to_string(),
                start,
                end,
                fields,
            }));
            let (len, pos) = counted(keep.as_ref(), strb.as_ref());
            let key = (self.value(values, "tid"), self.value(values, "tdest"));
            let i = match self.packets.iter().position(|p| p.key == key) {
                Some(i) => i,
                None => {
                    self.packets.push(Packet {
                        key,
                        start,
                        end,
                        beats: 0,
                        len: 0,
                        pos: 0,
                        data: Vec::new(),
                    });
                    self.packets.len() - 1
                }
            };
            let bytes = data_bytes(
                self.value(values, "tdata"),
                mask(keep.as_ref(), strb.as_ref()).as_ref(),
            );
            let last = self.bit(values, "tlast") == Some(true);
            let p = &mut self.packets[i];
            p.end = end;
            p.beats += 1;
            p.len += len;
            p.pos += pos;
            p.data.extend(bytes);
            if last {
                let packet = self.packets.remove(i);
                out.push(self.frame(packet, true));
            }
        }
        self.prev = Prev {
            valid,
            ready,
            wakeup: wake,
        };
        out
    }

    fn finish(&mut self) -> Vec<Out> {
        let packets = std::mem::take(&mut self.packets);
        packets.into_iter().map(|p| self.frame(p, false)).collect()
    }
}

impl Guest for Component {
    fn describe() -> Result<Card, String> {
        Ok(Card {
            about: "AXI4-Stream: beats, packets by tlast and (tid, tdest), the handshake's errors, from a component".to_string(),
            example: "decode rx axis-wasm clock=aclk prefix=s_axis_".to_string(),
            clocked: true,
            sides: Vec::new(),
            roles: ROLES
                .iter()
                .map(|r| Role {
                    name: r.name.to_string(),
                    required: r.required,
                    default: r.fill.as_ref().map(fill),
                    about: r.about.to_string(),
                })
                .collect(),
            params: vec!["reset_active".to_string()],
        })
    }

    fn create(
        roles: Vec<(String, Option<u32>)>,
        params: Vec<(String, String)>,
    ) -> Result<Vec<StreamDesc>, String> {
        let mut reset_low = true;
        for (k, v) in &params {
            match (k.as_str(), v.as_str()) {
                ("reset_active", "low") => reset_low = true,
                ("reset_active", "high") => reset_low = false,
                ("reset_active", other) => {
                    return Err(format!("reset_active: low or high, not {other}"));
                }
                _ => {}
            }
        }
        let given: Vec<String> = roles.iter().map(|(r, _)| r.clone()).collect();
        let widths: Vec<Option<u32>> = roles.iter().map(|(_, w)| *w).collect();
        let has = |r: &str| given.iter().any(|g| g == r);
        let width = |r: &str| given.iter().position(|g| g == r).and_then(|i| widths[i]);
        let field = |r: &str, w: Option<u32>| Field {
            name: r.to_string(),
            width: w,
            time: false,
            signed: false,
        };
        let mut beats = Vec::new();
        for &r in BEAT {
            if has(r) {
                beats.push(r.to_string());
            }
        }
        let mut frames = vec![field("beats", Some(32))];
        for r in ["tid", "tdest"] {
            if has(r) {
                frames.push(field(r, width(r)));
            }
        }
        frames.push(field("len", Some(32)));
        if has("tstrb") {
            frames.push(field("pos", Some(32)));
        }
        if has("tdata") {
            frames.push(field("data", None));
        }
        frames.push(field("complete", Some(1)));
        let mut about =
            vec!["a record per cycle where tvalid and tready, the payload given".to_string()];
        about.push("a packet of one (tid, tdest), its bytes tkeep marks; pos, the ones tstrb marks as position".to_string());
        about.push("a record per cycle where a rule holds, rule naming it:".to_string());
        for (name, severity, spec, what) in RULES {
            about.push(format!(
                "  {name}: {what} ({}, spec {spec})",
                severity_text(severity)
            ));
        }
        let fields = |r: &str| field(r, width(r));
        let streams = vec![
            StreamDesc {
                name: "beat".to_string(),
                fields: beats.iter().map(|r| fields(r)).collect(),
                violations: false,
                about: about.drain(..1).collect(),
            },
            StreamDesc {
                name: "frame".to_string(),
                fields: frames,
                violations: false,
                about: about.drain(..1).collect(),
            },
            StreamDesc {
                name: "errors".to_string(),
                fields: vec![
                    field("rule", None),
                    field("detail", None),
                    field("severity", None),
                ],
                violations: true,
                about,
            },
        ];
        let axis = Axis {
            given,
            widths,
            reset_low,
            in_reset: false,
            prev: Prev::default(),
            stall: None,
            packets: Vec::new(),
            beats,
        };
        STATE.with(|s| *s.borrow_mut() = axis);
        Ok(streams)
    }

    fn feed(start: u64, end: u64, values: Vec<Option<Value>>) -> Vec<Out> {
        let values: Vec<Option<Val>> = values.iter().map(|v| Val::of(v.as_ref())).collect();
        STATE.with(|s| s.borrow_mut().feed(start, end, &values))
    }

    fn finish() -> Vec<Out> {
        STATE.with(|s| s.borrow_mut().finish())
    }
}

fn fill(d: &Fill) -> DefaultValue {
    match d {
        Fill::Zero => DefaultValue::Zero,
        Fill::Ones => DefaultValue::Ones,
        Fill::Bytes(s) => DefaultValue::Bytes(s.to_string()),
        Fill::Same(s) => DefaultValue::Same(s.to_string()),
    }
}

fn severity_text(s: &Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// A record of a rule in `RULES`: its severity from there.
fn violation(rule: &str, start: u64, end: u64, detail: impl Into<String>) -> Out {
    let severity = match RULES.iter().find(|r| r.0 == rule).map(|r| &r.1) {
        Some(Severity::Warning) => Severity::Warning,
        _ => Severity::Error,
    };
    Out::Violation(Violation {
        rule: rule.to_string(),
        start,
        end,
        detail: detail.into(),
        severity,
    })
}

/// A bit that is x or z (or uninitialized), which the spec forbids for these.
fn unknown(v: &Val) -> bool {
    matches!(v, Val::Bits(b) if b.contains(['x', 'z', 'u', 'w', '-']))
}

/// Bit `k` of a bit string, from the lowest: None for x or z.
fn bit_of(b: &str, k: usize) -> Option<bool> {
    match b.len().checked_sub(k + 1).map(|i| b.as_bytes()[i]) {
        Some(b'1' | b'h') => Some(true),
        Some(b'0' | b'l') => Some(false),
        _ => None,
    }
}

/// A beat's bytes: those tkeep marks, and those of them tstrb marks as position.
fn counted(keep: Option<&Val>, strb: Option<&Val>) -> (u64, u64) {
    let (Some(k), Some(s)) = (keep.and_then(Val::bits), strb.and_then(Val::bits)) else {
        return (0, 0);
    };
    let n = k.len().min(s.len());
    let kept = (0..n).filter(|&b| bit_of(k, b) == Some(true)).count();
    let data = (0..n)
        .filter(|&b| bit_of(k, b) == Some(true) && bit_of(s, b) == Some(true))
        .count();
    (kept as u64, (kept - data) as u64)
}

/// The bytes both tkeep and tstrb mark, as a bit string for `data_bytes`.
fn mask(keep: Option<&Val>, strb: Option<&Val>) -> Option<Val> {
    let (k, s) = (keep.and_then(Val::bits), strb.and_then(Val::bits));
    let (k, s) = (k?, s?);
    let n = k.len().min(s.len());
    Some(Val::Bits(
        (0..n)
            .map(|b| {
                if bit_of(k, b) == Some(true) && bit_of(s, b) == Some(true) {
                    '1'
                } else {
                    '0'
                }
            })
            .rev()
            .collect(),
    ))
}

/// A beat's bytes: those tkeep marks (all of them without a mask), as hex.
fn data_bytes(v: Option<Val>, keep: Option<&Val>) -> Vec<String> {
    let Some(Val::Bits(b)) = v else {
        return Vec::new();
    };
    let b = format!("{}{b}", "0".repeat((8 - b.len() % 8) % 8));
    let kept = |k: usize| match keep.and_then(Val::bits) {
        Some(m) => m.len() > k && matches!(m.as_bytes()[m.len() - 1 - k], b'1' | b'h'),
        None => true,
    };
    (0..b.len() / 8)
        .filter(|&k| kept(k))
        .map(|k| {
            let byte = &b[b.len() - 8 * (k + 1)..b.len() - 8 * k];
            u8::from_str_radix(byte, 2).map_or("xx".to_string(), |n| format!("{n:02x}"))
        })
        .collect()
}

/// A number as a field holds it: `w` bits, the highest first.
fn num(n: u64, w: u32) -> Option<Value> {
    Some(Value::Bits(format!("{n:0w$b}", w = w as usize)))
}

bindings::export!(Component with_types_in bindings);
