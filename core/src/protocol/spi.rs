//! SPI (spec §16 "SPI"): a bit at each sclk edge `mode` samples on, `bits` of them a word, and a
//! transfer from cs falling to cs rising. One instance makes the bit, word and transfer streams.

use crate::decode::{invert, param};
use crate::protocol::{Def, Desc, Field, Given, Out, Params, Protocol, RoleDef, Roles, StreamDesc};
use crate::time::Tick;
use crate::trace::Val;

pub const DEF: Def = Def {
    about: "SPI: bits at sclk's sampling edge, words, transfers framed by cs (active low; cs_active=high)",
    example: "decode flash spi sclk=spi_clk cs=spi_cs_n mosi=spi_mosi miso=spi_miso mode=0",
    clocked: false,
    sides: &[],
    roles: &[
        RoleDef {
            name: "sclk",
            required: true,
            default: None,
            about: "the serial clock, 1 bit",
        },
        RoleDef {
            name: "cs",
            required: true,
            default: None,
            about: "the chip select, 1 bit",
        },
        RoleDef {
            name: "mosi",
            required: true,
            default: None,
            about: "the data out, 1 bit",
        },
        RoleDef {
            name: "miso",
            required: false,
            default: None,
            about: "the data in, 1 bit: adds the field `miso`",
        },
    ],
    params: &["mode", "bits", "order", "cs_active"],
    make,
};

const BIT: &str = "a bit of mosi, and miso if it is given, sampled at each sclk edge that `mode` (0 - 3, CPOL and CPHA) samples on, rising for modes 0 and 3, falling for 1 and 2, with cs low, from the values just before it, and lasting to the next one, the last of a transfer to sclk's next edge";
const WORD: &str =
    "`bits` bits (MSB first, or LSB with order=lsb) make a word, `complete` 0 if cs rose first";
const TRANSFER: &str = "cs low to high, its words as hex text, mosi and miso the bits one after the other, `complete` 0 if the trace ended first";

fn make(
    given: &Given,
    params: &Params,
    _fs_per_tick: Option<u128>,
) -> Result<Box<dyn Protocol>, String> {
    Spi::new(given, params).map(|s| Box::new(s) as Box<dyn Protocol>)
}

pub struct Spi {
    rising: bool,
    bits: usize,
    lsb: bool,
    miso: bool,
    /// cs is active high: read inverted.
    cs_high: bool,
    prev: Option<[u8; 4]>,
    end: Tick,
    /// The bit sampled last, waiting for its end: its start, mosi, miso.
    bit: Option<(Tick, u8, u8)>,
    /// sclk's first other edge after it, where the last bit of a transfer ends.
    shift: Option<Tick>,
    /// The word being shifted in: its start, and its bits.
    word: Option<(Tick, Vec<(u8, u8)>)>,
    /// The transfer since cs fell: its start, and its words' mosi and miso.
    transfer: Option<(Tick, Vec<String>, Vec<String>)>,
    desc: Desc,
}

impl Spi {
    fn new(given: &Given, params: &Params) -> Result<Self, String> {
        let mut errors = Vec::new();
        let mode = param(
            params,
            "mode",
            Some(0u8),
            |m| *m < 4,
            "0, 1, 2 or 3",
            &mut errors,
        );
        let bits = param(
            params,
            "bits",
            Some(8usize),
            |b| (1..=64).contains(b),
            "1 - 64",
            &mut errors,
        );
        let order = param(
            params,
            "order",
            Some("msb".to_string()),
            |o| o == "msb" || o == "lsb",
            "msb or lsb",
            &mut errors,
        );
        let cs_active = param(
            params,
            "cs_active",
            Some("low".to_string()),
            |a| a == "low" || a == "high",
            "low or high",
            &mut errors,
        );
        if let Some((k, _)) = params.iter().find(|p| !DEF.params.contains(&p.0.as_str())) {
            errors.push(format!("no param {k} ({})", DEF.params.join(", ")));
        }
        if !errors.is_empty() {
            return Err(format!("spi: {}", errors.join("; ")));
        }
        let miso = given.iter().any(|g| g.0 == "miso");
        let bits = bits.unwrap_or(8);
        let field = |name: &str, width: Option<u32>| Field {
            name: name.into(),
            width,
            time: false,
            signed: false,
        };
        let mut bit = vec![field("mosi", Some(1))];
        let mut word = vec![field("mosi", Some(bits as u32))];
        let mut transfer = vec![field("words", Some(32)), field("mosi", None)];
        if miso {
            bit.push(field("miso", Some(1)));
            word.push(field("miso", Some(bits as u32)));
            transfer.push(field("miso", None));
        }
        word.push(field("complete", Some(1)));
        transfer.push(field("complete", Some(1)));
        let streams = vec![
            StreamDesc {
                name: "bit",
                fields: bit,
                violations: false,
                about: vec![BIT.into()],
                from: &[],
            },
            StreamDesc {
                name: "word",
                fields: word,
                violations: false,
                about: vec![WORD.into()],
                from: &["bit"],
            },
            StreamDesc {
                name: "transfer",
                fields: transfer,
                violations: false,
                about: vec![TRANSFER.into()],
                from: &["word"],
            },
        ];
        let mode = mode.unwrap_or(0);
        Ok(Spi {
            rising: mode == 0 || mode == 3,
            bits,
            lsb: order.as_deref() == Some("lsb"),
            miso,
            cs_high: cs_active.as_deref() == Some("high"),
            prev: None,
            end: 0,
            bit: None,
            shift: None,
            word: None,
            transfer: None,
            desc: Desc { streams },
        })
    }

    /// The sampled bit ends at `t`: shifted into the word, which ends with it once full.
    fn end_bit(&mut self, t: Tick, out: &mut Vec<Out>) {
        let Some((start, mosi, miso)) = self.bit.take() else {
            return;
        };
        let mut fields = vec![Some(Val::Bits((mosi as char).into()))];
        if self.miso {
            fields.push(Some(Val::Bits((miso as char).into())));
        }
        out.push(Out::Record {
            stream: "bit",
            start,
            end: t,
            fields,
        });
        let word = self.word.get_or_insert((start, Vec::new()));
        word.1.push((mosi, miso));
        if word.1.len() == self.bits {
            self.end_word(t, true, out);
        }
    }

    fn end_word(&mut self, t: Tick, complete: bool, out: &mut Vec<Out>) {
        let Some((start, mut bits)) = self.word.take() else {
            return;
        };
        if self.lsb {
            bits.reverse();
        }
        let line = |k: usize| {
            bits.iter()
                .map(|b| [b.0, b.1][k] as char)
                .collect::<String>()
        };
        let (mosi, miso) = (line(0), line(1));
        if let Some(tr) = &mut self.transfer {
            tr.1.push(hex(&mosi));
            tr.2.push(hex(&miso));
        }
        let mut fields = vec![Some(Val::Bits(mosi))];
        if self.miso {
            fields.push(Some(Val::Bits(miso)));
        }
        fields.push(bits_val(complete));
        out.push(Out::Record {
            stream: "word",
            start,
            end: t,
            fields,
        });
    }

    fn end_transfer(&mut self, t: Tick, complete: bool, out: &mut Vec<Out>) {
        let Some((start, mosi, miso)) = self.transfer.take() else {
            return;
        };
        let mut fields = vec![
            Some(Val::Bits(format!("{:032b}", mosi.len()))),
            Some(Val::Text(mosi.join(" "))),
        ];
        if self.miso {
            fields.push(Some(Val::Text(miso.join(" "))));
        }
        fields.push(bits_val(complete));
        out.push(Out::Record {
            stream: "transfer",
            start,
            end: t,
            fields,
        });
    }
}

/// cs, sclk, mosi and miso at this cycle; a missing miso reads as 0.
fn pins(roles: &Roles<'_>) -> Option<[u8; 4]> {
    let mut v = [b'0'; 4];
    for (i, role) in ["cs", "sclk", "mosi"].into_iter().enumerate() {
        v[i] = level(roles, role)?;
    }
    v[3] = level(roles, "miso").unwrap_or(b'0');
    Some(v)
}

/// A role's level as the character it is: x and z stay, a wide signal is not read.
fn level(roles: &Roles<'_>, name: &str) -> Option<u8> {
    match roles.get(name)? {
        Val::Bits(b) => b.as_bytes().first().copied(),
        Val::Text(_) => None,
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

fn bits_val(b: bool) -> Option<Val> {
    Some(Val::Bits(if b { "1" } else { "0" }.into()))
}

impl Protocol for Spi {
    fn desc(&self) -> &Desc {
        &self.desc
    }

    fn feed(&mut self, start: Tick, end: Tick, roles: &Roles<'_>) -> Vec<Out> {
        // cs, sclk, mosi, miso; a missing miso reads as 0
        let Some(mut v) = pins(roles) else {
            return Vec::new();
        };
        if self.cs_high {
            v[0] = invert(v[0]);
        }
        if !self.miso {
            v[3] = b'0';
        }
        let mut out = Vec::new();
        self.end = end;
        if let Some(p) = self.prev.replace(v) {
            let edge = if self.rising {
                (p[1], v[1]) == (b'0', b'1')
            } else {
                (p[1], v[1]) == (b'1', b'0')
            };
            if p[0] == b'0' && edge {
                self.end_bit(start, &mut out);
                self.bit = Some((start, p[2], p[3]));
                self.shift = None;
            } else if p[1] != v[1] && self.bit.is_some() {
                self.shift = self.shift.or(Some(start));
            }
            if p[0] == b'0' && v[0] == b'1' {
                // the last bit ends where sclk shifts, not where cs rises however long after
                self.end_bit(self.shift.unwrap_or(start), &mut out);
                self.end_word(start, false, &mut out);
                self.end_transfer(start, true, &mut out);
            }
            if p[0] == b'1' && v[0] == b'0' {
                self.transfer = Some((start, Vec::new(), Vec::new()));
            }
        }
        out
    }

    fn finish(&mut self) -> Vec<Out> {
        let mut out = Vec::new();
        let end = self.end;
        self.end_bit(end, &mut out);
        self.end_word(end, false, &mut out);
        self.end_transfer(end, false, &mut out);
        out
    }
}
