//! wtf in a terminal: the real binary in a pseudo-terminal, its screen read back by a terminal
//! emulator (`vt100`). The mouse arrives as the escape sequences a terminal sends (SGR, mode 1006),
//! so what the TUI tests skip is covered here: the modes wtf turns on, crossterm decoding the
//! input, and the event loop.
#![cfg(unix)]

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const COLS: u16 = 100;
const ROWS: u16 = 22;

/// wtf running in a pseudo-terminal, and the screen it has drawn so far.
struct Term {
    child: Child,
    input: File,
    screen: Arc<Mutex<vt100::Parser>>,
    dir: PathBuf,
}

impl Term {
    /// `wtf <args>` in a `COLS` x `ROWS` terminal, with a config of its own (no first-start
    /// questions) and no colour (no background query to wait on).
    fn spawn(name: &str, args: &[&str]) -> Term {
        let dir = std::env::temp_dir().join(format!("wtf-term-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("wtf")).unwrap();
        std::fs::write(dir.join("wtf/config.toml"), "glyphs = \"unicode\"\ntheme = \"dark\"\n").unwrap();
        let (mut master, mut slave) = (0, 0);
        let mut size = libc::winsize { ws_row: ROWS, ws_col: COLS, ws_xpixel: 0, ws_ypixel: 0 };
        // SAFETY: openpty writes the two descriptors; no name, default termios
        let r = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &raw mut size) };
        assert_eq!(r, 0, "openpty: {}", std::io::Error::last_os_error());
        // SAFETY: both descriptors are fresh from openpty and owned here
        let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wtf"));
        cmd.args(args).env("XDG_CONFIG_HOME", &dir).env("XDG_RUNTIME_DIR", &dir).env("TERM", "xterm-256color").env("NO_COLOR", "1");
        cmd.stdin(slave.try_clone().unwrap()).stdout(slave.try_clone().unwrap()).stderr(slave);
        // SAFETY: only async-signal-safe calls between fork and exec: a new session whose
        // controlling terminal is the pty, as a terminal emulator starts its shell
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn().unwrap();
        drop(cmd);
        let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS.try_into().unwrap(), COLS.try_into().unwrap(), 0)));
        let (mut output, parser) = (master.try_clone().unwrap(), screen.clone());
        std::thread::spawn(move || {
            let mut buf = [0; 4096];
            // ends with an error (EIO) once wtf has exited
            while let Ok(n @ 1..) = output.read(&mut buf) {
                parser.lock().unwrap().process(&buf[..n]);
            }
        });
        Term { child, input: master, screen, dir }
    }

    /// Wait until `ok` holds for the screen; the screen as text.
    fn wait(&self, what: &str, ok: impl Fn(&vt100::Screen) -> bool) -> String {
        let start = Instant::now();
        loop {
            {
                let p = self.screen.lock().unwrap();
                if ok(p.screen()) {
                    return p.screen().contents();
                }
                if start.elapsed() > Duration::from_secs(10) {
                    panic!("waited for {what}; the screen:\n{}", p.screen().contents());
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn send(&mut self, bytes: &str) {
        self.input.write_all(bytes.as_bytes()).unwrap();
    }

    /// The mouse as a terminal reports it (SGR): `button` (0 left, +16 Ctrl, +32 moving) at
    /// column `x`, line `y` (from 0), pressed or let go.
    fn mouse(&mut self, button: u8, (x, y): (usize, usize), press: bool) {
        self.send(&format!("\x1b[<{button};{};{}{}", x + 1, y + 1, if press { 'M' } else { 'm' }));
    }

    /// Press at `from`, move to `to`, let go there.
    fn drag(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.mouse(0, from, true);
        self.mouse(32, to, true);
        self.mouse(0, to, false);
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Where `text` is on the screen, the first time from the left at or after column `from`.
fn find(s: &vt100::Screen, text: &str, from: usize) -> Option<(usize, usize)> {
    s.rows(0, COLS).enumerate().find_map(|(y, line)| {
        let chars: Vec<char> = line.chars().collect();
        let want: Vec<char> = text.chars().collect();
        (from..chars.len().saturating_sub(want.len() - 1)).find(|&x| chars[x..x + want.len()] == want[..]).map(|x| (x, y))
    })
}

/// The screen lines whose name column starts with `name`, top to bottom.
fn line_of(s: &vt100::Screen, name: &str) -> Option<usize> {
    s.rows(0, COLS).position(|l| l.starts_with(name))
}

fn session() -> Vec<String> {
    let data = std::fs::canonicalize("tests/data").unwrap();
    vec![data.join("simple.vcd").display().to_string(), "-S".into(), data.join("simple.session.toml").display().to_string()]
}

#[test]
fn mouse_reporting_on_and_off() {
    // wtf asks for button-motion reporting in SGR, and turns it off again when it quits
    let args = session();
    let mut t = Term::spawn("modes", &args.iter().map(String::as_str).collect::<Vec<_>>());
    t.wait("the rows", |s| line_of(s, "dma.state").is_some());
    t.wait("mouse reporting", |s| {
        matches!(s.mouse_protocol_mode(), vt100::MouseProtocolMode::ButtonMotion | vt100::MouseProtocolMode::AnyMotion)
            && s.mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr
    });
    t.send(":q!\r");
    let status = t.child.wait().unwrap();
    assert!(status.success());
    t.wait("mouse reporting off", |s| s.mouse_protocol_mode() == vt100::MouseProtocolMode::None);
}

#[test]
fn drag_moves_a_row() {
    // simple's session: clk first, then cpu with state, data and busy; a row's name is on its
    // lower line, so pressing on clk's name and letting go on busy's moves clk below busy
    let args = session();
    let mut t = Term::spawn("rows", &args.iter().map(String::as_str).collect::<Vec<_>>());
    t.wait("the rows", |s| line_of(s, "dma.state").is_some());
    let (clk, busy) = {
        let p = t.screen.lock().unwrap();
        (line_of(p.screen(), "clk").unwrap(), find(p.screen(), "busy", 0).unwrap())
    };
    t.drag((1, clk), busy);
    let screen = t.wait("clk below busy", |s| find(s, "clk", 0).is_some_and(|(_, y)| y > find(s, "busy", 0).unwrap().1));
    assert!(screen.contains("[+]"), "the session changed:\n{screen}");
    // `u` takes it back in one step
    t.send("u");
    t.wait("clk first again", |s| line_of(s, "clk").is_some_and(|y| y < find(s, "busy", 0).unwrap().1));
}

#[test]
fn drag_adds_from_the_panel() {
    // the status line's button opens the panel; rst dragged out of it is added below dma.state
    let args = session();
    let mut t = Term::spawn("panel", &args.iter().map(String::as_str).collect::<Vec<_>>());
    t.wait("the rows", |s| line_of(s, "dma.state").is_some());
    let button = t.wait("the button", |s| find(s, "signals ▸", 0).is_some());
    let at = button.lines().position(|l| l.contains("signals ▸")).unwrap();
    let x = button.lines().nth(at).unwrap().chars().position(|c| c == 's').unwrap();
    t.mouse(0, (x + 1, at), true);
    t.mouse(0, (x + 1, at), false);
    t.wait("the panel", |s| find(s, "signals ◂", 0).is_some());
    let (rst, dma) = {
        let p = t.screen.lock().unwrap();
        let rst = find(p.screen(), "rst", 0).unwrap();
        // dma.state's row, right of the panel
        (rst, find(p.screen(), "dma.state", rst.0 + 4).unwrap())
    };
    t.drag(rst, dma);
    t.send("\x1b");
    t.wait("rst added below dma.state", |s| line_of(s, "rst").is_some_and(|y| y > line_of(s, "dma.state").unwrap()));
}
