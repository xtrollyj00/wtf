//! UART as a `Protocol`: bytes from an idle-high line at a baud rate, and the frames that broke
//! one of its rules (spec §16 "UART").

use crate::decode::{invert, param};
use crate::protocol::{
    Def, Desc, Given, Out, Params, Protocol, RoleDef, Roles, Rule, Severity, StreamDesc,
};
use crate::stream::Field;
use crate::time::Tick;
use crate::trace::Val;

/// The rules a violation's `rule` names, and what the frame asks of each.
pub const RULES: &[Rule] = &[
    Rule {
        name: "framing",
        severity: Severity::Error,
        spec: "EIA-232",
        about: "a stop bit that isn't 1",
    },
    Rule {
        name: "parity",
        severity: Severity::Error,
        spec: "EIA-232",
        about: "a parity bit that isn't the one `parity` asks for",
    },
    Rule {
        name: "break",
        severity: Severity::Warning,
        spec: "EIA-232",
        about: "the line low for a whole frame: a break, with no byte",
    },
];

pub const DEF: Def = Def {
    about: "UART: bytes from an idle-high line (idle=low: inverted) at a baud rate, framing and parity errors, breaks",
    example: "decode console uart line=uart_tx baud=115200",
    clocked: false,
    sides: &[],
    roles: &[RoleDef {
        name: "line",
        required: true,
        default: None,
        about: "the line, idle high (idle=low: inverted)",
    }],
    params: &["baud", "bits", "parity", "stop", "idle"],
    make,
};

fn make(
    _: &Given,
    params: &Params,
    fs_per_tick: Option<u128>,
) -> Result<Box<dyn Protocol>, String> {
    Uart::new(params, fs_per_tick).map(|u| Box::new(u) as Box<dyn Protocol>)
}

/// An instance: the line's level, and the frame it samples at the bit period the trace's
/// timescale gives `baud`.
pub struct Uart {
    /// Ticks a bit lasts: the trace's femtoseconds per tick against `baud`.
    period: f64,
    bits: usize,
    /// Parity asked for: `None` none, `Some(false)` even, `Some(true)` odd.
    parity: Option<bool>,
    stop: usize,
    /// The line idles low: read inverted.
    inverted: bool,
    level: Option<u8>,
    /// The frame being sampled: its start, and the bits sampled so far.
    frame: Option<(Tick, Vec<u8>)>,
    desc: Desc,
}

impl Uart {
    fn new(params: &Params, fs_per_tick: Option<u128>) -> Result<Self, String> {
        let mut errors = Vec::new();
        let baud = param(
            params,
            "baud",
            None::<f64>,
            |b| *b > 0.0,
            "a positive number of bits a second",
            &mut errors,
        );
        let bits = param(
            params,
            "bits",
            Some(8usize),
            |b| (5..=9).contains(b),
            "5 - 9",
            &mut errors,
        );
        let parity = param(
            params,
            "parity",
            Some("none".to_string()),
            |p| ["none", "even", "odd"].contains(&p.as_str()),
            "none, even or odd",
            &mut errors,
        );
        let stop = param(
            params,
            "stop",
            Some(1usize),
            |s| *s == 1 || *s == 2,
            "1 or 2",
            &mut errors,
        );
        let idle = param(
            params,
            "idle",
            Some("high".to_string()),
            |i| i == "high" || i == "low",
            "high or low",
            &mut errors,
        );
        if let Some((k, _)) = params
            .iter()
            .find(|p| !["baud", "bits", "parity", "stop", "idle"].contains(&p.0.as_str()))
        {
            errors.push(format!("no param {k} (baud, bits, parity, stop, idle)"));
        }
        if fs_per_tick.is_none() {
            errors.push("the trace has no timescale to time bits by".into());
        }
        if !errors.is_empty() {
            return Err(format!("uart: {}", errors.join("; ")));
        }
        let period = 1e15 / baud.unwrap_or(1.0) / fs_per_tick.unwrap_or(1) as f64;
        let parity = match parity.as_deref() {
            Some("even") => Some(false),
            Some("odd") => Some(true),
            _ => None,
        };
        let bits = bits.unwrap_or(8);
        let field = |name: &str, width: Option<u32>| Field {
            name: name.into(),
            width,
            time: false,
            signed: false,
        };
        let mut about =
            vec!["a record per frame where the frame breaks a rule, rule naming it:".to_string()];
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
                    name: "byte",
                    from: &[],
                    fields: vec![field("data", Some(bits as u32))],
                    violations: false,
                    about: vec!["a record per frame: its data bits, least significant bit first, each sampled in the middle of its bit".into()],
                },
                StreamDesc {
                    name: "errors",
                    from: &[],
                    fields: vec![field("rule", None), field("detail", None), field("severity", None)],
                    violations: true,
                    about,
                },
            ],
        };
        Ok(Uart {
            period,
            bits,
            parity,
            stop: stop.unwrap_or(1),
            inverted: idle.as_deref() == Some("low"),
            level: None,
            frame: None,
            desc,
        })
    }

    /// Sample the open frame's bits whose middles fall before `t`, at the line's level; the records
    /// of a frame sampled whole.
    fn sample_until(&mut self, t: f64) -> Vec<Out> {
        let need = 1 + self.bits + self.parity.is_some() as usize + self.stop;
        let mut out = Vec::new();
        while let (Some((start, got)), Some(level)) = (&mut self.frame, self.level) {
            if *start as f64 + (got.len() as f64 + 0.5) * self.period >= t {
                return out;
            }
            got.push(level);
            if got.len() == 1 && level != b'0' {
                // the start bit wasn't one: a glitch, not a frame
                self.frame = None;
            } else if got.len() == need {
                let (start, got) = self.frame.take().unwrap();
                out.extend(self.frame_record(start, &got));
            }
        }
        out
    }

    /// The records of a frame sampled whole: its byte, and the rule it breaks, if any.
    fn frame_record(&self, start: Tick, got: &[u8]) -> Vec<Out> {
        let len = 1 + self.bits + self.parity.is_some() as usize + self.stop;
        let end = start + (len as f64 * self.period).round() as Tick;
        // the line held low for the whole frame: a break, no byte
        if got.iter().all(|&b| b == b'0') {
            return vec![violation(
                "break",
                start,
                end,
                "the line was low for a whole frame",
            )];
        }
        let data: String = got[1..=self.bits]
            .iter()
            .rev()
            .map(|&b| b as char)
            .collect();
        let mut out = vec![Out::Record {
            stream: "byte",
            start,
            end,
            fields: vec![Some(Val::Bits(data))],
        }];
        if let Some(&b) = got[got.len() - self.stop..].iter().find(|&&b| b != b'1') {
            out.push(violation(
                "framing",
                start,
                end,
                format!("stop bit {}, expected 1", b as char),
            ));
            return out;
        }
        let ones = got[1..=self.bits].iter().filter(|&&b| b == b'1').count();
        let parity_bad = self
            .parity
            .is_some_and(|odd| (ones + (got[self.bits + 1] == b'1') as usize) % 2 != odd as usize);
        if parity_bad {
            let want = if self.parity == Some(true) { '1' } else { '0' };
            out.push(violation(
                "parity",
                start,
                end,
                format!("parity bit {}, expected {want}", got[self.bits + 1] as char),
            ));
        }
        out
    }
}

impl Protocol for Uart {
    fn desc(&self) -> &Desc {
        &self.desc
    }

    /// One cycle of the line: the level it held, from `start` to `end`. `end` is not read: a bit's
    /// length comes from `period`, and the line's edges are often far apart.
    fn feed(&mut self, start: Tick, _end: Tick, roles: &Roles<'_>) -> Vec<Out> {
        // a character, not a flag: a bit that is x or z reaches the byte as that character
        let Some(Val::Bits(b)) = roles.get("line") else {
            return Vec::new();
        };
        let v = b.as_bytes()[0].to_ascii_lowercase();
        let v = if self.inverted { invert(v) } else { v };
        let out = self.sample_until(start as f64);
        if self.frame.is_none() && self.level == Some(b'1') && v == b'0' {
            self.frame = Some((start, Vec::new()));
        }
        self.level = Some(v);
        out
    }

    /// The line keeps its last level: a frame still open is sampled to the end of the trace.
    fn finish(&mut self) -> Vec<Out> {
        self.sample_until(f64::INFINITY)
    }
}

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
