//! App state and the keys of each mode (spec §2, §3, §4 "Command line", §5).

use wtf_core::{command, query, session};
use crate::draw::gl;
use crate::tui::panel::Panel;
use wtf_core::rows::{self, Row};
use wtf_core::time;
use wtf_core::workspace::{Msg, Workspace};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::tui::keys::{Action as A, Entry, Key, Keymap, Meaning};

mod mouse;
pub use mouse::{Drag, Held, Hit, Item, Menu, Target};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    /// The keys for time and rows, and the keys that change the rows (spec §2).
    Normal,
    Command,
    /// Typing a value search (`/`); shares the line with Command.
    Search,
}

pub struct App {
    /// What is shown, and everything that changes it; the rest is how the terminal UI edits it.
    pub ws: Workspace,
    /// First row on screen, as a position among the visible rows.
    pub top: usize,
    /// Lines of rows on screen, as last drawn: what `Ctrl-f` / `Ctrl-b` move.
    pub page: usize,
    /// The screen column the waveforms start at, as last drawn: where a click places the cursor.
    pub wave_x: usize,
    /// The first line the help or `e` box shows, when it is longer than the screen.
    pub scroll: usize,
    pub mode: Mode,
    pub panel: Panel,
    /// The signal panel is open (`a`) and has the keys.
    pub adding: bool,
    pub line: String,
    /// Cursor in `line`, in characters; past the end means at the end.
    col: usize,
    /// Highlighted completion.
    pub hl: usize,
    history: Vec<String>,
    searches: Vec<String>,
    hist_pos: Option<usize>,
    /// The search `n` / `N` repeat.
    last_search: Option<String>,
    /// Its matches counted in the whole trace, for the query it was (as `Debug` shows it).
    matches: Option<(String, u64, bool)>,
    /// Status line message; true for errors.
    pub msg: Option<Msg>,
    /// Every message, oldest first (`:messages`); the newest show in the corner a while (spec §4
    /// "Notifications").
    pub notes: Vec<Note>,
    /// The message last put in `notes`, so each goes in once.
    logged: Option<Msg>,
    /// `:messages` is open, at this note (spec §4 "Notifications").
    pub messages: Option<usize>,
    /// Notes before this one are dismissed from the corner (`Esc`).
    pub dismissed: usize,
    /// What built-in protocols find in the trace (`protocol::detect`), worked out once when it
    /// opens and again on `:e`: the launch guide counts them.
    pub found: Vec<wtf_core::protocol::Found>,
    /// Drawn for plain text (`--snapshot` without `--ansi`): the guide's logo in plain characters.
    pub plain: bool,
    /// How long the picker's highlight rests before its preview reads a signal (spec §5).
    pub preview_wait: std::time::Duration,
    /// Where the mouse was last, to hold a notification under it (spec §4 "Notifications").
    pub pointer: Option<(usize, usize)>,
    /// When the corner was last drawn while a note showed, to draw again as they fade.
    noted: Option<std::time::Instant>,
    /// The last message an agent's joining, leaving or step put there: one replaces only that or
    /// none, never the user's own (an error after `:q`, say).
    agent_msg: Option<Msg>,
    count: Option<u64>,
    /// The first key of a Normal mode sequence (`g` of `gg`), typed.
    typed: Vec<Key>,
    /// The signal panel's `g` of `gg`.
    pending: Option<char>,
    /// Normal mode's keys (spec §19 "Keys").
    pub keymap: Keymap,
    /// When the keys pending (a count, the start of a sequence) were first typed; which-key shows
    /// what can follow once they have waited `which_key` (spec §2 "Which-key").
    typed_at: Option<std::time::Instant>,
    pub which_key: bool,
    pub which_delay: std::time::Duration,
    /// The leader menu, open at this group (the keys from the top) (spec §2 "Leader").
    pub leader: Option<Vec<Key>>,
    pub quit: bool,
    /// The `?` box with the keys of the current mode is open.
    pub help: bool,
    /// The `e` box: a title and the full value of the selected signal at the cursor.
    pub examine: Option<Examine>,
    /// The `:records` list, or a `:help <topic>` text.
    pub records: Option<Records>,
    /// `:records`: a stream's records as a table (spec §5 "Records").
    pub browser: Option<crate::tui::browser::Browser>,
    /// Text `yy` / `yt` copied, for the terminal's clipboard (OSC 52).
    pub yank: Option<String>,
    /// The agents working on this trace (spec §15 "Joining"), by id, with the session each had
    /// when last read.
    pub agents: Vec<(String, String)>,
    /// The agent this window follows (`:follow`).
    pub follow: Option<Follow>,
    /// The trace file as last loaded, and a change to it seen since, with when it was first seen.
    seen: Stamp,
    changed: Option<(Stamp, std::time::Instant)>,
    /// Where the last screen drew what the mouse can hit.
    pub hit: Hit,
    /// The left button held in the signal panel (spec §5).
    pub drag: Option<Drag>,
    /// The left button held on the rows: moving a row, zooming, or moving a mark (spec §2 "Drags").
    pub held: Option<Held>,
    /// The last click, for a double click: what it was on and when.
    click: Option<(Target, std::time::Instant)>,
    /// The right click's menu (spec §2 "Menu").
    pub menu: Option<Menu>,
    /// The picker (`:pick`, spec §5 "Picker").
    pub picker: Option<crate::tui::picker::Picker>,
    /// The decode form (`:decode` alone, spec §5 "Decode form").
    pub form: Option<crate::tui::form::Form>,
}

/// The agent a window follows (`:follow`, spec §15 "Following"): each change it makes to its
/// workspace is shown as `present` shows it.
pub struct Follow {
    pub id: String,
    /// The agent's session as last shown.
    pub shown: String,
    /// The view `:follow off` brings back, as a session: the user's own from before `:follow`, or
    /// what the agent presented since, and whether it was presented.
    pub own: String,
    pub presented: bool,
}

/// A message as `:messages` keeps it: what it said, whether an error, who caused it (an agent's
/// id, or none for the user), and when.
#[derive(Clone, Debug)]
pub struct Note {
    pub text: String,
    pub err: bool,
    pub who: Option<String>,
    /// An agent's step: its tool, what it asked and the answer, shown apart in the card.
    pub step: Option<(String, String, String)>,
    /// Seconds since the epoch, for the clock time `:messages` shows.
    pub wall: u64,
    pub at: std::time::Instant,
}

/// When a note came, as the local clock showed it.
pub fn clock_of(n: &Note) -> String {
    clock(n.wall)
}

/// How long a note shows in the corner: an error longer; it fades over the last second.
pub fn note_life(err: bool) -> std::time::Duration {
    std::time::Duration::from_secs(if err { 8 } else { 4 })
}

/// How far a note has faded, 0 until its last second, 1 when it goes (spec §4 "Notifications").
pub fn note_fade(n: &Note, now: std::time::Instant) -> f32 {
    let left = note_life(n.err).saturating_sub(now.duration_since(n.at));
    (1.0 - left.as_secs_f32() / NOTE_FADE.as_secs_f32()).clamp(0.0, 1.0)
}

/// The last part of a note's life, over which it fades.
const NOTE_FADE: std::time::Duration = std::time::Duration::from_secs(1);

/// The messages `:messages` keeps.
const NOTES: usize = 200;

/// A file's modification time and size: a new simulation run changes them.
pub type Stamp = Option<(std::time::SystemTime, u64)>;

pub fn stamp(path: &std::path::Path) -> Stamp {
    std::fs::metadata(path).ok().and_then(|m| Some((m.modified().ok()?, m.len())))
}

/// The `e` float (spec §2): what is examined and when, and its lines, (field name, value); a
/// line without a name is the value alone.
#[derive(Clone, Debug, PartialEq)]
pub struct Examine {
    pub title: String,
    pub at: String,
    pub lines: Vec<(String, String)>,
    /// The var and bit examined, for its kind's icon (a stream's for a record).
    pub of: (usize, Option<u32>),
}

/// `:records`: a stream's records, one a line, to pick one and jump to it. `:help <topic>`
/// shows a topic's text in it, lines without times, to scroll.
pub struct Records {
    /// The box's title.
    pub name: String,
    /// The line above the list: how many records, of how many; none for a text.
    pub title: String,
    /// Each record's start and its line.
    pub items: Vec<(Option<time::Tick>, String)>,
    pub hl: usize,
    /// First item shown, and how many fit, as last drawn.
    pub top: usize,
    pub page: usize,
}

impl App {
    pub fn new(ws: Workspace) -> Self {
        let panel = Panel::new(&ws.trace);
        let seen = stamp(&ws.trace_path);
        let found = wtf_core::protocol::detect(&ws.trace);
        App {
            found,
            ws,
            top: 0,
            page: 1,
            wave_x: 0,
            scroll: 0,
            mode: Mode::Normal,
            panel,
            adding: false,
            line: String::new(),
            col: 0,
            hl: 0,
            history: Vec::new(),
            searches: Vec::new(),
            hist_pos: None,
            last_search: None,
            matches: None,
            msg: None,
            agent_msg: None,
            notes: Vec::new(),
            messages: None,
            logged: None,
            dismissed: 0,
            noted: None,
            count: None,
            typed: Vec::new(),
            pending: None,
            keymap: Keymap::default(),
            typed_at: None,
            which_key: false,
            which_delay: std::time::Duration::from_millis(300),
            leader: None,
            quit: false,
            help: false,
            examine: None,
            records: None,
            browser: None,
            yank: None,
            agents: Vec::new(),
            follow: None,
            seen,
            changed: None,
            hit: Hit::default(),
            pointer: None,
            plain: false,
            preview_wait: crate::tui::picker::PREVIEW_WAIT,
            drag: None,
            held: None,
            click: None,
            menu: None,
            picker: None,
            form: None,
        }
    }

    /// Show a session the agent prepared (`Workspace::present`) and say so.
    pub fn present(&mut self, text: &str) -> Result<String, String> {
        let msg = self.ws.present(text, false)?;
        self.mode = Mode::Normal;
        // the agent's: its next step may take the status line
        (self.msg, self.agent_msg) = (Some(msg.clone()), Some(msg.clone()));
        let who = self.follow.as_ref().map_or("agent".into(), |f| f.id.clone());
        self.log(&msg, Some(who));
        Ok(msg.0)
    }

    /// Show the session an agent left for this trace (`<trace>.agent.session.toml`) as `present`
    /// shows it, its steps in `:trail` (spec §15 "Present").
    pub fn show_agent_file(&mut self) -> Result<(), String> {
        let path = session::agent_path(&self.ws.trace_path);
        let text = std::fs::read_to_string(&path).map_err(|_| format!("no agent session for this trace ({})", path.display()))?;
        let (msg, err) = self.ws.present(&text, false)?;
        self.mode = Mode::Normal;
        let steps = match self.ws.trail.len() {
            0 => String::new(),
            n => format!("; :trail lists its {n} steps"),
        };
        self.msg = Some((format!("{msg}{steps}; u gives your own view"), err));
        Ok(())
    }

    /// A `present` from an agent over the link: shown, and while following, it is what
    /// `:follow off` comes back to (spec §15 "Following").
    pub fn take_present(&mut self, text: &str) -> Result<String, String> {
        let msg = self.present(text)?;
        if let Some(f) = self.follow.as_mut() {
            (f.own, f.presented) = (session::to_shared_toml(&self.ws), true);
        }
        Ok(msg)
    }

    /// One of the agent's steps as a line: the tool, what it asked, the answer's first line.
    pub fn step_line(s: &wtf_core::workspace::Step) -> String {
        let dot = format!("  {}  ", gl().dot);
        format!("{} {}{dot}{}", s.tool, s.ask, s.answer)
    }

    /// A `:` command: the window's own (`:w`, `:wq`, `:q`, `:q!`, `:help`, and `:e`, which also
    /// rebuilds the signal panel), or one on the workspace. Its message goes to the status line.
    pub fn run(&mut self, line: &str) -> Result<(), String> {
        let line = line.trim();
        let (cmd, arg) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let msg = match cmd {
            "help" if arg.trim().is_empty() => {
                self.help = true;
                None
            }
            // the agents' reference, one source for both (spec §15)
            "help" => {
                let topics = || crate::mcp::help::TOPICS.iter().map(|t| t.0).collect::<Vec<_>>().join(" ");
                let text = crate::mcp::help::answer(Some(arg), wtf_core::pattern::Syntax::Glob).map_err(|_| format!("no help topic {}; topics: {}", arg.trim(), topics()))?;
                let items = text.lines().map(|l| (None, l.to_string())).collect();
                let name = format!("help {} {}", gl().dot, arg.trim().to_lowercase());
                self.records = Some(Records { name, title: String::new(), items, hl: 0, top: 0, page: 1 });
                None
            }
            // the agent's steps, newest last; Enter jumps to a step's time (spec §15 "Trail")
            "trail" => {
                if self.ws.trail.is_empty() {
                    return Err("no trail yet: the steps of an agent working on this trace show here".into());
                }
                let items: Vec<_> = self.ws.trail.iter().map(|s| (s.time, Self::step_line(s))).collect();
                let (title, hl) = (format!("{} steps", items.len()), items.len() - 1);
                self.records = Some(Records { name: "trail".into(), title, items, hl, top: 0, page: 1 });
                None
            }
            // every message, with its time and who caused it (spec §4 "Notifications")
            "messages" => {
                self.note_msg();
                if self.notes.is_empty() {
                    return Err("no messages yet".into());
                }
                self.messages = Some(self.notes.len() - 1);
                None
            }
            "w" => Some(command::save(&mut self.ws, arg.trim())?),
            "wq" => {
                command::save(&mut self.ws, arg.trim())?;
                self.quit = true;
                None
            }
            "q" if self.ws.modified() => return Err("unsaved session (:w to save, :q! to quit)".into()),
            "q" | "q!" => {
                self.quit = true;
                None
            }
            "e" => return self.reload(),
            "follow" => return self.set_follow(arg.trim()),
            "agent" => return self.show_agent_file(),
            "records" => return self.open_browser(arg.trim()),
            "pick" => return self.open_picker(arg.trim()),
            "decode" if arg.trim().is_empty() => return self.open_form(""),
            // the window picks the unit that fits; `unit` is for agents' answers (spec §15)
            "unit" => return Err("unit sets the time unit of an agent's answers; the window picks what fits".into()),
            _ => command::run(&mut self.ws, line)?,
        };
        if msg.is_some() {
            self.msg = msg;
        }
        Ok(())
    }

    /// `:e`, with the signal panel rebuilt for the new file.
    pub fn reload(&mut self) -> Result<(), String> {
        self.seen = stamp(&self.ws.trace_path);
        self.msg = Some(self.ws.reload()?);
        self.matches = None;
        self.panel = Panel::new(&self.ws.trace);
        self.found = wtf_core::protocol::detect(&self.ws.trace);
        Ok(())
    }


    /// The agents on this trace and their sessions now (spec §15 "Joining"): says who joined and
    /// left and each new step, takes the trail of the agent followed (or else of the one that made
    /// the step), and shows the followed agent's changes. Returns true when there is something new
    /// to draw.
    pub fn agents_now(&mut self, now: Vec<(String, String)>) -> bool {
        if now == self.agents {
            return false;
        }
        let old = std::mem::replace(&mut self.agents, now);
        let ids = |l: &[(String, String)]| l.iter().map(|a| a.0.clone()).collect::<Vec<_>>();
        let (was, is) = (ids(&old), ids(&self.agents));
        // each with the agent it is about, for `:messages`
        let mut said: Vec<(String, bool)> = Vec::new();
        let mut whose: Vec<String> = Vec::new();
        let mut steps: Vec<Option<(String, String, String)>> = Vec::new();
        for id in is.iter().filter(|id| !was.contains(id)) {
            said.push((format!("agent {id} joined (:follow shows its workspace)"), false));
            whose.push(id.clone());
            steps.push(None);
        }
        for id in was.iter().filter(|id| !is.contains(id)) {
            if self.follow.as_ref().is_some_and(|f| &f.id == id) {
                self.follow = None;
                said.push((format!("agent {id} left; not following it"), true));
            } else {
                said.push((format!("agent {id} left"), false));
            }
            whose.push(id.clone());
            steps.push(None);
        }
        let several = self.agents.len() > 1;
        for (id, text) in self.agents.clone() {
            let before = old.iter().find(|a| a.0 == id).map_or(String::new(), |a| a.1.clone());
            if text == before {
                continue;
            }
            let followed = self.follow.as_ref().is_some_and(|f| f.id == id);
            if followed && session::without_trail(&text) != session::without_trail(&self.follow.as_ref().unwrap().shown) {
                let r = self.present(&text).map(drop);
                self.report(r);
            }
            if let Some(f) = self.follow.as_mut().filter(|f| f.id == id) {
                f.shown = text.clone();
            }
            let trail = session::read_trail(&self.ws, &text);
            if trail != session::read_trail(&self.ws, &before) && !trail.is_empty() && (followed || self.follow.is_none()) {
                self.ws.trail = trail;
                // an agent just joined says so; its steps before are in `:trail`
                if let Some(s) = self.ws.trail.last().filter(|_| was.contains(&id)) {
                    let who = if several { format!("agent {id}") } else { "agent".into() };
                    said.push((format!("{who}: {}", Self::step_line(s)), s.answer.starts_with("error: ")));
                    whose.push(id.clone());
                    steps.push(Some((s.tool.clone(), s.ask.clone(), s.answer.clone())));
                }
            }
        }
        // every one in the corner and `:messages`; the last on the status line, unless the
        // user's own message is there
        for ((m, id), step) in said.iter().zip(whose).zip(steps) {
            self.log(m, Some(id));
            self.notes.last_mut().expect("just put there").step = step;
        }
        if let Some(last) = said.pop()
            && (self.msg.is_none() || self.msg == self.agent_msg)
        {
            self.msg = Some(last.clone());
            self.agent_msg = Some(last);
        }
        true
    }

    /// `:follow [<id>]` / `:follow off` (spec §15 "Following"): the agents are listed afresh, so it
    /// works before the first look and in `--snapshot`.
    fn set_follow(&mut self, arg: &str) -> Result<(), String> {
        if arg == "off" {
            let f = self.follow.take().ok_or("not following an agent")?;
            // back to the user's own view, or what the agent presented: one undo step, u shows
            // what was followed
            self.ws.present(&f.own, false)?;
            let back = if f.presented { "the agent's present is back" } else { "your view is back" };
            self.msg = Some((format!("not following {}; {back} (u: what you followed)", f.id), false));
            return Ok(());
        }
        let agents = crate::live::on_trace("agent", &self.ws.trace_path);
        let id = match (arg, agents.as_slice()) {
            (_, []) => return Err("no agent is working on this trace".into()),
            ("", [one]) => one.clone(),
            ("", _) => return Err(format!("several agents work on this trace: :follow <id> ({})", agents.join(", "))),
            (id, _) if agents.iter().any(|a| a == id) => id.to_string(),
            (id, _) => return Err(format!("no agent {id} on this trace (agents: {})", agents.join(", "))),
        };
        let (_, text) = crate::live::get(&id)?;
        // following another agent keeps the view from before the first
        let (own, presented) = self.follow.take().map_or_else(|| (session::to_shared_toml(&self.ws), false), |f| (f.own, f.presented));
        self.present(&text)?;
        self.follow = Some(Follow { id: id.clone(), shown: text.clone(), own, presented });
        if let Some(a) = self.agents.iter_mut().find(|a| a.0 == id) {
            a.1 = text;
        } else {
            self.agents.push((id.clone(), text));
        }
        self.msg = Some((format!("following {id}"), false));
        Ok(())
    }

    /// Reload the trace when its file changed and then stayed the same for a second, so a
    /// simulator still writing it isn't read half way. Returns true when it reloaded.
    pub fn watch(&mut self, now: Stamp, at: std::time::Instant) -> bool {
        if now.is_none() || now == self.seen {
            self.changed = None;
            return false;
        }
        match self.changed {
            Some((s, t)) if s == now && at.duration_since(t) >= std::time::Duration::from_secs(1) => {
                self.changed = None;
                let r = self.reload();
                self.report(r);
                true
            }
            Some((s, _)) if s == now => false,
            _ => {
                self.changed = Some((now, at));
                false
            }
        }
    }

    /// Feed keys written as in Vim's mappings: plain characters, `<Esc>`, `<CR>`, `<Tab>`, `<BS>`,
    /// `<Space>`, `<lt>` and `<C-x>`; raw control characters work too (`\n`, `\x1b`, `\x01` is
    /// Ctrl-a). Another `<name>` is an error, as it would be typed letter by letter; a `<` that
    /// starts no name is itself (spec §12 `--keys`).
    #[cfg(test)]
    pub fn keys(&mut self, s: &str) -> Result<(), String> {
        parse_keys(s)?.into_iter().for_each(|k| self.key(k));
        Ok(())
    }

    /// Every key that changes the rows records an undo step, except collapsing and expanding
    /// groups (not a change to the layout) and undo / redo themselves.
    pub fn key(&mut self, k: KeyEvent) {
        // a menu takes the keys while open; Esc cancels a drag
        if self.menu.is_some() {
            return self.menu_key(k);
        }
        if self.drag.is_some() && k.code == KeyCode::Esc {
            return self.cancel_drag();
        }
        if self.held.is_some() && k.code == KeyCode::Esc {
            return self.cancel_held();
        }
        // the help and e boxes take no other keys: they scroll and close
        if self.help || self.examine.is_some() {
            let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
            match k.code {
                KeyCode::Esc | KeyCode::Char('q') => (self.help, self.examine, self.scroll) = (false, None, 0),
                KeyCode::Char('?') if self.help => (self.help, self.scroll) = (false, 0),
                KeyCode::Char('e') if !self.help => (self.examine, self.scroll) = (None, 0),
                KeyCode::Char('j') | KeyCode::Down => self.scroll += 1,
                KeyCode::Char('k') | KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
                KeyCode::Char('d' | 'f') if ctrl => self.scroll += 10,
                KeyCode::Char('u' | 'b') if ctrl => self.scroll = self.scroll.saturating_sub(10),
                KeyCode::Char('g') => self.scroll = 0,
                // the box keeps it in range when drawn
                KeyCode::Char('G') => self.scroll = usize::MAX,
                _ => {}
            }
            return;
        }
        if self.browser.is_some() {
            return self.browser_key(k);
        }
        if self.records.is_some() {
            return self.records_key(k);
        }
        if self.messages.is_some() {
            return self.messages_key(k);
        }
        let mark = self.ws.mark();
        if self.leader.is_some() {
            self.leader_key(k);
            return self.ws.record(mark);
        }
        match self.mode {
            _ if self.picker.is_some() => self.picker_key(k),
            _ if self.form.is_some() => self.form_key(k),
            Mode::Normal if self.adding => self.panel_key(k),
            Mode::Normal => self.normal_key(k),
            Mode::Command | Mode::Search => self.command_key(k),
        }
        self.ws.record(mark);
        // a line under a row stays selected only while the row shows it
        if let Some(b) = self.ws.bit
            && !matches!(self.ws.rows.get(self.ws.sel), Some(Row::Signal(s)) if (b as usize) < self.ws.subs(s).len())
        {
            self.ws.bit = None;
        }
    }


    /// The `:help <topic>` and `:trail` box has the keys: `j` / `k` and `Ctrl-f` / `Ctrl-b` move,
    /// `Enter` jumps to a line's time, `Esc` / `q` close.
    fn records_key(&mut self, k: KeyEvent) {
        let Some(r) = &mut self.records else { return };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let last = r.items.len() - 1;
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.records = None,
            KeyCode::Char('j') | KeyCode::Down => r.hl = (r.hl + 1).min(last),
            KeyCode::Char('k') | KeyCode::Up => r.hl = r.hl.saturating_sub(1),
            KeyCode::Char('f') if ctrl => r.hl = (r.hl + r.page).min(last),
            KeyCode::Char('b') if ctrl => r.hl = r.hl.saturating_sub(r.page),
            KeyCode::Char('G') => r.hl = last,
            KeyCode::Char('g') => r.hl = 0,
            KeyCode::Enter => {
                let Some(t) = r.items[r.hl].0 else { return };
                self.records = None;
                self.ws.jump(t, false);
            }
            _ => {}
        }
    }

    /// `yy`: copy the selected signal's value at the cursor (a stream's record there); `yt`: the
    /// cursor's time.
    fn copy(&mut self, what: char) -> Result<(), String> {
        let fpt = self.ws.trace.fs_per_tick;
        let text = match what {
            't' => time::format(self.ws.view.cursor, fpt),
            'y' => {
                let Some(Row::Signal(s)) = self.ws.rows.get(self.ws.sel) else { return Err("select a signal to copy its value".into()) };
                if let Some((st, i)) = self.ws.record_at(s.var) {
                    let r = &st.records[i];
                    let fields: Vec<String> = st.fields_text(i, fpt).into_iter().map(|(n, v)| format!("{n}={v}")).collect();
                    format!("{} #{i} {}{}{} {}", st.name, time::format(r.start, fpt), gl().dash, time::format(r.end, fpt), fields.join(" "))
                } else {
                    let (var, bit) = self.ws.probe(s, self.ws.bit);
                    match self.ws.trace.value_of(var, bit, self.ws.view.cursor) {
                        Some(v) if self.ws.stream_of(var).is_none() => rows::label(&self.ws.trace.vars[var], &self.ws.look(s, var), &v),
                        _ => return Err("no value here".into()),
                    }
                }
            }
            _ => return Ok(()),
        };
        self.msg = Some((format!("copied: {text}"), false));
        self.yank = Some(text);
        Ok(())
    }

    /// `e`: open a box with the selected signal's whole value at the cursor, or on a stream its
    /// record's: the times and every field.
    fn examine(&mut self) {
        let fpt = self.ws.trace.fs_per_tick;
        let Some(Row::Signal(s)) = self.ws.rows.get(self.ws.sel) else {
            return self.msg = Some(("select a signal to examine".into(), true));
        };
        if let Some((st, i)) = self.ws.record_at(s.var) {
            let r = &st.records[i];
            let at = format!("{} {} {}  {}", time::format(r.start, fpt), gl().dash, time::format(r.end, fpt), time::format(r.end - r.start, fpt));
            let mut lines = Vec::new();
            for (name, v) in st.fields_text(i, fpt) {
                for (k, part) in wrap(&v).into_iter().enumerate() {
                    // a long value's further lines under its first
                    lines.push((if k == 0 { name.to_string() } else { " ".to_string() }, part));
                }
            }
            return self.examine = Some(Examine { title: format!("{} {}", st.name, st.label(i, fpt)), at, lines, of: (s.var, None) });
        }
        let (var, bit) = self.ws.probe(s, self.ws.bit);
        let name = self.ws.trace.probe_path((var, bit));
        let line = crate::draw::line_sig(&self.ws, s, var);
        let value = match self.ws.trace.value_of(var, bit, self.ws.view.cursor) {
            Some(_) if self.ws.stream_of(var).is_some() => vec!["no record here".into()],
            Some(v) => wrap(&rows::label(&self.ws.trace.vars[var], &line.look, &v)),
            None => vec!["no value here".into()],
        };
        let mut lines: Vec<(String, String)> = value.into_iter().map(|v| (String::new(), v)).collect();
        if bit.is_none() && self.ws.trace.is_composite(var) {
            lines.push(Default::default());
            lines.extend(self.fields(var));
        }
        self.examine = Some(Examine { title: name, at: time::format(self.ws.view.cursor, fpt), lines, of: (var, bit) });
    }

    /// A struct's or array's fields at the cursor, a line each, nested ones indented under their
    /// struct, an array's elements by index (spec §2 `e`).
    fn fields(&self, var: usize) -> Vec<(String, String)> {
        fn walk(app: &App, var: usize, depth: usize, out: &mut Vec<(String, Option<String>)>) {
            let t = &app.ws.trace;
            for &c in &t.vars[var].children {
                let name = format!("{}{}", "  ".repeat(depth), wtf_core::trace::parts(&t.vars[c].path).last().copied().unwrap_or_default());
                if t.is_composite(c) {
                    out.push((name, None));
                    walk(app, c, depth + 1, out);
                } else {
                    // in the look its line has under the selected row
                    let v = &t.vars[c];
                    let look = match app.ws.rows.get(app.ws.sel) {
                        Some(Row::Signal(s)) => app.ws.look(s, c),
                        _ => rows::Look::of(v),
                    };
                    let value = t.value_at(c, app.ws.view.cursor).map_or("-".into(), |x| rows::label(v, &look, &x));
                    out.push((name, Some(value)));
                }
            }
        }
        let mut lines = Vec::new();
        walk(self, var, 0, &mut lines);
        lines.into_iter().map(|(n, v)| (n, v.unwrap_or_default())).collect()
    }

    /// A value search (spec §8) that says where the match is: `match 3 of 157`.
    fn search(&mut self, text: &str, forward: bool, n: u64) -> Result<(), String> {
        // shortcut (#13): counting stops at MAX matches, so `n` stays quick on huge traces
        const MAX: u64 = 100_000;
        let q = self.ws.search(text, forward, n)?;
        let count = |to| query::find(&self.ws.trace, &q, &query::Search { from: 0, to, forward: true, merge: false, runs: usize::MAX, matches: MAX });
        let key = format!("{q:?}");
        if self.matches.as_ref().is_none_or(|m| m.0 != key) {
            let all = count(None);
            self.matches = Some((key, all.total, all.more));
        }
        let at = count(Some(self.ws.view.cursor));
        let (_, total, more) = self.matches.as_ref().unwrap();
        let i = if at.more { format!("{MAX}+") } else { at.total.to_string() };
        self.msg = Some((format!("match {i} of {total}{}", if *more { "+" } else { "" }), false));
        Ok(())
    }

    /// Move the selection by `d` screen lines (bit lines of expanded buses included).
    fn move_sel(&mut self, d: isize) {
        let lines = self.ws.lines();
        let Some(p) = lines.iter().position(|&l| l == (self.ws.sel, self.ws.bit)) else { return };
        (self.ws.sel, self.ws.bit) = lines[p.saturating_add_signed(d).min(lines.len() - 1)];
    }


    /// Select the rows `b` as a `V` range, ending on its last row on screen (a collapsed group's
    /// rows are not).
    pub fn select_block(&mut self, b: std::ops::Range<usize>) {
        let last = rows::visible(&self.ws.rows).into_iter().rfind(|v| b.contains(v)).unwrap_or(b.start);
        (self.ws.anchor, self.ws.sel, self.ws.bit) = (Some(b.start), last, None);
    }


    /// The signal panel is shown, the command line open over it or not (spec §2: it is no mode).
    pub fn panel_shown(&self) -> bool {
        self.adding
    }


    /// Counts and two-key sequences. Returns the first key of a sequence (if one is pending)
    /// and the count, or None while a count is being typed.
    fn counted(&mut self, c: char) -> Option<(Option<char>, u64)> {
        let pending = self.pending.take();
        if pending.is_none()
            && let Some(d) = c.to_digit(10)
            && (d > 0 || self.count.is_some())
        {
            self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(d as u64));
            return None;
        }
        self.msg = None;
        Some((pending, self.count.take().unwrap_or(1)))
    }


    /// Esc and other non-character keys cancel a count or a pending sequence.
    fn cancel(&mut self) {
        self.count = None;
        self.typed.clear();
        (self.typed_at, self.which_key) = (None, false);
        self.pending = None;
    }


    fn open_command(&mut self) {
        self.open_line(Mode::Command);
    }


    fn open_line(&mut self, mode: Mode) {
        self.mode = mode;
        self.line.clear();
        self.hl = 0;
        self.hist_pos = None;
    }


    fn report(&mut self, r: Result<(), String>) {
        if let Err(e) = r {
            self.msg = Some((e, true));
        }
    }


    /// Normal mode: time, rows and the keys that change the rows, all at once (spec §2, §3); the
    /// keys are the keymap's (spec §19 "Keys").
    fn normal_key(&mut self, k: KeyEvent) {
        if k.code == KeyCode::Esc {
            self.ws.anchor = None;
            self.dismissed = self.notes.len();
            return self.cancel();
        }
        let key = Key::of(k);
        // a count comes before a key: `5l`, `3m`
        if self.typed.is_empty()
            && !key.ctrl
            && let KeyCode::Char(c) = k.code
            && let Some(d) = c.to_digit(10)
            && (d > 0 || self.count.is_some())
        {
            self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(d as u64));
            self.wait();
            return;
        }
        self.typed.push(key);
        let hit = self.keymap.hit(&self.typed);
        if hit == Meaning::Prefix {
            self.wait();
            return;
        }
        self.typed.clear();
        (self.typed_at, self.which_key) = (None, false);
        self.msg = None;
        // `3m`: the count names the mark; `m` alone takes a free one
        let given = self.count.map(|n| n.saturating_sub(1) as usize);
        let n = self.count.take().unwrap_or(1);
        match hit {
            Meaning::Action(a) => self.action(a, n, given),
            Meaning::Jump(i) => self.action(A::Jump, 1, Some(i)),
            Meaning::Prefix | Meaning::None => {}
        }
    }

    /// Keys pending: which-key waits from the first, and shows at once if it already shows.
    fn wait(&mut self) {
        self.typed_at.get_or_insert_with(std::time::Instant::now);
        self.which_key |= self.which_delay.is_zero();
    }

    /// `:messages` has the keys: `j` / `k` move, `g` / `G` to the first / last, `y` copies the
    /// line's text, `Enter` on an agent's step opens the trail, `Esc` / `q` close.
    fn messages_key(&mut self, k: KeyEvent) {
        let Some(i) = self.messages else { return };
        let last = self.notes.len().saturating_sub(1);
        self.messages = match k.code {
            KeyCode::Char('j') | KeyCode::Down => Some((i + 1).min(last)),
            KeyCode::Char('k') | KeyCode::Up => Some(i.saturating_sub(1)),
            KeyCode::Char('g') => Some(0),
            KeyCode::Char('G') => Some(last),
            KeyCode::Char('y') => {
                self.yank = self.notes.get(i).map(|n| n.text.clone());
                Some(i)
            }
            KeyCode::Enter if self.notes.get(i).is_some_and(|n| n.step.is_some()) => {
                self.messages = None;
                let r = self.run("pick trail");
                return self.report(r);
            }
            KeyCode::Esc | KeyCode::Char('q') => None,
            _ => Some(i),
        };
    }

    /// Which-key's time come: true when it starts showing, to draw again.
    pub fn which_tick(&mut self, now: std::time::Instant) -> bool {
        let due = !self.which_key && self.typed_at.is_some_and(|t| now.duration_since(t) >= self.which_delay);
        self.which_key |= due;
        due
    }

    /// Put a message in `notes`, as said by `who` (an agent's id, none for the user).
    fn log(&mut self, (text, err): &Msg, who: Option<String>) {
        let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.notes.push(Note { text: text.clone(), err: *err, who, step: None, wall, at: std::time::Instant::now() });
        if self.notes.len() > NOTES {
            self.notes.remove(0);
            self.dismissed = self.dismissed.saturating_sub(1);
        }
    }

    /// The status line's message, once, into `notes`: the user's own, as the agents' are put there
    /// when they come (drawn each screen, so every way a message is set goes through here).
    pub fn note_msg(&mut self) {
        if self.msg != self.logged {
            // a drag's status line says what letting go does, as it moves: not a message
            let dragging = self.drag.is_some() || self.held.is_some();
            match self.msg.clone() {
                Some(m) if self.msg != self.agent_msg && !dragging => self.log(&m, None),
                // an agent's is put there when it comes
                _ => {}
            }
            self.logged = self.msg.clone();
        }
    }

    /// The notes the corner shows at `now`: not dismissed, not gone, the newest first.
    pub fn shown_notes(&self, now: std::time::Instant) -> Vec<&Note> {
        self.notes[self.dismissed.min(self.notes.len())..].iter().rev().filter(|n| now.duration_since(n.at) < note_life(n.err)).collect()
    }

    /// The corner while notes show: true every quarter second, to draw them gone, and every 40 ms
    /// while one fades, so it fades smoothly.
    pub fn note_tick(&mut self, now: std::time::Instant) -> bool {
        self.hold_notes(now);
        let shown = self.shown_notes(now);
        if shown.is_empty() && self.noted.is_none() {
            return false;
        }
        let every = if shown.iter().any(|n| note_fade(n, now) > 0.0) { 40 } else { 250 };
        let due = self.noted.is_none_or(|t| now.duration_since(t) >= std::time::Duration::from_millis(every));
        if due {
            self.noted = (!self.shown_notes(now).is_empty()).then_some(now);
        }
        due
    }

    /// The picker's highlight has rested long enough for its preview to read the signal: true
    /// once, to draw it.
    pub fn picker_tick(&self, now: std::time::Instant) -> bool {
        self.picker.as_ref().and_then(|p| p.rest).is_some_and(|(_, since, drawn)| !drawn && now.duration_since(since) >= self.preview_wait)
    }

    /// A notification under the pointer doesn't fade: it is put back to full colour, a second
    /// before its fade, so it fades once the pointer has left it (spec §4 "Notifications").
    pub fn hold_notes(&mut self, now: std::time::Instant) {
        let Some((x, y)) = self.pointer else { return };
        for (r, at) in &mut self.hit.notes {
            if !(r.x as usize..r.right() as usize).contains(&x) || !(r.y as usize..r.bottom() as usize).contains(&y) {
                continue;
            }
            let Some(n) = self.notes.iter_mut().find(|n| n.at == *at) else { continue };
            let held = now.checked_sub(note_life(n.err).saturating_sub(NOTE_FADE + std::time::Duration::from_secs(1)));
            if let Some(h) = held.filter(|&h| h > n.at) {
                (n.at, *at) = (h, h);
            }
        }
    }

    /// The start of a sequence typed so far.
    pub fn typed_keys(&self) -> Vec<Key> {
        self.typed.clone()
    }

    /// The keys pending, as the status line shows them: `5`, `g`, `Space a`.
    pub fn pending(&self) -> String {
        let keys = |ks: &[Key]| ks.iter().map(|k| k.show()).collect::<Vec<_>>();
        if let Some(path) = &self.leader {
            return [vec![self.keymap.show(A::Leader)], keys(path)].concat().join(" ");
        }
        let typed = keys(&self.typed);
        let sep = if typed.iter().any(|k| k.chars().count() > 1) { " " } else { "" };
        format!("{}{}", self.count.map_or(String::new(), |n| n.to_string()), typed.join(sep))
    }

    /// The leader menu has the keys: an entry runs, a group opens, `Backspace` goes up, `Esc` and
    /// any other key close it (spec §2 "Leader").
    fn leader_key(&mut self, k: KeyEvent) {
        let Some(path) = self.leader.as_mut() else { return };
        match k.code {
            KeyCode::Backspace => {
                if path.pop().is_none() {
                    self.leader = None;
                }
            }
            KeyCode::Esc => self.leader = None,
            _ => {
                let mut next = path.clone();
                next.push(Key::of(k));
                self.leader_pick(next);
            }
        }
    }

    /// The leader menu's entry at `path`: a group opens, a command runs (or, ending in a space,
    /// waits on the command line), anything else closes the menu.
    pub fn leader_pick(&mut self, path: Vec<Key>) {
        match self.keymap.leader.at(&path).cloned() {
            Some(Entry::Group { .. }) => self.leader = Some(path),
            Some(Entry::Run { line, .. }) => {
                self.leader = None;
                self.msg = None;
                let line = &line[1..];
                if line.ends_with(' ') {
                    self.open_command();
                    (self.line, self.col) = (line.into(), usize::MAX);
                } else {
                    let r = self.run(line);
                    self.report(r);
                }
            }
            None => self.leader = None,
        }
    }

    /// What a Normal mode key does, `n` times or on cursor `given` (`3m`, `'3`).
    pub fn action(&mut self, a: A, n: u64, given: Option<usize>) {
        let v = &mut self.ws.view;
        let half = (v.width / 2).max(1) as i64;
        let n_i = n.min(u32::MAX as u64) as i64;
        match a {
            // half a screen, the cursor along; at the start or end it moves on alone
            A::HalfRight | A::HalfLeft => {
                let right = a == A::HalfRight;
                let cols = if right { half * n_i } else { -half * n_i };
                v.scroll(cols);
                let d = cols.unsigned_abs().saturating_mul(v.ticks) / v.cols;
                v.set_cursor(if right { v.cursor.saturating_add(d) } else { v.cursor.saturating_sub(d) });
            }
            A::PageDown => self.move_sel(n_i.saturating_mul(self.page as i64) as isize),
            A::PageUp => self.move_sel(-(n_i.saturating_mul(self.page as i64) as isize)),
            A::Redo => {
                let r = self.ws.redo();
                self.report(r);
            }
            // the keys that change the rows need a row
            A::MoveDown | A::MoveUp | A::Remove | A::Range | A::Radix if self.ws.rows.is_empty() => {}
            // a `V` range moves whole, past the next row on screen (spec §3)
            A::MoveDown | A::MoveUp if self.ws.anchor.is_some() => {
                let r = self.ws.selection();
                let mut b = rows::block(&self.ws.rows, r.start, r.end - 1);
                for _ in 0..n_i {
                    let Some(n) = rows::step(&mut self.ws.rows, b.clone(), a == A::MoveDown) else { break };
                    b = n;
                }
                self.select_block(b);
            }
            A::MoveDown | A::MoveUp => {
                for _ in 0..n_i {
                    self.ws.sel = rows::move_row(&mut self.ws.rows, self.ws.sel, a == A::MoveDown);
                }
            }
            A::Remove => {
                let r = self.ws.delete();
                self.report(r);
            }
            A::Range => self.ws.anchor = if self.ws.anchor.is_some() { None } else { Some(self.ws.sel) },
            A::Radix => {
                let r = self.ws.set_radix(None);
                self.report(r);
            }
            A::Undo => {
                let r = self.ws.undo();
                self.report(r);
            }
            // the screen alone; the cursor keeps its time
            A::ScreenLeft => v.scroll(-half * n_i),
            A::ScreenRight => v.scroll(half * n_i),
            A::Center => v.center(v.cursor),
            A::Left => v.set_cursor(v.cursor.saturating_sub(n.saturating_mul(v.step()))),
            A::Right => v.set_cursor(v.cursor.saturating_add(n.saturating_mul(v.step()))),
            A::NextChange | A::PrevChange => {
                let fwd = a == A::NextChange;
                // on a group: the nearest change of any of its signals
                let vars = self.ws.sel_vars();
                let mut t = self.ws.view.cursor;
                for _ in 0..n {
                    let next = vars.iter().filter_map(|&(v, b)| {
                        if fwd { self.ws.trace.next_change_of(v, b, t) } else { self.ws.trace.prev_change_of(v, b, t) }
                    });
                    match if fwd { next.min() } else { next.max() } {
                        Some(x) => t = x,
                        None => break,
                    }
                }
                self.ws.view.set_cursor(t);
            }
            A::Down => self.move_sel(n.min(u32::MAX as u64) as isize),
            A::Up => self.move_sel(-(n.min(u32::MAX as u64) as isize)),
            A::Fold => self.ws.toggle_group(),
            A::Start => v.set_cursor(self.ws.trace.start()),
            A::End => v.set_cursor(v.end),
            A::ZoomIn => (0..n).for_each(|_| v.zoom_in()),
            A::ZoomOut => (0..n).for_each(|_| v.zoom_out()),
            A::Search => self.open_line(Mode::Search),
            A::Jump => {
                let r = self.ws.goto_mark(given.unwrap_or(0));
                self.report(r);
            }
            A::JumpBack => {
                let r = self.ws.jump_back();
                self.report(r);
            }
            A::CopyValue | A::CopyTime => {
                let r = self.copy(if a == A::CopyValue { 'y' } else { 't' });
                self.report(r);
            }
            A::Mark => {
                let r = self.ws.set_mark(given);
                self.report(r);
            }
            A::Unmark => {
                let r = self.ws.remove_mark(given);
                self.report(r);
            }
            A::NextMatch | A::PrevMatch => {
                let r = match self.last_search.clone() {
                    Some(text) => self.search(&text, a == A::NextMatch, n),
                    None => Err("no search yet".into()),
                };
                self.report(r);
            }
            A::Panel => {
                self.ws.anchor = None;
                self.adding = true;
            }
            A::Command => self.open_command(),
            A::Help => self.help = true,
            A::Examine => self.examine(),
            A::Quit => {
                let r = self.run("q");
                self.report(r);
            }
            A::NextMark | A::PrevMark => {
                let r = self.ws.next_mark(a == A::NextMark);
                self.report(r);
            }
            A::Leader => self.leader = Some(Vec::new()),
        }
    }


    /// The signal panel, open (spec §5 "Signal panel").
    fn panel_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(q) = &mut self.panel.query {
            // typing a search, as in vim: everything goes into it until Enter or Esc
            self.msg = None;
            match k.code {
                KeyCode::Esc => self.panel.cancel_search(),
                KeyCode::Enter => {
                    let q = q.clone();
                    if !self.panel.confirm() && !q.is_empty() {
                        self.msg = Some((format!("not found: {q}"), true));
                    }
                }
                KeyCode::Backspace if q.is_empty() => self.panel.cancel_search(),
                KeyCode::Backspace => {
                    q.pop();
                    self.panel.search();
                }
                KeyCode::Char('a') if ctrl => self.mark_matches(),
                KeyCode::Char(c) if !ctrl => {
                    q.push(c);
                    self.panel.search();
                }
                _ => {}
            }
            return;
        }
        let c = match k.code {
            KeyCode::Esc => {
                // the range first, then the marks, then the panel
                self.cancel();
                self.msg = None;
                if self.panel.anchor.take().is_none() && std::mem::take(&mut self.panel.marked).is_empty() {
                    self.adding = false;
                }
                return;
            }
            KeyCode::Enter => {
                self.cancel();
                self.msg = None;
                if self.panel.marked.is_empty() && self.panel.anchor.is_none() {
                    return self.add_current();
                }
                let vars = self.picked();
                if vars.is_empty() {
                    self.msg = Some(("nothing to add: already shown".into(), true));
                }
                return self.ws.add_rows(&vars);
            }
            KeyCode::Char(' ') => {
                self.cancel();
                self.msg = None;
                return self.toggle_mark();
            }
            KeyCode::Char('r') if ctrl => {
                self.cancel();
                let r = self.ws.redo();
                return self.report(r);
            }
            KeyCode::Char('a') if ctrl => {
                self.cancel();
                return self.mark_matches();
            }
            KeyCode::Char('g') if ctrl => {
                self.cancel();
                return self.add_as_group();
            }
            KeyCode::Char(c) if !ctrl => c,
            _ => return self.cancel(),
        };
        let Some((pending, n)) = self.counted(c) else { return };
        if let Some(p) = pending {
            if (p, c) == ('g', 'g') {
                self.panel.step(isize::MIN / 2);
            }
            return;
        }
        let n = n.min(u32::MAX as u64) as isize;
        match c {
            'j' => self.panel.step(n),
            'k' => self.panel.step(-n),
            'l' => self.panel.open(true),
            'h' => self.panel.open(false),
            'V' => self.panel.anchor = if self.panel.anchor.is_some() { None } else { Some(self.panel.sel) },
            'n' | 'N' if self.panel.matches.is_empty() => {
                let msg = if self.panel.pattern.is_empty() { "no search yet".into() } else { format!("not found: {}", self.panel.pattern) };
                self.msg = Some((msg, true));
            }
            'g' => self.pending = Some('g'),
            'G' => self.panel.step(isize::MAX / 2),
            'a' => {
                self.panel.anchor = None;
                self.adding = false;
            }
            'n' => self.panel.next(true, n as usize),
            'N' => self.panel.next(false, n as usize),
            '/' => self.panel.start_search(),
            'u' => {
                let r = self.ws.undo();
                self.report(r);
            }
            ':' => self.open_command(),
            '?' => self.help = true,
            _ => {}
        }
    }


    /// Space: mark or unmark the highlighted signal, or the signals directly in a scope.
    /// With a `V` range, its signals; the range ends.
    fn toggle_mark(&mut self) {
        let vars = if self.panel.anchor.is_some() { self.panel.range_vars() } else { self.panel.vars_here() };
        self.panel.anchor = None;
        let vars: Vec<usize> = vars.into_iter().filter(|&v| !self.ws.shown(v)).collect();
        let marked = &mut self.panel.marked;
        if vars.is_empty() {
            self.msg = Some(("nothing to mark: already shown".into(), true));
        } else if vars.iter().all(|v| marked.contains(v)) {
            marked.retain(|v| !vars.contains(v));
        } else {
            for v in vars {
                if !marked.contains(&v) {
                    marked.push(v);
                }
            }
        }
    }


    /// The marked signals and those in the `V` range, not shown yet; marks and range are cleared.
    fn picked(&mut self) -> Vec<usize> {
        let mut vars = std::mem::take(&mut self.panel.marked);
        for v in self.panel.range_vars() {
            if !vars.contains(&v) {
                vars.push(v);
            }
        }
        self.panel.anchor = None;
        vars.retain(|&v| !self.ws.shown(v));
        vars
    }


    /// `Ctrl-a`: mark every signal the search matches.
    fn mark_matches(&mut self) {
        let vars: Vec<usize> = self.panel.matches.iter().filter_map(|&i| self.panel.nodes[i].var).collect();
        let new: Vec<usize> = vars.into_iter().filter(|&v| !self.ws.shown(v) && !self.panel.marked.contains(&v)).collect();
        if new.is_empty() {
            self.msg = Some(("nothing to mark".into(), true));
        }
        self.panel.marked.extend(new);
    }


    /// `Ctrl-g`: the marked signals and the `V` range (or the highlighted signal or scope) as a new group, named after
    /// the scope they share. The command line then offers `:rename` with that name: Enter keeps
    /// it, typing changes it, Esc keeps it too.
    fn add_as_group(&mut self) {
        let vars = match self.panel.marked.is_empty() && self.panel.anchor.is_none() {
            true => self.panel.vars_here(),
            false => self.picked(),
        };
        let vars: Vec<usize> = vars.into_iter().filter(|&v| !self.ws.shown(v)).collect();
        if vars.is_empty() {
            self.msg = Some(("nothing to add: already shown".into(), true));
            return;
        }
        let paths: Vec<&str> = vars.iter().map(|&v| self.ws.trace.vars[v].path.as_str()).collect();
        let scope = common_scope(&paths);
        let name = wtf_core::trace::parts(&scope).into_iter().rfind(|n| !n.is_empty() && !n.starts_with('[')).unwrap_or("group").to_string();
        self.ws.add_group(&name, &vars);
        self.open_command();
        self.line = format!("rename {name}");
        self.col = usize::MAX;
    }


    /// Enter in the panel with nothing marked: a signal becomes a row, a scope a group.
    fn add_current(&mut self) {
        let i = self.panel.sel;
        if i >= self.panel.nodes.len() {
            return;
        }
        let path = self.panel.nodes[i].path.clone();
        if self.panel.nodes[i].var.is_some_and(|v| self.ws.shown(v)) {
            return;
        }
        match command::add(&mut self.ws, &path) {
            Ok(m) => self.msg = m,
            Err(e) => self.report(Err(e)),
        }
    }


    /// The command line, and the search line, which works the same without completions.
    fn command_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let search = self.mode == Mode::Search;
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                let line = std::mem::take(&mut self.line);
                let hist = if search { &mut self.searches } else { &mut self.history };
                if !line.trim().is_empty() {
                    hist.retain(|h| *h != line);
                    hist.push(line.clone());
                }
                let r = match search {
                    true => {
                        self.last_search = Some(line.clone());
                        self.search(&line, true, 1)
                    }
                    false => self.run(&line),
                };
                self.report(r);
            }
            KeyCode::Backspace if self.line.is_empty() => self.mode = Mode::Normal,
            // editing at the cursor, with the keys of vim's command line
            KeyCode::Left => self.col = self.cursor().saturating_sub(1),
            KeyCode::Right => self.col = self.cursor() + 1,
            KeyCode::Home => self.col = 0,
            KeyCode::Char('b') if ctrl => self.col = 0,
            KeyCode::End => self.col = usize::MAX,
            KeyCode::Char('e') if ctrl => self.col = usize::MAX,
            KeyCode::Backspace => {
                let c = self.cursor();
                self.edit(c.saturating_sub(1)..c, "");
            }
            KeyCode::Delete => {
                let c = self.cursor();
                self.edit(c..c + 1, "");
            }
            KeyCode::Char('w') if ctrl => {
                // the word before the cursor, and the spaces after it
                let c = self.cursor();
                let before: Vec<char> = self.line.chars().take(c).collect();
                let spaces = before.iter().rev().take_while(|ch| ch.is_whitespace()).count();
                let word = before.iter().rev().skip(spaces).take_while(|ch| !ch.is_whitespace()).count();
                self.edit(c - spaces - word..c, "");
            }
            KeyCode::Char('u') if ctrl => {
                let c = self.cursor();
                self.edit(0..c, "");
            }
            KeyCode::Up => self.move_hl(true),
            KeyCode::Down => self.move_hl(false),
            KeyCode::Char('p') if ctrl => self.move_hl(true),
            KeyCode::Char('n') if ctrl => self.move_hl(false),
            KeyCode::Tab if search => {}
            KeyCode::Tab => {
                if let Some(line) = crate::tui::commands::complete(&self.line, self).take(&self.line, self.hl) {
                    self.line = line;
                    self.col = usize::MAX;
                    self.hl = 0;
                }
            }
            KeyCode::Char(ch) if !ctrl => {
                let c = self.cursor();
                self.edit(c..c, ch.encode_utf8(&mut [0; 4]));
            }
            _ => {}
        }
    }


    /// The cursor in the command or search line, in characters.
    pub fn cursor(&self) -> usize {
        self.col.min(self.line.chars().count())
    }


    /// Replace the characters in `r` (clamped to the line) with `text`; the cursor goes after it.
    fn edit(&mut self, r: std::ops::Range<usize>, text: &str) {
        let byte = |c: usize| self.line.char_indices().nth(c).map_or(self.line.len(), |(i, _)| i);
        let (a, b) = (byte(r.start), byte(r.end));
        self.line.replace_range(a..b, text);
        self.col = r.start + text.chars().count();
        self.hl = 0;
        self.hist_pos = None;
    }


    /// Up / down: walk earlier commands on an empty (or recalled) line, otherwise the completion list.
    fn move_hl(&mut self, up: bool) {
        let search = self.mode == Mode::Search;
        if self.line.is_empty() || self.hist_pos.is_some() || search {
            let hist = if search { &self.searches } else { &self.history };
            let len = hist.len();
            if len == 0 {
                return;
            }
            let pos = match (self.hist_pos, up) {
                (None, true) => Some(len - 1),
                (None, false) => None,
                (Some(p), true) => Some(p.saturating_sub(1)),
                (Some(p), false) => (p + 1 < len).then_some(p + 1),
            };
            self.hist_pos = pos;
            self.line = pos.map(|p| hist[p].clone()).unwrap_or_default();
            self.col = usize::MAX;
            return;
        }
        let n = crate::tui::commands::complete(&self.line, self).items.len();
        if n > 0 {
            self.hl = if up { (self.hl + n - 1) % n } else { (self.hl + 1) % n };
        }
    }
}


/// The scope all these paths are in: `top.cpu.a`, `top.cpu.sub.b` -> `top.cpu`.
/// A long value in lines: text by 16 words (a frame's bytes), digits by 64.
fn wrap(v: &str) -> Vec<String> {
    let lines: Vec<String> = match v.contains(' ') {
        true => v.split(' ').collect::<Vec<_>>().chunks(16).map(|c| c.join(" ")).collect(),
        false => v.chars().collect::<Vec<_>>().chunks(64).map(|c| c.iter().collect()).collect(),
    };
    if lines.is_empty() { vec![String::new()] } else { lines }
}

fn common_scope(paths: &[&str]) -> String {
    let scopes: Vec<Vec<&str>> = paths.iter().map(|p| wtf_core::trace::parts(p)).map(|mut v| {
        v.pop();
        v
    }).collect();
    let first = scopes.first().cloned().unwrap_or_default();
    let n = (0..first.len()).take_while(|&i| scopes.iter().all(|s| s.get(i) == Some(&first[i]))).count();
    wtf_core::trace::join(first[..n].iter().copied())
}

/// `secs` since the epoch as the local clock shows it, `14:03:05`.
fn clock(secs: u64) -> String {
    let t = secs as libc::time_t;
    // SAFETY: localtime_r reads `t` and writes only `tm`, which it is given
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return "--:--:--".into();
    }
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// Keys as `App::keys` takes them, each a key event.
pub fn parse_keys(s: &str) -> Result<Vec<KeyEvent>, String> {
    let key = KeyEvent::new;
    let none = KeyModifiers::NONE;
    let mut rest = s;
    let mut out = Vec::new();
    while let Some(c) = rest.chars().next() {
        let name = rest.strip_prefix('<').and_then(|r| r.split_once('>')).map(|(n, _)| n);
        let named = match name.map(|n| n.to_ascii_lowercase()).as_deref() {
            Some("esc") => Some(key(KeyCode::Esc, none)),
            Some("cr" | "enter") => Some(key(KeyCode::Enter, none)),
            Some("tab") => Some(key(KeyCode::Tab, none)),
            Some("s-tab") => Some(key(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Some("bs") => Some(key(KeyCode::Backspace, none)),
            Some("lt") => Some(key(KeyCode::Char('<'), none)),
            Some("space") => Some(key(KeyCode::Char(' '), none)),
            Some("c-space") => Some(key(KeyCode::Char(' '), KeyModifiers::CONTROL)),
            Some("left") => Some(key(KeyCode::Left, none)),
            Some("right") => Some(key(KeyCode::Right, none)),
            Some("up") => Some(key(KeyCode::Up, none)),
            Some("down") => Some(key(KeyCode::Down, none)),
            Some(n) if n.len() == 3 && n.starts_with("c-") => Some(key(KeyCode::Char(n.chars().nth(2).unwrap()), KeyModifiers::CONTROL)),
            Some(n) if !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
                return Err(format!("unknown key <{}> (keys: <Esc> <CR> <Tab> <S-Tab> <BS> <Space> <C-Space> <Left> <Right> <Up> <Down> <lt> <C-x>; < itself is <lt>)", name.unwrap()));
            }
            _ => None,
        };
        if let (Some(k), Some(n)) = (named, name) {
            out.push(k);
            rest = &rest[n.len() + 2..];
            continue;
        }
        out.push(match c {
            '\n' => key(KeyCode::Enter, none),
            '\x1b' => key(KeyCode::Esc, none),
            '\x08' => key(KeyCode::Backspace, none),
            '\t' => key(KeyCode::Tab, none),
            c if c < ' ' => key(KeyCode::Char((c as u8 + b'a' - 1) as char), KeyModifiers::CONTROL),
            c => key(KeyCode::Char(c), none),
        });
        rest = &rest[c.len_utf8()..];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    #[test]
    fn clock_out_of_range() {
        // a time past the years localtime_r can hold fails it: no made-up 00:00:00
        assert_eq!(clock(i64::MAX as u64), "--:--:--");
    }

    fn simple() -> App {
        let t = wtf_core::trace::Trace::open("tests/data/simple.vcd".as_ref()).unwrap();
        App::new(wtf_core::workspace::Workspace::new(t, "x".into()))
    }

    fn wheel(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE }
    }

    #[test]
    fn messages_list() {
        // spec §4 "Notifications": `:messages` moves, copies, opens an agent's step, closes
        let mut app = simple();
        assert_eq!(app.run("messages").unwrap_err(), "no messages yet");
        for t in ["a", "b", "c"] {
            app.log(&(t.into(), false), None);
        }
        app.log(&("agent: find".into(), false), Some("1-1".into()));
        app.notes[3].step = Some(("find".into(), "x".into(), "1 matches".into()));
        app.keys(":messages\n").unwrap();
        assert_eq!(app.messages, Some(3));
        app.keys("kk").unwrap();
        assert_eq!(app.messages, Some(1));
        app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.keys("j").unwrap();
        app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.messages, Some(3));
        app.keys("g").unwrap();
        assert_eq!(app.messages, Some(0));
        app.keys("y").unwrap();
        assert_eq!(app.yank.as_deref(), Some("a"));
        // Enter on a plain note, and a key with no meaning, change nothing
        app.keys("<CR>z").unwrap();
        assert_eq!(app.messages, Some(0));
        // the wheel moves, a click outside closes
        app.mouse(wheel(MouseEventKind::ScrollDown, 5, 5));
        app.mouse(wheel(MouseEventKind::ScrollDown, 5, 5));
        assert_eq!(app.messages, Some(2));
        app.mouse(wheel(MouseEventKind::ScrollUp, 5, 5));
        app.mouse(wheel(MouseEventKind::Moved, 5, 5));
        assert_eq!(app.messages, Some(1));
        app.mouse(wheel(MouseEventKind::Down(MouseButton::Left), 0, 0));
        assert!(app.messages.is_none());
        // Enter on an agent's step opens the trail
        app.ws.trail = vec![wtf_core::workspace::Step { tool: "find".into(), ask: "x".into(), answer: "1 matches".into(), time: Some(20) }];
        app.keys(":messages\n<CR>").unwrap();
        assert!(app.messages.is_none() && app.picker.is_some(), "{:?} {:?}", app.msg, app.messages);
        app.keys("<Esc>:messages\nG").unwrap();
        assert_eq!(app.messages, Some(3));
        app.keys("<Esc>").unwrap();
        assert!(app.messages.is_none());
    }

    #[test]
    fn leader_and_line_mouse() {
        let mut app = simple();
        // a right click closes the leader menu
        app.keys("<Space>").unwrap();
        assert!(app.leader.is_some());
        app.mouse(wheel(MouseEventKind::Down(MouseButton::Right), 0, 0));
        assert!(app.leader.is_none());
        // the wheel moves the highlight in the completions, and wraps
        app.keys(":a").unwrap();
        let n = crate::tui::commands::complete(&app.line, &app).items.len();
        assert!(n > 1);
        app.mouse(wheel(MouseEventKind::ScrollDown, 5, 5));
        assert_eq!(app.hl, 1);
        app.mouse(wheel(MouseEventKind::ScrollUp, 5, 5));
        app.mouse(wheel(MouseEventKind::ScrollUp, 5, 5));
        assert_eq!(app.hl, n - 1);
    }

    #[test]
    fn notes_keep_the_latest() {
        let mut app = simple();
        for k in 0..NOTES + 5 {
            app.log(&(format!("m{k}"), false), None);
        }
        assert_eq!((app.notes.len(), app.notes[0].text.as_str()), (NOTES, "m5"));
    }

    #[test]
    fn menu_search_and_zoom_out() {
        let mut app = simple();
        app.keys("/").unwrap();
        assert_eq!(app.mode, Mode::Search);
        app.keys("<Esc>:fit 0 20ns\n").unwrap();
        let (cols, ticks) = (app.ws.view.cols, app.ws.view.ticks);
        app.ws.view.width = 80;
        app.keys("x").unwrap();
        assert!((app.ws.view.cols, app.ws.view.ticks) != (cols, ticks));
    }

    #[test]
    fn common_scope() {
        assert_eq!(super::common_scope(&["top.cpu.a", "top.cpu.sub.b"]), "top.cpu");
        assert_eq!(super::common_scope(&["top.cpu.a", "top.dma.b"]), "top");
        assert_eq!(super::common_scope(&["a", "top.b"]), "");
    }
}
