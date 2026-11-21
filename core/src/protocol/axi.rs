//! AXI channel handshakes, shared by AXI4-Lite and AXI4 (spec §16): a record per channel handshake,
//! the handshake's rules, and each side's joined into a transaction.

use std::collections::{HashMap, VecDeque};

use crate::decode;
use crate::protocol::{
    Default, Field, Given, Out, Params, RoleDef, Roles, Severity, StreamDesc, bit_of,
};
use crate::stream::{self, Record, Stream};
use crate::time::Tick;
use crate::trace::Val;

/// A channel of an AXI port: the stem of its roles (`aw`, `w`, `b`, `ar`, `r`), the fields its
/// handshakes record (the field, then the role it samples), and the names of its rules, element by
/// element of its port's `RuleKind`s.
pub struct Chan {
    pub name: &'static str,
    pub payload: &'static [(&'static str, &'static str)],
    pub rules: &'static [&'static str],
}

/// A rule a port's `errors` stream can hold: its name after the channel's (`aw_dropped` is
/// `dropped`), what it means, the clause of IHI 0022 it comes from, and how sure that is.
pub struct RuleKind {
    pub suffix: &'static str,
    pub about: &'static str,
    pub spec: &'static str,
    pub severity: Severity,
}

/// The channels an instance gives, their records, and what the last cycle left them for the rules
/// that look back.
pub struct Handshakes {
    chans: Vec<Present>,
    rules: &'static [RuleKind],
    /// The reset role, when the port has one: no handshake counts in it, nor in the cycle after.
    reset: Option<String>,
    /// The reset is active low (`reset_active=low`), the spec's default.
    reset_low: bool,
    in_reset: bool,
    /// AXI4: `early` and `exokay` are decided after the join, so `feed` says nothing of them.
    defer: bool,
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
    /// The handshakes recorded so far: a response's ordinal, which its request's must have
    /// reached (A2.3). An early response is not one: it answers no request.
    beats: u64,
    /// The starts of the early responses' handshakes: the join's `no request` rows that `errors`
    /// already says.
    early: Vec<Tick>,
    // the cycle before, for the rules that look back
    then_valid: Option<bool>,
    then_ready: Option<bool>,
    then: Vec<Option<Val>>,
}

impl Present {
    /// The channel has that rule: it lists it, and `changed` and `payload_unknown` need a
    /// payload, `resp` and `exokay` a response role. A rule the port does not list never fires.
    fn has(&self, suffix: &str) -> bool {
        self.rule(suffix).is_some()
            && match suffix {
                "changed" | "payload_unknown" => !self.payload.is_empty(),
                "resp" | "exokay" => self.resp.is_some(),
                _ => true,
            }
    }

    /// The rule's name on this channel (`aw_dropped` for `dropped`), when it lists it: the whole
    /// name, so `unknown` is not `payload_unknown`.
    fn rule(&self, suffix: &str) -> Option<&'static str> {
        self.chan.rules.iter().copied().find(|r| {
            r.strip_prefix(self.chan.name)
                .and_then(|r| r.strip_prefix('_'))
                == Some(suffix)
        })
    }
}

impl Handshakes {
    /// The handshakes of the channels `given` has both a valid and a ready for, in `chans` order.
    /// A role the instance has no signal for is there when the spec gives it a value (`roles`, the
    /// protocol's): its default plays it, at the default's width.
    /// With `aresetn`, no channel counts while the reset is active (`reset_low`: active low).
    pub fn new(
        given: &Given,
        chans: &'static [Chan],
        rules: &'static [RuleKind],
        roles: &[RoleDef],
        reset_low: bool,
    ) -> Self {
        let def = |r: &str| roles.iter().find(|d| d.name == r);
        let signal = |r: &str| given.iter().find(|g| g.0 == r).and_then(|g| g.1);
        let has =
            |r: &str| given.iter().any(|g| g.0 == r) || def(r).is_some_and(|d| d.default.is_some());
        let width = |r: &str| {
            signal(r).or_else(|| match def(r)?.default? {
                Default::Bits(b) => Some(b.len() as u32),
                Default::Bytes(src) => Some(signal(src)?.div_ceil(8)),
                Default::Size(_) => Some(3),
                Default::Zero | Default::Ones | Default::Same(_) => None,
            })
        };
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
                    beats: 0,
                    early: Vec::new(),
                    then_valid: None,
                    then_ready: None,
                    then,
                }
            })
            .collect();
        Handshakes {
            chans,
            rules,
            reset: has("aresetn").then(|| "aresetn".to_string()),
            reset_low,
            in_reset: false,
            defer: false,
        }
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
        // a channel lists the reset rule, but only a port with a reset signal can break it
        let rules: Vec<&RuleKind> = self
            .rules
            .iter()
            .filter(|k| k.suffix != "reset" || self.reset.is_some())
            .collect();
        for p in &self.chans {
            for kind in rules.iter().filter(|k| p.has(k.suffix)) {
                if let Some(rule) = p.rule(kind.suffix) {
                    about.push(format!(
                        "  {rule}: {} ({}, spec {})",
                        kind.about,
                        kind.severity.text(),
                        kind.spec
                    ));
                }
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
        // a port whose responses carry EXOKAY splits SLVERR and DECERR from it
        let split = rules.iter().any(|k| k.suffix == "exokay");
        let defer = self.defer;
        let mut out = Vec::new();
        // a port with a reset: a valid in it, or in the cycle it is released in, is the error, and
        // nothing is recorded (A2.1.2); an x on it is out of reset
        if let Some(reset) = &self.reset {
            let in_reset = roles.bit(reset).is_some_and(|high| high != self.reset_low);
            if in_reset || self.in_reset {
                for p in &mut self.chans {
                    let valid = roles.bit(&p.valid);
                    let ready = roles.bit(&p.ready);
                    let now: Vec<Option<Val>> = p
                        .payload
                        .iter()
                        .map(|(_, r)| roles.get(r).cloned())
                        .collect();
                    if valid == Some(true) {
                        out.push(violation(
                            rules,
                            p,
                            "reset",
                            start,
                            end,
                            if in_reset {
                                format!("{} in reset", p.valid)
                            } else {
                                format!("{} in the cycle reset is released", p.valid)
                            },
                        ));
                    }
                    // nothing stands through a reset: the cycle it is released in starts afresh
                    if in_reset {
                        p.then_valid = None;
                        p.then_ready = None;
                        p.then = vec![None; now.len()];
                    } else {
                        p.then_valid = valid;
                        p.then_ready = ready;
                        p.then = now;
                    }
                }
                self.in_reset = in_reset;
                return out;
            }
        }
        // the handshakes each channel had recorded before this cycle: a response may not come
        // before its request's (A2.3), and the requests of this cycle are not there yet
        let count = |chan: &str| self.present(chan).map_or(0, |p| p.beats);
        let (aw, w, b, ar, r) = (count("aw"), count("w"), count("b"), count("ar"), count("r"));
        for p in &mut self.chans {
            let valid = roles.bit(&p.valid);
            let ready = roles.bit(&p.ready);
            // an x or z says nothing: it is not also a valid that fell, nor a payload that moved
            // (A2.3 asks for every signal to be defined when it is sampled, and out of reset)
            for role in [&p.valid, &p.ready] {
                if p.has("unknown") && roles.get(role).is_some_and(unknown) {
                    out.push(violation(
                        rules,
                        p,
                        "unknown",
                        start,
                        end,
                        format!("{role} is x or z out of reset"),
                    ));
                }
            }
            // a payload of x is one row per cycle, not one per signal: the first one names it
            if valid == Some(true)
                && p.has("payload_unknown")
                && let Some((_, role)) = p
                    .payload
                    .iter()
                    .find(|(_, r)| roles.get(r).is_some_and(unknown))
            {
                out.push(violation(
                    rules,
                    p,
                    "payload_unknown",
                    start,
                    end,
                    format!("{role} is x or z while {}", p.valid),
                ));
            }
            // a valid standing since before, with no ready then
            let stalled = p.then_valid == Some(true) && p.then_ready == Some(false);
            if stalled && valid == Some(false) {
                out.push(violation(
                    rules,
                    p,
                    "dropped",
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
                    "changed",
                    start,
                    end,
                    format!("{} changed while {} stalled", p.payload[i].1, p.valid),
                ));
            }
            if let Some(resp) = &p.resp
                && valid == Some(true)
                && ready == Some(true)
                && let Some(v) = roles.get(resp)
            {
                if split && is_resp_err(v) {
                    out.push(violation(
                        rules,
                        p,
                        "resp",
                        start,
                        end,
                        format!("{resp} is SLVERR or DECERR"),
                    ));
                } else if !defer && split && is_exokay(v) {
                    out.push(violation(
                        rules,
                        p,
                        "exokay",
                        start,
                        end,
                        format!("{resp} is EXOKAY"),
                    ));
                } else if !split && not_okay(v) {
                    out.push(violation(
                        rules,
                        p,
                        "resp",
                        start,
                        end,
                        format!("{resp} is not OKAY"),
                    ));
                }
            }
            // a response before its request's handshakes: a B needs an AW and a W, an R an AR,
            // recorded before this cycle (A2.3.2). It is checked when the response begins, so a
            // valid raised too early is one even when its handshake waits for them
            // AXI4's `early` is decided once its requests are paired: `join_streams` says it
            let early = !defer
                && p.has("early")
                && match p.chan.name {
                    "b" => b >= aw.min(w),
                    "r" => r >= ar,
                    _ => false,
                };
            if early && valid == Some(true) && !stalled {
                let detail = if p.chan.name == "b" {
                    format!(
                        "response {} before write {}'s AW and W",
                        b + 1,
                        aw.min(w) + 1
                    )
                } else {
                    format!("response {} before read {}'s AR", r + 1, ar + 1)
                };
                out.push(violation(rules, p, "early", start, end, detail));
            }
            if valid == Some(true) && ready == Some(true) {
                // a response still early at its handshake answers no request: not counted, so
                // the next one answers the oldest request, as the join pairs them
                if early {
                    p.early.push(start);
                } else {
                    p.beats += 1;
                }
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
        let early: &[Tick] = self.present(response).map_or(&[], |p| &p.early);
        let response = stream(response)?;
        let requests: Vec<(&str, Stream)> = requests
            .iter()
            .map(|&c| stream(c).map(|s| (c, s)))
            .collect::<Result<_, String>>()?;
        let reqs: Vec<(&str, &Stream)> = requests.iter().map(|(c, s)| (*c, s)).collect();
        let (done, mut rest) = decode::join(name, &reqs, &response, None, timeout)?;
        // an early response is `b_early` or `r_early` in errors, so its `no request` row would say
        // it twice; a late one, after `timeout`, is said here alone. `early` is in time order, as
        // `feed` pushed it
        rest.records.retain(|q| {
            !(early.binary_search(&q.start).is_ok()
                && matches!(q.fields.first(), Some(Some(Val::Text(k))) if k == "no request"))
        });
        let mut out = from_stream(name, &done);
        out.extend(from_stream(unmatched, &rest));
        Ok(out)
    }

    /// AXI4: `early` and `exokay` are decided after the join, on the transactions it pairs, so
    /// `feed` never says them.
    pub fn defer_to_join(&mut self) {
        self.defer = true;
    }

    /// A violation of one of the port's rules, made after the cycle: AXI4's `early` is the join's,
    /// on the response it pairs with no request.
    pub fn finish_violation(
        &self,
        channel: &str,
        suffix: &str,
        start: Tick,
        end: Tick,
        detail: String,
    ) -> Out {
        let p = self
            .present(channel)
            .expect("the channel's rules are its own");
        violation(self.rules, p, suffix, start, end, detail)
    }

    /// An AXI4 side's transactions, its early responses told from its late ones: those of
    /// `requests`, answered by `response`, with a response that comes before its request's parts
    /// started as `early_chan`'s `early` rule (A2.3.2), and one that comes after its request
    /// expired under `timeout` as `timeout`, never `no request` (a late response is not an early
    /// one, and says which it is).
    /// A request is complete when all its parts started, and with a `burst`, when the burst ended
    /// too: a response in its last beat's cycle waits for it rather than being early.
    #[allow(clippy::too_many_arguments)]
    pub fn join_streams(
        &self,
        name: &'static str,
        unmatched: &'static str,
        requests: &[(&'static str, &Stream)],
        response: &Stream,
        id: Option<(&'static str, &'static str, &'static str)>,
        burst: Option<&str>,
        early_chan: &'static str,
        timeout: Option<Tick>,
    ) -> Result<Vec<Out>, String> {
        let n = requests
            .iter()
            .map(|r| r.1.records.len())
            .min()
            .unwrap_or(0);
        let parts = |k: usize| requests.iter().map(move |r| &r.1.records[k]);
        let complete = |k: usize| parts(k).map(|q| q.start).max().unwrap_or(0);
        // a response answers a request that started before it, and whose burst has ended: a B in
        // its burst's last beat's cycle is not early
        let ready = |k: usize, start: Tick| {
            parts(k).all(|q| q.start < start)
                && burst.is_none_or(|b| {
                    requests
                        .iter()
                        .find(|r| r.0 == b)
                        .is_none_or(|r| r.1.records[k].end <= start)
                })
        };
        // a response takes its request by the ID both name
        let key = match id {
            Some((label, field, rfield)) => {
                let i = requests
                    .iter()
                    .position(|r| r.0 == label)
                    .ok_or_else(|| format!("join: no {label} channel given"))?;
                let q = requests[i]
                    .1
                    .schema
                    .iter()
                    .position(|f| f.name == field)
                    .ok_or_else(|| format!("join: {label} has no field {field}"))?;
                let r = response
                    .schema
                    .iter()
                    .position(|f| f.name == rfield)
                    .ok_or_else(|| format!("join: {} has no field {rfield}", response.name))?;
                Some((i, q, r))
            }
            None => None,
        };
        let req_key =
            |k: usize| key.and_then(|(i, q, _)| requests[i].1.records[k].fields[q].clone());
        let resp_key = |r: &Record| key.and_then(|(_, _, i)| r.fields[i].clone());
        // the requests of a key, in order: the front is the one a response answers first
        let mut waiting: HashMap<Option<Val>, VecDeque<usize>> = HashMap::new();
        // how many of a key are done (`expired` or answered): a response with one and no front is
        // late, not early
        let mut answered: HashMap<Option<Val>, usize> = HashMap::new();
        // the ordinal of each request among those of its key: what an early response names
        let mut ordinals = vec![0usize; n];
        let mut seen: HashMap<Option<Val>, usize> = HashMap::new();
        for (k, ordinal) in ordinals.iter_mut().enumerate() {
            let count = seen.entry(req_key(k)).or_default();
            *count += 1;
            *ordinal = *count;
        }
        // how many of a key were said early
        let mut said: HashMap<Option<Val>, usize> = HashMap::new();
        let mut next = 0;
        // which responses were said early, by index
        let mut early = vec![false; response.records.len()];
        let mut out = Vec::new();
        for (i, r) in response.records.iter().enumerate() {
            while next < n && complete(next) < r.start {
                waiting.entry(req_key(next)).or_default().push_back(next);
                next += 1;
            }
            // the join's expiry: a request that waited more than `timeout` is gone, and its
            // response comes late rather than early
            if let Some(t) = timeout {
                for (k, w) in &mut waiting {
                    while w
                        .front()
                        .is_some_and(|&f| r.start.saturating_sub(complete(f)) > t)
                    {
                        w.pop_front();
                        *answered.entry(k.clone()).or_default() += 1;
                    }
                }
            }
            let k = resp_key(r);
            let front = waiting.get(&k).and_then(|w| w.front().copied());
            let before = match front {
                Some(f) if ready(f, r.start) => {
                    if let Some(w) = waiting.get_mut(&k) {
                        w.pop_front();
                    }
                    *answered.entry(k.clone()).or_default() += 1;
                    None
                }
                // the front request's burst has not ended: the response is before it
                Some(f) => Some(ordinals[f]),
                // nothing waits: the one it would answer expired, or never came
                None if answered.get(&k).copied().unwrap_or(0) > 0 => None,
                None => Some(answered.get(&k).copied().unwrap_or(0) + 1),
            };
            let Some(ord) = before else { continue };
            let m = {
                let count = said.entry(k).or_default();
                *count += 1;
                *count
            };
            let detail = if name == "wr" {
                format!("response {m} before write {ord}'s AW and W")
            } else {
                format!("response {m} before read {ord}'s AR")
            };
            early[i] = true;
            out.push(self.finish_violation(early_chan, "early", r.start, r.end, detail));
        }
        // the rest, joined as `decode::join` does: no early response is left to take a request it
        // does not answer, so a `no request` left is a late one
        let rest = Stream {
            name: response.name.clone(),
            schema: response.schema.clone(),
            records: response
                .records
                .iter()
                .enumerate()
                .filter(|(i, _)| !early[*i])
                .map(|(_, r)| r.clone())
                .collect(),
            violations: response.violations,
            inputs: response.inputs.clone(),
        };
        let (done, mut left) = decode::join(name, requests, &rest, id, timeout)?;
        for r in &mut left.records {
            if matches!(r.fields.first(), Some(Some(Val::Text(k))) if k == "no request") {
                r.fields[0] = Some(Val::Text("timeout".into()));
            }
        }
        out.extend(from_stream(name, &done));
        out.extend(from_stream(unmatched, &left));
        Ok(out)
    }
}

/// A violation of one of the port's rules: its name on the channel, its severity from the port's
/// table.
fn violation(
    rules: &'static [RuleKind],
    p: &Present,
    suffix: &str,
    start: Tick,
    end: Tick,
    detail: String,
) -> Out {
    let kind = rules.iter().find(|k| k.suffix == suffix);
    Out::Violation {
        // every rule a port fires is in the channel's list: a missing one is a bug
        rule: kind
            .and_then(|k| p.rule(k.suffix))
            .expect("the channel lists its rules"),
        start,
        end,
        detail,
        severity: kind.map_or(Severity::Error, |k| k.severity),
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

/// Whether a signal's value is x, z or an unset bit: it says nothing, so no rule reads a meaning
/// into it.
pub(crate) fn unknown(v: &Val) -> bool {
    matches!(v, Val::Bits(b) if b.contains(['x', 'z', 'u', 'w', '-']))
}

/// A response's two bits, from the lowest: `None` when either is x or z.
fn resp_bits(v: &Val) -> Option<(bool, bool)> {
    let Val::Bits(b) = v else { return None };
    Some((bit_of(b, 0)?, bit_of(b, 1)?))
}

/// A response of EXOKAY (`2'b01`): exclusive access, which AXI4-Lite has none of and AXI4 allows
/// only when the lock asked for one.
pub(crate) fn is_exokay(v: &Val) -> bool {
    resp_bits(v) == Some((true, false))
}

/// A response of SLVERR or DECERR (`2'b10` or `2'b11`).
fn is_resp_err(v: &Val) -> bool {
    matches!(resp_bits(v), Some((_, true)))
}
