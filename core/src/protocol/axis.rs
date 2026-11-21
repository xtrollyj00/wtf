//! AXI4-Stream, from Arm's spec (IHI 0051B): beats, packets by `tlast` and (tid, tdest), the
//! handshake's errors, and the three warnings the spec gives (spec §16 "AXI4-Stream").

use crate::decode::data_bytes;
use crate::protocol::axi::{shown, unknown};
use crate::protocol::{
    Def, Default, Desc, Field, Given, Out, Params, Protocol, RoleDef, Roles, Rule, Severity,
    StreamDesc, bit_of, reset_low,
};
use crate::time::Tick;
use crate::trace::Val;

/// The rules a violation's `rule` names, and what the spec says of each.
pub const RULES: &[Rule] = &[
    Rule {
        name: "dropped",
        severity: Severity::Error,
        spec: "2.2",
        about: "tvalid fell before tready: it must stay until the handshake",
    },
    Rule {
        name: "changed",
        severity: Severity::Error,
        spec: "2.2.1",
        about: "a payload signal changed while tvalid was stalled",
    },
    Rule {
        name: "unknown",
        severity: Severity::Error,
        spec: "2.2",
        about: "tvalid or tready is x or z out of reset",
    },
    Rule {
        name: "payload_unknown",
        severity: Severity::Error,
        spec: "2.4",
        about: "a payload signal is x or z while tvalid",
    },
    Rule {
        name: "keep",
        severity: Severity::Error,
        spec: "2.5.3",
        about: "tkeep is 0 where tstrb is 1",
    },
    Rule {
        name: "reset",
        severity: Severity::Error,
        spec: "2.8.2",
        about: "tvalid while the interface is in reset, or in its first cycle out",
    },
    Rule {
        name: "wakeup",
        severity: Severity::Error,
        spec: "2.3",
        about: "twakeup fell while a transfer was stalled",
    },
    Rule {
        name: "wakeup_early",
        severity: Severity::Warning,
        spec: "2.3",
        about: "twakeup in the same cycle as tvalid",
    },
];

/// The beat's fields, in the spec's order; those of roles not given are left out.
const BEAT: &[&str] = &["tdata", "tkeep", "tstrb", "tlast", "tid", "tdest", "tuser"];

pub const DEF: Def = Def {
    about: "AXI4-Stream: beats, packets by tlast and (tid, tdest), the handshake's errors",
    example: "decode rx axis clock=aclk prefix=s_axis_ aresetn=aresetn",
    clocked: true,
    sides: &[],
    roles: &[
        RoleDef {
            name: "tvalid",
            required: true,
            default: None,
            about: "the transmitter has a beat",
        },
        RoleDef {
            name: "tready",
            required: false,
            default: Some(Default::Ones),
            about: "the receiver takes a beat (not given: always)",
        },
        RoleDef {
            name: "tdata",
            required: false,
            default: Some(Default::Zero),
            about: "the beat's bytes (not given: 0)",
        },
        RoleDef {
            name: "tkeep",
            required: false,
            default: Some(Default::Bytes("tdata")),
            about: "which bytes are data, one bit each",
        },
        RoleDef {
            name: "tstrb",
            required: false,
            default: Some(Default::Same("tkeep")),
            about: "which bytes of tdata hold information",
        },
        RoleDef {
            name: "tlast",
            required: false,
            default: Some(Default::Ones),
            about: "the packet's last beat",
        },
        RoleDef {
            name: "tid",
            required: false,
            default: Some(Default::Zero),
            about: "the packet's stream",
        },
        RoleDef {
            name: "tdest",
            required: false,
            default: Some(Default::Zero),
            about: "the packet's destination",
        },
        RoleDef {
            name: "tuser",
            required: false,
            default: Some(Default::Zero),
            about: "the beat's sideband",
        },
        RoleDef {
            name: "twakeup",
            required: false,
            default: None,
            about: "activity, to wake the clock",
        },
        RoleDef {
            name: "aresetn",
            required: true,
            default: None,
            about: "the reset, active low (reset_active=high: active high)",
        },
    ],
    params: &["reset_active"],
    make,
};

fn make(given: &Given, params: &Params, _: Option<u128>) -> Result<Box<dyn Protocol>, String> {
    Axis::new(given, params).map(|a| Box::new(a) as Box<dyn Protocol>)
}

/// A packet of one (tid, tdest), from its first beat until tlast or the end of the trace.
struct Packet {
    key: (Option<Val>, Option<Val>),
    start: Tick,
    end: Tick,
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

/// An instance: the roles it has signals for, its streams' schema, and what it has seen so far.
pub struct Axis {
    given: Vec<String>,
    desc: Desc,
    /// Reset is active low (`reset_active=low`), the spec's default.
    reset_low: bool,
    in_reset: bool,
    prev: Prev,
    /// The payload held by a stalled transfer, in `beats`' order.
    stall: Option<Vec<Option<Val>>>,
    packets: Vec<Packet>,
    /// The `beat` stream's fields: those of the roles it has.
    beats: Vec<Field>,
}

impl Axis {
    fn new(given: &Given, params: &Params) -> Result<Self, String> {
        let reset_low = reset_low(params)?;
        let width = |r: &str| given.iter().find(|g| g.0 == r).and_then(|g| g.1);
        let has = |r: &str| given.iter().any(|g| g.0 == r);
        let field = |r: &str, w: Option<u32>| Field {
            name: r.into(),
            width: w,
            time: false,
            signed: false,
        };
        let beats: Vec<Field> = BEAT
            .iter()
            .filter(|r| has(r))
            .map(|r| field(r, width(r)))
            .collect();
        let mut frames = vec![field("beats", Some(32))];
        frames.extend(
            ["tid", "tdest"]
                .iter()
                .filter(|r| has(r))
                .map(|r| field(r, width(r))),
        );
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
        about.push("a packet of one (tid, tdest), its bytes tkeep marks; pos, the ones tstrb marks as position".into());
        about.push("a record per cycle where a rule holds, rule naming it:".into());
        about.extend(RULES.iter().map(|r| {
            format!(
                "  {}: {} ({}, spec {})",
                r.name,
                r.about,
                r.severity.text(),
                r.spec
            )
        }));
        let desc = Desc {
            streams: vec![
                StreamDesc {
                    name: "beat",
                    fields: beats.clone(),
                    violations: false,
                    about: about.drain(..1).collect(),
                    from: &[],
                },
                StreamDesc {
                    name: "frame",
                    fields: frames,
                    violations: false,
                    about: about.drain(..1).collect(),
                    from: &["beat"],
                },
                StreamDesc {
                    name: "errors",
                    fields: vec![
                        field("rule", None),
                        field("detail", None),
                        field("severity", None),
                    ],
                    violations: true,
                    about,
                    from: &[],
                },
            ],
        };
        Ok(Axis {
            given: given.iter().map(|g| g.0.to_string()).collect(),
            desc,
            reset_low,
            in_reset: false,
            prev: Prev::default(),
            stall: None,
            packets: Vec::new(),
            beats,
        })
    }

    /// Whether the instance has a signal for the role, rather than a default.
    fn has(&self, role: &str) -> bool {
        self.given.iter().any(|g| g == role)
    }

    /// A `frame` record of a packet.
    fn frame(&self, p: Packet, complete: bool) -> Out {
        let mut fields = vec![num(p.beats, 32)];
        if self.has("tid") {
            fields.push(p.key.0);
        }
        if self.has("tdest") {
            fields.push(p.key.1);
        }
        fields.push(num(p.len, 32));
        if self.has("tstrb") {
            fields.push(num(p.pos, 32));
        }
        if self.has("tdata") {
            fields.push(Some(Val::Text(p.data.join(" "))));
        }
        fields.push(num(complete as u64, 1));
        Out::Record {
            stream: "frame",
            start: p.start,
            end: p.end,
            fields,
        }
    }
}

impl Protocol for Axis {
    fn desc(&self) -> &Desc {
        &self.desc
    }

    fn feed(&mut self, start: Tick, end: Tick, roles: &Roles<'_>) -> Vec<Out> {
        let mut out = Vec::new();
        let valid = roles.bit("tvalid");
        let ready = roles.bit("tready").unwrap_or(true);
        let (wake, has_wake) = (roles.bit("twakeup"), roles.get("twakeup").is_some());
        // the spec's reset is active low (2.8.2); an x on it (a testbench's reset before it is
        // driven) records nothing and says nothing
        let level = roles.bit("aresetn");
        let in_reset = level.is_none_or(|high| high != self.reset_low);
        if in_reset {
            if level.is_some() && valid != Some(false) {
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
            if let Some(v) = roles.get(role).filter(|v| unknown(v)) {
                out.push(violation(
                    "unknown",
                    start,
                    end,
                    format!("{role} is {} out of reset", shown(v)),
                ));
            }
        }
        if let (Some(true), Some((role, v))) = (
            valid,
            self.beats
                .iter()
                .find_map(|f| Some((f, roles.get(&f.name).filter(|v| unknown(v))?))),
        ) {
            out.push(violation(
                "payload_unknown",
                start,
                end,
                format!("{} is {} while tvalid", role.name, shown(v)),
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
                .zip(&self.beats)
                .find(|(was, f)| roles.get(&f.name) != was.as_ref());
            if valid != Some(true) {
                out.push(violation(
                    "dropped",
                    start,
                    end,
                    "tvalid fell before tready",
                ));
            } else if let Some((_, f)) = changed {
                out.push(violation(
                    "changed",
                    start,
                    end,
                    format!("{} changed while tvalid stalled", f.name),
                ));
            }
        }
        if valid == Some(true) && !ready {
            self.stall = Some(
                self.beats
                    .iter()
                    .map(|f| roles.get(&f.name).cloned())
                    .collect(),
            );
        }
        if valid == Some(true) && ready {
            let (keep, strb) = (roles.get("tkeep"), roles.get("tstrb"));
            // tkeep is one bit per byte, tstrb says which of those hold information (2.5.3)
            if let (Some(Val::Bits(k)), Some(Val::Bits(s))) = (keep, strb) {
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
            out.push(Out::Record {
                stream: "beat",
                start,
                end,
                fields: roles.pick(&self.beats),
            });
            let (len, pos) = counted(keep, strb);
            let key = (roles.get("tid").cloned(), roles.get("tdest").cloned());
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
            let p = &mut self.packets[i];
            p.end = end;
            p.beats += 1;
            p.len += len;
            p.pos += pos;
            let tdata = roles.get("tdata").cloned();
            p.data.extend(data_bytes(&tdata, mask(keep, strb).as_ref()));
            if roles.bit("tlast") == Some(true) {
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

/// A record of a rule in `RULES`: its severity from there.
fn violation(rule: &'static str, start: Tick, end: Tick, detail: impl Into<String>) -> Out {
    let severity = RULES
        .iter()
        .find(|r| r.name == rule)
        .map_or(Severity::Error, |r| r.severity);
    Out::Violation {
        rule,
        start,
        end,
        detail: detail.into(),
        severity,
    }
}

/// A beat's bytes: those tkeep marks, and those of them tstrb marks as position.
fn counted(keep: Option<&Val>, strb: Option<&Val>) -> (u64, u64) {
    let (Some(Val::Bits(k)), Some(Val::Bits(s))) = (keep, strb) else {
        return (0, 0);
    };
    let n = k.len().min(s.len());
    let kept = (0..n).filter(|&b| bit_of(k, b) == Some(true)).count();
    let data = (0..n)
        .filter(|&b| bit_of(k, b) == Some(true) && bit_of(s, b) == Some(true))
        .count();
    ((kept - data) as u64 + data as u64, (kept - data) as u64)
}

/// The bytes both tkeep and tstrb mark, as a bit string for `decode::data_bytes`.
fn mask(keep: Option<&Val>, strb: Option<&Val>) -> Option<Val> {
    let (Val::Bits(k), Val::Bits(s)) = (keep?, strb?) else {
        return None;
    };
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

/// A number as a field holds it: `w` bits, the highest first.
fn num(n: u64, w: u32) -> Option<Val> {
    Some(Val::Bits(format!("{n:0w$b}", w = w as usize)))
}
