//! AXI4, from Arm's spec (IHI 0022): the five channels, write and read bursts as transactions
//! paired by ID with their latency, error responses (spec §16 "AXI4").

use crate::decode::{self, Decoder};
use crate::protocol::axi::{self, Chan, Handshakes, RuleKind};
use crate::protocol::{
    Def, Desc, Given, Out, Params, Protocol, RoleDef, Roles, Severity, StreamDesc,
};
use crate::stream::{Field, Record, Stream};
use crate::time::Tick;

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
        ],
        rules: &["aw_dropped", "aw_changed", "aw_resp"],
    },
    Chan {
        name: "w",
        payload: &[("data", "wdata"), ("strb", "wstrb"), ("last", "wlast")],
        rules: &["w_dropped", "w_changed", "w_resp"],
    },
    Chan {
        name: "b",
        payload: &[("id", "bid"), ("resp", "bresp")],
        rules: &["b_dropped", "b_changed", "b_resp"],
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
        ],
        rules: &["ar_dropped", "ar_changed", "ar_resp"],
    },
    Chan {
        name: "r",
        payload: &[
            ("data", "rdata"),
            ("id", "rid"),
            ("resp", "rresp"),
            ("last", "rlast"),
        ],
        rules: &["r_dropped", "r_changed", "r_resp"],
    },
];

/// The rules of IHI 0022, all three of them errors. A response that is not OKAY counts EXOKAY with
/// them: AXI4 allows it for exclusive access, which wtf does not decode.
const RULES: &[RuleKind] = &[
    RuleKind {
        suffix: "dropped",
        about: "a valid fell before its ready: it stays until the handshake",
        spec: "A3.2.1",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "changed",
        about: "a payload signal changed while its valid was stalled",
        spec: "A3.2.1",
        severity: Severity::Error,
    },
    RuleKind {
        suffix: "resp",
        about: "a response that is not OKAY",
        spec: "A6.1",
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
        burst_about: "the W beats of a burst up to wlast; data the bytes wstrb marks",
        about: "writes: AW and its W burst in order, then the B with its ID; delay from the later of AW and the burst's first beat to B",
        unmatched_about: "a B without a write (no request), a write never answered, or not within `timeout` (no response), a W burst without an AW (no aw) or the reverse (no wburst)",
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
        burst_about: "the R beats of a burst up to rlast, each ID's apart (bursts of different IDs interleave)",
        about: "reads: AR, then the R burst with its ID; delay (latency) from AR to the first R beat",
        unmatched_about: "an R burst without an AR (no request), an AR never answered, or not within `timeout` (no response)",
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
        let burst = decode::run_records(side.burst, channel(records, side.channel), self.frame);
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
        // the fields are the ones `make` checked, so this cannot fail; a port the trace cannot
        // join gets no join rather than a broken decode
        if let Ok((done, unmatched)) =
            decode::join(side.name, &requests, response, self.id, timeout)
        {
            out.extend(axi::from_stream(side.name, &done));
            out.extend(axi::from_stream(side.unmatched, &unmatched));
        }
        out
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

/// The burst's decoder: `frame` over the channel, its params owned (`Frame::new` takes them so).
fn burst_decoder(
    fields: &[Field],
    params: &[(&str, &str)],
    fs_per_tick: Option<u128>,
) -> Result<Box<dyn Decoder>, String> {
    let params: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    decode::Frame::new(fields, &params, fs_per_tick).map(|f| Box::new(f) as Box<dyn Decoder>)
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
}

impl Axi4 {
    fn new(given: &Given, params: &Params, fs_per_tick: Option<u128>) -> Result<Self, String> {
        let timeout = axi::timeout(params, fs_per_tick)?;
        let channels = Handshakes::new(given, CHANS, RULES, DEF.roles, true);
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
            let mut frame_params = vec![("last", "last"), ("data", "data")];
            let (key, role) = side.burst_param;
            if fields.iter().any(|f| f.name == role) {
                frame_params.push((key, role));
            }
            let frame = burst_decoder(&fields, &frame_params, fs_per_tick)?;
            let id = side
                .pairs_id(&channels, &frame_params)
                .then_some((side.address, "id", "id"));
            let live = Live { side, id, frame };
            streams.extend(live.descs(&channels));
            sides.push(live);
        }
        Ok(Axi4 {
            channels,
            desc: Desc { streams },
            timeout,
            sides,
            records: vec![Vec::new(); CHANS.len()],
        })
    }
}

impl Protocol for Axi4 {
    fn desc(&self) -> &Desc {
        &self.desc
    }

    fn feed(&mut self, start: Tick, end: Tick, roles: &Roles<'_>) -> Vec<Out> {
        let out = self.channels.feed(start, end, roles);
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
    about: "AXI4: write and read bursts as transactions, paired by ID, with their latency; handshake errors, error responses",
    example: "decode mem axi4 clock=aclk prefix=m_axi_",
    clocked: true,
    sides: &[
        &[
            "awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "wlast", "bvalid",
            "bready",
        ],
        &[
            "arvalid", "arready", "araddr", "rvalid", "rready", "rdata", "rlast",
        ],
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
            required: true,
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
            required: true,
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
            default: None,
            about: "the burst's length in beats, less one",
        },
        RoleDef {
            name: "awsize",
            required: false,
            default: None,
            about: "the bytes per beat, as a power of two",
        },
        RoleDef {
            name: "awburst",
            required: false,
            default: None,
            about: "the burst's type (A3.1)",
        },
        RoleDef {
            name: "awprot",
            required: false,
            default: None,
            about: "the write's protection",
        },
        RoleDef {
            name: "wstrb",
            required: false,
            default: None,
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
            default: None,
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
            default: None,
            about: "the burst's length in beats, less one",
        },
        RoleDef {
            name: "arsize",
            required: false,
            default: None,
            about: "the bytes per beat, as a power of two",
        },
        RoleDef {
            name: "arburst",
            required: false,
            default: None,
            about: "the burst's type (A3.1)",
        },
        RoleDef {
            name: "arprot",
            required: false,
            default: None,
            about: "the read's protection",
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
            default: None,
            about: "the read's response (0: OKAY)",
        },
    ],
    params: &["timeout"],
    make,
};

fn make(
    given: &Given,
    params: &Params,
    fs_per_tick: Option<u128>,
) -> Result<Box<dyn Protocol>, String> {
    Axi4::new(given, params, fs_per_tick).map(|a| Box::new(a) as Box<dyn Protocol>)
}
