//! AXI4-Lite on the `Protocol` trait (spec §16 "AXI4-Lite"): its five channels, and each side's
//! handshakes joined into a transaction.

use crate::decode::join_schema;
use crate::protocol::axi::{self, Chan, Handshakes};
use crate::protocol::{
    Def, Default, Desc, Field, Given, Out, Params, Protocol, RoleDef, Roles, Severity, StreamDesc,
    reset_low,
};
use crate::time::Tick;

/// The channels, in the order their streams are made, and the rules each one can break: Lite has no
/// exclusive access, so `resp` and `exokay` split the response's two bits between them.
pub const CHANS: &[Chan] = &[
    Chan {
        name: "aw",
        payload: &[("addr", "awaddr"), ("prot", "awprot")],
        rules: &[
            "aw_dropped",
            "aw_changed",
            "aw_reset",
            "aw_unknown",
            "aw_payload_unknown",
        ],
    },
    Chan {
        name: "w",
        payload: &[("data", "wdata"), ("strb", "wstrb")],
        rules: &[
            "w_dropped",
            "w_changed",
            "w_reset",
            "w_unknown",
            "w_payload_unknown",
        ],
    },
    Chan {
        name: "b",
        payload: &[("resp", "bresp")],
        rules: &[
            "b_dropped",
            "b_changed",
            "b_resp",
            "b_exokay",
            "b_reset",
            "b_unknown",
            "b_payload_unknown",
            "b_early",
        ],
    },
    Chan {
        name: "ar",
        payload: &[("addr", "araddr"), ("prot", "arprot")],
        rules: &[
            "ar_dropped",
            "ar_changed",
            "ar_reset",
            "ar_unknown",
            "ar_payload_unknown",
        ],
    },
    Chan {
        name: "r",
        payload: &[("data", "rdata"), ("resp", "rresp")],
        rules: &[
            "r_dropped",
            "r_changed",
            "r_resp",
            "r_exokay",
            "r_reset",
            "r_unknown",
            "r_payload_unknown",
            "r_early",
        ],
    },
];

/// What the spec says of each rule, and how sure that is: it forbids a handshake error and an
/// EXOKAY alike, but a SLVERR is only the slave saying so.
pub const RULES: &[axi::RuleKind] = &[
    axi::RuleKind {
        suffix: "dropped",
        about: "a valid fell before its ready: it stays until the handshake",
        spec: "A2.3",
        severity: Severity::Error,
    },
    axi::RuleKind {
        suffix: "changed",
        about: "a payload signal changed while its valid was stalled",
        spec: "A2.3",
        severity: Severity::Error,
    },
    axi::RuleKind {
        suffix: "resp",
        about: "a SLVERR or DECERR response",
        spec: "A3.3",
        severity: Severity::Warning,
    },
    axi::RuleKind {
        suffix: "exokay",
        about: "an EXOKAY response: Lite has no exclusive access",
        spec: "A3.3",
        severity: Severity::Error,
    },
    axi::RuleKind {
        suffix: "reset",
        about: "a valid while the interface is in reset, or in its first cycle out",
        spec: "A2.1.2",
        severity: Severity::Error,
    },
    axi::RuleKind {
        suffix: "unknown",
        about: "a valid or ready is x or z out of reset",
        spec: "A2.3",
        severity: Severity::Warning,
    },
    axi::RuleKind {
        suffix: "payload_unknown",
        about: "a payload signal is x or z while its valid",
        spec: "A2.3",
        severity: Severity::Warning,
    },
    axi::RuleKind {
        suffix: "early",
        about: "a response before its request's handshakes",
        spec: "A2.3.2",
        severity: Severity::Error,
    },
];

pub const DEF: Def = Def {
    about: "AXI4-Lite: write and read transactions with their delay, handshake errors, error responses",
    example: "decode bus axilite clock=aclk prefix=s_axi_",
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
            about: "the master offers a write address",
        },
        RoleDef {
            name: "awready",
            required: true,
            default: None,
            about: "the slave takes the write address",
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
            about: "the master offers write data",
        },
        RoleDef {
            name: "wready",
            required: true,
            default: None,
            about: "the slave takes write data",
        },
        RoleDef {
            name: "wdata",
            required: true,
            default: None,
            about: "the write's data",
        },
        RoleDef {
            name: "bvalid",
            required: true,
            default: None,
            about: "the slave offers the write's response",
        },
        RoleDef {
            name: "bready",
            required: true,
            default: None,
            about: "the master takes the write's response",
        },
        RoleDef {
            name: "arvalid",
            required: true,
            default: None,
            about: "the master offers a read address",
        },
        RoleDef {
            name: "arready",
            required: true,
            default: None,
            about: "the slave takes the read address",
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
            about: "the slave offers read data",
        },
        RoleDef {
            name: "rready",
            required: true,
            default: None,
            about: "the master takes read data",
        },
        RoleDef {
            name: "rdata",
            required: true,
            default: None,
            about: "the read's data",
        },
        RoleDef {
            name: "awprot",
            required: false,
            default: Some(Default::Bits("000")),
            about: "the write's protection (not given: 0)",
        },
        RoleDef {
            name: "wstrb",
            required: false,
            default: Some(Default::Bytes("wdata")),
            about: "which write bytes are data (not given: all)",
        },
        RoleDef {
            name: "bresp",
            required: false,
            default: Some(Default::Bits("00")),
            about: "the write's response: OKAY or not (not given: OKAY)",
        },
        RoleDef {
            name: "arprot",
            required: false,
            default: Some(Default::Bits("000")),
            about: "the read's protection (not given: 0)",
        },
        RoleDef {
            name: "rresp",
            required: false,
            default: Some(Default::Bits("00")),
            about: "the read's response: OKAY or not (not given: OKAY)",
        },
        RoleDef {
            name: "aresetn",
            required: false,
            default: None,
            about: "the reset, active low (reset_active=high: active high)",
        },
    ],
    params: &["timeout", "reset_active"],
    make,
};

/// What a side's stream is, and what its unmatched stream means.
const WR: &str = "writes: AW and W in either order, then B; delay from the later of AW and W to B";
const NO_WR: &str = "a write never answered, or not within `timeout` (no response), its B after that (no request), a W without an AW (no aw) or the reverse (no w)";
const RD: &str = "reads: AR, then R; delay from AR to R";
const NO_RD: &str =
    "an AR never answered, or not within `timeout` (no response), its R after that (no request)";

/// The instance's channels, the streams it makes, and the timeout.
struct Axilite {
    h: Handshakes,
    desc: Desc,
    timeout: Option<Tick>,
}

fn make(given: &Given, params: &Params, fpt: Option<u128>) -> Result<Box<dyn Protocol>, String> {
    Axilite::new(given, params, fpt).map(|a| Box::new(a) as Box<dyn Protocol>)
}

impl Axilite {
    fn new(given: &Given, params: &Params, fpt: Option<u128>) -> Result<Self, String> {
        // the spec's reset is active low (A2.1.2)
        let h = Handshakes::new(given, CHANS, RULES, DEF.roles, reset_low(params)?);
        let field = |name: &str| Field {
            name: name.into(),
            width: None,
            time: false,
            signed: false,
        };
        // the channels the port gives, then the rules, then each side whose channels are all there
        let mut streams: Vec<StreamDesc> = CHANS.iter().filter_map(|c| h.desc(c.name)).collect();
        streams.push(h.errors());
        if let (Some(aw), Some(w), Some(b)) = (h.desc("aw"), h.desc("w"), h.desc("b")) {
            let requests = [("aw", &aw.fields[..]), ("w", &w.fields[..])];
            streams.push(StreamDesc {
                name: "wr",
                fields: join_schema(&requests, &b.fields),
                violations: false,
                from: &["aw", "w", "b"],
                about: vec![WR.into()],
            });
            streams.push(StreamDesc {
                name: "wr.unmatched",
                fields: vec![field("kind")],
                violations: true,
                from: &[],
                about: vec![NO_WR.into()],
            });
        }
        if let (Some(ar), Some(r)) = (h.desc("ar"), h.desc("r")) {
            let requests = [("ar", &ar.fields[..])];
            streams.push(StreamDesc {
                name: "rd",
                fields: join_schema(&requests, &r.fields),
                violations: false,
                from: &["ar", "r"],
                about: vec![RD.into()],
            });
            streams.push(StreamDesc {
                name: "rd.unmatched",
                fields: vec![field("kind")],
                violations: true,
                from: &[],
                about: vec![NO_RD.into()],
            });
        }
        Ok(Axilite {
            h,
            desc: Desc { streams },
            timeout: axi::timeout(params, fpt)?,
        })
    }
}

impl Protocol for Axilite {
    fn desc(&self) -> &Desc {
        &self.desc
    }

    fn feed(&mut self, start: Tick, end: Tick, roles: &Roles<'_>) -> Vec<Out> {
        self.h.feed(start, end, roles)
    }

    /// The transactions still open when the trace ends.
    fn finish(&mut self) -> Vec<Out> {
        let made = |name: &str| self.desc.streams.iter().any(|s| s.name == name);
        let (wr, rd) = (made("wr"), made("rd"));
        let mut out = Vec::new();
        if wr {
            out.extend(
                self.h
                    .join("wr", "wr.unmatched", &["aw", "w"], "b", self.timeout)
                    .unwrap_or_default(),
            );
        }
        if rd {
            out.extend(
                self.h
                    .join("rd", "rd.unmatched", &["ar"], "r", self.timeout)
                    .unwrap_or_default(),
            );
        }
        out
    }
}
