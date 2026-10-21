//! The link between wtf sessions and `wtf --mcp` (spec §15): each window, and
//! each agent workspace in the MCP server, listens on a local socket `wtf-<id>.sock`. The server
//! asks a window for its session (`GET`) or gives it one to show (`PUT`); `wtf --attach` gets a
//! workspace's. Anyone can ask who is there (`INFO`). Sessions travel as session TOML (spec §11).
//!
//! Wire format, one request per connection: `INFO\n`, `GET\n` or `PUT <bytes>\n<text>`; the answer
//! is `OK <bytes>\n<text>` or `ERR <message>\n`. `INFO`'s text is `<kind>\n<trace path>`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

pub enum Request {
    Get,
    Put(String),
}

/// A request for the window's main loop, with where its answer goes.
pub struct Incoming {
    pub request: Request,
    pub reply: mpsc::Sender<Result<String, String>>,
}

/// Where the sockets are: `$XDG_RUNTIME_DIR`, which is the user's own, or else a directory of the
/// user's in the temp directory, which all users share (spec §15 "Registry").
fn dir() -> std::io::Result<PathBuf> {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(d) => Ok(d.into()),
        None => private_dir(&std::env::temp_dir()),
    }
}

/// `<base>/wtf-<uid>`, made if missing so that only the user can enter it; refused if it is not a
/// directory (a link), belongs to someone else or others can enter it, as someone may have made it
/// first to read the sockets.
fn private_dir(base: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    // SAFETY: getuid has no preconditions and can't fail
    let uid = unsafe { libc::getuid() };
    let d = base.join(format!("wtf-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&d) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e),
        _ => {}
    }
    let m = std::fs::symlink_metadata(&d)?;
    if !m.is_dir() || m.uid() != uid || m.mode() & 0o077 != 0 {
        let why = format!("{}: not a directory only you can enter; remove it, or set XDG_RUNTIME_DIR", d.display());
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, why));
    }
    Ok(d)
}

fn sock(id: &str) -> std::io::Result<PathBuf> {
    Ok(dir()?.join(format!("wtf-{id}.sock")))
}

/// A session's socket. Removed when dropped.
pub struct Listener {
    pub path: PathBuf,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Listen as session `id` of `kind` (`window` or `agent`) on `trace`; `handle` answers `GET` and
/// `PUT`, on the listener's threads.
pub fn listen(id: &str, kind: &str, trace: &Path, handle: impl Fn(Request) -> Result<String, String> + Send + Sync + 'static) -> std::io::Result<Listener> {
    let path = sock(id)?;
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    let trace = std::fs::canonicalize(trace).unwrap_or_else(|_| trace.to_path_buf());
    let info: Arc<str> = format!("{kind}\n{}", trace.display()).into();
    let handle = Arc::new(handle);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (info, handle) = (info.clone(), handle.clone());
            std::thread::spawn(move || {
                // a client that goes away mid-request is no reason to stop the session
                let _ = serve(stream, &info, &*handle);
            });
        }
    });
    Ok(Listener { path })
}

/// Listen as a window: requests arrive on the receiver for the main loop to answer.
pub fn window(id: &str, trace: &Path) -> std::io::Result<(Listener, mpsc::Receiver<Incoming>)> {
    let (tx, rx) = mpsc::channel();
    let l = listen(id, "window", trace, move |request| {
        let (reply, answer) = mpsc::channel();
        let gone = || Err("the wtf window closed".to_string());
        match tx.send(Incoming { request, reply }) {
            Ok(()) => answer.recv().unwrap_or_else(|_| gone()),
            Err(_) => gone(),
        }
    })?;
    Ok((l, rx))
}

fn serve(stream: UnixStream, info: &str, handle: &dyn Fn(Request) -> Result<String, String>) -> std::io::Result<()> {
    let mut r = BufReader::new(stream.try_clone()?);
    let a = match read_head(&mut r)? {
        (head, None) if head == "INFO" => Ok(info.to_string()),
        (head, None) if head == "GET" => handle(Request::Get),
        (head, Some(text)) if head == "PUT" => handle(Request::Put(text)),
        (head, _) => Err(format!("unknown request: {head}")),
    };
    write_answer(stream, a)
}

/// A request or answer line: `GET`, `ERR <message>`, or `PUT` / `OK` with a length, followed by
/// that many bytes of text. Returns the line (without the length) and the text.
fn read_head(r: &mut impl BufRead) -> std::io::Result<(String, Option<String>)> {
    let mut line = String::new();
    r.read_line(&mut line)?;
    let line = line.trim_end();
    let sized = line.split_once(' ').filter(|(w, _)| matches!(*w, "PUT" | "OK")).and_then(|(w, n)| Some((w, n.parse::<usize>().ok()?)));
    let Some((word, n)) = sized else { return Ok((line.to_string(), None)) };
    let mut buf = vec![0; n];
    r.read_exact(&mut buf)?;
    Ok((word.to_string(), Some(String::from_utf8_lossy(&buf).into_owned())))
}

fn write_answer(mut w: UnixStream, a: Result<String, String>) -> std::io::Result<()> {
    match a {
        Ok(text) => write!(w, "OK {}\n{text}", text.len()),
        Err(e) => writeln!(w, "ERR {}", e.replace('\n', " ")),
    }
}

/// An open session, as `INFO` describes it.
pub struct Session {
    pub id: String,
    pub kind: String,
    pub trace: PathBuf,
}

/// The sessions that answer, most recently started first; an error if the sockets' directory
/// can't be used.
pub fn sessions() -> Result<Vec<Session>, String> {
    let entries = dir().and_then(std::fs::read_dir).map_err(|e| format!("can't list sessions: {e}"))?;
    let mut socks: Vec<(std::time::SystemTime, String)> = entries
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_str()?.strip_prefix("wtf-")?.strip_suffix(".sock")?.to_string();
            Some((e.metadata().ok()?.modified().ok()?, id))
        })
        .collect();
    socks.sort();
    Ok(socks
        .into_iter()
        .rev()
        .filter_map(|(_, id)| {
            let info = call(&id, "INFO\n".into()).ok()?;
            let (kind, trace) = info.split_once('\n')?;
            Some(Session { kind: kind.into(), trace: trace.into(), id })
        })
        .collect())
}

/// The most recently started window on this trace.
pub fn window_on(trace: &Path) -> Option<String> {
    let trace = std::fs::canonicalize(trace).ok()?;
    sessions().ok()?.into_iter().find(|s| s.kind == "window" && s.trace == trace).map(|s| s.id)
}

/// Send a request to session `id` and wait for its answer.
pub fn request(id: &str, req: Request) -> Result<String, String> {
    match req {
        Request::Get => call(id, "GET\n".into()),
        Request::Put(text) => call(id, format!("PUT {}\n{text}", text.len())),
    }
}

fn call(id: &str, req: String) -> Result<String, String> {
    let path = sock(id).map_err(|e| e.to_string())?;
    let mut s = match UnixStream::connect(&path) {
        Ok(s) => s,
        // left behind by a process that was killed: nothing listens there any more
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            let _ = std::fs::remove_file(&path);
            return Err(format!("no session {id}"));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(format!("no session {id}")),
        Err(e) => return Err(format!("session {id}: {e}")),
    };
    let err = |e: std::io::Error| format!("session {id}: {e}");
    s.write_all(req.as_bytes()).map_err(err)?;
    let mut r = BufReader::new(s);
    match read_head(&mut r).map_err(err)? {
        (ok, Some(text)) if ok == "OK" => Ok(text),
        (head, _) => Err(head.strip_prefix("ERR ").unwrap_or(&head).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_dir_is_only_yours() {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let base = std::env::temp_dir().join(format!("wtf-private-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let d = private_dir(&base).unwrap();
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(private_dir(&base).unwrap(), d); // found again
        // opened to others (or made so by someone else): refused
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(private_dir(&base).unwrap_err().to_string().contains("not a directory only you can enter"));
        // a link in its place: refused, even to a directory of the user's
        std::fs::remove_dir(&d).unwrap();
        let other = base.join("other");
        std::fs::DirBuilder::new().mode(0o700).create(&other).unwrap();
        std::os::unix::fs::symlink(&other, &d).unwrap();
        assert!(private_dir(&base).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn round_trip() {
        let trace = Path::new("tests/data/simple.vcd");
        let id = format!("test-live-{}", std::process::id());
        let (l, rx) = window(&id, trace).unwrap();
        // the window: answers GET with its session, takes PUT
        let window = std::thread::spawn(move || {
            for _ in 0..2 {
                let inc = rx.recv().unwrap();
                let a = match inc.request {
                    Request::Get => Ok("version = 1\n".to_string()),
                    Request::Put(t) => Err(format!("got {} bytes", t.len())),
                };
                inc.reply.send(a).unwrap();
            }
        });
        let s = sessions().unwrap().into_iter().find(|s| s.id == id).expect("listed");
        assert_eq!((s.kind.as_str(), s.trace), ("window", std::fs::canonicalize(trace).unwrap()));
        assert_eq!(window_on(trace), Some(id.clone()));
        assert_eq!(request(&id, Request::Get), Ok("version = 1\n".into()));
        assert_eq!(request(&id, Request::Put("héllo\nworld".into())), Err("got 12 bytes".into()));
        window.join().unwrap();
        let path = l.path.clone();
        drop(l);
        assert!(!path.exists(), "the socket is removed with its session");
        assert_eq!(request(&id, Request::Get), Err(format!("no session {id}")));
    }
}
