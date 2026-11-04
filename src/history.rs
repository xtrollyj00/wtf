//! The traces opened on this machine, for the start screen's Recent (spec §4 "Start screen"):
//! `$XDG_STATE_HOME/wtf/history`, one open a line: `<unix seconds>\t<you|agent>\t<trace>`, then
//! `\t<session>` when one was named with `-S`. The newest `KEEP` lines are kept.

use std::path::{Path, PathBuf};

const KEEP: usize = 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub at: u64,
    pub agent: bool,
    pub trace: PathBuf,
    pub session: Option<PathBuf>,
}

/// `$XDG_STATE_HOME/wtf/history`, or `~/.local/state/wtf/history`.
pub fn path() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/state")))?;
    Some(state.join("wtf/history"))
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The opens in the file, oldest first; none if there is no file. Lines that don't parse are
/// skipped.
pub fn read(file: &Path) -> Vec<Open> {
    std::fs::read_to_string(file).unwrap_or_default().lines().filter_map(parse).collect()
}

fn parse(line: &str) -> Option<Open> {
    let mut f = line.split('\t');
    let at = f.next()?.parse().ok()?;
    let agent = match f.next()? {
        "you" => false,
        "agent" => true,
        _ => return None,
    };
    let trace = PathBuf::from(f.next().filter(|t| !t.is_empty())?);
    Some(Open { at, agent, trace, session: f.next().map(PathBuf::from) })
}

/// Adds an open to the file, made if missing. Paths are made absolute; one with a tab or a line
/// break in it isn't kept.
pub fn record(file: &Path, trace: &Path, session: Option<&Path>, agent: bool) -> Result<(), String> {
    let abs = |p: &Path| std::fs::canonicalize(p).or_else(|_| std::path::absolute(p)).map_err(|e| format!("{}: {e}", p.display()));
    let mut line = format!("{}\t{}\t{}", now(), if agent { "agent" } else { "you" }, abs(trace)?.display());
    if let Some(s) = session {
        line += &format!("\t{}", abs(s)?.display());
    }
    if line.matches('\t').count() != 2 + session.is_some() as usize || line.contains('\n') {
        return Ok(());
    }
    let err = |e: std::io::Error| format!("{}: {e}", file.display());
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().collect();
    lines.push(&line);
    let keep = &lines[lines.len().saturating_sub(KEEP)..];
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    // written whole, then renamed over: a reader never sees half a file
    let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, keep.join("\n") + "\n").map_err(err)?;
    std::fs::rename(&tmp, file).map_err(err)
}

/// Records an open in the user's history; a history that can't be written changes nothing else.
pub fn opened(trace: &Path, session: Option<&Path>, agent: bool) {
    if let Some(f) = path() {
        let _ = record(&f, trace, session, agent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let d = std::env::temp_dir().join(format!("wtf-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let file = d.join("wtf/history");
        assert!(read(&file).is_empty());
        let trace = Path::new("tests/data/simple.vcd");
        record(&file, trace, None, false).unwrap();
        record(&file, trace, Some(Path::new("tests/data/simple.session.toml")), true).unwrap();
        let h = read(&file);
        let abs = std::fs::canonicalize(trace).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].agent, &h[0].trace, &h[0].session), (false, &abs, &None));
        assert_eq!((h[1].agent, h[1].session.as_ref().unwrap().file_name().unwrap().to_str()), (true, Some("simple.session.toml")));
        assert!(now() - h[1].at < 60);
        // bounded: the newest kept
        std::fs::write(&file, "1\tyou\t/old.vcd\n".repeat(KEEP)).unwrap();
        record(&file, trace, None, false).unwrap();
        let h = read(&file);
        assert_eq!((h.len(), &h[KEEP - 1].trace), (KEEP, &abs));
        // a line of another kind, or with no trace, is skipped
        std::fs::write(&file, "1\tbot\t/a.vcd\n2\tyou\t\n3\tyou\t/b.vcd\n").unwrap();
        assert_eq!(read(&file).len(), 1);
        // a path with a tab is not kept; the file is made in a directory that is missing
        let odd = d.join("a\tb.vcd");
        record(&file, &odd, None, false).unwrap();
        assert_eq!(read(&file).len(), 1);
        let deep = d.join("x/y/history");
        record(&deep, trace, None, false).unwrap();
        assert_eq!(read(&deep).len(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
