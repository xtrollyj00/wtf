//! AXI channel handshakes, shared by AXI4-Lite and AXI4 (spec §16): a record per channel handshake,
//! the handshake's rules, and each side's joined into a transaction.

use crate::decode;
use crate::protocol::{Field, Given, Out, Params, Roles, Severity, StreamDesc};
use crate::stream::{self, Stream};
use crate::time::Tick;
use crate::trace::Val;

/// A channel of an AXI port: the stem of its roles (`aw`, `w`, `b`, `ar`, `r`), the fields its
/// handshakes record (the field, then the role it samples), and its rules' names in `Kind` order.
pub struct Chan {
    pub name: &'static str,
    pub payload: &'static [(&'static str, &'static str)],
    pub rules: [&'static str; 3],
}

/// The three rules every AXI channel has.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A valid that fell before its ready.
    Dropped,
    /// A payload that moved while stalled.
    Changed,
    /// A response that is not OKAY.
    Resp,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Dropped, Kind::Changed, Kind::Resp];

    fn at(self) -> usize {
        match self {
            Kind::Dropped => 0,
            Kind::Changed => 1,
            Kind::Resp => 2,
        }
    }

    /// What the rule is, for `protocols <name>`.
    pub fn about(self) -> &'static str {
        match self {
            Kind::Dropped => "a valid fell before its ready: it stays until the handshake",
            Kind::Changed => "a payload signal changed while its valid was stalled",
            Kind::Resp => "a response that is not OKAY",
        }
    }

    /// The clause of IHI 0022 it comes from.
    pub fn spec(self) -> &'static str {
        match self {
            Kind::Dropped | Kind::Changed => "A3.2.1",
            Kind::Resp => "A6.1",
        }
    }
}

/// A port's rules' severities, by kind.
#[derive(Clone, Copy)]
pub struct ChanRules {
    pub dropped: Severity,
    pub changed: Severity,
    pub resp: Severity,
}

impl ChanRules {
    pub fn severity(&self, kind: Kind) -> Severity {
        match kind {
            Kind::Dropped => self.dropped,
            Kind::Changed => self.changed,
            Kind::Resp => self.resp,
        }
    }
}

/// The channels an instance gives, their records, and what the last cycle left them for the rules
/// that look back.
pub struct Handshakes {
    chans: Vec<Present>,
    rules: ChanRules,
}

/// One channel the instance gives: its two handshake roles and the payload it has.
struct Present {
    chan: &'static Chan,
    valid: String,
    ready: String,
    /// The response role, when the port gives it.
    resp: Option<String>,
    /// (the record's field, the role).
    payload: Vec<(&'static str, String)>,
    fields: Vec<Field>,
    records: Vec<stream::Record>,
    // the cycle before, for the rules that look back
    then_valid: Option<bool>,
    then_ready: Option<bool>,
    then: Vec<Option<Val>>,
}

impl Present {
    /// The channel has that rule: `changed` needs a payload, `resp` a response role.
    fn has(&self, kind: Kind) -> bool {
        match kind {
            Kind::Dropped => true,
            Kind::Changed => !self.payload.is_empty(),
            Kind::Resp => self.resp.is_some(),
        }
    }
}

impl Handshakes {
    /// The handshakes of the channels `given` has both a valid and a ready for, in `chans` order.
    pub fn new(given: &Given, chans: &'static [Chan], rules: ChanRules) -> Self {
        let has = |r: &str| given.iter().any(|g| g.0 == r);
        let width = |r: &str| given.iter().find(|g| g.0 == r).and_then(|g| g.1);
        let chans = chans
            .iter()
            .filter(|c| has(&format!("{}valid", c.name)) && has(&format!("{}ready", c.name)))
            .map(|c| {
                let payload: Vec<(&'static str, String)> = c
                    .payload
                    .iter()
                    .filter(|p| has(p.1))
                    .map(|&(f, r)| (f, r.into()))
                    .collect();
                let fields: Vec<Field> = payload
                    .iter()
                    .map(|(f, r)| Field {
                        name: f.to_string(),
                        width: width(r),
                        time: false,
                        signed: false,
                    })
                    .collect();
                let then = vec![None; payload.len()];
                Present {
                    chan: c,
                    valid: format!("{}valid", c.name),
                    ready: format!("{}ready", c.name),
                    resp: has(&format!("{}resp", c.name)).then(|| format!("{}resp", c.name)),
                    payload,
                    fields,
                    records: Vec::new(),
                    then_valid: None,
                    then_ready: None,
                    then,
                }
            })
            .collect();
        Handshakes { chans, rules }
    }

    fn present(&self, chan: &str) -> Option<&Present> {
        self.chans.iter().find(|p| p.chan.name == chan)
    }

    /// A channel's stream: a record per handshake, the payload given.
    pub fn desc(&self, chan: &str) -> Option<StreamDesc> {
        let p = self.present(chan)?;
        let from: Vec<String> = p
            .payload
            .iter()
            .filter(|(f, r)| f != r)
            .map(|(f, r)| format!("{f} = {r}"))
            .collect();
        let about = format!(
            "a record per cycle where {} && {}{}",
            p.valid,
            p.ready,
            if from.is_empty() {
                String::new()
            } else {
                format!("; {}", from.join(", "))
            }
        );
        Some(StreamDesc {
            name: p.chan.name,
            fields: p.fields.clone(),
            violations: false,
            from: &[],
            about: vec![about],
        })
    }

    /// The rules' stream: a record per cycle where one holds, `rule`, `detail` and `severity`.
    pub fn errors(&self) -> StreamDesc {
        let text = |n: &str| Field {
            name: n.into(),
            width: None,
            time: false,
            signed: false,
        };
        let mut about = vec!["a record per cycle where a rule holds, rule naming it:".to_string()];
        for p in &self.chans {
            for kind in Kind::ALL.into_iter().filter(|k| p.has(*k)) {
                about.push(format!(
                    "  {}: {} ({}, spec {})",
                    p.chan.rules[kind.at()],
                    kind.about(),
                    self.rules.severity(kind).text(),
                    kind.spec()
                ));
            }
        }
        StreamDesc {
            name: "errors",
            fields: vec![text("rule"), text("detail"), text("severity")],
            violations: true,
            from: &[],
            about,
        }
    }

    /// A cycle's violations and handshakes, channel by channel.
    pub fn feed(&mut self, start: Tick, end: Tick, roles: &Roles<'_>) -> Vec<Out> {
        let rules = self.rules;
        let mut out = Vec::new();
        for p in &mut self.chans {
            let valid = roles.bit(&p.valid);
            let ready = roles.bit(&p.ready);
            // a valid standing since before, with no ready then
            let stalled = p.then_valid == Some(true) && p.then_ready == Some(false);
            if stalled && valid == Some(false) {
                out.push(violation(
                    rules,
                    p,
                    Kind::Dropped,
                    start,
                    end,
                    format!("{} fell before {}", p.valid, p.ready),
                ));
            }
            let now: Vec<Option<Val>> = p
                .payload
                .iter()
                .map(|(_, r)| roles.get(r).cloned())
                .collect();
            if stalled
                && valid == Some(true)
                && let Some(i) = (0..now.len()).find(|&i| now[i] != p.then[i])
            {
                out.push(violation(
                    rules,
                    p,
                    Kind::Changed,
                    start,
                    end,
                    format!("{} changed while {} stalled", p.payload[i].1, p.valid),
                ));
            }
            if let Some(resp) = &p.resp
                && valid == Some(true)
                && ready == Some(true)
                && roles.get(resp).is_some_and(not_okay)
            {
                out.push(violation(
                    rules,
                    p,
                    Kind::Resp,
                    start,
                    end,
                    format!("{resp} is not OKAY"),
                ));
            }
            if valid == Some(true) && ready == Some(true) {
                p.records.push(stream::Record {
                    start,
                    end,
                    fields: now.clone(),
                });
                out.push(Out::Record {
                    stream: p.chan.name,
                    start,
                    end,
                    fields: now.clone(),
                });
            }
            p.then_valid = valid;
            p.then_ready = ready;
            p.then = now;
        }
        out
    }

    /// The transactions of `requests`, answered by `response`, and the unmatched ones, as the
    /// records of `name` and `unmatched`.
    pub fn join(
        &self,
        name: &'static str,
        unmatched: &'static str,
        requests: &[&'static str],
        response: &'static str,
        timeout: Option<Tick>,
    ) -> Result<Vec<Out>, String> {
        let stream = |chan: &str| {
            let p = self
                .present(chan)
                .ok_or_else(|| format!("no {chan} channel given"))?;
            Ok(Stream {
                name: p.chan.name.into(),
                schema: p.fields.clone(),
                records: p.records.clone(),
                violations: false,
                inputs: Vec::new(),
            })
        };
        let response = stream(response)?;
        let requests: Vec<(&str, Stream)> = requests
            .iter()
            .map(|&c| stream(c).map(|s| (c, s)))
            .collect::<Result<_, String>>()?;
        let reqs: Vec<(&str, &Stream)> = requests.iter().map(|(c, s)| (*c, s)).collect();
        let (done, rest) = decode::join(name, &reqs, &response, None, timeout)?;
        let mut out = from_stream(name, &done);
        out.extend(from_stream(unmatched, &rest));
        Ok(out)
    }
}

/// A violation of one of a channel's rules.
fn violation(
    rules: ChanRules,
    p: &Present,
    kind: Kind,
    start: Tick,
    end: Tick,
    detail: String,
) -> Out {
    Out::Violation {
        rule: p.chan.rules[kind.at()],
        start,
        end,
        detail,
        severity: rules.severity(kind),
    }
}

/// A duration the instance gives a tick for, as `timeout=`: how long a request waits for its
/// response.
pub fn timeout(params: &Params, fpt: Option<u128>) -> Result<Option<Tick>, String> {
    match params.iter().find(|p| p.0 == "timeout") {
        Some((_, v)) => crate::time::parse(v, fpt)
            .map(|t| Some(t.round() as Tick))
            .map_err(|e| format!("join: timeout: {e}")),
        None => Ok(None),
    }
}

/// A join's stream as `Out`s, its records under the name they take.
pub fn from_stream(name: &'static str, s: &Stream) -> Vec<Out> {
    s.records
        .iter()
        .map(|r| Out::Record {
            stream: name,
            start: r.start,
            end: r.end,
            fields: r.fields.clone(),
        })
        .collect()
}

/// A response that is not OKAY: any bit set (a response of x or z counts too).
fn not_okay(v: &Val) -> bool {
    matches!(v, Val::Bits(b) if b.bytes().any(|c| matches!(c, b'1' | b'h')))
}
