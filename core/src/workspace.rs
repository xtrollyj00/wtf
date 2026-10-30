//! A workspace: a trace and what is shown of it (rows, selection, cursors, zoom), with undo.
//! The core both the terminal UI and the MCP server work on (spec §5, §6, §9, §11).

use crate::computed::{self, Form};
use crate::decode;
use crate::protocol;
use crate::radix;
use crate::rows::{self, Row, Sig};
use crate::search;
use crate::session;
use crate::stream::{self, Decode, Sampler, Stream};
use std::collections::HashMap;
use crate::time::{self, Tick};
use crate::trace::{Trace, Val};
use crate::view::View;
use std::path::PathBuf;

mod decoding;

/// A message for the user: the text, and whether it is an error or a warning.
pub type Msg = (String, bool);

/// A line under a row (`Workspace::subs`): a var, or a bit of a bus, and how deep it is nested.
#[derive(Clone, PartialEq, Debug)]
pub struct Sub {
    pub var: usize,
    pub bit: Option<u32>,
    pub depth: usize,
}

pub struct Workspace {
    pub trace: Trace,
    pub file_name: String,
    /// The trace file, for `:e`, and the session file `:w` writes (spec §11).
    pub trace_path: PathBuf,
    pub session_path: PathBuf,
    /// The layout as last saved or loaded (`session::rows_toml`); `[+]` when it differs.
    pub saved: String,
    /// Counts `:e` reloads: a reload replaces the rows without being an undo step.
    reloads: u32,
    pub view: View,
    /// Marks by number, index 1 - 8 for cursors 2 - 9 (spec §9). Cursor 1 is the cursor, whose
    /// time is `view.cursor`, so index 0 stays None.
    pub marks: [Option<Tick>; 9],
    /// Where cursor 1 was before the last jump that moved it, for `''` (spec §9).
    pub back: Option<Tick>,
    /// The time unit of the agent's answers (`:unit`), or None for the one the fastest clock used
    /// so far fits (`clock_unit`), or without either one per answer (spec §15).
    pub unit: Option<usize>,
    clock_unit: Option<usize>,
    /// Names resolve under this scope first (`:scope dut`), so `s_axi_awvalid` is
    /// `dut.s_axi_awvalid`; saved in the session.
    pub scope: Option<String>,
    pub rows: Vec<Row>,
    /// Named computed signals (`:signal name = expr`) in the order defined, made again on `:e`.
    pub signals: Vec<(String, String)>,
    /// Names given to signals' values (`:names state 0=IDLE 1=BUSY`): path and the pairs as typed,
    /// given again on `:e`.
    pub value_names: Vec<(String, String)>,
    /// Protocols and their instances in the order defined, sampled again on `:e`, and the streams
    /// they make (spec §16).
    pub protocols: Vec<Sampler>,
    pub decodes: Vec<Decode>,
    pub streams: Vec<Stream>,
    /// While an instance samples: its roles, names that stand for its signals.
    roles: HashMap<String, String>,
    /// Index into `rows`; always a visible row.
    pub sel: usize,
    /// A bit line of the selected (expanded) bus, if one of those is selected.
    pub bit: Option<u32>,
    /// The other end of a `V` selection in Edit mode.
    pub anchor: Option<usize>,
    /// Row layouts before each change, and after each undo (spec §5 "Undo"), each with the
    /// selection it had, so undo returns to where the change was made.
    undo: Vec<(Vec<Row>, usize)>,
    redo: Vec<(Vec<Row>, usize)>,
}

/// What `Workspace::record` compares against.
pub struct Mark {
    rows: Vec<Row>,
    sel: usize,
    counts: (usize, usize, u32),
}

impl Workspace {
    /// Open the trace file at `path`, with nothing shown yet.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let name = path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
        let mut app = Workspace::new(Trace::open(path)?, name);
        app.trace_path = path.to_path_buf();
        Ok(app)
    }

    pub fn new(trace: Trace, file_name: String) -> Self {
        let view = View::new(trace.end(), 80);
        Workspace {
            trace,
            file_name,
            trace_path: PathBuf::new(),
            session_path: PathBuf::new(),
            saved: String::new(),
            reloads: 0,
            view,
            marks: [None; 9],
            back: None,
            unit: None,
            clock_unit: None,
            scope: None,
            rows: Vec::new(),
            signals: Vec::new(),
            value_names: Vec::new(),
            protocols: Vec::new(),
            decodes: Vec::new(),
            streams: Vec::new(),
            roles: HashMap::new(),
            sel: 0,
            bit: None,
            anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// The state an undo step would restore, taken before a change (`record`).
    pub fn mark(&self) -> Mark {
        Mark { rows: self.rows.clone(), sel: self.sel, counts: (self.undo.len(), self.redo.len(), self.reloads) }
    }

    /// Record one undo step if the rows changed since `mark` (unless that was an undo, redo or
    /// reload itself).
    pub fn record(&mut self, m: Mark) {
        let undid = (self.undo.len(), self.redo.len(), self.reloads) != m.counts;
        if !undid && !rows::same_layout(&m.rows, &self.rows) {
            self.undo.push((m.rows, m.sel));
            self.redo.clear();
        }
    }

    /// Run `f` as one undo step.
    pub fn undoable<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let m = self.mark();
        let r = f(self);
        self.record(m);
        r
    }


    /// Value search (spec §8): a bare value on the selected signal or bit, or an expression over
    /// signals (`valid && ready && data == 0x40`, `@clk ...`). Goes `n` matches forward (back);
    /// the cursor stays where it is when nothing matches. Returns the query, for counting matches.
    pub fn search(&mut self, text: &str, forward: bool, n: u64) -> Result<search::Query, String> {
        let text = text.trim();
        let selected = match self.rows.get(self.sel) {
            Some(Row::Signal(s)) => Some(self.probe(s, self.bit)),
            _ => None,
        };
        // the names the file gives the selected signal's values
        let names = selected.filter(|p| p.1.is_none()).and_then(|p| self.trace.vars[p.0].names.clone());
        // a number, x, z or a name of the selected signal's values is a value for it; another name
        // or an operator, an expression
        let value = !search::is_expr(text)
            && (text.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '\'')
                || matches!(text.to_lowercase().as_str(), "x" | "u" | "z")
                || names.as_ref().is_some_and(|n| n.iter().any(|(_, name)| name.eq_ignore_ascii_case(text))));
        let query = if value {
            let (var, bit) = match (selected, self.rows.get(self.sel)) {
                (Some(p), _) => p,
                (None, Some(_)) => return Err("select a signal to search".into()),
                (None, None) => return Err("no rows".into()),
            };
            let width = self.trace.probe_width((var, bit));
            let target = Some(search::parse(text, width, names.as_ref())?);
            search::Query { clock: None, expr: search::Expr::Cmp { sig: (var, bit), past: 0, op: search::Op::Eq, target } }
        } else {
            self.query(text)?
        };
        let mut t = self.view.cursor;
        for _ in 0..n {
            match search::find_query(&self.trace, &query, t, forward) {
                Some(x) => t = x,
                None if t == self.view.cursor => {
                    let at = time::format(t, self.trace.fs_per_tick);
                    return Err(format!("not found {} {at}: {text}", if forward { "after" } else { "before" }));
                }
                None => break,
            }
        }
        self.jump(t, false);
        Ok(query)
    }

    /// A search expression (`@clk valid && ready`), its signals named as `resolve` names them, with
    /// their values loaded.
    pub fn query(&mut self, text: &str) -> Result<search::Query, String> {
        let q = search::parse_query(text, self.trace.fs_per_tick, &mut |name| {
            let p = self.resolve(name)?;
            let var = &self.trace.vars[p.0];
            Ok(if p.1.is_some() { (p, Some(1), None, false) } else { (p, var.width, var.names.clone(), var.signed) })
        })?;
        let mut sigs: Vec<usize> = q.clock.iter().map(|c| c.0).collect();
        let mut probes = Vec::new();
        search::probes(&q.expr, &mut probes);
        sigs.extend(probes.iter().map(|p| p.0.0));
        self.trace.load(&sigs);
        if let Some((v, b)) = q.clock {
            // its shortest phase, between its 2nd, 3rd and 4th changes (the 1st may leave X)
            let mut t = self.trace.start();
            let edges: Vec<Tick> = std::iter::from_fn(|| self.trace.next_change_of(v, b, t).inspect(|&x| t = x)).take(4).collect();
            let phase = edges.windows(2).skip(1).map(|w| w[1] - w[0]).min();
            // the largest unit in which it is 1 or more: ns for a 2.5ns phase
            if let (Some(p), Some(_)) = (phase, self.trace.fs_per_tick) {
                let u = time::unit_for([p], self.trace.fs_per_tick);
                self.clock_unit = Some(self.clock_unit.map_or(u, |c| c.max(u)));
            }
        }
        Ok(q)
    }

    /// `:scope name`: names resolve under it first; empty: no scope.
    pub fn set_scope(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim().trim_end_matches('.');
        if !name.is_empty() && !self.trace.vars.iter().any(|v| v.path.starts_with(&format!("{name}."))) {
            return Err(format!("no scope {name}"));
        }
        self.scope = (!name.is_empty()).then(|| name.to_string());
        Ok(())
    }

    /// The time unit of the agent's answers, if the workspace has one (`unit`).
    pub fn time_unit(&self) -> Option<usize> {
        self.unit.or(self.clock_unit)
    }

    /// A signal named in a search: as in the name column, by full path, or by the unique end of a
    /// path (`s_axi_wvalid`). `name[3]` is one bit of a bus; a slice `name[7:4]` or a
    /// concatenation `{a, b}` is a computed var, made the first time it is named. A name that
    /// matches nothing as written is matched in either HDL's form (spec §8): `HEADERS_VLD`,
    /// `gen(4).x` for `gen[4].x`, `data(7 downto 4)`.
    pub fn resolve(&mut self, name: &str) -> Result<search::Probe, String> {
        let name = name.trim();
        if let Some(sig) = self.roles.get(name).cloned() {
            // the signal is named as usual, so a role can be played by a signal of the same name
            let roles = std::mem::take(&mut self.roles);
            let p = self.resolve(&sig);
            self.roles = roles;
            return p;
        }
        if let Some(&v) = self.scope.as_ref().and_then(|s| self.trace.by_path.get(&format!("{s}.{name}"))) {
            return Ok((v, None));
        }
        let names = crate::rows::display_names(&self.trace, &self.rows);
        if let Some(s) = self.rows.iter().zip(&names).find_map(|(r, n)| r.sig().filter(|_| n.trim() == name)) {
            return Ok((s.var, None));
        }
        if let Some(&v) = self.trace.by_path.get(name) {
            return Ok((v, None));
        }
        // an expression before VHDL's parentheses become brackets: `(a + b)` is no index
        if let Some(form @ Form::Expr(_)) = computed::form(name) {
            return self.compute(form).map(|v| (v, None));
        }
        let name = &crate::trace::brackets(name);
        if let Some(form) = computed::form(name) {
            return self.compute(form).map(|v| (v, None));
        }
        // a bit of a bus; a base that is no signal may be an array (`axi_tdata[6]`), named below,
        // and an array's index is its element (`mem[2]`), a struct's a bit (`pkt[10]`)
        if let Some((base, rest)) = name.rsplit_once('[')
            && !base.is_empty()
            && let Some(b) = rest.strip_suffix(']').and_then(|n| n.parse::<u32>().ok())
            && let Ok((v, None)) = self.resolve(base)
        {
            if self.trace.is_composite(v) {
                let array = self.trace.is_array(v);
                match self.by_end(name) {
                    Ok(e) => return Ok((e, None)),
                    Err(e) if array => return Err(e),
                    Err(_) => {}
                }
            }
            let w = self.trace.vars[v].width.unwrap_or(1);
            return if b < w { Ok((v, Some(b))) } else { Err(format!("{base} has no bit {b}")) };
        }
        self.by_end(name).map(|v| (v, None))
    }

    /// `unknown signal`, naming the signals in it when `name` is a scope: a module has no value of
    /// its own (spec §18), its signals do.
    fn not_a_signal(&self, name: &str) -> String {
        let key = crate::trace::parts(&crate::trace::canon(name)).into_iter().map(str::to_string).collect::<Vec<_>>();
        let inside: Vec<&str> = self
            .trace
            .vars
            .iter()
            .map(|v| v.path.as_str())
            .filter(|p| {
                let c = crate::trace::canon(p);
                let parts = crate::trace::parts(&c);
                parts.len() > key.len() && parts.windows(key.len()).take(parts.len() - key.len()).any(|w| w == key)
            })
            .collect();
        match inside.len() {
            0 => format!("unknown signal: {name}"),
            n => format!(
                "unknown signal: {name} is a scope of {n} signals ({}{}); name one, or `add` the scope to show them all",
                inside.iter().take(4).copied().collect::<Vec<_>>().join(", "),
                if n > 4 { ", ..." } else { "" }
            ),
        }
    }

    /// The signal a path ends with, as written or else in either HDL's form; of several, the one
    /// in the rows, in the scope, of the file, or the shallowest.
    fn by_end(&self, name: &str) -> Result<usize, String> {
        let vars = 0..self.trace.vars.len();
        let end = format!(".{name}");
        let mut found: Vec<usize> = vars.clone().filter(|&v| self.trace.vars[v].path.ends_with(&end)).collect();
        if found.is_empty() {
            // shortcut: every path in canonical form on each such lookup, O(signals); keep the
            // forms in the trace if names that match only this way get slow on huge traces
            let canon: Vec<String> = self.trace.vars.iter().map(|v| crate::trace::canon(&v.path)).collect();
            let key = crate::trace::canon(name);
            let scoped = self.scope.as_ref().map(|s| crate::trace::canon(&format!("{s}.{name}")));
            found = vars.clone().filter(|&v| canon[v] == key || scoped.as_ref() == Some(&canon[v])).collect();
            if found.is_empty() {
                let end = format!(".{key}");
                found = vars.filter(|&v| canon[v].ends_with(&end)).collect();
            }
        }
        match found[..] {
            [v] => Ok(v),
            [] => Err(self.not_a_signal(name)),
            _ => {
                // the only one of them in the rows is the one meant; else the only one of the file
                // (decoding `tdata` makes `rx.beat.tdata`, which mustn't make `tdata` ambiguous)
                if let [v] = found.iter().copied().filter(|&v| self.shown(v)).collect::<Vec<_>>()[..] {
                    return Ok(v);
                }
                if let Some(s) = &self.scope
                    && let [v] = found.iter().copied().filter(|&v| self.trace.vars[v].path.starts_with(&format!("{s}."))).collect::<Vec<_>>()[..]
                {
                    return Ok(v);
                }
                let file: Vec<usize> = found.iter().copied().filter(|&v| !self.trace.is_computed(v)).collect();
                if let [v] = file[..] {
                    return Ok(v);
                }
                // a port passed down the hierarchy (`clk` in every instance): the top one
                let depth = |v: usize| self.trace.vars[v].path.matches('.').count();
                let pool = if file.is_empty() { &found } else { &file };
                let top = pool.iter().map(|&v| depth(v)).min();
                if let [v] = pool.iter().copied().filter(|&v| Some(depth(v)) == top).collect::<Vec<_>>()[..] {
                    return Ok(v);
                }
                let paths: Vec<&str> = found.iter().take(8).map(|&v| self.trace.vars[v].path.as_str()).collect();
                let more = if found.len() > 8 { format!(", and {} more", found.len() - 8) } else { String::new() };
                Err(format!("ambiguous: {name} ({} signals: {}{more})", found.len(), paths.join(", ")))
            }
        }
    }


    /// A signal named in full: a bit of a bus is a one-bit slice of it.
    fn whole(&mut self, name: &str) -> Result<usize, String> {
        match self.resolve(name)? {
            (v, None) => Ok(v),
            (v, Some(b)) => self.compute(Form::Slice(&self.trace.vars[v].path.clone(), b, b)),
        }
    }

    /// A part of a concatenation: a signal, or a sized SV number (`1'b0`) as a constant var.
    fn part(&mut self, name: &str) -> Result<usize, String> {
        match search::sv_literal(name) {
            Some(Err(e)) => Err(e),
            Some(Ok(search::Sv::Bits { bits, sized: true, .. })) => match self.trace.by_path.get(name) {
                Some(&v) => Ok(v),
                None => self.trace.add_computed(name, Some(bits.len() as u32), vec![(0, Val::Bits(bits))]),
            },
            Some(Ok(_)) => Err(format!("{name}: a number in a concatenation needs a width, as 1'b0")),
            None if name.starts_with(|c: char| c.is_ascii_digit()) => Err(format!("{name}: a number in a concatenation needs a width, as 1'b0")),
            None => self.whole(name),
        }
    }

    /// `:signal name = expr` (spec §16): a computed signal under a name of its own,
    /// with the values of any signal name (`cmd[7:0]`, `{a, b}`). Defining it again replaces it.
    pub fn define(&mut self, name: &str, expr: &str) -> Result<usize, String> {
        let (name, expr) = (name.trim(), expr.trim());
        // names go into search expressions, so no brackets, braces, spaces or operators
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || "_.$".contains(c)) {
            return Err(format!("not a signal name: {name}"));
        }
        let v = self.whole(expr)?;
        let values = computed::values(&mut self.trace, v);
        let var = self.trace.add_computed(name, self.trace.vars[v].width, values)?;
        self.trace.vars[var].signed = self.trace.vars[v].signed;
        match self.signals.iter_mut().find(|s| s.0 == name) {
            Some(s) => s.1 = expr.to_string(),
            None => self.signals.push((name.to_string(), expr.to_string())),
        }
        Ok(var)
    }

    /// `:names <signal> <value>=<name>...`: names for a signal's values, shown by the `enum` radix
    /// and searched by name as an FST enum table's are (spec §10); `off` removes them. Rows showing
    /// the signal switch to `enum` (back to the default with `off`).
    pub fn name_values(&mut self, sig: &str, spec: &str) -> Result<(), String> {
        let usage = "usage: :names <signal> <value>=<name>..., e.g. :names state 0=IDLE 1=BUSY, or :names <signal> off";
        let (var, bit) = self.resolve(sig)?;
        let width = match (bit, self.trace.vars[var].width) {
            (None, Some(w)) => w,
            _ => return Err(format!("{sig}: names are for a whole bus or bit signal, not a bit or a string")),
        };
        let path = self.trace.vars[var].path.clone();
        let names = match spec.trim() {
            "" => return Err(usage.into()),
            "off" => None,
            spec => {
                let mut names = Vec::new();
                for pair in spec.split_whitespace() {
                    let (v, n) = pair.split_once('=').filter(|(v, n)| !v.is_empty() && !n.is_empty()).ok_or(usage)?;
                    let search::Target::Bits(bits) = search::parse(v, Some(width), None)? else { return Err(format!("{v}: a name is for a value without x or z bits")) };
                    names.push((crate::trace::names_key(&bits).to_string(), n.to_string()));
                }
                Some(names.into())
            }
        };
        let named = names.is_some();
        self.trace.vars[var].names = names;
        self.value_names.retain(|(p, _)| *p != path);
        if named {
            self.value_names.push((path, spec.split_whitespace().collect::<Vec<_>>().join(" ")));
        }
        let signed = self.trace.vars[var].signed;
        for r in &mut self.rows {
            if let Row::Signal(s) = r
                && s.var == var
                && (named || s.radix == radix::ENUM)
            {
                (s.radix, s.frac) = (radix::default_for(named, signed), 0);
            }
        }
        Ok(())
    }

    /// A var by full path, made again if it is a computed one (sessions, `:e`).
    pub fn var_named(&mut self, path: &str) -> Option<usize> {
        // sessions saved before paths had one form name nvc's `arr.[1]` and `gen(1)`
        match self.trace.by_path.get(path).or_else(|| self.trace.by_path.get(&crate::trace::brackets(path))) {
            Some(&v) => Some(v),
            None => self.compute(computed::form(path)?).ok(),
        }
    }

    /// The computed var of a slice or concatenation, named by the full paths of its parts
    /// (`top.x[7:4]`, `{top.a,top.b}`), so naming it again in any form finds the same var.
    fn compute(&mut self, form: Form) -> Result<usize, String> {
        if let Form::Expr(text) = form {
            return self.compute_expr(text);
        }
        // a part as a whole var; a bit of a bus is a one-bit slice
        let (path, parts) = match &form {
            Form::Expr(_) => unreachable!(),
            Form::Slice(base, hi, lo) => {
                let v = self.whole(base)?;
                (format!("{}[{hi}:{lo}]", self.trace.vars[v].path), vec![v])
            }
            Form::Concat(names) => {
                let vs = names.iter().map(|n| self.part(n)).collect::<Result<Vec<_>, _>>()?;
                let paths: Vec<&str> = vs.iter().map(|&v| self.trace.vars[v].path.as_str()).collect();
                (format!("{{{}}}", paths.join(",")), vs)
            }
        };
        if let Some(&v) = self.trace.by_path.get(&path) {
            return Ok(v);
        }
        let (width, values) = match form {
            Form::Slice(_, hi, lo) => (hi - lo + 1, computed::slice(&mut self.trace, parts[0], hi, lo)?),
            Form::Concat(_) => computed::concat(&mut self.trace, &parts)?,
            Form::Expr(_) => unreachable!(),
        };
        self.trace.add_computed(&path, Some(width), values)
    }

    /// The computed var of an expression (`pc + 4`), named by it with its signals' full paths and
    /// no spaces (`top.pc+4`), so naming it again finds the same var. It is as wide as its value
    /// can need, and signed if that can be negative (spec §16).
    fn compute_expr(&mut self, text: &str) -> Result<usize, String> {
        let (term, names) = search::parse_term(text, self.trace.fs_per_tick, &mut |name| {
            let p = self.resolve(name)?;
            let var = &self.trace.vars[p.0];
            Ok(if p.1.is_some() { (p, Some(1), None, false) } else { (p, var.width, var.names.clone(), var.signed) })
        })?;
        let mut path = String::new();
        let mut end = 0;
        for (range, (v, bit)) in names {
            path.push_str(&text[end..range.start]);
            path.push_str(&self.trace.vars[v].path);
            path.extend(bit.map(|b| format!("[{b}]")));
            end = range.end;
        }
        path.push_str(&text[end..]);
        let path: String = path.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(&v) = self.trace.by_path.get(&path) {
            return Ok(v);
        }
        let shape = term.shape();
        let values = computed::term(&mut self.trace, &term);
        let v = self.trace.add_computed(&path, Some(shape.bits), values)?;
        self.trace.vars[v].signed = shape.signed;
        Ok(v)
    }

    /// Show a session the agent prepared (`present`, spec §15): its rows and cursors
    /// replace the window's, as one undo step; the session file stays as it was (`[+]`). Returns
    /// the status message, `agent: presented …` (an error if parts didn't fit).
    pub fn present(&mut self, text: &str) -> Result<Msg, String> {
        // check it reads before anything changes
        text.parse::<toml::Table>().map_err(|e| format!("session: {}", e.message()))?;
        let before = (self.rows.clone(), self.sel);
        let saved = std::mem::take(&mut self.saved);
        (self.marks, self.view.cursor, self.back) = ([None; 9], 0, None);
        let warn = session::apply(self, text);
        self.saved = saved;
        let warn = warn?;
        if !rows::same_layout(&before.0, &self.rows) {
            self.undo.push(before);
            self.redo.clear();
        }
        self.fix_sel();
        let signals = self.rows.iter().filter(|r| r.sig().is_some()).count();
        let msg = format!("agent: presented {signals} signals, {} cursors", self.cursor_list().len());
        Ok(if warn.is_empty() { (msg, false) } else { (format!("{msg}; {}", warn.join("; ")), true) })
    }


    /// The layout differs from the session file: `[+]`, and `:q` refuses.
    pub fn modified(&self) -> bool {
        session::rows_toml(self) != self.saved
    }


    /// Make each again, in passes while some are made, so one that uses one made after it is made
    /// too. Those left, with their errors.
    fn again<T: Clone>(&mut self, items: Vec<T>, mut make: impl FnMut(&mut Self, T) -> Result<(), String>) -> Vec<(T, String)> {
        let mut left: Vec<(T, String)> = items.into_iter().map(|t| (t, String::new())).collect();
        loop {
            let n = left.len();
            left = left.into_iter().filter_map(|(t, _)| make(self, t.clone()).err().map(|e| (t, e))).collect();
            if left.is_empty() || left.len() == n {
                return left;
            }
        }
    }

    /// `:e`: reopen the trace, keeping the layout and the view, and make computed signals again
    /// from the new values. Signals no longer in the file (or that can't be made) are dropped and
    /// listed. If the file can't be read, nothing changes.
    pub fn reload(&mut self) -> Result<Msg, String> {
        let old = std::mem::replace(&mut self.trace, Trace::open(&self.trace_path)?);
        // no rows while computed ones are made again: theirs name vars of the old trace
        let old_rows = std::mem::take(&mut self.rows);
        let mut missing = Vec::new();
        for ((name, _), e) in self.again(self.signals.clone(), |ws, (n, e)| ws.define(&n, &e).map(drop)) {
            missing.push(format!("{name} ({e})"));
        }
        for (path, spec) in self.value_names.clone() {
            if let Err(e) = self.name_values(&path, &spec) {
                missing.push(format!("names of {path} ({e})"));
            }
        }
        self.streams.clear();
        // taken first: a protocol added would make its instances again
        let decodes = std::mem::take(&mut self.decodes);
        // one that fails is kept, to try again on the next `:e`
        let protocols = std::mem::take(&mut self.protocols);
        for (p, e) in self.again(protocols, |ws, p| ws.add_protocol(p).map(drop)) {
            missing.push(format!("{} ({e})", p.name));
            self.protocols.push(p);
        }
        for (d, e) in self.again(decodes, |ws, d| ws.add_decode(d).map(drop)) {
            missing.push(format!("{} ({e})", d.name));
            self.decodes.push(d);
        }
        let mut rows: Vec<Row> = Vec::new();
        for r in old_rows {
            match r {
                Row::Signal(s) => {
                    let path = &old.vars[s.var].path;
                    match self.var_named(path) {
                        Some(var) => {
                            // the open levels by path too: indices may differ in the new trace
                            let open = s.open.iter().filter_map(|&o| self.var_named(&old.vars[o].path)).collect();
                            rows.push(Row::Signal(Sig { var, open, ..s }))
                        }
                        None => missing.push(path.clone()),
                    }
                }
                g => rows.push(g),
            }
        }
        rows::tidy(&mut rows);
        let vars: Vec<usize> = rows.iter().filter_map(Row::sig).map(|s| s.var).collect();
        self.trace.load(&vars);
        self.rows = rows;
        let end = self.trace.end();
        self.view.end = end;
        self.view.cursor = self.view.cursor.min(end);
        self.marks.iter_mut().flatten().for_each(|c| *c = (*c).min(end));
        self.view.start = self.view.start.min(end);
        self.undo.clear();
        self.redo.clear();
        self.reloads += 1;
        self.fix_sel();
        Ok(match missing.is_empty() {
            true => ("reloaded".into(), false),
            false => (format!("reloaded; not in trace: {}", missing.join(", ")), true),
        })
    }


    pub fn undo(&mut self) -> Result<(), String> {
        let (rows, sel) = self.undo.pop().ok_or("already at oldest change")?;
        self.redo.push((std::mem::replace(&mut self.rows, rows), self.sel));
        self.sel = sel;
        self.fix_sel();
        Ok(())
    }


    pub fn redo(&mut self) -> Result<(), String> {
        let (rows, sel) = self.redo.pop().ok_or("already at newest change")?;
        self.undo.push((std::mem::replace(&mut self.rows, rows), self.sel));
        self.sel = sel;
        self.fix_sel();
        Ok(())
    }


    /// The stream a var shows, if it is one's own var.
    pub fn stream_of(&self, var: usize) -> Option<&Stream> {
        self.streams.iter().find(|s| s.name == self.trace.vars[var].path)
    }

    /// The record of a stream's own var at the cursor, if there is one there.
    pub fn record_at(&self, var: usize) -> Option<(&Stream, usize)> {
        let s = self.stream_of(var)?;
        let Some(crate::trace::Val::Text(t)) = self.trace.value_at(var, self.view.cursor) else { return None };
        let i = t.strip_prefix('#')?.parse().ok().filter(|&i| i < s.records.len())?;
        Some((s, i))
    }

    /// What opening a var shows under it: a composite its fields or elements, a stream its field
    /// vars, a bus its bits (MSB first); nothing for a 1-bit or text signal.
    pub fn children(&self, var: usize) -> Vec<(usize, Option<u32>)> {
        if let Some(s) = self.stream_of(var) {
            return s.schema.iter().filter_map(|f| self.trace.by_path.get(&format!("{}.{}", s.name, f.name)).map(|&v| (v, None))).collect();
        }
        let v = &self.trace.vars[var];
        if !v.children.is_empty() {
            return v.children.iter().map(|&c| (c, None)).collect();
        }
        (0..v.width.filter(|&w| w > 1).unwrap_or(0)).rev().map(|b| (var, Some(b))).collect()
    }

    /// The lines under a row, top to bottom: the children of each open var, nested (spec §6).
    pub fn subs(&self, s: &Sig) -> Vec<Sub> {
        fn walk(ws: &Workspace, open: &[usize], var: usize, depth: usize, out: &mut Vec<Sub>) {
            if !open.contains(&var) {
                return;
            }
            for (v, bit) in ws.children(var) {
                out.push(Sub { var: v, bit, depth });
                if bit.is_none() {
                    walk(ws, open, v, depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self, &s.open, s.var, 0, &mut out);
        out
    }

    /// Screen lines: each visible row, followed by the lines open under it, each as its row and
    /// how many of the row's lines are below it (`sub`, which `probe` reads).
    pub fn lines(&self) -> Vec<(usize, Option<u32>)> {
        let mut out = Vec::new();
        for i in rows::visible(&self.rows) {
            out.push((i, None));
            if let Row::Signal(s) = &self.rows[i] {
                let n = self.subs(s).len() as u32;
                out.extend((0..n).rev().map(|b| (i, Some(b))));
            }
        }
        out
    }

    /// What line `sub` under a row shows (`lines`): the var, or the bit of a bus.
    pub fn probe(&self, s: &Sig, sub: Option<u32>) -> (usize, Option<u32>) {
        let Some(b) = sub else { return (s.var, None) };
        let subs = self.subs(s);
        match subs.len().checked_sub(b as usize + 1) {
            Some(k) => (subs[k].var, subs[k].bit),
            None => (s.var, None),
        }
    }

    /// The selected signal (or bit), or all signals of the selected group.
    pub fn sel_vars(&self) -> Vec<(usize, Option<u32>)> {
        match self.rows.get(self.sel) {
            Some(Row::Signal(s)) => vec![self.probe(s, self.bit)],
            Some(Row::Group { .. }) => rows::members(&self.rows, self.sel).into_iter().map(|v| (v, None)).collect(),
            None => Vec::new(),
        }
    }


    /// Keep the selection on a row that exists and is visible.
    pub fn fix_sel(&mut self) {
        self.anchor = None;
        self.bit = None;
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
        let vis = rows::visible(&self.rows);
        if !self.rows.is_empty() && !vis.contains(&self.sel) {
            self.sel = rows::header_of(&self.rows, self.sel).unwrap_or(0);
        }
    }


    /// Space: collapse / expand a group, or open a bus into its bits, a stream into its fields, a
    /// composite one level / fold it with every level under it. On a line that opens nothing (a
    /// bit, a 1-bit field) it folds the level the line is in. The selection stays on the line
    /// opened or folded.
    pub fn toggle_group(&mut self) {
        let s = match self.rows.get_mut(self.sel) {
            Some(Row::Group { collapsed, .. }) => {
                *collapsed = !*collapsed;
                self.bit = None;
                return;
            }
            Some(Row::Signal(s)) => s.clone(),
            None => return,
        };
        let subs = self.subs(&s);
        let line = self.bit.and_then(|b| subs.len().checked_sub(b as usize + 1));
        // the line's var if it opens, else the level it is in
        let mut target = s.var;
        if let Some(k) = line {
            let l = &subs[k];
            if l.bit.is_none() && !self.children(l.var).is_empty() {
                target = l.var;
            } else if let Some(p) = subs[..k].iter().rposition(|p| p.depth + 1 == l.depth) {
                target = subs[p].var;
            }
        }
        let mut open = s.open.clone();
        if open.contains(&target) {
            open.retain(|&v| v != target);
        } else if !self.children(target).is_empty() {
            open.push(target);
        }
        // levels under a folded one are folded with it
        let mut row = Sig { open, ..s };
        let reach: Vec<usize> = std::iter::once(row.var).chain(self.subs(&row).iter().filter(|l| l.bit.is_none()).map(|l| l.var)).collect();
        row.open.retain(|v| reach.contains(v));
        let subs = self.subs(&row);
        self.bit = (target != row.var).then(|| subs.iter().position(|l| l.var == target && l.bit.is_none()).map(|k| (subs.len() - 1 - k) as u32)).flatten();
        self.rows[self.sel] = Row::Signal(row);
    }


    /// The `V` selection, or the selected row; a header brings its whole group.
    pub fn selection(&self) -> std::ops::Range<usize> {
        rows::range(&self.rows, self.anchor.unwrap_or(self.sel), self.sel)
    }


    /// Remove the selected rows (`dd`, `:rm`).
    pub fn delete(&mut self) -> Result<(), String> {
        if self.rows.is_empty() {
            return Err("no rows".into());
        }
        let r = self.selection();
        self.sel = r.start;
        self.rows.drain(r);
        self.anchor = None;
        rows::tidy(&mut self.rows);
        self.fix_sel();
        Ok(())
    }


    pub fn group(&mut self, name: &str) -> Result<(), String> {
        if self.rows.is_empty() {
            return Err("no rows".into());
        }
        let r = self.selection();
        rows::group(&mut self.rows, r.clone(), name)?;
        self.anchor = None;
        self.bit = None;
        self.sel = r.start;
        Ok(())
    }


    /// The cursor and the marks as (index, time), by number (index 0 is cursor 1).
    pub fn cursor_list(&self) -> Vec<(usize, Tick)> {
        std::iter::once((0, self.view.cursor)).chain((1..9).filter_map(|i| self.marks[i].map(|t| (i, t)))).collect()
    }

    /// `m` / `Nm`: mark `n` (an index, 1 - 8), or the lowest free one, at the cursor.
    pub fn set_mark(&mut self, n: Option<usize>) -> Result<(), String> {
        let i = match n {
            Some(0) => return Err("cursor 1 is the cursor; marks are 2-9".into()),
            Some(i) if i < 9 => i,
            Some(i) => return Err(format!("no cursor {}: marks are 2-9", i + 1)),
            None => (1..9).find(|&i| self.marks[i].is_none()).ok_or("all 8 marks in use")?,
        };
        self.marks[i] = Some(self.view.cursor);
        Ok(())
    }

    /// `M` / `NM`: remove mark `n` (an index), or the one under the cursor.
    pub fn remove_mark(&mut self, n: Option<usize>) -> Result<(), String> {
        let i = match n {
            Some(0) => return Err("cursor 1 can't be removed".into()),
            Some(i) if self.marks.get(i).is_some_and(Option::is_some) => i,
            Some(i) => return Err(format!("no cursor {}", i + 1)),
            None => (1..9).find(|&i| self.marks[i] == Some(self.view.cursor)).ok_or("no mark under the cursor")?,
        };
        self.marks[i] = None;
        Ok(())
    }

    /// A jump of cursor 1 to `t` (spec §9), keeping where it was for `''`. A jump to a mark
    /// centres the screen on it; others scroll only if it is off screen.
    pub fn jump(&mut self, t: Tick, center: bool) {
        let from = self.view.cursor;
        if center { self.view.center(t) } else { self.view.jump(t) }
        if self.view.cursor != from {
            self.back = Some(from);
        }
    }

    /// `''`: cursor 1 back to where it was before the last jump (and `''` again returns).
    pub fn jump_back(&mut self) -> Result<(), String> {
        self.jump(self.back.ok_or("no jump to go back from")?, true);
        Ok(())
    }

    /// `'N`: jump to mark `n` (an index); `'1`, the cursor, centers the screen on it.
    pub fn goto_mark(&mut self, n: usize) -> Result<(), String> {
        match self.marks.get(n).copied().flatten() {
            Some(t) => self.jump(t, true),
            None if n == 0 => self.view.center(self.view.cursor),
            None => return Err(format!("no cursor {}", n + 1)),
        }
        Ok(())
    }

    /// `Tab` / `Shift-Tab`: jump to the next / previous mark in time, wrapping around.
    pub fn next_mark(&mut self, forward: bool) -> Result<(), String> {
        let mut times: Vec<Tick> = self.marks.iter().flatten().copied().collect();
        times.sort();
        let now = self.view.cursor;
        let t = match forward {
            true => times.iter().find(|&&t| t > now).or(times.first()),
            false => times.iter().rev().find(|&&t| t < now).or(times.last()),
        };
        self.jump(*t.ok_or("no marks: m puts one at the cursor")?, true);
        Ok(())
    }

    /// Set the radix of the selected signal, or of every signal in the selected group that it
    /// fits (`radix::parse_name`'s radix, fraction bits and needed width). None cycles to the next
    /// radix that fits (spec §10).
    pub fn set_radix(&mut self, r: Option<(usize, u32, Option<u32>)>) -> Result<(), String> {
        let end = match self.rows.get(self.sel) {
            None => return Err("no rows".into()),
            Some(Row::Group { .. }) => rows::group_end(&self.rows, self.sel),
            Some(Row::Signal(_)) => self.sel + 1,
        };
        let vars = &self.trace.vars;
        let width = |s: &Sig| vars[s.var].width.unwrap_or(0);
        let named = |s: &Sig| vars[s.var].names.is_some();
        let mut sigs: Vec<&mut Sig> = self.rows[self.sel..end]
            .iter_mut()
            .filter_map(|r| match r {
                Row::Signal(s) if vars[s.var].width != Some(1) => Some(s),
                _ => None,
            })
            .collect();
        let Some(first) = sigs.first() else { return Err("1-bit signals have no radix".into()) };
        let (r, frac, need) = r.unwrap_or((radix::next(first.radix, width(first), named(first)), 0, None));
        let fits = |s: &Sig| radix::fits(r, need, width(s), named(s));
        if !sigs.iter().any(|s| fits(s)) {
            if r == radix::ENUM {
                return Err("enum: the file names no values of this signal".into());
            }
            let w = width(first);
            return Err(format!("{} doesn't fit a {w}-bit signal", radix::name(r, frac, need.unwrap_or(w))));
        }
        for s in sigs.iter_mut().filter(|s| fits(s)) {
            (s.radix, s.frac) = (r, frac);
        }
        Ok(())
    }

    /// Set the colour of the selected signal, or of every signal in the selected group; None
    /// gives them the theme's (spec §14 "Colours").
    pub fn set_color(&mut self, color: Option<usize>) -> Result<(), String> {
        let end = match self.rows.get(self.sel) {
            None => return Err("no rows".into()),
            Some(Row::Group { .. }) => rows::group_end(&self.rows, self.sel),
            Some(Row::Signal(_)) => self.sel + 1,
        };
        for r in &mut self.rows[self.sel..end] {
            if let Row::Signal(s) = r {
                s.color = color;
            }
        }
        Ok(())
    }

    pub fn shown(&self, var: usize) -> bool {
        self.rows.iter().any(|r| r.sig().is_some_and(|s| s.var == var))
    }

    /// A row for a var in its default radix. Its values must be loaded (`Trace::load`).
    pub fn sig(&self, var: usize, grouped: bool) -> Row {
        Row::Signal(Sig { grouped, ..Sig::of(var, &self.trace.vars[var]) })
    }

    /// New rows go below the selected row and join its group, if it is in one; with a group's
    /// header selected, after the group.
    /// The last one added becomes selected.
    pub fn add_rows(&mut self, vars: &[usize]) {
        if vars.is_empty() {
            return;
        }
        self.trace.load(vars);
        // a selected group header: after the whole group, outside it
        let header = self.rows.get(self.sel).is_some_and(Row::is_group);
        let at = match self.rows.is_empty() {
            true => 0,
            false if header => rows::group_end(&self.rows, self.sel),
            false => self.sel + 1,
        };
        let grouped = !self.rows.is_empty() && !header && rows::header_of(&self.rows, self.sel).is_some();
        let new: Vec<Row> = vars.iter().map(|&v| self.sig(v, grouped)).collect();
        self.rows.splice(at..at, new);
        self.sel = at + vars.len() - 1;
        self.fix_sel();
    }

    /// A new group goes below the selected row, or below the whole group the selected row is in.
    pub fn add_group(&mut self, name: &str, vars: &[usize]) {
        let at = match self.rows.is_empty() {
            true => 0,
            false => rows::header_of(&self.rows, self.sel).map_or(self.sel + 1, |h| rows::group_end(&self.rows, h)),
        };
        self.trace.load(vars);
        let mut new = vec![Row::Group { name: name.into(), collapsed: false }];
        new.extend(vars.iter().map(|&v| self.sig(v, true)));
        self.rows.splice(at..at, new);
        self.sel = at + vars.len();
        self.bit = None;
    }
}
