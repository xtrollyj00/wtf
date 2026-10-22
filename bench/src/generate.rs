//! Writes `<dir>/bench.vcd` or `.fst`: a clock, a reset, `top.hot` (a 32-bit count of the cycles,
//! so it has as many transitions as the trace has cycles) and `top.u<i>` modules, each an
//! AXI4-Lite slave port with random transactions, a count of them and its state. The same
//! arguments write the same trace. It runs until the file reaches the size asked for.

use fst_writer::{FstBodyWriter, FstFileType, FstInfo, FstScopeType, FstSignalId, FstSignalType, FstVarDirection, FstVarType};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Clock period in ticks (1 ns); the design changes on rising edges, at multiples of it.
pub const PERIOD: u64 = 10;
const RESET_CYCLES: u64 = 4;

/// Each module's signals, in this order.
const PORT: [(&str, u32); 19] = [
    ("s_axi_awvalid", 1), ("s_axi_awready", 1), ("s_axi_awaddr", 32),
    ("s_axi_wvalid", 1), ("s_axi_wready", 1), ("s_axi_wdata", 32), ("s_axi_wstrb", 4),
    ("s_axi_bvalid", 1), ("s_axi_bready", 1), ("s_axi_bresp", 2),
    ("s_axi_arvalid", 1), ("s_axi_arready", 1), ("s_axi_araddr", 32),
    ("s_axi_rvalid", 1), ("s_axi_rready", 1), ("s_axi_rdata", 32), ("s_axi_rresp", 2),
    ("count", 16), ("state", 3),
];
const AWVALID: usize = 0;
const AWREADY: usize = 1;
const AWADDR: usize = 2;
const WVALID: usize = 3;
const WREADY: usize = 4;
const WDATA: usize = 5;
const WSTRB: usize = 6;
const BVALID: usize = 7;
const BREADY: usize = 8;
const BRESP: usize = 9;
const ARVALID: usize = 10;
const ARREADY: usize = 11;
const ARADDR: usize = 12;
const RVALID: usize = 13;
const RREADY: usize = 14;
const RDATA: usize = 15;
const RRESP: usize = 16;
const COUNT: usize = 17;
const STATE: usize = 18;
/// Top-level signals: clk, rst_n, hot.
const TOP: [(&str, u32); 3] = [("clk", 1), ("rst_n", 1), ("hot", 32)];

pub fn main(args: &[String]) -> Result<(), String> {
    let (opts, dir) = super::options(args, &["size", "format", "modules"])?;
    let (mut size, mut format, mut modules) = (1u64 << 30, "fst".to_string(), 200usize);
    for (n, v) in opts {
        match n.as_str() {
            "size" => size = parse_size(&v)?,
            "format" => format = v,
            _ => modules = v.parse().map_err(|_| format!("--modules {v}: not a number"))?,
        }
    }
    let path = PathBuf::from(dir).join(format!("bench.{format}"));
    let widths: Vec<u32> = TOP.iter().map(|s| s.1).chain((0..modules).flat_map(|_| PORT.iter().map(|s| s.1))).collect();
    let mut sink: Box<dyn Sink> = match format.as_str() {
        "vcd" => Box::new(Vcd::create(&path, modules)?),
        "fst" => Box::new(Fst::create(&path, modules)?),
        _ => return Err(format!("--format {format}: vcd or fst")),
    };
    let start = std::time::Instant::now();
    let cycles = simulate(sink.as_mut(), &widths, modules, size).map_err(|e| format!("{}: {e}", path.display()))?;
    sink.finish().map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    println!(
        "{}: {} signals, {cycles} cycles (top.hot: {cycles} transitions), {:.2} GiB in {:.0?}",
        path.display(),
        widths.len(),
        bytes as f64 / (1u64 << 30) as f64,
        start.elapsed()
    );
    Ok(())
}

fn parse_size(s: &str) -> Result<u64, String> {
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let shift = match unit.to_ascii_uppercase().as_str() {
        "" => 0,
        "K" => 10,
        "M" => 20,
        "G" => 30,
        _ => return Err(format!("--size {s}: a number with K, M or G")),
    };
    Ok(n.parse::<u64>().map_err(|_| format!("--size {s}: a number with K, M or G"))? << shift)
}

/// A trace being written: changes at a time, signals by index (TOP, then each module's PORT).
trait Sink {
    fn time(&mut self, t: u64) -> std::io::Result<()>;
    fn change(&mut self, sig: usize, width: u32, v: u64) -> std::io::Result<()>;
    /// Bytes in the file so far.
    fn size(&self) -> u64;
    fn finish(self: Box<Self>) -> std::io::Result<()>;
}

/// Runs the design until the sink holds `size` bytes; returns the cycles written.
fn simulate(sink: &mut dyn Sink, widths: &[u32], modules: usize, size: u64) -> std::io::Result<u64> {
    let mut values: Vec<u64> = vec![0; widths.len()];
    let mut ms: Vec<Module> = (0..modules).map(|i| Module { phase: 0, wait: 0, count: 0, rng: 0x9E37_79B9_7F4A_7C15 ^ (i as u64 + 1) }).collect();
    sink.time(0)?;
    for (i, &w) in widths.iter().enumerate() {
        sink.change(i, w, 0)?;
    }
    let set = |sink: &mut dyn Sink, values: &mut Vec<u64>, i: usize, v: u64| -> std::io::Result<()> {
        if values[i] != v {
            values[i] = v;
            sink.change(i, widths[i], v)?;
        }
        Ok(())
    };
    let mut cycle = 0u64;
    // checking the size costs a syscall for FST, so not every cycle
    while !cycle.is_multiple_of(1024) || sink.size() < size {
        let t = cycle * PERIOD;
        if t > 0 {
            sink.time(t)?;
        }
        set(sink, &mut values, 0, 1)?;
        set(sink, &mut values, 1, (cycle >= RESET_CYCLES) as u64)?;
        set(sink, &mut values, 2, cycle & 0xFFFF_FFFF)?;
        if cycle >= RESET_CYCLES {
            for (m, module) in ms.iter_mut().enumerate() {
                let base = TOP.len() + m * PORT.len();
                for (k, v) in module.step() {
                    set(sink, &mut values, base + k, v)?;
                }
            }
        } else {
            for m in 0..modules {
                let base = TOP.len() + m * PORT.len();
                set(sink, &mut values, base + BREADY, 1)?;
                set(sink, &mut values, base + RREADY, 1)?;
            }
        }
        sink.time(t + PERIOD / 2)?;
        set(sink, &mut values, 0, 0)?;
        cycle += 1;
    }
    Ok(cycle)
}

/// An AXI4-Lite slave answering random writes and reads: one in 64 idle cycles starts one, the
/// address waits 0 - 3 cycles for ready, one read in 32 gets SLVERR.
struct Module {
    phase: u64,
    wait: u64,
    count: u64,
    rng: u64,
}

impl Module {
    fn rand(&mut self) -> u64 {
        // xorshift64
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// The signals changed by this rising edge.
    fn step(&mut self) -> Vec<(usize, u64)> {
        let r = self.rand();
        let mut ch = match self.phase {
            0 if !r.is_multiple_of(64) => vec![],
            0 if r & 64 != 0 => {
                (self.phase, self.wait) = (1, r >> 8 & 3);
                vec![(AWVALID, 1), (AWADDR, r >> 16 & 0xFFC), (WVALID, 1), (WDATA, r >> 32), (WSTRB, 0xF)]
            }
            0 => {
                (self.phase, self.wait) = (3, r >> 8 & 3);
                vec![(ARVALID, 1), (ARADDR, r >> 16 & 0xFFC)]
            }
            1 | 3 if self.wait > 0 => {
                self.wait -= 1;
                vec![]
            }
            1 => {
                self.phase = 2;
                vec![(AWREADY, 1), (WREADY, 1)]
            }
            2 => {
                self.phase = 5;
                vec![(AWVALID, 0), (AWREADY, 0), (WVALID, 0), (WREADY, 0), (BVALID, 1), (BRESP, 0)]
            }
            3 => {
                self.phase = 4;
                vec![(ARREADY, 1)]
            }
            4 => {
                self.phase = 5;
                vec![(ARVALID, 0), (ARREADY, 0), (RVALID, 1), (RDATA, r >> 32), (RRESP, if r >> 8 & 31 == 0 { 2 } else { 0 })]
            }
            _ => {
                self.phase = 0;
                self.count = (self.count + 1) & 0xFFFF;
                vec![(BVALID, 0), (RVALID, 0), (COUNT, self.count)]
            }
        };
        ch.push((STATE, self.phase));
        ch
    }
}

/// The hierarchy, in signal order: scope `top` with TOP, then `top.u<i>` with PORT.
fn hierarchy(modules: usize) -> impl Iterator<Item = (Option<usize>, &'static str, u32)> {
    TOP.iter().map(|&(n, w)| (None, n, w)).chain((0..modules).flat_map(|m| PORT.iter().map(move |&(n, w)| (Some(m), n, w))))
}

struct Vcd {
    out: BufWriter<File>,
    written: u64,
    ids: Vec<String>,
    line: Vec<u8>,
}

impl Vcd {
    fn create(path: &Path, modules: usize) -> Result<Self, String> {
        let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut v = Vcd { out: BufWriter::with_capacity(1 << 20, file), written: 0, ids: vec![], line: vec![] };
        let mut head = String::from("$timescale 1ns $end\n$scope module top $end\n");
        let mut scope = None;
        for (i, (m, name, w)) in hierarchy(modules).enumerate() {
            if m != scope {
                if scope.is_some() {
                    head += "$upscope $end\n";
                }
                head += &format!("$scope module u{} $end\n", m.unwrap());
                scope = m;
            }
            // identifiers from the printable characters, base 94
            let (mut n, mut id) = (i, String::new());
            loop {
                id.push((b'!' + (n % 94) as u8) as char);
                n /= 94;
                if n == 0 {
                    break;
                }
            }
            head += &format!("$var wire {w} {id} {name} $end\n");
            v.ids.push(id);
        }
        head += if scope.is_some() { "$upscope $end\n$upscope $end\n" } else { "$upscope $end\n" };
        head += "$enddefinitions $end\n";
        v.write(head.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(v)
    }

    fn write(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.written += b.len() as u64;
        self.out.write_all(b)
    }
}

impl Sink for Vcd {
    fn time(&mut self, t: u64) -> std::io::Result<()> {
        self.write(format!("#{t}\n").as_bytes())
    }

    fn change(&mut self, sig: usize, width: u32, v: u64) -> std::io::Result<()> {
        let mut line = std::mem::take(&mut self.line);
        line.clear();
        if width == 1 {
            line.push(b'0' + v as u8);
        } else {
            write!(line, "b{v:b} ")?;
        }
        line.extend_from_slice(self.ids[sig].as_bytes());
        line.push(b'\n');
        let r = self.write(&line);
        self.line = line;
        r
    }

    fn size(&self) -> u64 {
        self.written
    }

    fn finish(mut self: Box<Self>) -> std::io::Result<()> {
        self.out.flush()
    }
}

struct Fst {
    out: FstBodyWriter<BufWriter<File>>,
    path: PathBuf,
    ids: Vec<FstSignalId>,
    bits: Vec<u8>,
}

/// Buffered changes written out at this size, as fst-writer's own converter does.
const FLUSH_AT: usize = 128 << 20;

fn fst_err(e: fst_writer::FstWriteError) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

impl Fst {
    fn create(path: &Path, modules: usize) -> Result<Self, String> {
        let err = |e: fst_writer::FstWriteError| format!("{}: {e}", path.display());
        let info = FstInfo { start_time: 0, timescale_exponent: -9, version: "wtf-bench".into(), date: String::new(), file_type: FstFileType::Verilog };
        let mut h = fst_writer::open_fst(path, &info).map_err(err)?;
        h.scope("top", "", FstScopeType::Module).map_err(err)?;
        let (mut ids, mut scope) = (vec![], None);
        for (m, name, w) in hierarchy(modules) {
            if m != scope {
                if scope.is_some() {
                    h.up_scope().map_err(err)?;
                }
                h.scope(format!("u{}", m.unwrap()), "", FstScopeType::Module).map_err(err)?;
                scope = m;
            }
            ids.push(h.var(name, FstSignalType::bit_vec(w), FstVarType::Wire, FstVarDirection::Implicit, None).map_err(err)?);
        }
        if scope.is_some() {
            h.up_scope().map_err(err)?;
        }
        h.up_scope().map_err(err)?;
        Ok(Fst { out: h.finish().map_err(err)?, path: path.to_path_buf(), ids, bits: vec![] })
    }
}

impl Sink for Fst {
    fn time(&mut self, t: u64) -> std::io::Result<()> {
        // fst-writer flushes only right before a new time step
        if self.out.size() >= FLUSH_AT {
            self.out.flush().map_err(fst_err)?;
        }
        self.out.time_change(t).map_err(fst_err)
    }

    fn change(&mut self, sig: usize, width: u32, v: u64) -> std::io::Result<()> {
        self.bits.clear();
        self.bits.extend((0..width).rev().map(|b| b'0' + (v >> b & 1) as u8));
        self.out.signal_change(self.ids[sig], &self.bits).map_err(fst_err)
    }

    fn size(&self) -> u64 {
        // shortcut: what is flushed so far, so a file stops up to one buffer (128 MB of changes,
        // compressed) past the size; fine for sizes in GB.
        std::fs::metadata(&self.path).map_or(0, |m| m.len())
    }

    fn finish(self: Box<Self>) -> std::io::Result<()> {
        self.out.finish().map_err(fst_err)
    }
}
