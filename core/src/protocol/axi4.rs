//! AXI4, from Arm's spec (IHI 0022): the five channels, write and read bursts as transactions
//! paired by ID with their latency, error responses (spec §16 "AXI4").

use crate::decode::{self, Decoder};
use crate::protocol::axi::{self, Chan, Handshakes, RuleKind};
use crate::protocol::{
    Def, Default, Desc, Given, Out, Params, Protocol, RoleDef, Roles, Severity, StreamDesc, bit_of,
    reset_low,
};
use crate::stream::{Field, Record, Stream};
use crate::time::Tick;
use crate::trace::Val;

/// The channels, in the spec's order (A1.2): a channel is present when it has `<c>valid` and
/// `<c>ready`; a payload role that is not given is not a field.
const CHANS: &[Chan] = &[
    Chan {
        name: "aw",
        payload: &[
            ("addr", "awaddr"),
            ("id", "awid"),
            ("len", "awlen"),
            ("size", "awsize"),
            ("burst", "awburst"),
            ("prot", "awprot"),
            ("lock", "awlock"),
            ("cache", "awcache"),
            ("qos", "awqos"),
            ("region", "awregion"),
        ],
        rules: &[
            "aw_dropped",
            "aw_changed",
            "aw_reset",
            "aw_unknown",
            "aw_payload_unknown",
            "aw_burst",
            "aw_wrap",
            "aw_fixed",
            "aw_4k",
            "aw_size",
            "aw_exclusive",
        ],
    },
    Chan {
        name: "w",
        payload: &[("data", "wdata"), ("strb", "wstrb"), ("last", "wlast")],
        rules: &[
            "w_dropped",
            "w_changed",
            "w_reset",
            "w_unknown",
            "w_payload_unknown",
            "w_last",
            "w_strb",
        ],
    },
    Chan {
        name: "b",
        payload: &[("id", "bid"), ("resp", "bresp")],
        rules: &[
            "b_dropped",
            "b_changed",
            "b_reset",
            "b_unknown",
            "b_payload_unknown",
            "b_resp",
            "b_early",
            "b_exokay",
        ],
    },
    Chan {
        name: "ar",
        payload: &[
            ("addr", "araddr"),
            ("id", "arid"),
            ("len", "arlen"),
            ("size", "arsize"),
            ("burst", "arburst"),
            ("prot", "arprot"),
            ("lock", "arlock"),
            ("cache", "arcache"),
            ("qos", "arqos"),
            ("region", "arregion"),
        ],
        rules: &[
            "ar_dropped",
            "ar_changed",
            "ar_reset",
            "ar_unknown",
            "ar_payload_unknown",
            "ar_burst",
            "ar_wrap",
            "ar_fixed",
            "ar_4k",
            "ar_size",
            "ar_exclusive",
        ],
    },
    Chan {
        name: "r",
        payload: &[
            ("data", "rdata"),
            ("id", "rid"),
            ("resp", "rresp"),
            ("last", "rlast"),
        ],
        rules: &[
            "r_dropped",
            "r_changed",
            "r_reset",
            "r_unknown",
            "r_payload_unknown",
            "r_resp",
            "r_early",
            "r_exokay",
            "r_last",
        ],
    },
];

/// The rules of IHI 0022: a handshake error is the port's fault, a SLVERR or DECERR is the slave's
/// answer (a warning), and EXOKAY belongs to an exclusive access (A3.3).
const RULES: &[RuleKind] = &[
    RuleKind {
        suffix: "dropped",
        about: "a valid fell before its ready: it stays until the handshake",
        spec: "A2.3",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "changed",
        about: "a payload signal changed while its valid was stalled",
        spec: "A2.3",
        severity: Severity::Error,
    },
    // a request's own fields, at its address channel's handshake (A3.1, A6.3.3)
    RuleKind {
        suffix: "burst",
        about: "AxBURST is reserved (0b11)",
        spec: "A3.1.4",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "wrap",
        about: "a WRAP burst of other than 2, 4, 8 or 16 beats, or not aligned to its size",
        spec: "A3.1.4",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "fixed",
        about: "a FIXED burst of more than 16 beats",
        spec: "A3.1.2",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "4k",
        about: "a burst crossing a 4KB boundary",
        spec: "A3.1.2",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "size",
        about: "AxSIZE wider than the data bus",
        spec: "A3.1.1",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "exclusive",
        about: "an exclusive access misaligned to its bytes, not a power of two up to 128 bytes, or over 16 beats",
        spec: "A6.3.3",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "resp",
        about: "a SLVERR or DECERR response",
        spec: "A3.3",
        severity: Severity::Warning,
    },
    RuleKind {
        suffix: "exokay",
        about: "an EXOKAY response to a non-exclusive access",
        spec: "A3.3",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "early",
        about: "a response before its request's handshakes",
        spec: "A2.3.2",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "reset",
        about: "a valid while the interface is in reset, or in its first cycle out",
        spec: "A2.1.2",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "unknown",
        about: "a valid or ready is x or z out of reset",
        spec: "A2.3",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "payload_unknown",
        about: "a payload signal is x or z while its valid",
        spec: "A2.3",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "last",
        about: "LAST on a beat other than AxLEN+1, or missing on it",
        // A3.2.1 for WLAST, A3.2.2 for RLAST
        spec: "A3.2",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "strb",
        about: "a strobe high outside the beat's byte lanes",
        // "Outside of the transaction container all write strobes must be LOW"
        spec: "A3.2.1",
        severity: Severity::Error,
    },
];

/// A side of the port: its channel's bursts, the channel that answers them, and what
/// `protocols axi4` says of its three streams.
struct Side {
    name: &'static str,
    unmatched: &'static str,
    /// The channel whose handshakes are the requests (`aw`, `ar`).
    address: &'static str,
    /// The channel that bursts.
    channel: &'static str,
    burst: &'static str,
    answers: Answers,
    from: &'static [&'static str],
    burst_from: &'static [&'static str],
    /// The burst's frame param for the one optional field: `bytes` of `wstrb`, `key` of `rid`.
    burst_param: (&'static str, &'static str),
    burst_about: &'static str,
    about: &'static str,
    unmatched_about: &'static str,
}

/// What answers a burst: a channel of its own (a B per write), or the burst (an R per read).
#[derive(Clone, Copy)]
enum Answers {
    Channel(&'static str),
    Burst,
}

const SIDES: [Side; 2] = [
    Side {
        name: "wr",
        unmatched: "wr.unmatched",
        address: "aw",
        channel: "w",
        burst: "wburst",
        answers: Answers::Channel("b"),
        from: &["aw", "wburst", "b"],
        burst_from: &["w"],
        burst_param: ("bytes", "strb"),
        burst_about: "the W beats of a burst, framed by wlast when given and by AxLEN when not; data the bytes wstrb marks",
        about: "writes: AW and its W burst in order, then the B with its ID; delay from the later of AW and the burst's first beat to B",
        unmatched_about: "a write never answered, or not within `timeout` (`no response`), its B after that (`timeout`), a W burst without an AW (`no aw`) or the reverse (`no wburst`); a B before its write's handshakes is `b_early` in `errors`",
    },
    Side {
        name: "rd",
        unmatched: "rd.unmatched",
        address: "ar",
        channel: "r",
        burst: "rburst",
        answers: Answers::Burst,
        from: &["ar", "rburst"],
        burst_from: &["r"],
        burst_param: ("key", "id"),
        burst_about: "the R beats of a burst, each ID's apart (bursts of different IDs interleave), framed by rlast when given and by AxLEN when not",
        about: "reads: AR, then the R burst with its ID; delay (latency) from AR to the first R beat",
        unmatched_about: "an AR never answered, or not within `timeout` (`no response`), its R burst after that (`timeout`); an R before its AR is `r_early` in `errors`",
    },
];

impl Side {
    /// The instance has every channel this side needs: a write-only port has no `ar`.
    fn present(&self, channels: &Handshakes) -> bool {
        let has = |chan: &str| channels.desc(chan).is_some();
        has(self.address)
            && has(self.channel)
            && match self.answers {
                Answers::Channel(code) => has(code),
                Answers::Burst => true,
            }
    }

    /// Both ID roles are given, so the join pairs by ID: the address's `id` and the answer's
    /// (`bid`, or `rid` through the burst's `key`).
    fn pairs_id(&self, channels: &Handshakes, burst: &[(&str, &str)]) -> bool {
        let has_id = |chan: &str| {
            channels
                .desc(chan)
                .is_some_and(|d| d.fields.iter().any(|f| f.name == "id"))
        };
        has_id(self.address)
            && match self.answers {
                Answers::Channel(code) => has_id(code),
                Answers::Burst => burst.contains(&("key", "id")),
            }
    }
}

/// A side the instance has: its burst's decoder, made at `make`, and its ID rule.
struct Live {
    side: &'static Side,
    id: Option<(&'static str, &'static str, &'static str)>,
    frame: Box<dyn Decoder>,
    /// The burst channel gives `wlast`/`rlast`: without it the frames come from AxLEN (A3.2).
    has_last: bool,
}

impl Live {
    /// What `protocols axi4` prints: the burst, the join and the join's unmatched requests, with
    /// the fields `join` will make, so the two never drift.
    fn descs(&self, channels: &Handshakes) -> [StreamDesc; 3] {
        let side = self.side;
        let schema = |chan: &str| channels.desc(chan).map(|d| d.fields).unwrap_or_default();
        let burst = self.frame.schema();
        let address = schema(side.address);
        let fields = match side.answers {
            Answers::Channel(code) => decode::join_schema(
                &[(side.address, &address[..]), (side.burst, &burst[..])],
                &schema(code),
            ),
            Answers::Burst => decode::join_schema(&[(side.address, &address[..])], &burst[..]),
        };
        let kind = vec![Field {
            name: "kind".into(),
            width: None,
            time: false,
            signed: false,
        }];
        [
            StreamDesc {
                name: side.burst,
                fields: burst,
                violations: false,
                from: side.burst_from,
                about: vec![side.burst_about.into()],
            },
            StreamDesc {
                name: side.name,
                fields,
                violations: false,
                from: side.from,
                about: vec![side.about.into()],
            },
            StreamDesc {
                name: side.unmatched,
                fields: kind,
                violations: true,
                from: side.from,
                about: vec![side.unmatched_about.into()],
            },
        ]
    }

    /// The side's records, from the channels' whole streams: the burst, its join, its unmatched.
    fn out(
        self,
        channels: &Handshakes,
        records: &[Vec<Record>],
        timeout: Option<Tick>,
    ) -> Vec<Out> {
        let side = self.side;
        // the join's schema, as `descs` makes it: the fields a rule reads by name
        let fields = self.descs(channels)[1].fields.clone();
        let beats = channel(records, side.channel);
        // each beat's request, by AxLEN and ID: the LAST and strobe rules read it, and a port
        // without wlast/rlast frames its bursts by it, the beats gaining their last
        let (places, assigns) = beat_places(side, channels, records, beats);
        let framed = (!self.has_last).then(|| by_len(beats, &assigns));
        let beats = framed.as_deref().unwrap_or(beats);
        let burst = decode::run_records(side.burst, beats, self.frame);
        let mut out = axi::from_stream(side.burst, &burst);
        let address = stream(channels, records, side.address);
        let answer;
        let (requests, response): (Vec<(&'static str, &Stream)>, &Stream) = match side.answers {
            Answers::Channel(code) => {
                answer = stream(channels, records, code);
                (
                    vec![(side.address, &address), (side.burst, &burst)],
                    &answer,
                )
            }
            Answers::Burst => (vec![(side.address, &address)], &burst),
        };
        // a response before its request is `b_early`/`r_early` in errors, one after it expired
        // `timeout`: the join tells them from the ones it answers
        let early_chan = match side.answers {
            Answers::Channel(code) => code,
            Answers::Burst => side.channel,
        };
        // the burst that must have ended: only a write's, an R burst is the response
        let ended = match side.answers {
            Answers::Channel(_) => Some(side.burst),
            Answers::Burst => None,
        };
        out.extend(channels.join_streams(
            side.name,
            side.unmatched,
            &requests,
            response,
            self.id,
            ended,
            early_chan,
            timeout,
        ));
        // A3.3: an EXOKAY is for an exclusive access; the join has `lock` and the response
        let exokay = exokays(side, channels, records, &burst, &fields, &out);
        out.extend(exokay);
        // LAST reads the beats against their requests, which the join does not: a stray beat is no
        // request's, so an unmatched burst is told nowhere
        out.extend(side_rules(side, channels, records, &places));
        out
    }
}

/// A signal's bits as a number: a text value (an enum) is not one.
fn number(v: Option<&Val>) -> Option<u64> {
    match v? {
        Val::Bits(b) => u64::from_str_radix(b, 2).ok(),
        Val::Text(_) => None,
    }
}

/// A channel's handshakes so far.
fn channel<'a>(records: &'a [Vec<Record>], name: &str) -> &'a [Record] {
    CHANS
        .iter()
        .position(|c| c.name == name)
        .map_or(&[], |i| &records[i])
}

/// A channel's whole stream, as `join` takes it (one copy per transaction).
fn stream(channels: &Handshakes, records: &[Vec<Record>], name: &'static str) -> Stream {
    Stream {
        name: name.into(),
        schema: channels.desc(name).map(|d| d.fields).unwrap_or_default(),
        records: channel(records, name).to_vec(),
        violations: false,
        inputs: Vec::new(),
    }
}

/// A join's records: `(start, end, fields)`.
fn joined<'a>(
    out: &'a [Out],
    name: &'a str,
) -> impl Iterator<Item = (Tick, Tick, &'a [Option<Val>])> + 'a {
    out.iter().filter_map(move |o| match o {
        Out::Record {
            stream,
            start,
            end,
            fields,
        } if *stream == name => Some((*start, *end, &fields[..])),
        _ => None,
    })
}

/// A3.3: EXOKAY answers an exclusive access. `fields` is the join's schema and `out` its records:
/// the join carries `lock` with the response, so the rule reads them together. A read's `resp` is
/// per beat, and the burst window says which read it is.
fn exokays(
    side: &'static Side,
    channels: &Handshakes,
    records: &[Vec<Record>],
    burst: &Stream,
    fields: &[Field],
    out: &[Out],
) -> Vec<Out> {
    let at = |name: &str| fields.iter().position(|f| f.name == name);
    let pos = |chan: &str, name: &str| {
        channels
            .desc(chan)
            .and_then(|d| d.fields.iter().position(|f| f.name == name))
    };
    // a normal access is the lock at 0: an x or z claims nothing
    let normal =
        |f: &[Option<Val>], lock: usize| matches!(&f[lock], Some(Val::Bits(b)) if b == "0");
    let lock = at("lock").expect("lock has a default, so the join has it");
    let mut rows = Vec::new();
    match side.answers {
        Answers::Channel(code) => {
            let resp = pos(code, "resp").expect("bresp has a default");
            let answers = channel(records, code);
            for (_, end, _) in joined(out, side.name).filter(|(_, _, f)| normal(f, lock)) {
                // a B ends its write, and its `resp` is the write's; the Bs are in time order
                let i = answers.partition_point(|b| b.end < end);
                let b = &answers[i];
                if b.fields[resp].as_ref().is_some_and(axi::is_exokay) {
                    rows.push(channels.finish_violation(
                        code,
                        "exokay",
                        b.start,
                        b.end,
                        "bresp is EXOKAY to a non-exclusive write".to_string(),
                    ));
                }
            }
        }
        Answers::Burst => {
            let resp = pos(side.channel, "resp").expect("rresp has a default");
            // a port without IDs pairs its reads in order (spec §16): no ID to compare
            let ids = match (
                pos(side.channel, "id"),
                burst.schema.iter().position(|f| f.name == "id"),
                at("resp_id"),
            ) {
                (Some(id), Some(key), Some(key_at)) => Some((id, key, key_at)),
                _ => None,
            };
            let beats = channel(records, side.channel);
            for (_, end, f) in joined(out, side.name).filter(|(_, _, f)| normal(f, lock)) {
                // the burst the join ended with: its window holds this read's beats; the bursts
                // and the beats are in time order
                let i = burst.records.partition_point(|r| r.end < end);
                let b = burst.records[i..]
                    .iter()
                    .take_while(|r| r.end == end)
                    .find(|r| ids.is_none_or(|(_, key, key_at)| r.fields[key] == f[key_at]))
                    .expect("a read ends with its burst");
                let from = beats.partition_point(|beat| beat.start < b.start);
                let to = beats.partition_point(|beat| beat.start < b.end);
                let mine = beats[from..to]
                    .iter()
                    .filter(|beat| ids.is_none_or(|(id, _, key_at)| beat.fields[id] == f[key_at]));
                for beat in mine {
                    if beat.fields[resp].as_ref().is_some_and(axi::is_exokay) {
                        rows.push(channels.finish_violation(
                            side.channel,
                            "exokay",
                            beat.start,
                            beat.end,
                            "rresp is EXOKAY to a non-exclusive read".to_string(),
                        ));
                    }
                }
            }
        }
    }
    rows
}

/// The burst's decoder: `frame` over the channel, its params owned (`Frame::new` takes them so).
fn burst_decoder(
    fields: &[Field],
    params: &[(&str, &str)],
    fs_per_tick: Option<u128>,
) -> Box<dyn Decoder> {
    let params: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    // `last` and `data` are always fields, `key` and `bytes` asked for only when they are
    let frame = decode::Frame::new(fields, &params, fs_per_tick).expect("the burst's own fields");
    Box::new(frame)
}

/// WDATA's width in bytes: the lanes `strb` can mark, `None` when it is not whole bytes.
fn strb_bus(channels: &Handshakes, chan: &str) -> Option<u64> {
    // the side's channel is given, and its data is a required role
    channels
        .desc(chan)
        .and_then(|d| d.fields.into_iter().find(|f| f.name == "data"))
        .expect("the channel's data")
        .width
        .filter(|w| *w >= 8 && w % 8 == 0)
        .map(|w| u64::from(w / 8))
}

/// The byte lanes of beat `n` (0-based) of a burst: its first and last, inclusive, in bus bytes from
/// the bus's low end, per A3.1.6. `burst` is `00` FIXED, `01` INCR or `10` WRAP; `size` is the
/// beat's bytes. The address's own transfer covers an unaligned start, then the size counts on.
pub(crate) fn strobe_lanes(
    bus: u64,
    addr: u64,
    size: u64,
    burst: &str,
    beats: u64,
    n: u64,
) -> (u64, u64) {
    let aligned = addr / size * size;
    let (lower, upper) = match burst {
        // FIXED repeats the address: every beat is the first
        "00" => (addr % bus, aligned % bus + size - 1),
        "01" if n == 0 => (addr % bus, aligned % bus + size - 1),
        "01" => {
            let a = aligned + n * size;
            let lower = a % bus;
            (lower, lower + size - 1)
        }
        // WRAP counts its beats up in a box that starts at the aligned address and starts again at
        // the box's bottom; the box may be smaller than the bus
        _ => {
            let window = size * beats;
            let lo = aligned / window * window;
            let a = lo + (aligned - lo + n * size) % window;
            let lower = a % bus;
            (lower, lower + size - 1)
        }
    };
    (lower, upper)
}

/// The lanes a beat's `strb` marks that are outside its own, lowest first: the bus's lane k is the
/// k-th character from the end of the MSB-first bits, as `data_bytes` reads them.
pub(crate) fn strobe_bad_lanes(
    bus: u64,
    addr: u64,
    size: u64,
    burst: &str,
    beats: u64,
    n: u64,
    strb: &str,
) -> Vec<u64> {
    let (lower, upper) = strobe_lanes(bus, addr, size, burst, beats, n);
    let bits = strb.as_bytes();
    (0..bits.len() as u64)
        .filter(|&k| !(lower..=upper).contains(&k))
        .filter(|&k| matches!(bits[bits.len() - 1 - k as usize], b'1' | b'h'))
        .collect()
}

/// A beat's place: the request it belongs to, and how many of that request's beats came before it.
type Place = Option<(usize, u64)>;

/// The requests' beat counts (AxLEN + 1, `None` when it cannot be read), and where each beat fell.
type Places = (Vec<Option<u64>>, Vec<Place>);

/// Each beat's request, and how many of that request's beats came before it: a stray beat has no
/// request, so nothing to count it against.
fn beat_ordinals(assigns: &[BeatAssign]) -> Vec<Place> {
    let mut seen: Vec<u64> = Vec::new();
    assigns
        .iter()
        .map(|a| {
            let k = a.req?;
            seen.resize(seen.len().max(k + 1), 0);
            let i = seen[k];
            seen[k] += 1;
            Some((k, i))
        })
        .collect()
}

/// The requests behind the side's beats: the places the LAST and strobe rules read, and each beat's
/// assignment, whose `last` frames a port without WLAST/RLAST (`by_len`). Read off the signals, not
/// the burst's stream, so a beat the join could not place is still counted.
fn beat_places(
    side: &Side,
    channels: &Handshakes,
    records: &[Vec<Record>],
    beats: &[Record],
) -> (Places, Vec<BeatAssign>) {
    let fields = |name: &str| channels.desc(name).map(|d| d.fields).unwrap_or_default();
    let (address, beat_fields) = (fields(side.address), fields(side.channel));
    let index = |fields: &[Field], name: &str| fields.iter().position(|f| f.name == name);
    let (len, id) = (index(&address, "len"), index(&address, "id"));
    let requests = channel(records, side.address);
    // AxLEN is the length less one: an absent or unreadable one is `None`, counted as one beat
    let lens: Vec<Option<u64>> = requests
        .iter()
        .map(|r| number(len.and_then(|i| r.fields.get(i)).and_then(Option::as_ref)).map(|n| n + 1))
        .collect();
    let reqs: Vec<(Option<Val>, u64)> = requests
        .iter()
        .zip(&lens)
        .map(|(r, of)| {
            (
                id.and_then(|i| r.fields.get(i).cloned()).flatten(),
                of.unwrap_or(1),
            )
        })
        .collect();
    let assigns = assign_beats(beats, id.and_then(|_| index(&beat_fields, "id")), &reqs);
    ((lens, beat_ordinals(&assigns)), assigns)
}

/// A beat's LAST against the AxLEN of the request it belongs to, on the beat's own handshake: high
/// before the last beat, or low on it (spec A3.2).
fn check_last(
    channels: &Handshakes,
    chan: &'static str,
    beats: &[Record],
    last: usize,
    ords: &[Place],
    lens: &[Option<u64>],
) -> Vec<Out> {
    let mut out = Vec::new();
    for (beat, ord) in beats.iter().zip(ords) {
        let Some((k, i)) = *ord else { continue };
        let Some(of) = lens[k] else { continue };
        // a signal with no value yet says nothing
        let Some(v) = beat.fields[last].clone() else {
            continue;
        };
        let Val::Bits(bits) = &v else { continue };
        if axi::unknown(&v) {
            continue;
        }
        // 0, 1, h or l: the x, z, u, w and - are `unknown`
        let high = bit_of(bits, 0).expect("a known bit");
        let n = i + 1;
        let detail = match (high, n == of) {
            (true, false) => format!("{chan}last on beat {n} of {of}"),
            (false, true) => format!("missing {chan}last on beat {n} of {of}"),
            _ => continue,
        };
        out.push(channels.finish_violation(chan, "last", beat.start, beat.end, detail));
    }
    out
}

/// A write beat's strobe against the byte lanes its request's address, size and burst give it, on
/// the beat's own handshake (A3.2.1).
fn check_strb(
    side: &Side,
    channels: &Handshakes,
    records: &[Vec<Record>],
    beats: &[Record],
    strb: usize,
    bus: u64,
    places: &Places,
) -> Vec<Out> {
    let (lens, ords) = places;
    let fields = channels
        .desc(side.address)
        .map(|d| d.fields)
        .unwrap_or_default();
    let index = |name: &str| fields.iter().position(|f| f.name == name);
    // addr is required, size and burst have defaults: the address channel has all three
    let [a, s, t] = ["addr", "size", "burst"].map(|f| index(f).expect("a request field"));
    let requests = channel(records, side.address);
    let mut out = Vec::new();
    for (beat, ord) in beats.iter().zip(ords) {
        let Some((k, n)) = *ord else { continue };
        let Some(of) = lens[k] else { continue };
        let request = &requests[k];
        let field = |i: usize| request.fields.get(i).and_then(|v| v.as_ref());
        let (Some(addr), Some(size), Some(burst)) =
            (number(field(a)), number(field(s)), number(field(t)))
        else {
            continue;
        };
        // a beat wider than the bus is `size`'s row: its lanes are not its own to judge
        let bytes = 1u64.checked_shl(size as u32).unwrap_or(u64::MAX);
        if bytes > bus {
            continue;
        }
        let burst = match burst {
            0 => "00",
            1 => "01",
            2 => "10",
            // 0b11 is `burst`'s row
            _ => continue,
        };
        let Some(v) = beat.fields.get(strb).and_then(|v| v.as_ref()) else {
            continue;
        };
        let Val::Bits(bits) = v else { continue };
        // an x or z strobe says nothing
        if axi::unknown(v) {
            continue;
        }
        let bad = strobe_bad_lanes(bus, addr, bytes, burst, of, n, bits);
        if bad.is_empty() {
            continue;
        }
        let lanes = bad.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
        let what = if bad.len() == 1 { "lane" } else { "lanes" };
        out.push(channels.finish_violation(
            side.channel,
            "strb",
            beat.start,
            beat.end,
            format!(
                "{}strb {bits} enables {what} {lanes} outside beat {} of {of}",
                side.channel,
                n + 1
            ),
        ));
    }
    out
}

/// The side's LAST and strobe rules: the beats, their requests, and what each said.
fn side_rules(
    side: &Side,
    channels: &Handshakes,
    records: &[Vec<Record>],
    places: &Places,
) -> Vec<Out> {
    let beats = channel(records, side.channel);
    let (lens, ords) = places;
    let mut out = Vec::new();
    if let Some(last) = channels
        .desc(side.channel)
        .and_then(|d| d.fields.iter().position(|f| f.name == "last"))
    {
        out.extend(check_last(channels, side.channel, beats, last, ords, lens));
    }
    if let (Some(strb), Some(bus)) = (
        channels
            .desc(side.channel)
            .and_then(|d| d.fields.iter().position(|f| f.name == "strb")),
        strb_bus(channels, side.channel),
    ) {
        out.extend(check_strb(
            side, channels, records, beats, strb, bus, places,
        ));
    }
    out
}

/// Which request a beat belongs to (`req`, its index in the address channel's records) and whether
/// it is that request's last beat.
pub(crate) struct BeatAssign {
    /// Which of the address channel's records it belongs to: the `w_last`/`r_last` and `w_strb`
    /// rules read it.
    pub req: Option<usize>,
    /// It is its request's last beat: `by_len` frames a port without WLAST/RLAST by it.
    pub last: bool,
}

/// Each beat's request, from the requests' IDs and their lengths in beats: a request takes
/// AxLEN+1 beats, and one ID's beats take that ID's requests in order (A3.2). `beat_id` is the
/// index of the beats' `id` field; without IDs, or without `beat_id`, one queue takes every
/// request in order. A beat past every length belongs to none.
pub(crate) fn assign_beats(
    beats: &[Record],
    beat_id: Option<usize>,
    reqs: &[(Option<Val>, u64)],
) -> Vec<BeatAssign> {
    struct Queue {
        id: Option<Val>,
        /// The requests of this ID, in the address channel's order.
        reqs: Vec<usize>,
        /// How many of them are done, and how many beats the current one has taken.
        done: usize,
        taken: u64,
    }
    let mut queues: Vec<Queue> = Vec::new();
    for (k, (id, _)) in reqs.iter().enumerate() {
        let id = beat_id.and(id.clone());
        match queues.iter_mut().find(|q| q.id == id) {
            Some(q) => q.reqs.push(k),
            None => queues.push(Queue {
                id,
                reqs: vec![k],
                done: 0,
                taken: 0,
            }),
        }
    }
    beats
        .iter()
        .map(|beat| {
            let id = beat_id.and_then(|i| beat.fields.get(i).cloned()).flatten();
            let Some(q) = queues.iter_mut().find(|q| q.id == id) else {
                return BeatAssign {
                    req: None,
                    last: false,
                };
            };
            let Some(&req) = q.reqs.get(q.done) else {
                return BeatAssign {
                    req: None,
                    last: false,
                };
            };
            q.taken += 1;
            // a length that cannot be read counts one beat, so no beat after it belongs to none
            let last = q.taken >= reqs[req].1.max(1);
            if last {
                q.done += 1;
                q.taken = 0;
            }
            BeatAssign {
                req: Some(req),
                last,
            }
        })
        .collect()
}

/// The burst channel's beats with the `last` their request's AxLEN gives them, as `frame` takes
/// them from `wlast`/`rlast` when the port gives those: a burst is AxLEN+1 beats (A3.2).
fn by_len(beats: &[Record], assigns: &[BeatAssign]) -> Vec<Record> {
    assigns
        .iter()
        .zip(beats)
        .map(|(assign, beat)| {
            let mut fields = beat.fields.clone();
            let last = if assign.last { "1" } else { "0" };
            fields.push(Some(Val::Bits(last.into())));
            Record {
                start: beat.start,
                end: beat.end,
                fields,
            }
        })
        .collect()
}

/// An instance: the channels it has, its schema, and the sides to finish.
struct Axi4 {
    channels: Handshakes,
    desc: Desc,
    timeout: Option<Tick>,
    sides: Vec<Live>,
    /// The channels' handshakes of the whole trace, a `Vec` each in `CHANS` order: the sides join
    /// them, which needs them all.
    records: Vec<Vec<Record>>,
    /// The data bus's bytes: `wdata`'s for a write, `rdata`'s for a read, when the port gives it.
    wdata: Option<u64>,
    rdata: Option<u64>,
}

impl Axi4 {
    fn new(given: &Given, params: &Params, fs_per_tick: Option<u128>) -> Result<Self, String> {
        let timeout = axi::timeout(params, fs_per_tick)?;
        let mut channels = Handshakes::new(given, CHANS, RULES, DEF.roles, reset_low(params)?);
        channels.defer_to_join();
        let mut streams: Vec<StreamDesc> =
            CHANS.iter().filter_map(|c| channels.desc(c.name)).collect();
        streams.push(channels.errors());
        let mut sides = Vec::new();
        for side in &SIDES {
            if !side.present(&channels) {
                continue;
            }
            let fields = channels
                .desc(side.channel)
                .map(|d| d.fields)
                .unwrap_or_default();
            // a port without wlast/rlast frames its bursts by AxLEN, so the frame needs a `last`
            let has_last = fields.iter().any(|f| f.name == "last");
            let mut framed = fields.clone();
            if !has_last {
                framed.push(Field {
                    name: "last".into(),
                    width: Some(1),
                    time: false,
                    signed: false,
                });
            }
            let mut frame_params = vec![("last", "last"), ("data", "data")];
            let (key, role) = side.burst_param;
            if fields.iter().any(|f| f.name == role) {
                frame_params.push((key, role));
            }
            let frame = burst_decoder(&framed, &frame_params, fs_per_tick);
            let id = side
                .pairs_id(&channels, &frame_params)
                .then_some((side.address, "id", "id"));
            let live = Live {
                side,
                id,
                frame,
                has_last,
            };
            streams.extend(live.descs(&channels));
            sides.push(live);
        }
        // the data bus's bytes: `wdata` for a write, `rdata` for a read
        let bus = |role: &str| {
            given
                .iter()
                .find(|g| g.0 == role)
                .and_then(|g| g.1)
                .map(|bits| bits as u64 / 8)
        };
        Ok(Axi4 {
            channels,
            desc: Desc { streams },
            timeout,
            sides,
            records: vec![Vec::new(); CHANS.len()],
            wdata: bus("wdata"),
            rdata: bus("rdata"),
        })
    }

    /// The rules of a request's own fields, at its address channel's handshake (A3.1, A6.3.3): the
    /// address, length, size, burst and lock the cycle samples, so an absent field reads its
    /// default and a rule with an x or z input says nothing.
    fn check_request(
        &self,
        chan: &'static str,
        roles: &Roles<'_>,
        start: Tick,
        end: Tick,
    ) -> Vec<Out> {
        let role = |field: &str| roles.get(&format!("{chan}{field}"));
        let fields = ["addr", "len", "size", "burst", "lock"];
        if fields.iter().any(|f| role(f).is_some_and(axi::unknown)) {
            return Vec::new();
        }
        let (Some(addr), Some(len), Some(size), Some(burst)) = (
            number(role("addr")),
            number(role("len")),
            number(role("size")),
            number(role("burst")),
        ) else {
            return Vec::new();
        };
        // a width or a length that would overflow says nothing: no real bus is past these
        let Some(bytes) = 1u64.checked_shl(size as u32) else {
            return Vec::new();
        };
        let beats = len.saturating_add(1);
        let mut out = Vec::new();
        let mut fire = |suffix: &str, detail: String| {
            out.push(
                self.channels
                    .finish_violation(chan, suffix, start, end, detail),
            );
        };
        if burst == 0b11 {
            fire("burst", format!("{chan}burst is reserved (0b11)"));
        }
        if burst == 0b10 {
            if !matches!(beats, 2 | 4 | 8 | 16) {
                fire(
                    "wrap",
                    format!("{chan}len {len} with WRAP: length must be 2, 4, 8 or 16"),
                );
            } else if addr % bytes != 0 {
                fire(
                    "wrap",
                    format!("{chan}addr 0x{addr:x} with WRAP: not aligned to {bytes} bytes"),
                );
            }
        }
        if burst == 0b00 && beats > 16 {
            fire(
                "fixed",
                format!("{chan}len {len} with FIXED: at most 16 beats"),
            );
        }
        let span = beats.saturating_mul(bytes);
        // an unaligned INCR's transfers count from the address aligned to its size (A3.1.6): the
        // first covers only the bytes from the address up
        let last = (addr & !(bytes - 1)).saturating_add(span - 1);
        if burst == 0b01 && addr >> 12 != last >> 12 {
            fire(
                "4k",
                format!(
                    "{chan}addr 0x{addr:x}, {beats} beats of {bytes} bytes: crosses a 4KB boundary"
                ),
            );
        }
        let bus = if chan == "aw" { self.wdata } else { self.rdata };
        if let Some(bus) = bus.filter(|&bus| bytes > bus) {
            fire(
                "size",
                format!("{chan}size {size} wider than the data bus ({bus} bytes)"),
            );
        }
        if roles.bit(&format!("{chan}lock")) == Some(true) {
            if !span.is_power_of_two() || span > 128 {
                fire(
                    "exclusive",
                    format!("{chan} exclusive: {span} bytes is not a power of two up to 128"),
                );
            } else if addr % span != 0 {
                fire(
                    "exclusive",
                    format!("{chan} exclusive: address not aligned to {span} bytes"),
                );
            } else if beats > 16 {
                fire(
                    "exclusive",
                    format!("{chan} exclusive: {beats} beats, at most 16"),
                );
            }
        }
        out
    }
}

impl Protocol for Axi4 {
    fn desc(&self) -> &Desc {
        &self.desc
    }

    fn feed(&mut self, start: Tick, end: Tick, roles: &Roles<'_>) -> Vec<Out> {
        let mut out = self.channels.feed(start, end, roles);
        // a request's own fields, at its handshake: a reset cycle records none, so none is checked
        let mut checked = Vec::new();
        for o in &out {
            if let Out::Record {
                stream, start, end, ..
            } = o
                && (*stream == "aw" || *stream == "ar")
            {
                checked.extend(self.check_request(stream, roles, *start, *end));
            }
        }
        for o in &out {
            if let Out::Record {
                stream,
                start,
                end,
                fields,
            } = o
                && let Some(i) = CHANS.iter().position(|c| c.name == *stream)
            {
                self.records[i].push(Record {
                    start: *start,
                    end: *end,
                    fields: fields.clone(),
                });
            }
        }
        out.extend(checked);
        out
    }

    fn finish(&mut self) -> Vec<Out> {
        let timeout = self.timeout;
        // the sides' records are made from the channels' whole streams, so they come last
        std::mem::take(&mut self.sides)
            .into_iter()
            .flat_map(|s| s.out(&self.channels, &self.records, timeout))
            .collect()
    }
}

pub const DEF: Def = Def {
    about: "AXI4: write and read bursts as transactions, paired by ID, with their latency; AxLEN framing, reset, request-legality and strobe checks; SLVERR/DECERR warnings, EXOKAY-to-non-exclusive errors",
    example: "decode mem axi4 clock=aclk prefix=m_axi_",
    clocked: true,
    sides: &[
        &[
            "awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "bvalid", "bready",
        ],
        &["arvalid", "arready", "araddr", "rvalid", "rready", "rdata"],
    ],
    roles: &[
        RoleDef {
            name: "awvalid",
            required: true,
            default: None,
            about: "the write address is valid",
        },
        RoleDef {
            name: "awready",
            required: true,
            default: None,
            about: "the write address is taken",
        },
        RoleDef {
            name: "awaddr",
            required: true,
            default: None,
            about: "the write's address",
        },
        RoleDef {
            name: "wvalid",
            required: true,
            default: None,
            about: "a write datum is valid",
        },
        RoleDef {
            name: "wready",
            required: true,
            default: None,
            about: "the write datum is taken",
        },
        RoleDef {
            name: "wdata",
            required: true,
            default: None,
            about: "the write's bytes",
        },
        RoleDef {
            name: "wlast",
            required: false,
            default: None,
            about: "the burst's last beat",
        },
        RoleDef {
            name: "bvalid",
            required: true,
            default: None,
            about: "a write response is valid",
        },
        RoleDef {
            name: "bready",
            required: true,
            default: None,
            about: "the write response is taken",
        },
        RoleDef {
            name: "arvalid",
            required: true,
            default: None,
            about: "the read address is valid",
        },
        RoleDef {
            name: "arready",
            required: true,
            default: None,
            about: "the read address is taken",
        },
        RoleDef {
            name: "araddr",
            required: true,
            default: None,
            about: "the read's address",
        },
        RoleDef {
            name: "rvalid",
            required: true,
            default: None,
            about: "a read datum is valid",
        },
        RoleDef {
            name: "rready",
            required: true,
            default: None,
            about: "the read datum is taken",
        },
        RoleDef {
            name: "rdata",
            required: true,
            default: None,
            about: "the read's bytes",
        },
        RoleDef {
            name: "rlast",
            required: false,
            default: None,
            about: "the burst's last beat",
        },
        RoleDef {
            name: "awid",
            required: false,
            default: None,
            about: "the write's ID, pairing it with its B",
        },
        RoleDef {
            name: "awlen",
            required: false,
            default: Some(Default::Bits("00000000")),
            about: "the burst's length in beats, less one",
        },
        RoleDef {
            name: "awsize",
            required: false,
            default: Some(Default::Size("wdata")),
            about: "the bytes per beat, as a power of two",
        },
        RoleDef {
            name: "awburst",
            required: false,
            default: Some(Default::Bits("01")),
            about: "the burst's type (A3.1)",
        },
        RoleDef {
            name: "awprot",
            required: false,
            default: Some(Default::Bits("000")),
            about: "the write's protection",
        },
        RoleDef {
            name: "awlock",
            required: false,
            default: Some(Default::Bits("0")),
            about: "the write's exclusive access (0: normal)",
        },
        RoleDef {
            name: "awcache",
            required: false,
            default: Some(Default::Bits("0000")),
            about: "the write's cache attributes (A4.4)",
        },
        RoleDef {
            name: "awqos",
            required: false,
            default: Some(Default::Bits("0000")),
            about: "the write's quality of service",
        },
        RoleDef {
            name: "awregion",
            required: false,
            default: Some(Default::Bits("0000")),
            about: "the write's address region (A5.1)",
        },
        RoleDef {
            name: "wstrb",
            required: false,
            default: Some(Default::Bytes("wdata")),
            about: "which bytes of wdata hold information",
        },
        RoleDef {
            name: "bid",
            required: false,
            default: None,
            about: "the ID of the write it answers",
        },
        RoleDef {
            name: "bresp",
            required: false,
            default: Some(Default::Bits("00")),
            about: "the write's response (0: OKAY)",
        },
        RoleDef {
            name: "arid",
            required: false,
            default: None,
            about: "the read's ID, pairing it with its R burst",
        },
        RoleDef {
            name: "arlen",
            required: false,
            default: Some(Default::Bits("00000000")),
            about: "the burst's length in beats, less one",
        },
        RoleDef {
            name: "arsize",
            required: false,
            default: Some(Default::Size("rdata")),
            about: "the bytes per beat, as a power of two",
        },
        RoleDef {
            name: "arburst",
            required: false,
            default: Some(Default::Bits("01")),
            about: "the burst's type (A3.1)",
        },
        RoleDef {
            name: "arprot",
            required: false,
            default: Some(Default::Bits("000")),
            about: "the read's protection",
        },
        RoleDef {
            name: "arlock",
            required: false,
            default: Some(Default::Bits("0")),
            about: "the read's exclusive access (0: normal)",
        },
        RoleDef {
            name: "arcache",
            required: false,
            default: Some(Default::Bits("0000")),
            about: "the read's cache attributes (A4.4)",
        },
        RoleDef {
            name: "arqos",
            required: false,
            default: Some(Default::Bits("0000")),
            about: "the read's quality of service",
        },
        RoleDef {
            name: "arregion",
            required: false,
            default: Some(Default::Bits("0000")),
            about: "the read's address region (A5.1)",
        },
        RoleDef {
            name: "rid",
            required: false,
            default: None,
            about: "the ID of the read it answers",
        },
        RoleDef {
            name: "rresp",
            required: false,
            default: Some(Default::Bits("00")),
            about: "the read's response (0: OKAY)",
        },
        RoleDef {
            name: "aresetn",
            required: true,
            default: None,
            about: "the reset, active low (reset_active=high: active high)",
        },
    ],
    params: &["timeout", "reset_active"],
    make,
};

fn make(
    given: &Given,
    params: &Params,
    fs_per_tick: Option<u128>,
) -> Result<Box<dyn Protocol>, String> {
    Axi4::new(given, params, fs_per_tick).map(|a| Box::new(a) as Box<dyn Protocol>)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A beat with an `id` field when its channel has one, as `Handshakes` makes its records.
    fn beat(id: Option<&str>) -> Record {
        Record {
            start: 0,
            end: 1,
            fields: vec![id.map(|i| Val::Bits(i.into()))],
        }
    }

    /// The requests: an ID each, and their lengths in beats.
    fn reqs(ids: &[Option<&str>], beats: &[u64]) -> Vec<(Option<Val>, u64)> {
        ids.iter()
            .zip(beats)
            .map(|(id, n)| (id.map(|i| Val::Bits(i.into())), *n))
            .collect()
    }

    /// The assignments, as `(request, last)` per beat.
    fn seen(a: &[BeatAssign]) -> Vec<(Option<usize>, bool)> {
        a.iter().map(|b| (b.req, b.last)).collect()
    }

    #[test]
    fn assign_beats_w_fifo() {
        // W has no ID: one queue takes the AWs in order, and a beat past every length is no one's
        let beats = [beat(None), beat(None), beat(None), beat(None)];
        let reqs = reqs(&[None, None], &[2, 1]);
        assert_eq!(
            seen(&assign_beats(&beats, None, &reqs)),
            [
                (Some(0), false),
                (Some(0), true),
                (Some(1), true),
                (None, false)
            ]
        );
    }

    #[test]
    fn assign_beats_r_by_id() {
        // R pairs by ID: the two reads of ID 1 take turns while ID 2's interleave
        let beats = [
            beat(Some("10")),
            beat(Some("01")),
            beat(Some("01")),
            beat(Some("10")),
        ];
        let reqs = reqs(&[Some("01"), Some("10"), Some("01")], &[2, 2, 1]);
        assert_eq!(
            seen(&assign_beats(&beats, Some(0), &reqs)),
            [
                (Some(1), false),
                (Some(0), false),
                (Some(0), true),
                (Some(1), true)
            ]
        );
    }

    #[test]
    fn assign_beats_by_order_without_ids() {
        // no IDs given: one queue takes the ARs in order, as `pairs_id` joins them
        let beats = [beat(None), beat(None), beat(None)];
        let reqs = reqs(&[None, None], &[2, 1]);
        assert_eq!(
            seen(&assign_beats(&beats, None, &reqs)),
            [(Some(0), false), (Some(0), true), (Some(1), true)]
        );
    }

    /// The byte lanes of a burst's beats: an unaligned INCR start takes what the address leaves of
    /// its own transfer and the next beat is aligned, FIXED repeats the first beat's lanes, and a
    /// WRAP whose box is smaller than the bus counts up in the box.
    #[test]
    fn strobe_lanes_beats() {
        // INCR, four-byte bus, two bytes a beat from 0x102: lanes 2,3, then the bus's low half
        assert_eq!(strobe_lanes(4, 0x102, 2, "01", 4, 0), (2, 3));
        assert_eq!(strobe_lanes(4, 0x102, 2, "01", 4, 1), (0, 1));
        // FIXED: every beat is the first
        assert_eq!(strobe_lanes(4, 0x102, 2, "00", 4, 0), (2, 3));
        assert_eq!(strobe_lanes(4, 0x102, 2, "00", 4, 2), (2, 3));
        // WRAP, a box of two bytes under a four-byte bus, one byte a beat from 1: lane 1, then 0
        assert_eq!(strobe_lanes(4, 1, 1, "10", 2, 0), (1, 1));
        assert_eq!(strobe_lanes(4, 1, 1, "10", 2, 1), (0, 0));
        // ... and a four-beat box counts to the bus's bottom
        assert_eq!(strobe_lanes(4, 1, 1, "10", 4, 3), (0, 0));
        // an aligned INCR of the whole bus
        assert_eq!(strobe_lanes(4, 0x100, 4, "01", 2, 1), (0, 3));
        // lane k is the k-th character from the end of the MSB-first bits
        assert_eq!(strobe_bad_lanes(4, 0x100, 2, "01", 2, 0, "1111"), [2, 3]);
        assert!(strobe_bad_lanes(4, 0x100, 2, "01", 2, 0, "0011").is_empty());
    }

    #[test]
    fn assign_beats_len_one() {
        // a length that cannot be read counts one beat, so the next beat is another request's
        let beats = [beat(None), beat(None)];
        let reqs = reqs(&[None, None], &[0, 1]);
        assert_eq!(
            seen(&assign_beats(&beats, None, &reqs)),
            [(Some(0), true), (Some(1), true)]
        );
    }
}
