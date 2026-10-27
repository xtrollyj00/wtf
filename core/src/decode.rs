//! Stateful decoders (spec §16): read a stream's records in time order, keep state,
//! and make records of their own, so protocols stack (`axis.beat -> axis.frame`). Each is a
//! `Decoder`, made by name by `make` (the registry).

use crate::stream::{Field, Record, Stream};
use crate::time::{self, Tick};
use crate::trace::Val;

pub trait Decoder {
    /// The fields of the records it makes.
    fn schema(&self) -> Vec<Field>;
    /// The next input record, in time order; the records it finishes.
    fn feed(&mut self, r: &Record) -> Vec<Record>;
    /// The input ended; the records still open.
    fn finish(&mut self) -> Vec<Record>;
    /// Its records are violations (drawn red).
    fn violations(&self) -> bool {
        false
    }
}

/// The decoders by name. `params` (name, value) set one up on a stream with fields `input`; all
/// the mistakes in them are reported at once.
pub fn make(name: &str, input: &[Field], params: &[(String, String)], fs_per_tick: Option<u128>) -> Result<Box<dyn Decoder>, String> {
    match name {
        "frame" => Frame::new(input, params, fs_per_tick).map(|f| Box::new(f) as Box<dyn Decoder>),
        "spi" => Spi::new(input, params).map(|f| Box::new(f) as Box<dyn Decoder>),
        "uart" => Uart::new(params, fs_per_tick).map(|f| Box::new(f) as Box<dyn Decoder>),
        _ => Err(format!("unknown decoder: {name} (decoders: frame, spi, uart)")),
    }
}

/// Feed a whole stream through a decoder: the stream `name` it makes.
pub fn run(name: &str, input: &Stream, mut d: Box<dyn Decoder>) -> Stream {
    let mut records: Vec<Record> = input.records.iter().flat_map(|r| d.feed(r)).collect();
    records.extend(d.finish());
    records.sort_by_key(|r| r.start);
    Stream { name: name.to_string(), schema: d.schema(), records, violations: d.violations() }
}

/// A duration, as a `time` field holds it: 64 bits of ticks.
fn ticks(t: Tick) -> Option<Val> {
    bits(t, 64)
}

fn bits(n: u64, w: u32) -> Option<Val> {
    Some(Val::Bits(format!("{n:0w$b}", w = w as usize)))
}

/// As in an `if`: some bit is 1 and none is unknown.
fn truthy(v: &Option<Val>) -> bool {
    matches!(v, Some(Val::Bits(b)) if b.contains(['1', 'h']) && !b.contains(['x', 'u', 'w', '-', 'z']))
}

/// `frame`: consecutive records grouped into one, from the first's start to the last's end. A frame
/// ends at a record whose `last` field is 1, before a record starting more than `gap` after the
/// previous one ended (`gap = 0`: records back to back), and before a record whose `select` field
/// differs. Its fields: `beats`, the records in it; `select`'s value, if it has one; `complete`, 0 if
/// the input ended before the frame did. With `bytes` (a field whose 1 bits each count a byte, as
/// AXIS tkeep, or a number of bytes per record) also `len`, the bytes in it. With `data` (a field)
/// also `data`, the frame's bytes as hex text (`de ad be ef`), each record's lowest byte first; with
/// `bytes` a field, only the bytes its 1 bits mark. With `key` (a field), records of each value of it
/// make frames of their own, built side by side (AXI4 read bursts, interleaved by ID), and the
/// frame has that field.
pub struct Frame {
    last: Option<usize>,
    gap: Option<Tick>,
    select: Option<(usize, Field)>,
    key: Option<(usize, Field)>,
    bytes: Option<Bytes>,
    data: Option<usize>,
    /// The frames being built: one, or with `key` one per value.
    open: Vec<Open>,
}

struct Open {
    key: Option<Val>,
    /// The first record's start, the last's end.
    start: Tick,
    end: Tick,
    beats: u64,
    select: Option<Val>,
    bytes: u64,
    /// As hex, `de`.
    data: Vec<String>,
}

enum Bytes {
    Ones(usize),
    Each(u64),
}

impl Frame {
    fn new(input: &[Field], params: &[(String, String)], fs_per_tick: Option<u128>) -> Result<Self, String> {
        let mut errors = Vec::new();
        let field = |name: &str, errors: &mut Vec<String>| {
            let i = input.iter().position(|f| f.name == name);
            if i.is_none() {
                let names: Vec<&str> = input.iter().map(|f| f.name.as_str()).collect();
                errors.push(format!("frame: no field {name} (fields: {})", names.join(", ")));
            }
            i
        };
        let mut f = Frame { last: None, gap: None, select: None, key: None, bytes: None, data: None, open: Vec::new() };
        for (k, v) in params {
            match k.as_str() {
                "last" => f.last = field(v, &mut errors),
                "select" => f.select = field(v, &mut errors).map(|i| (i, Field { name: v.clone(), width: input[i].width, time: input[i].time, signed: input[i].signed })),
                "key" => f.key = field(v, &mut errors).map(|i| (i, Field { name: v.clone(), width: input[i].width, time: input[i].time, signed: input[i].signed })),
                "gap" => match time::parse(v, fs_per_tick) {
                    Ok(t) => f.gap = Some(t.round() as Tick),
                    Err(e) => errors.push(format!("frame: gap: {e}")),
                },
                "data" => f.data = field(v, &mut errors),
                "bytes" => match v.parse() {
                    Ok(n) => f.bytes = Some(Bytes::Each(n)),
                    Err(_) => f.bytes = field(v, &mut errors).map(Bytes::Ones),
                },
                _ => errors.push(format!("frame: no parameter {k} (last, gap, select, key, bytes, data)")),
            }
        }
        if params.is_empty() {
            errors.push("frame: needs last, gap or select".into());
        }
        if errors.is_empty() { Ok(f) } else { Err(errors.join("; ")) }
    }

    fn close(&mut self, i: usize, complete: bool) -> Record {
        let Open { key, start, end, beats, select, bytes, data } = self.open.remove(i);
        let mut fields = vec![bits(beats, 32)];
        if self.select.is_some() {
            fields.push(select);
        }
        if self.key.is_some() {
            fields.push(key);
        }
        if self.bytes.is_some() {
            fields.push(bits(bytes, 32));
        }
        if self.data.is_some() {
            fields.push(Some(Val::Text(data.join(" "))));
        }
        fields.push(bits(complete as u64, 1));
        Record { start, end, fields }
    }
}

impl Decoder for Frame {
    fn schema(&self) -> Vec<Field> {
        let mut out = vec![Field { name: "beats".into(), width: Some(32), time: false, signed: false }];
        out.extend(self.select.as_ref().map(|s| s.1.clone()));
        out.extend(self.key.as_ref().map(|s| s.1.clone()));
        out.extend(self.bytes.as_ref().map(|_| Field { name: "len".into(), width: Some(32), time: false, signed: false }));
        out.extend(self.data.map(|_| Field { name: "data".into(), width: None, time: false, signed: false }));
        out.push(Field { name: "complete".into(), width: Some(1), time: false, signed: false });
        out
    }

    fn feed(&mut self, r: &Record) -> Vec<Record> {
        let mut out = Vec::new();
        let sel = self.select.as_ref().and_then(|s| r.fields[s.0].clone());
        let key = self.key.as_ref().and_then(|k| r.fields[k.0].clone());
        // a gap or another select ends the open frame (of this key) before this record
        let mut i = self.open.iter().position(|o| o.key == key);
        if let Some(k) = i
            && (self.gap.is_some_and(|g| r.start.saturating_sub(self.open[k].end) > g) || (self.select.is_some() && self.open[k].select != sel))
        {
            out.push(self.close(k, true));
            i = None;
        }
        let bytes = match self.bytes {
            Some(Bytes::Each(n)) => n,
            Some(Bytes::Ones(i)) => match &r.fields[i] {
                Some(Val::Bits(b)) => b.chars().filter(|&c| c == '1' || c == 'h').count() as u64,
                _ => 0,
            },
            None => 0,
        };
        let data = self.data.map_or(Vec::new(), |i| {
            let keep = match self.bytes {
                Some(Bytes::Ones(k)) => r.fields[k].clone(),
                _ => None,
            };
            data_bytes(&r.fields[i], keep.as_ref())
        });
        let i = i.unwrap_or_else(|| {
            self.open.push(Open { key, start: r.start, end: r.end, beats: 0, select: sel, bytes: 0, data: Vec::new() });
            self.open.len() - 1
        });
        let open = &mut self.open[i];
        (open.end, open.beats, open.bytes) = (r.end, open.beats + 1, open.bytes + bytes);
        open.data.extend(data);
        if self.last.is_some_and(|l| truthy(&r.fields[l])) {
            out.push(self.close(i, true));
        }
        out
    }

    fn finish(&mut self) -> Vec<Record> {
        (0..self.open.len()).map(|_| self.close(0, false)).collect()
    }
}

/// A value's bytes as hex, the lowest first (`xx` where a bit is unknown); with `keep`, only those
/// whose bit in it is 1.
fn data_bytes(v: &Option<Val>, keep: Option<&Val>) -> Vec<String> {
    let Some(Val::Bits(b)) = v else { return Vec::new() };
    let b = format!("{}{b}", "0".repeat((8 - b.len() % 8) % 8));
    let kept = |k: usize| match keep {
        Some(Val::Bits(m)) => m.len() > k && matches!(m.as_bytes()[m.len() - 1 - k], b'1' | b'h'),
        _ => true,
    };
    (0..b.len() / 8)
        .filter(|&k| kept(k))
        .map(|k| {
            let byte = &b[b.len() - 8 * (k + 1)..b.len() - 8 * k];
            u8::from_str_radix(byte, 2).map_or("xx".into(), |n| format!("{n:02x}"))
        })
        .collect()
}

/// `join`: transactions made of several request streams answered by one, in order (AXI-Lite: AW
/// and W, then B). Request k is the k-th record of each request stream, complete with the last of
/// them; each response answers the oldest complete request before it, with `id` (request stream,
/// its field, the response's field) the oldest with the same id (AXI4's BID). A transaction runs
/// from its first request record's start to the response's end, with `delay` (from the last
/// request record's start to the response's) and the fields of all, in order (a name in more than
/// one stream gets the stream's label: `aw_addr`). The unmatched ones are violations,
/// `<name>.unmatched` with `kind`: `no request`, `no response`, or `no w` for a request missing a
/// part.
pub fn join(name: &str, requests: &[(&str, &Stream)], response: &Stream, id: Option<(&str, &str, &str)>) -> Result<(Stream, Stream), String> {
    let field = |s: &Stream, f: &str| s.schema.iter().position(|x| x.name == f).ok_or(format!("join: {} has no field {f}", s.name));
    let ids = match id {
        Some((label, q, r)) => {
            let (k, s) = requests.iter().enumerate().find(|(_, x)| x.0 == label).map(|(k, x)| (k, x.1)).ok_or(format!("join: no request {label}"))?;
            Some((k, field(s, q)?, field(response, r)?))
        }
        None => None,
    };
    let n = requests.iter().map(|r| r.1.records.len()).min().unwrap_or(0);
    let parts = |k: usize| requests.iter().map(move |r| &r.1.records[k]);
    let complete = |k: usize| parts(k).map(|q| q.start).max().unwrap_or(0);
    let req_key = |k: usize| ids.and_then(|(s, f, _)| requests[s].1.records[k].fields[f].clone());
    let kind = |start, end, k: String| Record { start, end, fields: vec![Some(Val::Text(k))] };
    let mut waiting: std::collections::HashMap<Option<Val>, std::collections::VecDeque<usize>> = Default::default();
    let (mut done, mut unmatched, mut next) = (Vec::new(), Vec::new(), 0);
    for r in &response.records {
        while next < n && complete(next) < r.start {
            waiting.entry(req_key(next)).or_default().push_back(next);
            next += 1;
        }
        let key = ids.and_then(|(_, _, f)| r.fields[f].clone());
        match waiting.get_mut(&key).and_then(|w| w.pop_front()) {
            Some(k) => {
                let delay = ticks(r.start - complete(k));
                let fields = std::iter::once(delay).chain(parts(k).flat_map(|q| q.fields.iter().cloned())).chain(r.fields.iter().cloned()).collect();
                let start = parts(k).map(|q| q.start).min().unwrap_or(r.start);
                done.push(Record { start, end: r.end.max(start), fields });
            }
            None => unmatched.push(kind(r.start, r.end, "no request".into())),
        }
    }
    for k in waiting.into_values().flatten().chain(next..n) {
        let (start, end) = (parts(k).map(|q| q.start).min().unwrap_or(0), parts(k).map(|q| q.end).max().unwrap_or(0));
        unmatched.push(kind(start, end, "no response".into()));
    }
    // parts of requests whose other parts never came
    for (_, s) in requests {
        for (i, q) in s.records.iter().enumerate().skip(n) {
            let missing: Vec<&str> = requests.iter().filter(|r| r.1.records.len() <= i).map(|r| r.0).collect();
            unmatched.push(kind(q.start, q.end, format!("no {}", missing.join(", "))));
        }
    }
    done.sort_by_key(|r| r.start);
    unmatched.sort_by_key(|r| r.start);
    let reqs: Vec<(&str, &[Field])> = requests.iter().map(|(l, s)| (*l, &s.schema[..])).collect();
    let schema = join_schema(&reqs, &response.schema);
    let kinds = vec![Field { name: "kind".into(), width: None, time: false, signed: false }];
    let unmatched = Stream { name: format!("{name}.unmatched"), schema: kinds, records: unmatched, violations: true };
    Ok((Stream { name: name.into(), schema, records: done, violations: false }, unmatched))
}

/// The fields of `join`'s transactions, from its inputs' (label, fields).
pub fn join_schema(requests: &[(&str, &[Field])], response: &[Field]) -> Vec<Field> {
    let inputs: Vec<(&str, &[Field])> = requests.iter().copied().chain([("resp", response)]).collect();
    let names: Vec<&str> = inputs.iter().flat_map(|(_, s)| s.iter().map(|f| f.name.as_str())).collect();
    let mut schema = vec![Field { name: "delay".into(), width: Some(64), time: true, signed: false }];
    for (label, s) in &inputs {
        for f in *s {
            let twice = names.iter().filter(|&&x| x == f.name).count() > 1;
            schema.push(Field { name: if twice { format!("{label}_{}", f.name) } else { f.name.clone() }, width: f.width, time: f.time, signed: f.signed });
        }
    }
    schema
}

/// `pair`: each response with the request it answers, the oldest one waiting (with ids: the oldest
/// one with the same id), which started no later. A pair runs from the request's start to the
/// response's end, with `delay` (from start to start) and the fields of both, `req_` and `resp_`
/// before their names. The unmatched ones are violations, the stream `<name>.unmatched` with `kind`:
/// `no response` or `no request`. `id` names the id field of the requests and of the responses.
pub fn pair(name: &str, req: &Stream, resp: &Stream, id: Option<(&str, &str)>) -> Result<(Stream, Stream), String> {
    let field = |s: &Stream, f: &str| s.schema.iter().position(|x| x.name == f).ok_or(format!("pair: {} has no field {f}", s.name));
    let ids = match id {
        Some((a, b)) => {
            let (a, b) = (field(req, a), field(resp, b));
            match (a, b) {
                (Ok(a), Ok(b)) => Some((a, b)),
                (a, b) => return Err([a.err(), b.err()].into_iter().flatten().collect::<Vec<_>>().join("; ")),
            }
        }
        None => None,
    };
    let key = |r: &Record, i: Option<usize>| i.map_or(String::new(), |i| format!("{:?}", r.fields[i]));
    let mut waiting: std::collections::HashMap<String, std::collections::VecDeque<&Record>> = Default::default();
    let (mut pairs, mut unmatched) = (Vec::new(), Vec::new());
    let mut reqs = req.records.iter().peekable();
    let kind = |r: &Record, k: &str| Record { start: r.start, end: r.end, fields: vec![Some(Val::Text(k.into()))] };
    for r in &resp.records {
        while let Some(q) = reqs.next_if(|q| q.start <= r.start) {
            waiting.entry(key(q, ids.map(|i| i.0))).or_default().push_back(q);
        }
        match waiting.get_mut(&key(r, ids.map(|i| i.1))).and_then(|w| w.pop_front()) {
            Some(q) => {
                let delay = ticks(r.start - q.start);
                let fields = std::iter::once(delay).chain(q.fields.iter().cloned()).chain(r.fields.iter().cloned()).collect();
                pairs.push(Record { start: q.start, end: r.end.max(q.end), fields });
            }
            None => unmatched.push(kind(r, "no request")),
        }
    }
    unmatched.extend(waiting.into_values().flatten().chain(reqs).map(|q| kind(q, "no response")));
    pairs.sort_by_key(|r| r.start);
    unmatched.sort_by_key(|r| r.start);
    let prefixed = |p: &str, s: &Stream| s.schema.iter().map(|f| Field { name: format!("{p}{}", f.name), width: f.width, time: f.time, signed: f.signed }).collect::<Vec<_>>();
    let mut schema = vec![Field { name: "delay".into(), width: Some(64), time: true, signed: false }];
    schema.extend(prefixed("req_", req));
    schema.extend(prefixed("resp_", resp));
    let kind_schema = vec![Field { name: "kind".into(), width: None, time: false, signed: false }];
    let unmatched = Stream { name: format!("{name}.unmatched"), schema: kind_schema, records: unmatched, violations: true };
    Ok((Stream { name: name.into(), schema, records: pairs, violations: false }, unmatched))
}

/// A param's value: the one given, or `default`; `what` says what it must be.
fn param<T: std::str::FromStr>(params: &[(String, String)], name: &str, default: Option<T>, ok: impl Fn(&T) -> bool, what: &str, errors: &mut Vec<String>) -> Option<T> {
    match params.iter().find(|p| p.0 == name) {
        None if default.is_none() => {
            errors.push(format!("needs {name}=<{what}>"));
            None
        }
        None => default,
        Some((_, v)) => match v.parse::<T>().ok().filter(|v| ok(v)) {
            Some(v) => Some(v),
            None => {
                errors.push(format!("{name}={v}: not {what}"));
                None
            }
        },
    }
}

/// Bits, MSB first, as a number's hex digits for `width` bits (`x` digits where a bit isn't 0 or 1).
fn hex(b: &str) -> String {
    let digits = b.len().div_ceil(4);
    match u64::from_str_radix(b, 2) {
        Ok(n) if b.len() <= 64 => format!("{n:0digits$x}"),
        _ => "x".repeat(digits),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SpiOut {
    Bit,
    Word,
    Transfer,
}

/// `spi`: SPI from the changes of `{cs, sclk, mosi}` or `{cs, sclk, mosi, miso}` (the field
/// `value`), cs active low (`cs_active=high`: high). A bit is sampled at each edge of sclk that `mode` (0 - 3, CPOL and CPHA)
/// samples on, rising for modes 0 and 3, falling for 1 and 2, with cs low, from the values just
/// before it; it lasts to the next one. `bits` of them (MSB first, or LSB with `order=lsb`) make a
/// word; a transfer runs from cs falling to cs rising, with its words as hex text. `out` picks the
/// stream: `bit` (mosi, miso), `word` (mosi, miso, complete: 0 if cs rose first) or `transfer`
/// (words, mosi, miso, complete: 0 if the trace ended first).
pub struct Spi {
    out: SpiOut,
    rising: bool,
    bits: usize,
    lsb: bool,
    miso: bool,
    /// cs is active high: read inverted.
    cs_high: bool,
    prev: Option<Vec<u8>>,
    end: Tick,
    /// The bit sampled last, waiting for its end: its start, mosi, miso.
    bit: Option<(Tick, u8, u8)>,
    /// The word being shifted in: its start, and its bits.
    word: Option<(Tick, Vec<(u8, u8)>)>,
    /// The transfer since cs fell: its start, and its words' mosi and miso.
    transfer: Option<(Tick, Vec<String>, Vec<String>)>,
}

impl Spi {
    fn new(input: &[Field], params: &[(String, String)]) -> Result<Self, String> {
        let mut errors = Vec::new();
        let miso = match input.first().and_then(|f| f.width) {
            Some(3) => false,
            Some(4) => true,
            _ => {
                errors.push("spi: the input is {cs, sclk, mosi} or {cs, sclk, mosi, miso}, each 1 bit".into());
                false
            }
        };
        let mode = param(params, "mode", Some(0u8), |m| *m < 4, "0, 1, 2 or 3", &mut errors);
        let bits = param(params, "bits", Some(8usize), |b| (1..=64).contains(b), "1 - 64", &mut errors);
        let order = param(params, "order", Some("msb".to_string()), |o| o == "msb" || o == "lsb", "msb or lsb", &mut errors);
        let cs_active = param(params, "cs_active", Some("low".to_string()), |a| a == "low" || a == "high", "low or high", &mut errors);
        let out = match param(params, "out", None::<String>, |o| ["bit", "word", "transfer"].contains(&o.as_str()), "bit, word or transfer", &mut errors).as_deref() {
            Some("bit") => SpiOut::Bit,
            Some("word") => SpiOut::Word,
            _ => SpiOut::Transfer,
        };
        if let Some((k, _)) = params.iter().find(|p| !["mode", "bits", "order", "cs_active", "out"].contains(&p.0.as_str())) {
            errors.push(format!("no param {k} (mode, bits, order, cs_active)"));
        }
        if !errors.is_empty() {
            return Err(format!("spi: {}", errors.join("; ")));
        }
        let mode = mode.unwrap_or(0);
        Ok(Spi {
            out,
            rising: mode == 0 || mode == 3,
            bits: bits.unwrap_or(8),
            lsb: order.as_deref() == Some("lsb"),
            miso,
            cs_high: cs_active.as_deref() == Some("high"),
            prev: None,
            end: 0,
            bit: None,
            word: None,
            transfer: None,
        })
    }

    /// The sampled bit ends at `t`: shifted into the word, which ends with it once full.
    fn end_bit(&mut self, t: Tick, out: &mut Vec<Record>) {
        let Some((start, mosi, miso)) = self.bit.take() else { return };
        if self.out == SpiOut::Bit {
            let mut fields = vec![Some(Val::Bits((mosi as char).into()))];
            if self.miso {
                fields.push(Some(Val::Bits((miso as char).into())));
            }
            out.push(Record { start, end: t, fields });
        }
        let word = self.word.get_or_insert((start, Vec::new()));
        word.1.push((mosi, miso));
        if word.1.len() == self.bits {
            self.end_word(t, true, out);
        }
    }

    fn end_word(&mut self, t: Tick, complete: bool, out: &mut Vec<Record>) {
        let Some((start, mut bits)) = self.word.take() else { return };
        if self.lsb {
            bits.reverse();
        }
        let line = |k: usize| bits.iter().map(|b| [b.0, b.1][k] as char).collect::<String>();
        let (mosi, miso) = (line(0), line(1));
        if let Some(tr) = &mut self.transfer {
            tr.1.push(hex(&mosi));
            tr.2.push(hex(&miso));
        }
        if self.out == SpiOut::Word {
            let mut fields = vec![Some(Val::Bits(mosi))];
            if self.miso {
                fields.push(Some(Val::Bits(miso)));
            }
            fields.push(bits_val(complete));
            out.push(Record { start, end: t, fields });
        }
    }

    fn end_transfer(&mut self, t: Tick, complete: bool, out: &mut Vec<Record>) {
        let Some((start, mosi, miso)) = self.transfer.take() else { return };
        if self.out == SpiOut::Transfer {
            let mut fields = vec![Some(Val::Bits(format!("{:032b}", mosi.len()))), Some(Val::Text(mosi.join(" ")))];
            if self.miso {
                fields.push(Some(Val::Text(miso.join(" "))));
            }
            fields.push(bits_val(complete));
            out.push(Record { start, end: t, fields });
        }
    }
}

/// A level read the other way round; x and z stay.
fn invert(b: u8) -> u8 {
    match b {
        b'0' => b'1',
        b'1' => b'0',
        b => b,
    }
}

fn bits_val(b: bool) -> Option<Val> {
    Some(Val::Bits(if b { "1" } else { "0" }.into()))
}

impl Decoder for Spi {
    fn schema(&self) -> Vec<Field> {
        let w = match self.out {
            SpiOut::Bit => Some(1),
            SpiOut::Word => Some(self.bits as u32),
            SpiOut::Transfer => None,
        };
        let mut out = Vec::new();
        if self.out == SpiOut::Transfer {
            out.push(Field { name: "words".into(), width: Some(32), time: false, signed: false });
        }
        out.push(Field { name: "mosi".into(), width: w, time: false, signed: false });
        if self.miso {
            out.push(Field { name: "miso".into(), width: w, time: false, signed: false });
        }
        if self.out != SpiOut::Bit {
            out.push(Field { name: "complete".into(), width: Some(1), time: false, signed: false });
        }
        out
    }

    fn feed(&mut self, r: &Record) -> Vec<Record> {
        let mut out = Vec::new();
        let Some(Val::Bits(b)) = &r.fields[0] else { return out };
        // cs, sclk, mosi, miso; a missing miso reads as 0
        let mut v = b.as_bytes().to_vec();
        v.resize(4, b'0');
        if self.cs_high {
            v[0] = invert(v[0]);
        }
        let t = r.start;
        self.end = r.end;
        if let Some(p) = self.prev.replace(v.clone()) {
            let edge = if self.rising { (p[1], v[1]) == (b'0', b'1') } else { (p[1], v[1]) == (b'1', b'0') };
            if p[0] == b'0' && edge {
                self.end_bit(t, &mut out);
                self.bit = Some((t, p[2], p[3]));
            }
            if p[0] == b'0' && v[0] == b'1' {
                self.end_bit(t, &mut out);
                self.end_word(t, false, &mut out);
                self.end_transfer(t, true, &mut out);
            }
            if p[0] == b'1' && v[0] == b'0' {
                self.transfer = Some((t, Vec::new(), Vec::new()));
            }
        }
        out
    }

    fn finish(&mut self) -> Vec<Record> {
        let mut out = Vec::new();
        let end = self.end;
        self.end_bit(end, &mut out);
        self.end_word(end, false, &mut out);
        self.end_transfer(end, false, &mut out);
        out
    }
}

/// `uart`: bytes from the changes of an idle-high line (the field `value`; `idle=low` for an inverted
/// one) at `baud` bits a second:
/// a frame starts where the line falls, and each bit is sampled in its middle: the start bit (a
/// frame whose start bit isn't 0 was a glitch, and is dropped), `bits` data bits (5 - 9, LSB first),
/// a `parity` bit (`none`, `even` or `odd`) and `stop` stop bits (1 or 2, each checked).
/// `out` picks the stream: `byte` (data) or `errors` (violations, `rule`: `framing`, a stop bit
/// that isn't 1, `parity`, or `break`, the line low for a whole frame, which makes no byte).
pub struct Uart {
    errors: bool,
    /// Ticks a bit lasts.
    period: f64,
    bits: usize,
    parity: Option<bool>,
    stop: usize,
    /// The line idles low: read inverted.
    inverted: bool,
    level: Option<u8>,
    /// The frame being sampled: its start, and the bits sampled so far.
    frame: Option<(Tick, Vec<u8>)>,
}

impl Uart {
    fn new(params: &[(String, String)], fs_per_tick: Option<u128>) -> Result<Self, String> {
        let mut errors = Vec::new();
        let baud = param(params, "baud", None::<f64>, |b| *b > 0.0, "a positive number of bits a second", &mut errors);
        let bits = param(params, "bits", Some(8usize), |b| (5..=9).contains(b), "5 - 9", &mut errors);
        let parity = param(params, "parity", Some("none".to_string()), |p| ["none", "even", "odd"].contains(&p.as_str()), "none, even or odd", &mut errors);
        let stop = param(params, "stop", Some(1usize), |s| *s == 1 || *s == 2, "1 or 2", &mut errors);
        let idle = param(params, "idle", Some("high".to_string()), |i| i == "high" || i == "low", "high or low", &mut errors);
        let out = param(params, "out", None::<String>, |o| o == "byte" || o == "errors", "byte or errors", &mut errors);
        if let Some((k, _)) = params.iter().find(|p| !["baud", "bits", "parity", "stop", "idle", "out"].contains(&p.0.as_str())) {
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
        Ok(Uart { errors: out.as_deref() == Some("errors"), period, bits: bits.unwrap_or(8), parity, stop: stop.unwrap_or(1), inverted: idle.as_deref() == Some("low"), level: None, frame: None })
    }

    /// Sample the open frame's bits whose middles fall before `t`, at the line's level; the record
    /// of a frame sampled whole.
    fn sample_until(&mut self, t: f64, out: &mut Vec<Record>) {
        let need = 1 + self.bits + self.parity.is_some() as usize + self.stop;
        while let (Some((start, got)), Some(level)) = (&mut self.frame, self.level) {
            if *start as f64 + (got.len() as f64 + 0.5) * self.period >= t {
                return;
            }
            got.push(level);
            if got.len() == 1 && level != b'0' {
                self.frame = None;
            } else if got.len() == need {
                let (start, got) = self.frame.take().unwrap();
                out.extend(self.frame_record(start, &got));
            }
        }
    }

    fn frame_record(&self, start: Tick, got: &[u8]) -> Option<Record> {
        let len = 1 + self.bits + self.parity.is_some() as usize + self.stop;
        let end = start + (len as f64 * self.period).round() as Tick;
        let data: String = got[1..=self.bits].iter().rev().map(|&b| b as char).collect();
        // the line held low for the whole frame: a break, no byte
        let brk = got.iter().all(|&b| b == b'0');
        if brk {
            return self.errors.then(|| Record { start, end, fields: vec![Some(Val::Text("break".into()))] });
        }
        if !self.errors {
            return Some(Record { start, end, fields: vec![Some(Val::Bits(data))] });
        }
        let ones = got[1..=self.bits].iter().filter(|&&b| b == b'1').count();
        let parity_bad = self.parity.is_some_and(|odd| (ones + (got[self.bits + 1] == b'1') as usize) % 2 != odd as usize);
        let rule = if got[got.len() - self.stop..].iter().any(|&b| b != b'1') {
            "framing"
        } else if parity_bad {
            "parity"
        } else {
            return None;
        };
        Some(Record { start, end, fields: vec![Some(Val::Text(rule.into()))] })
    }
}

impl Decoder for Uart {
    fn schema(&self) -> Vec<Field> {
        if self.errors { vec![Field { name: "rule".into(), width: None, time: false, signed: false }] } else { vec![Field { name: "data".into(), width: Some(self.bits as u32), time: false, signed: false }] }
    }

    fn feed(&mut self, r: &Record) -> Vec<Record> {
        let mut out = Vec::new();
        let Some(Val::Bits(b)) = &r.fields[0] else { return out };
        let v = b.as_bytes()[0].to_ascii_lowercase();
        let v = if self.inverted { invert(v) } else { v };
        self.sample_until(r.start as f64, &mut out);
        if self.frame.is_none() && self.level == Some(b'1') && v == b'0' {
            self.frame = Some((r.start, Vec::new()));
        }
        self.level = Some(v);
        out
    }

    fn finish(&mut self) -> Vec<Record> {
        // the line keeps its last level
        let mut out = Vec::new();
        self.sample_until(f64::INFINITY, &mut out);
        out
    }

    fn violations(&self) -> bool {
        self.errors
    }
}
