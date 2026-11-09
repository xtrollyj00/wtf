//! The decode form (spec §5 "Decode form"): a card with a new instance's name, protocol, clock,
//! prefix, each role and param as a field. While it is filled in it checks the names, reading no
//! value, and shows the `:decode` line `Enter` runs, so the command is learnt by using it. The
//! protocol form is the same card for `:protocol`: name, roles, each role's signal, `when` and
//! the fields (spec §5 "Protocol form").

use crate::draw::{gl, pal, put};
use crate::tui::app::App;
use crate::tui::keys::{self, Fixed, Key};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use wtf_core::protocol::{self, Found};
use wtf_core::stream::Decode;
use wtf_core::workspace::Workspace;

/// The suggestions a field offers at most.
pub const HINTS: usize = 20;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Name,
    Protocol,
    Clock,
    Prefix,
    Role { required: bool },
    Param,
    /// The protocol form's: its roles' names, `when`, and a field `name = expression`.
    Roles,
    When,
    Expr,
}

pub struct Field {
    pub key: String,
    pub kind: Kind,
    pub value: String,
}

impl Field {
    fn new(key: &str, kind: Kind, value: &str) -> Self {
        Field { key: key.into(), kind, value: value.into() }
    }

    /// It names a signal: the picker and the suggestions are signals.
    pub fn signal(&self) -> bool {
        matches!(self.kind, Kind::Clock | Kind::Role { .. })
    }

    /// The name errors give it: a field's name, before its `=`, else its key.
    fn label(&self) -> &str {
        match self.kind {
            Kind::Expr => self.value.split_once('=').map_or("", |(n, _)| n.trim()),
            _ => &self.key,
        }
    }
}

pub struct Form {
    pub fields: Vec<Field>,
    /// The field being filled in.
    pub at: usize,
    /// The field's suggestions, best first, and the one shown (`Ctrl-n` / `Ctrl-p` go through
    /// them, `Right` takes it).
    pub hints: Vec<String>,
    pub hint: usize,
    /// The names as last checked (`Workspace::check_decode`): fine, or what is wrong with them.
    pub check: Result<(), String>,
    /// What `Enter` was told, on the field it names, if one.
    pub error: Option<(Option<usize>, String)>,
    /// The first field shown, and the fields' screen lines (y, field) as last drawn.
    pub top: usize,
    /// The decode being edited (from the decodes list): its name isn't taken, and `Enter`
    /// decodes it again in place.
    pub editing: Option<String>,
    /// The protocol form: `Enter` defines the protocol and goes on to the decode form.
    pub protocol: bool,
}

/// What a key does in the form (`App::FORM_KEYS`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fill {
    Next,
    Prev,
    Take,
    Other,
    Back,
    Choose,
    Delete,
    Clear,
    Run,
    Close,
}

/// The protocols the form decodes: the built-in ones, then the session's (spec §16). `frame`,
/// `pair` and `check` work on streams and rules: their `:decode` line is typed.
pub fn protocols(ws: &Workspace) -> Vec<String> {
    protocol::BUILTINS.iter().map(|s| s.to_string()).chain(ws.protocols.iter().map(|p| p.name.clone())).collect()
}

impl Form {
    /// A form with `d`'s values, its fields those of its protocol; a `<placeholder>` (the
    /// interfaces picker's `clock=<clk>`) is left empty.
    pub fn new(ws: &mut Workspace, d: &Decode, found: &[Found]) -> Self {
        let value = |v: &str| if v.starts_with('<') && v.ends_with('>') { String::new() } else { v.to_string() };
        let mut f = Form { fields: vec![Field::new("name", Kind::Name, &d.name), Field::new("protocol", Kind::Protocol, &d.protocol)], at: 0, hints: Vec::new(), hint: 0, check: Ok(()), error: None, top: 0, editing: None, protocol: false };
        let given: Vec<(String, String)> = std::iter::once(("clock".to_string(), d.clock.clone())).chain(d.signals.iter().cloned()).chain(d.params.iter().cloned()).collect();
        f.reshape(ws);
        for (k, v) in given {
            if let Some(field) = f.fields.iter_mut().find(|x| x.key == k) {
                field.value = value(&v);
            }
        }
        // the first field left to fill in
        f.at = f.fields.iter().position(|x| x.value.is_empty() && !matches!(x.kind, Kind::Role { required: false } | Kind::Param | Kind::Prefix)).unwrap_or(0);
        f.suggest(ws, found);
        f
    }

    /// The protocol form, with roles (role, signal), as the selected rows give them.
    pub fn protocol(ws: &mut Workspace, roles: &[(String, String)]) -> Self {
        let names = roles.iter().map(|r| r.0.as_str()).collect::<Vec<_>>().join(", ");
        let fields = vec![Field::new("name", Kind::Name, ""), Field::new("roles", Kind::Roles, &names)];
        let mut f = Form { fields, at: 0, hints: Vec::new(), hint: 0, check: Ok(()), error: None, top: 0, editing: None, protocol: true };
        f.reshape(ws);
        for (r, sig) in roles {
            if let Some(field) = f.fields.iter_mut().find(|x| x.key == *r && x.signal()) {
                field.value = sig.clone();
            }
        }
        f
    }

    /// The fields after the protocol, as the protocol has them; the values of those kept. In
    /// the protocol form: a row per role, `when`, the fields and an empty one to add another.
    fn reshape(&mut self, ws: &Workspace) {
        let old: Vec<Field> = self.fields.drain(2..).collect();
        if self.protocol {
            let mut roles: Vec<String> = Vec::new();
            for r in self.fields[1].value.split(',').map(str::trim) {
                if !r.is_empty() && !roles.iter().any(|x| x == r) {
                    roles.push(r.to_string());
                }
            }
            let keep = |key: &str, kind: Kind| old.iter().find(|x| x.key == key && x.kind == kind).map_or(String::new(), |x| x.value.clone());
            for r in roles {
                let value = keep(&r, Kind::Role { required: true });
                self.fields.push(Field { key: r, kind: Kind::Role { required: true }, value });
            }
            self.fields.push(Field { key: "when".into(), kind: Kind::When, value: keep("when", Kind::When) });
            self.fields.extend(old.into_iter().filter(|x| x.kind == Kind::Expr));
            if self.fields.last().is_none_or(|x| x.kind != Kind::Expr || !x.value.trim().is_empty()) {
                self.fields.push(Field::new("field", Kind::Expr, ""));
            }
            return;
        }
        let name = self.fields[1].value.trim().to_string();
        let mut add = |key: &str, kind: Kind| {
            let value = old.iter().find(|x| x.key == key).map_or(String::new(), |x| x.value.clone());
            self.fields.push(Field { key: key.into(), kind, value });
        };
        if let Some(b) = protocol::builtin(&name) {
            if b.clocked {
                add("clock", Kind::Clock);
            }
            add("prefix", Kind::Prefix);
            b.required.iter().for_each(|r| add(r, Kind::Role { required: true }));
            b.optional.iter().for_each(|r| add(r, Kind::Role { required: false }));
            b.params.iter().for_each(|p| add(p, Kind::Param));
        } else if let Some(p) = ws.protocols.iter().find(|p| p.name == name) {
            add("clock", Kind::Clock);
            p.roles.iter().for_each(|r| add(r, Kind::Role { required: true }));
        }
    }

    fn value(&self, key: &str) -> &str {
        self.fields.iter().find(|f| f.key == key).map_or("", |f| f.value.trim())
    }

    /// The `:decode` line the fields make, without the `:`; the fields left empty aren't in it.
    pub fn line(&self) -> String {
        let quote = |v: &str| if v.contains(char::is_whitespace) { format!("\"{v}\"") } else { v.to_string() };
        if self.protocol {
            let p = self.sampler();
            let mut s = format!("protocol {} roles={} when={}", p.name, p.roles.join(","), quote(&p.when));
            for (f, e) in &p.fields {
                s += &format!(" {f}={}", quote(e));
            }
            return s;
        }
        let mut s = format!("decode {} {}", self.value("name"), self.value("protocol"));
        for f in self.fields[2..].iter().filter(|f| !f.value.trim().is_empty()) {
            s += &format!(" {}={}", f.key, quote(f.value.trim()));
        }
        s
    }

    /// The protocol the protocol form defines, its fields those with a name and an expression.
    pub fn sampler(&self) -> wtf_core::stream::Sampler {
        let of = |k: Kind| self.fields.iter().filter(move |f| f.kind == k);
        wtf_core::stream::Sampler {
            name: self.value("name").into(),
            roles: of(Kind::Role { required: true }).map(|f| f.key.clone()).collect(),
            when: self.value("when").into(),
            fields: of(Kind::Expr).filter_map(|f| f.value.split_once('=')).map(|(n, e)| (n.trim().to_string(), e.trim().to_string())).filter(|(n, e)| !n.is_empty() || !e.is_empty()).collect(),
        }
    }

    /// The roles given a signal in the protocol form (role, signal).
    fn role_signals(&self) -> Vec<(String, String)> {
        self.fields.iter().filter(|f| f.signal() && !f.value.trim().is_empty()).map(|f| (f.key.clone(), f.value.trim().to_string())).collect()
    }

    /// The signal a role empty in the form is played by under the prefix, if there is one.
    pub fn by_prefix(&self, ws: &mut Workspace, i: usize) -> Option<String> {
        let (f, prefix) = (&self.fields[i], self.value("prefix"));
        if !matches!(f.kind, Kind::Role { .. }) || !f.value.trim().is_empty() || prefix.is_empty() {
            return None;
        }
        ws.resolve(&format!("{prefix}{}", f.key)).ok().map(|p| ws.trace.probe_path(p))
    }

    /// The suggestion shown, if any.
    pub fn hint(&self) -> Option<&str> {
        self.hints.get(self.hint).map(String::as_str)
    }

    /// The field's suggestions, worked out once per change (spec §5 "Decode form"). Empty, a
    /// field suggests what fits it: the protocols of the interfaces found, their prefixes for the
    /// protocol, the clock found with the prefix, then signals ending in `clk` or `clock`, a role's
    /// signals ending in its name, the name the prefix's last part. Typed, the text matches as the
    /// picker's query does, those ending in the role's name first.
    fn suggest(&mut self, ws: &mut Workspace, found: &[Found]) {
        let (i, typed) = (self.at, self.fields[self.at].value.trim().to_string());
        let (protocol, prefix) = (self.value("protocol").to_string(), self.value("prefix").to_string());
        let mine = |x: &&Found| x.protocol == protocol;
        let with_prefix = |x: &&Found| mine(x) && !prefix.is_empty() && x.prefix.ends_with(prefix.as_str());
        let mut hints: Vec<String> = match self.fields[i].kind {
            Kind::Name => {
                let leaf = prefix.rsplit('.').next().unwrap_or("").trim_matches('_');
                if typed.is_empty() && !leaf.is_empty() { vec![leaf.to_string()] } else { Vec::new() }
            }
            Kind::Protocol if typed.is_empty() => found.iter().map(|x| x.protocol.to_string()).collect(),
            Kind::Protocol => fuzzy(&typed, protocols(ws).iter().map(String::as_str)),
            Kind::Prefix => fuzzy(&typed, found.iter().filter(mine).map(|x| x.prefix.as_str())),
            Kind::Clock => {
                let of_prefix = found.iter().filter(with_prefix).filter_map(|x| x.clock.clone()).filter(|_| typed.is_empty());
                of_prefix.chain(signals(ws, &typed, &["clk", "clock"])).collect()
            }
            Kind::Role { .. } if self.by_prefix(ws, i).is_some() => Vec::new(),
            Kind::Role { .. } => signals(ws, &typed, &[self.fields[i].key.to_ascii_lowercase().as_str()]),
            Kind::Param | Kind::Roles => Vec::new(),
            // the word being typed completed to a role or a field (`when` any, a field those before it)
            Kind::When | Kind::Expr => {
                let when = self.fields[i].kind == Kind::When;
                let names: Vec<String> = self.fields.iter().enumerate().filter_map(|(j, f)| match f.kind {
                    Kind::Role { .. } => Some(f.key.clone()),
                    Kind::Expr if when || j < i => Some(f.label().to_string()),
                    _ => None,
                }).filter(|n| !n.is_empty()).collect();
                complete(&self.fields[i].value, self.fields[i].kind == Kind::Expr, &names)
            }
        };
        let mut seen = std::collections::HashSet::new();
        hints.retain(|h| *h != typed && seen.insert(h.clone()));
        hints.truncate(HINTS);
        (self.hints, self.hint) = (hints, 0);
    }

    /// The fields an error names (`awready: unknown signal`, `no clock`), each of its `; `
    /// parts naming one at most.
    fn fields_of(&self, e: &str) -> Vec<usize> {
        let e = e.strip_prefix(&format!("{}: ", self.value("name"))).unwrap_or(e);
        if e.contains("the name is taken") || e.ends_with("is a built-in protocol") || e.starts_with("not a protocol name") {
            return vec![0];
        }
        let names = |part: &str, f: &Field| {
            let key = f.label();
            !key.is_empty() && (part.starts_with(&format!("{key}:")) || part.contains(&format!("no signal for {key}")) || part == format!("no {key}"))
        };
        e.split("; ").filter_map(|part| self.fields.iter().position(|f| names(part, f))).collect()
    }

    /// The error shown, from `Enter` or the check, and the fields it names.
    fn failed(&self) -> Option<(&str, Vec<usize>)> {
        let e = match (&self.error, &self.check) {
            (Some((_, e)), _) => e,
            (None, Err(e)) if self.missing().is_none() => e,
            _ => return None,
        };
        Some((e, self.fields_of(e)))
    }

    /// What the form says before checking the names, if anything: what is missing.
    fn missing(&self) -> Option<String> {
        if self.value("name").is_empty() {
            return Some(format!("name the {}", if self.protocol { "protocol" } else { "decode" }));
        }
        if self.protocol {
            return match () {
                _ if self.value("roles").is_empty() => Some("give it roles, the names when and the fields use: select rows before opening the form, or type them".into()),
                _ if self.value("when").is_empty() => Some("when: where a record is made, as valid && ready".into()),
                _ if self.fields.iter().any(|f| f.kind == Kind::Expr && !f.value.trim().is_empty() && !f.value.contains('=')) => Some("a field is a name and an expression: op = data[7:0]".into()),
                _ => None,
            };
        }
        let p = self.value("protocol");
        if p.is_empty() || self.fields.len() == 2 {
            return Some(format!("{} protocol to decode with", if p.is_empty() { "a" } else { "no such" }));
        }
        None
    }
}

impl App {
    /// The form's keys (spec §5 "Decode form"): `form_key` and the keys page both read them.
    pub const FORM_KEYS: &Fixed<Fill> = &[
        (&[(Key::code(KeyCode::Tab), Fill::Next), (Key::code(KeyCode::Down), Fill::Next), (Key::code(KeyCode::BackTab), Fill::Prev), (Key::code(KeyCode::Up), Fill::Prev)], "next / previous field"),
        (&[(Key::code(KeyCode::Right), Fill::Take)], "take the suggestion"),
        (&[(Key::ctrl('n'), Fill::Other), (Key::ctrl('p'), Fill::Back)], "the next / previous suggestion"),
        (&[(Key::ctrl(' '), Fill::Choose)], "pick a signal; on the protocol, the next one"),
        (&[(Key::code(KeyCode::Backspace), Fill::Delete)], "delete a character"),
        (&[(Key::ctrl('u'), Fill::Clear)], "clear the field"),
        (&[(Key::code(KeyCode::Enter), Fill::Run)], "run the line; a field it names keeps the form open"),
        (&[(Key::code(KeyCode::Esc), Fill::Close)], "close"),
    ];

    /// `:decode` without arguments, or a line to finish (`line` without the `decode`): the form.
    pub fn open_form(&mut self, line: &str) -> Result<(), String> {
        let d = match line.trim() {
            "" => Decode { name: String::new(), protocol: String::new(), clock: String::new(), signals: Vec::new(), params: Vec::new() },
            l => wtf_core::command::parse_decode(l)?,
        };
        self.form = Some(Form::new(&mut self.ws, &d, &self.found));
        self.form_check();
        Ok(())
    }

    /// `:protocol` without arguments: the protocol form, a role for each selected signal, named
    /// after it without the start they share (`rx_valid`, `rx_data`: `valid`, `data`).
    pub fn open_protocol_form(&mut self) -> Result<(), String> {
        let sel = if self.ws.rows.is_empty() { 0..0 } else { self.ws.selection() };
        let paths: Vec<String> = self.ws.rows[sel].iter().filter_map(wtf_core::rows::Row::sig).map(|s| self.ws.trace.vars[s.var].path.clone()).collect();
        let leaves: Vec<String> = paths.iter().map(|p| wtf_core::rows::child_name(p)).collect();
        let roles: Vec<(String, String)> = role_names(&leaves).into_iter().zip(paths).filter(|r| !r.0.is_empty()).collect();
        let mut f = Form::protocol(&mut self.ws, &roles);
        f.suggest(&mut self.ws, &self.found);
        self.form = Some(f);
        self.form_check();
        Ok(())
    }

    /// The form on decode `name`, to edit it (the decodes list's `e`).
    pub fn edit_decode(&mut self, name: &str) -> Result<(), String> {
        let d = self.ws.decodes.iter().find(|d| d.name == name).cloned().ok_or_else(|| format!("no decode {name}"))?;
        let mut f = Form::new(&mut self.ws, &d, &self.found);
        (f.editing, f.at) = (Some(name.to_string()), 1);
        self.form = Some(f);
        self.form_check();
        Ok(())
    }

    /// The form has the keys; they are fixed. A character the table doesn't have types into the
    /// field.
    pub fn form_key(&mut self, k: KeyEvent) {
        let Some(f) = &mut self.form else { return };
        let act = keys::fixed(Self::FORM_KEYS, k);
        let before = (f.at, f.fields[f.at].value.clone(), f.fields[1].value.clone());
        match act {
            None => match k.code {
                KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => f.fields[f.at].value.push(c),
                _ => return,
            },
            Some(Fill::Close) => return self.form = None,
            Some(Fill::Run) => return self.form_run(),
            Some(Fill::Next) => f.at = (f.at + 1) % f.fields.len(),
            Some(Fill::Prev) => f.at = (f.at + f.fields.len() - 1) % f.fields.len(),
            Some(Fill::Delete) => _ = f.fields[f.at].value.pop(),
            Some(Fill::Clear) => f.fields[f.at].value.clear(),
            Some(Fill::Take) => {
                if let Some(h) = f.hint().map(String::from) {
                    f.fields[f.at].value = h;
                }
            }
            Some(Fill::Other) => return f.hint = (f.hint + 1) % f.hints.len().max(1),
            Some(Fill::Back) => return f.hint = (f.hint + f.hints.len().max(1) - 1) % f.hints.len().max(1),
            Some(Fill::Choose) if f.fields[f.at].kind == Kind::Protocol => {
                let all = protocols(&self.ws);
                let next = all.iter().position(|p| *p == f.fields[1].value.trim()).map_or(0, |i| (i + 1) % all.len());
                f.fields[1].value = all[next].clone();
            }
            Some(Fill::Choose) if f.fields[f.at].signal() => {
                // the signals picker, its query the field's text or the role's name
                let (i, query) = (f.at, if f.fields[f.at].value.is_empty() { f.fields[f.at].key.clone() } else { f.fields[f.at].value.clone() });
                let query = if query == "clock" { "clk".to_string() } else { query };
                if self.open_picker("signals").is_ok()
                    && let Some(p) = &mut self.picker
                {
                    p.fill = Some(i);
                    p.set_query(&query);
                }
                return;
            }
            Some(Fill::Choose) => return,
        }
        let Some(f) = &mut self.form else { return };
        // the protocol form's rows follow its roles, and an empty field follows the last
        if f.fields[1].value != before.2 || (f.protocol && f.fields[f.at].value != before.1) {
            f.reshape(&self.ws);
        }
        if (f.at, &f.fields[f.at].value) != (before.0, &before.1) || f.fields[1].value != before.2 {
            let changed = f.fields[f.at].value != before.1 || f.fields[1].value != before.2;
            if changed {
                f.error = None;
            }
            f.suggest(&mut self.ws, &self.found);
            if changed {
                self.form_check();
            }
        }
    }

    /// The picker's signal for field `i`, picked with `Ctrl-Space`.
    pub fn form_fill(&mut self, i: usize, path: String) {
        let Some(f) = &mut self.form else { return };
        if let Some(field) = f.fields.get_mut(i) {
            field.value = path;
            (f.at, f.error) = (i, None);
            f.suggest(&mut self.ws, &self.found);
            self.form_check();
        }
    }

    /// Check the form's names, as typed: they name signals, and the instance's name is new.
    /// Reading no value, it is quick on any trace (spec §5 "Decode form").
    pub fn form_check(&mut self) {
        let Some(f) = &mut self.form else { return };
        if f.missing().is_some() {
            return f.check = Ok(());
        }
        let (line, name) = (f.line(), f.value("name").to_string());
        let taken = match f.protocol {
            true => self.ws.protocols.iter().any(|p| p.name == name),
            false => self.ws.decodes.iter().any(|d| d.name == name) && f.editing.as_deref() != Some(name.as_str()),
        };
        let r = match taken {
            true => Err(format!("{name}: the name is taken; give another")),
            // a role without a signal leaves the expressions unchecked: their widths are its
            false if f.protocol && f.fields.iter().any(|x| x.signal() && x.value.trim().is_empty()) => Ok(()),
            false if f.protocol => self.ws.check_protocol(&f.sampler(), &f.role_signals()),
            false => wtf_core::command::parse_decode(&line["decode ".len()..]).and_then(|d| self.ws.check_decode(&d)),
        };
        if let Some(f) = &mut self.form {
            f.check = r;
        }
    }

    /// `Enter`: run the line, the results added as a group as the interfaces picker does, and
    /// close; an error stays on the field it names.
    fn form_run(&mut self) {
        let Some(f) = &mut self.form else { return };
        if let Some(m) = f.missing() {
            let at = match () {
                _ if f.value("name").is_empty() => 0,
                _ if !f.protocol || f.value("roles").is_empty() => 1,
                _ if f.value("when").is_empty() => f.fields.iter().position(|x| x.kind == Kind::When).unwrap_or(1),
                _ => f.fields.iter().position(|x| x.kind == Kind::Expr && !x.value.trim().is_empty() && !x.value.contains('=')).unwrap_or(1),
            };
            return (f.at, f.error) = (at, Some((Some(at), m)));
        }
        if f.protocol {
            return self.protocol_run();
        }
        let (line, name) = (f.line(), f.value("name").to_string());
        let editing = f.editing.as_deref() == Some(name.as_str());
        if self.ws.decodes.iter().any(|d| d.name == name) && !editing {
            let f = self.form.as_mut().expect("checked above");
            return (f.at, f.error) = (0, Some((Some(0), format!("{name}: the name is taken; give another"))));
        }
        // edited: made again in place, its rows kept where it still makes their streams
        let r = match editing {
            true => wtf_core::command::parse_decode(&line["decode ".len()..]).and_then(|d| self.ws.redecode(d)).map(|failed| {
                self.msg = Some(match failed.is_empty() {
                    true => (format!("{name}: decoded again"), false),
                    false => (format!("{name}: decoded again, but its readers failed: {failed}"), true),
                });
            }),
            false => self.decode_and_show(&line, &name),
        };
        match r {
            Ok(()) => self.form = None,
            Err(e) => {
                let f = self.form.as_mut().expect("checked above");
                let at = f.fields_of(&e).first().copied();
                if let Some(i) = at {
                    f.at = i;
                    f.suggest(&mut self.ws, &self.found);
                }
                f.error = Some((at, e));
            }
        }
    }
}

impl App {
    /// The protocol form's `Enter`: define it as `:protocol` does, then the decode form on it with
    /// the roles' signals filled in. A name taken is refused, never replaced; an error stays on the
    /// field it names.
    fn protocol_run(&mut self) {
        let Some(f) = &mut self.form else { return };
        let (line, name) = (f.line(), f.value("name").to_string());
        // `:protocol` checks only names; the form's check read the expressions against the signals
        let r = match self.ws.protocols.iter().any(|p| p.name == name) {
            true => Err(format!("{name}: the name is taken; give another")),
            false if f.check.is_err() => f.check.clone(),
            false => wtf_core::command::run(&mut self.ws, &line).map(drop),
        };
        let f = self.form.as_mut().expect("checked above");
        match r {
            Ok(()) => {
                let d = Decode { name: String::new(), protocol: name.clone(), clock: String::new(), signals: f.role_signals(), params: Vec::new() };
                self.msg = Some((format!("protocol {name} defined: put it on signals"), false));
                self.form = Some(Form::new(&mut self.ws, &d, &self.found));
                self.form_check();
            }
            Err(e) => {
                let e = e.strip_prefix(&format!("protocol {name} not made: ")).unwrap_or(&e).to_string();
                let at = f.fields_of(&e).first().copied();
                if let Some(i) = at {
                    f.at = i;
                    f.suggest(&mut self.ws, &self.found);
                }
                f.error = Some((at, e));
            }
        }
    }
}

/// Draws the form: a card in the middle with a line per field (a role the prefix plays shows its
/// signal dim), then what is wrong with the names, and the line `Enter` runs.
pub fn draw(app: &mut App, buf: &mut Buffer, w: usize, body: usize) {
    let Some(f) = &app.form else { return };
    let (bw, bh) = (w.saturating_sub(4).min(90), body.saturating_sub(2));
    if bw < 40 || bh < 10 {
        return;
    }
    let card = crate::tui::render::card_style();
    let (dim, red) = (card.fg(pal().dim), card.fg(pal().unknown));
    let acc = card.fg(pal().cursor).add_modifier(Modifier::BOLD);
    // the result lines under the fields: what is wrong or missing, and the line
    let mut result: Vec<(String, Style)> = Vec::new();
    let failed = f.failed();
    match (&failed, f.missing()) {
        // at most three lines of it, broken at its parts
        (Some((e, _)), _) => result.extend(wrap(e, bw - 4).into_iter().take(3).map(|l| (l, red))),
        (_, Some(m)) => result.push((m, dim)),
        _ if f.protocol && f.fields.iter().any(|x| x.signal() && x.value.trim().is_empty()) => result.push(("a signal for each role checks when and the fields; Enter defines it as it is".into(), dim)),
        _ if f.protocol => result.push(("the names match: Enter defines it, then decodes it".into(), card)),
        _ => result.push((format!("the names match: Enter decodes{}", if f.editing.is_some() { " it again" } else { "" }), card)),
    }
    let bad: Vec<usize> = failed.map_or(Vec::new(), |(_, b)| b);
    let line = format!(":{}", f.line());
    let rows = f.fields.len();
    let room = bh.saturating_sub(4 + result.len());
    let shown = rows.min(room.max(3));
    let height = shown + 4 + result.len();
    let (x0, y0) = ((w - bw) / 2, (body.saturating_sub(height)) / 2);
    let r = Rect::new(x0 as u16, y0 as u16, bw as u16, height.min(body) as u16);
    // what is behind fades, as behind the picker, so the card stands out
    if !pal().mono {
        for y in 0..body {
            for x in 0..w {
                buf[(x as u16, y as u16)].set_fg(pal().dim);
            }
        }
    }
    let what = match &f.editing {
        Some(n) => format!("edit decode {n}"),
        None if f.protocol => "protocol".into(),
        None => "decode".into(),
    };
    let title = vec![(format!("{}{what}", crate::tui::render::icon_text(crate::draw::icon::RECORD)), acc)];
    let keys = format!(
        "Tab next  {}{}{}Enter run  Esc close",
        if f.hint().is_some() { "→ take  " } else { "" },
        if f.hints.len() > 1 { "C-n other  " } else { "" },
        if f.fields[f.at].signal() { "C-Space pick  " } else { "" }
    );
    let keys = keys.as_str();
    crate::tui::render::card_frame(buf, r, pal().dim, &title, "", keys);
    let Some(f) = &mut app.form else { return };
    // the fields, scrolled to show the one being filled in
    if f.at < f.top {
        f.top = f.at;
    } else if f.at >= f.top + shown {
        f.top = f.at + 1 - shown;
    }
    let (kx, vx) = (x0 + 2, x0 + 13);
    let vw = bw.saturating_sub(16);
    let mut lines = Vec::new();
    for i in (f.top..rows).take(shown) {
        let y = y0 + 1 + i - f.top;
        lines.push((y, i));
        let (field, on) = (&f.fields[i], i == f.at);
        let required = !matches!(field.kind, Kind::Prefix | Kind::Role { required: false } | Kind::Param | Kind::Expr);
        let bad = bad.contains(&i);
        let key_st = match (on, bad, required) {
            (true, _, _) => acc,
            (_, true, _) => red,
            (_, _, false) => dim,
            _ => card,
        };
        if on {
            put(buf, x0 + 1, y, &gl().hl.to_string(), acc);
        }
        put(buf, kx, y, &field.key.chars().take(10).collect::<String>(), key_st);
        let text: String = field.value.chars().rev().take(vw.saturating_sub(1)).collect::<Vec<_>>().into_iter().rev().collect();
        put(buf, vx, y, &text, card.add_modifier(Modifier::BOLD));
        let mut x = vx + text.chars().count();
        if on {
            put(buf, x, y, " ", card.add_modifier(Modifier::REVERSED));
            x += 1;
        }
        // after the text: the suggestion, the prefix's signal, the protocol's line
        let after = match field.kind {
            _ if on && f.hint().is_some() => f.hint().map(|h| match f.hints.len() {
                1 => format!("{} {h}", gl().arrow),
                n => format!("{} {h}  {}/{n}", gl().arrow, f.hint + 1),
            }),
            Kind::Role { .. } => f.by_prefix(&mut app.ws, i),
            Kind::Prefix if !field.value.trim().is_empty() => {
                let roles: Vec<usize> = (0..rows).filter(|&j| matches!(f.fields[j].kind, Kind::Role { .. })).collect();
                let n = roles.iter().filter(|&&j| f.by_prefix(&mut app.ws, j).is_some()).count();
                Some(format!("plays {n} of {} roles", roles.len()))
            }
            Kind::Protocol => protocol::builtin(field.value.trim()).map(|b| b.about.to_string()),
            Kind::Param if field.value.is_empty() => Some("default".into()),
            _ => None,
        };
        if let Some(a) = after {
            // a role's signal under the prefix stands where its value would; the rest apart
            let x = if text.is_empty() && !on { x } else { x + 2 };
            let room = (vx + vw).saturating_sub(x);
            put(buf, x, y, &a.chars().take(room).collect::<String>(), dim);
        }
    }
    // the rule, what is wrong or missing, and the line
    let mut y = y0 + 1 + shown;
    put(buf, x0 + 1, y, &gl().card[4].to_string().repeat(bw - 2), dim);
    for (text, st) in &result {
        y += 1;
        put(buf, kx, y, &text.chars().take(bw - 4).collect::<String>(), *st);
    }
    y += 1;
    put(buf, kx, y, &line.chars().take(bw - 4).collect::<String>(), dim);
    app.hit.boxed = Some(r);
    app.hit.records = lines;
}

/// A long text in lines of at most `w` characters, broken after its `; ` parts or at spaces.
fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    for word in s.split(' ') {
        let last = out.last_mut().expect("never empty");
        if !last.is_empty() && (last.chars().count() + 1 + word.chars().count() > w || last.ends_with(';')) {
            out.push(word.into());
        } else {
            if !last.is_empty() {
                last.push(' ');
            }
            last.push_str(word);
        }
    }
    out
}

/// Role names for signals by their leaves: without the start they all share up to a `_` or `.`
/// (`rx_valid`, `rx_data`: `valid`, `data`; one `s_axi_awvalid`: `awvalid`), each made a plain
/// name (`data[3]`: `data_3`).
fn role_names(leaves: &[String]) -> Vec<String> {
    let first = leaves.first().map_or("", String::as_str);
    let len = leaves.iter().fold(first.len(), |n, l| first.bytes().zip(l.bytes()).take(n).take_while(|(a, b)| a == b).count());
    let cut = first[..len].rfind(['_', '.']).map_or(0, |i| i + 1);
    let plain = |l: &str| l.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect::<String>().trim_matches('_').to_string();
    leaves.iter().map(|l| plain(if l.len() > cut { &l[cut..] } else { l })).collect()
}

/// The text with its last word completed to each of `names` it starts (`valid && re` to
/// `valid && ready`); in a field, only after its `=`.
fn complete(text: &str, field: bool, names: &[String]) -> Vec<String> {
    if field && !text.contains('=') {
        return Vec::new();
    }
    let start = text.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    let word = &text[start..];
    if word.is_empty() || word.starts_with(|c: char| c.is_ascii_digit()) {
        return Vec::new();
    }
    names.iter().filter(|n| n.starts_with(word) && n.as_str() != word).map(|n| format!("{}{n}", &text[..start])).collect()
}

/// The texts `query` matches as the picker's query does, best first; all of them for no query.
fn fuzzy<'a>(query: &str, texts: impl Iterator<Item = &'a str>) -> Vec<String> {
    let pat = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let (mut m, mut buf) = (Matcher::new(Config::DEFAULT.match_paths()), Vec::new());
    let mut scored: Vec<(u32, &str)> = texts.filter_map(|t| pat.score(Utf32Str::new(t, &mut buf), &mut m).map(|s| (s, t))).collect();
    scored.sort_by_key(|&(s, t)| (std::cmp::Reverse(s), t.len()));
    scored.into_iter().map(|(_, t)| t.to_string()).collect()
}

/// The file's signals for a field: those whose name ends in one of `ends` (lower case), shortest
/// first; with text typed, those it matches, ending so first.
fn signals(ws: &Workspace, typed: &str, ends: &[&str]) -> Vec<String> {
    let t = &ws.trace;
    let ends_in = |p: &str| ends.iter().any(|e| p.to_ascii_lowercase().ends_with(e));
    let paths = (0..t.vars.len()).filter(|&v| !t.is_computed(v)).map(|v| t.vars[v].path.as_str());
    if typed.is_empty() {
        let mut v: Vec<&str> = paths.filter(|p| ends_in(p)).collect();
        v.sort_by_key(|p| p.len());
        return v.into_iter().take(HINTS).map(String::from).collect();
    }
    let mut v = fuzzy(typed, paths);
    v.sort_by_key(|p| !ends_in(p));
    v.truncate(HINTS);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let t = wtf_core::trace::Trace::open("tests/data/axilite.vcd".as_ref()).unwrap();
        App::new(Workspace::new(t, "x".into()))
    }

    fn form(a: &App) -> &Form {
        a.form.as_ref().expect("the form")
    }

    /// Filled in by typing and suggestions, the form checks the names as typed, reading no value,
    /// shows its line, and `Enter` makes it as the interfaces picker does: its results grouped
    /// (spec §5 "Decode form").
    #[test]
    fn fills_checks_and_runs() {
        let mut a = app();
        a.keys(":decode\n").unwrap();
        assert_eq!((form(&a).at, form(&a).fields.len(), form(&a).missing()), (0, 2, Some("name the decode".into())));
        // the protocol by its suggestion: its roles and params come with it
        a.keys("s\taxil<Right>\t").unwrap();
        assert_eq!(form(&a).fields[1].value, "axilite");
        assert_eq!(form(&a).fields.iter().map(|f| f.key.as_str()).take(5).collect::<Vec<_>>(), ["name", "protocol", "clock", "prefix", "awvalid"]);
        // the clock: a signal ending in clk first
        a.keys("a").unwrap();
        assert_eq!(form(&a).hint(), Some("top.aclk"));
        a.keys("<Right>\ts_axi_").unwrap();
        assert_eq!(form(&a).line(), "decode s axilite clock=top.aclk prefix=s_axi_");
        let f = a.form.take().unwrap();
        assert_eq!(f.by_prefix(&mut a.ws, 4).as_deref(), Some("top.s_axi_awvalid"));
        a.form = Some(f);
        // the names match; nothing decoded or read yet
        assert_eq!(form(&a).check, Ok(()));
        let read = |a: &App| a.ws.trace.by_path.get("top.s_axi_awaddr").is_some_and(|&v| a.ws.trace.loaded(v));
        assert!(a.ws.decodes.is_empty() && a.ws.streams.is_empty() && !read(&a));
        a.keys("\n").unwrap();
        assert!(a.form.is_none());
        assert_eq!((a.ws.decodes.len(), read(&a)), (1, true));
        assert!(a.ws.rows.iter().any(|r| matches!(r, wtf_core::rows::Row::Group { name, .. } if name == "s")), "{:?}", a.msg);
    }

    /// What is wrong stays on the fields it names: the check's error, and `Enter`'s, which keeps
    /// the form open on the first such field; a taken name is refused, never replaced.
    #[test]
    fn errors_on_fields() {
        let mut a = app();
        a.keys(":decode\ns\taxilite\t\tm_axi_").unwrap();
        let f = form(&a);
        let (_, bad) = f.failed().expect("an error");
        let keys: Vec<&str> = bad.iter().map(|&i| f.fields[i].key.as_str()).collect();
        assert!(keys.starts_with(&["awvalid", "awready"]) && keys.contains(&"clock"), "{keys:?}");
        a.keys("\n").unwrap();
        assert_eq!(form(&a).fields[form(&a).at].key, "awvalid");
        assert!(a.ws.decodes.is_empty());
        // made, then the same name again
        a.keys("<Esc>:decode s axilite clock=aclk prefix=s_axi_\n:decode\ns\taxilite\t").unwrap();
        a.keys("aclk\ts_axi_\n").unwrap();
        let f = form(&a);
        assert_eq!((f.at, f.error.as_ref().map(|e| e.1.as_str())), (0, Some("s: the name is taken; give another")));
        let s = crate::snapshot(&mut a, 100, Some(30));
        assert!(s.contains("│ s: the name is taken;") && s.contains("│ give another"), "{s}");
        // the check says so too, on the name, as it is typed
        a.keys("<BS>s").unwrap();
        assert_eq!((form(&a).error.as_ref(), form(&a).failed().map(|(_, b)| b)), (None, Some(vec![0])));
    }

    /// A protocol's params: optional, "default" while empty, no suggestions; a screen too narrow
    /// for the card draws none.
    #[test]
    fn params_and_narrow() {
        let mut a = app();
        a.keys(":decode\nu\tuart").unwrap();
        let at = form(&a).fields.iter().position(|f| f.key == "baud").expect("uart's baud");
        (0..at - 1).for_each(|_| a.keys("\t").unwrap());
        assert!(form(&a).hints.is_empty());
        let s = crate::snapshot(&mut a, 100, Some(30));
        assert!(s.lines().any(|l| l.contains("▌baud") && l.contains("default")), "{s}");
        assert!(!crate::snapshot(&mut a, 40, Some(20)).contains("decode"));
    }

    /// `Ctrl-Space` on a signal field opens the signals picker on the role's name; `Enter` there
    /// fills the field and goes back to the form. On the protocol, it goes to the next one.
    #[test]
    fn picks_a_signal() {
        let mut a = app();
        a.keys(":decode\ns\taxilite\t\t\t").unwrap();
        assert_eq!(form(&a).fields[form(&a).at].key, "awvalid");
        a.keys("<C-Space>").unwrap();
        let p = a.picker.as_ref().expect("the picker");
        assert_eq!((p.fill, p.query.as_str()), (Some(form(&a).at), "awvalid"));
        a.keys("\n").unwrap();
        assert!(a.picker.is_none());
        assert_eq!(form(&a).fields[4].value, "top.s_axi_awvalid");
        // Ctrl-s and Tab do nothing in it: a signal for the field, nothing else
        a.keys("<C-Space><C-s>\t").unwrap();
        assert_eq!(a.picker.as_ref().map(|p| (p.source, p.marked.len())), Some((0, 0)));
        a.keys("<Esc>").unwrap();
        assert!(a.picker.is_none() && a.form.is_some());
        a.keys("<S-Tab><S-Tab><S-Tab><C-Space>").unwrap();
        assert_eq!(form(&a).fields[1].value, "axi4");
    }

    /// Without typing a name: the found interface's protocol, prefix and clock, the name from the
    /// prefix, each taken with `→`. Typed text matches fuzzily, `Ctrl-n` goes to the next match.
    #[test]
    fn suggests_what_fits() {
        let mut a = app();
        a.keys(":decode\n\t").unwrap();
        assert_eq!(form(&a).hint(), Some("axilite"));
        a.keys("<Right>\t").unwrap();
        assert_eq!(form(&a).hint(), Some("top.aclk"));
        a.keys("<Right>\t").unwrap();
        assert_eq!(form(&a).hint(), Some("top.s_axi_"));
        a.keys("<Right><S-Tab><S-Tab><S-Tab>").unwrap();
        assert_eq!(form(&a).hint(), Some("s_axi"));
        a.keys("<Right>").unwrap();
        assert_eq!(form(&a).line(), "decode s_axi axilite clock=top.aclk prefix=top.s_axi_");
        // a role the prefix plays suggests nothing; without the prefix, its signal by its ending
        a.keys("\t\t\t<C-u>\t").unwrap();
        assert_eq!((form(&a).fields[form(&a).at].key.as_str(), form(&a).hint()), ("awvalid", Some("top.s_axi_awvalid")));
        a.keys("awv").unwrap();
        let hints = form(&a).hints.clone();
        assert!(hints.len() > 1 && hints[0] == "top.s_axi_awvalid", "{hints:?}");
        a.keys("<C-n>").unwrap();
        assert_eq!(form(&a).hint(), Some(hints[1].as_str()));
        a.keys("<C-p><Right>").unwrap();
        assert_eq!(form(&a).fields[4].value, "top.s_axi_awvalid");
    }

    /// As drawn: the check's error wrapped, then the names matching; a field's suggestions
    /// counted; a long form scrolled to its field.
    #[test]
    fn draws() {
        let mut a = app();
        a.keys(":decode\ns\taxilite\taclk\tm_axi_").unwrap();
        insta::assert_snapshot!("form_error", crate::snapshot(&mut a, 100, Some(30)));
        a.keys("<C-u>s_axi_").unwrap();
        insta::assert_snapshot!("form_match", crate::snapshot(&mut a, 100, Some(30)));
        // a role with two matches, the prefix cleared
        a.keys("<C-u>\tawv").unwrap();
        let s = crate::snapshot(&mut a, 100, Some(30));
        assert!(s.contains("awv   → top.s_axi_awvalid  1/2") && s.contains("C-n other"), "{s}");
        // too short for every field: scrolled to the one filled in, and back
        a.keys("<S-Tab><S-Tab><S-Tab><S-Tab><S-Tab>").unwrap();
        let s = crate::snapshot(&mut a, 100, Some(14));
        assert!(s.contains("▌rresp") && !s.contains(" name "), "{s}");
        a.keys("\t").unwrap();
        assert!(crate::snapshot(&mut a, 100, Some(14)).contains("▌name"));
    }

    /// The session's own protocol: a clock and its roles, no prefix (spec §16).
    #[test]
    fn session_protocol() {
        let mut a = app();
        a.keys(":protocol w roles=valid,ready when=valid && ready\n").unwrap();
        a.keys(":decode\nx\tw\t").unwrap();
        assert_eq!(form(&a).fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["name", "protocol", "clock", "valid", "ready"]);
        a.keys("aclk\ts_axi_wvalid\ts_axi_wready").unwrap();
        assert_eq!(form(&a).check, Ok(()));
        a.keys("\n").unwrap();
        assert_eq!(a.ws.streams.iter().map(|s| (s.name.as_str(), s.records.len())).collect::<Vec<_>>(), [("x", 2)]);
    }

    /// The other keys: `Backspace` deletes, a key not in the table does nothing, `Enter` with
    /// no protocol says so on that field, `Esc` closes; a click fills in the field clicked.
    #[test]
    fn keys_and_mouse() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mut a = app();
        a.keys(":decode\nsx<BS><Right>").unwrap();
        assert_eq!(form(&a).fields[0].value, "s");
        a.form_key(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE));
        a.keys("<C-x><C-Space>").unwrap();
        assert_eq!((form(&a).fields[0].value.as_str(), form(&a).at, a.picker.is_none()), ("s", 0, true));
        a.keys("\n").unwrap();
        assert_eq!((form(&a).at, form(&a).error.clone()), (1, Some((Some(1), "a protocol to decode with".into()))));
        a.keys("axilite").unwrap();
        crate::snapshot(&mut a, 100, Some(30));
        let (y, i) = a.hit.records[4];
        let x = a.hit.boxed.unwrap().x + 3;
        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y as u16, modifiers: KeyModifiers::NONE });
        assert_eq!(form(&a).at, i);
        // outside the card: nothing, and the rows under it take nothing either
        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 0, row: 0, modifiers: KeyModifiers::NONE });
        assert_eq!(form(&a).at, i);
        a.keys("<Esc>").unwrap();
        assert!(a.form.is_none());
    }
    /// The protocol form (spec §5 "Protocol form"): the selected rows its roles, named without
    /// the start they share; `when` and the fields completed and checked as typed; `Enter`
    /// defines it and goes on to the decode form with the roles' signals filled in.
    #[test]
    fn protocol_form() {
        let mut a = app();
        a.run("add top.s_axi_wvalid top.s_axi_wready top.s_axi_wdata").unwrap();
        a.run("select s_axi_wvalid s_axi_wdata").unwrap();
        a.keys(":protocol\n").unwrap();
        let keys = |a: &App| form(a).fields.iter().map(|f| f.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&a), ["name", "roles", "wvalid", "wready", "wdata", "when", "field"]);
        assert_eq!((form(&a).fields[1].value.as_str(), form(&a).fields[4].value.as_str()), ("wvalid, wready, wdata", "top.s_axi_wdata"));
        assert_eq!(form(&a).missing(), Some("name the protocol".into()));
        // when, a role's name completed; a field, and another row to add the next
        a.keys("w\t\t\t\t\twvalid && wr").unwrap();
        assert_eq!(form(&a).hint(), Some("wvalid && wready"));
        a.keys("<Right>\tlo wd").unwrap();
        assert_eq!(form(&a).hint(), None, "a field's name isn't completed");
        a.keys("<C-u>lo = wd<Right>[7:0]").unwrap();
        assert_eq!(keys(&a)[6..], ["field", "field"]);
        a.keys("\thi = lo[3").unwrap();
        assert!(matches!(&form(&a).check, Err(e) if e.starts_with("hi: ")), "{:?}", form(&a).check);
        a.keys(":0]").unwrap();
        assert_eq!(form(&a).check, Ok(()));
        assert_eq!(form(&a).line(), "protocol w roles=wvalid,wready,wdata when=\"wvalid && wready\" lo=wdata[7:0] hi=lo[3:0]");
        insta::assert_snapshot!("protocol_form", crate::snapshot(&mut a, 100, Some(24)));
        // defined, then the decode form on it
        a.keys("\n").unwrap();
        assert_eq!(a.ws.protocols.len(), 1);
        let f = form(&a);
        assert!(!f.protocol && f.fields[1].value == "w" && f.at == 0);
        assert_eq!(f.fields.iter().find(|x| x.key == "wready").map(|x| x.value.as_str()), Some("top.s_axi_wready"));
        a.keys("x\t\taclk\n").unwrap();
        assert!(a.form.is_none(), "{:?}", a.msg);
        assert_eq!(a.ws.streams.iter().map(|s| (s.name.as_str(), s.records.len())).collect::<Vec<_>>(), [("x", 2)]);
    }

    /// What is wrong stays on the field it names: a name no role, a field written without `=`,
    /// a name taken. Roles typed in without signals leave the expressions unchecked, and `Enter`
    /// defines the protocol as it is.
    #[test]
    fn protocol_form_errors() {
        let mut a = app();
        a.keys(":protocol\n").unwrap();
        assert_eq!(form(&a).fields.len(), 4, "no rows: no roles");
        a.keys("p\tvalid, ready").unwrap();
        assert_eq!(form(&a).fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["name", "roles", "valid", "ready", "when", "field"]);
        a.keys("\ts_axi_wvalid\ts_axi_wready\tvalid && go\tlo").unwrap();
        let f = form(&a);
        assert_eq!(f.missing(), Some("a field is a name and an expression: op = data[7:0]".into()));
        a.keys("\n").unwrap();
        assert_eq!(form(&a).at, 5);
        a.keys("<C-u>").unwrap();
        let (e, bad) = form(&a).failed().expect("an error");
        assert!(e.starts_with("when: go is no role"), "{e}");
        assert_eq!(bad, [4]);
        a.keys("\n").unwrap();
        assert_eq!((form(&a).at, a.ws.protocols.len()), (4, 0));
        // a role without a signal: not checked, defined as it is
        a.keys("<C-u>valid\t<S-Tab><S-Tab><C-u>").unwrap();
        assert_eq!(form(&a).check, Ok(()));
        assert!(crate::snapshot(&mut a, 100, Some(24)).contains("a signal for each role checks"));
        a.keys("\n").unwrap();
        assert_eq!((a.ws.protocols.len(), form(&a).protocol), (1, false));
        // the name taken now
        a.keys("<Esc>:protocol\np\tvalid\t\tvalid").unwrap();
        assert_eq!(form(&a).check, Err("p: the name is taken; give another".into()));
        assert_eq!(form(&a).fields_of("p: the name is taken; give another"), [0]);
        assert_eq!(super::role_names(&["rx_valid".into(), "rx_data[3]".into()]), ["valid", "data_3"]);
        assert_eq!(super::role_names(&["s_axi_awvalid".into()]), ["awvalid"]);
    }
}
