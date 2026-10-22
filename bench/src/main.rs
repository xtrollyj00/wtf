//! Huge generated traces and wtf's times on them, against the targets in spec §18 "Performance".
//!
//!   cargo run -p wtf-bench --release -- gen --size 10G --format fst <dir>
//!   cargo build --release && cargo run -p wtf-bench --release -- run <dir>/bench.fst

mod generate;
mod run;

const USAGE: &str = "usage:
  wtf-bench gen [--size 1G] [--format vcd|fst] [--modules 200] <dir>
  wtf-bench run [--wtf <path>] <trace>";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("gen") => generate::main(&args[1..]),
        Some("run") => run::main(&args[1..]),
        _ => Err(USAGE.into()),
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// `--name value` options and the one positional argument.
fn options(args: &[String], names: &[&str]) -> Result<(Vec<(String, String)>, String), String> {
    let (mut opts, mut pos) = (vec![], None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.strip_prefix("--") {
            Some(n) if names.contains(&n) => opts.push((n.to_string(), it.next().ok_or(format!("--{n} needs a value\n{USAGE}"))?.clone())),
            Some(n) => return Err(format!("unknown option --{n}\n{USAGE}")),
            None if pos.is_none() => pos = Some(a.clone()),
            None => return Err(format!("one path only\n{USAGE}")),
        }
    }
    Ok((opts, pos.ok_or(USAGE)?))
}
