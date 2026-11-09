//! Decoding in a workspace: protocols and decode instances made into streams, whose records and
//! fields become vars (spec §16).

use super::reason::{Reason, Reasons, Why};
use super::*;

impl Workspace {
    /// A stream sampled at `clock` where `when` holds, with fields (name, signal) named as in a
    /// search (`tdata[63:32]`, `{a, b}`). A field names its value in the fields after it and in
    /// `when` (`f3 = insn[14:12]`, then `ok = f3 == 0`), as an instance's roles do.
    pub fn sample(&mut self, name: &str, clock: &str, when: &str, fields: &[(&str, &str)]) -> Result<Stream, String> {
        let roles = self.roles.clone();
        let made = self.sample_fields(name, clock, when, fields);
        self.roles = roles;
        let (q, probes) = made?;
        self.trace.load(&probes.iter().map(|p| p.1.0).collect::<Vec<_>>());
        stream::sample(&self.trace, name, &q, &probes)
    }

    /// `sample`'s fields, each a role for the ones after it, then its condition.
    #[allow(clippy::type_complexity)]
    fn sample_fields(&mut self, name: &str, clock: &str, when: &str, fields: &[(&str, &str)]) -> Result<(search::Query, Vec<(String, search::Probe, Option<u32>)>), String> {
        if clock.is_empty() {
            return Err("needs a clock (`clock`: the signal it samples at)".into());
        }
        let mut probes = Vec::new();
        for &(field, sig) in fields {
            let p = self.resolve(sig).map_err(|e| format!("{name}.{field}: {e}"))?;
            probes.push((field.to_string(), p, self.trace.probe_width(p)));
            self.roles.insert(field.to_string(), self.trace.probe_path(p));
        }
        Ok((self.query(&format!("@{clock} {when}"))?, probes))
    }

    /// Define a protocol, replacing one of that name: names only, its roles, which instances put
    /// on signals (`add_decode`).
    pub fn add_protocol(&mut self, p: Sampler) -> Result<(), String> {
        protocol_names(&p).map_err(|r| r.to_string())?;
        let name = &p.name;
        // a type defined again: its instances sampled again, and those after them, which may stack on them
        let first = self.decodes.iter().position(|d| d.protocol == p.name);
        let name = name.clone();
        match self.protocols.iter_mut().find(|x| x.name == p.name) {
            Some(old) => *old = p,
            None => self.protocols.push(p),
        }
        if let Some(i) = first {
            let again = self.decodes[i..].to_vec();
            let failed: Vec<String> = again.into_iter().filter_map(|d| self.add_decode(d).err()).collect();
            if !failed.is_empty() {
                return Err(format!("{name} defined, but instances failed: {}", failed.join("; ")));
            }
        }
        Ok(())
    }

    /// The mistakes in a protocol's names and expressions, reading no value (the protocol form
    /// checks as it is filled in, spec §5 "Protocol form"): each role's signal (`signals`, role
    /// and signal), then each field and `when`, which name only the roles and the fields before
    /// them. Each mistake starts with what it is in: a role, a field, or `when`.
    pub fn check_protocol(&mut self, p: &Sampler, signals: &[(String, String)]) -> Result<(), Reasons> {
        protocol_names(p).map_err(|r| Reasons { name: None, list: vec![r] })?;
        let mut errors: Vec<Reason> = Vec::new();
        // a name's width, signedness and value names: its role's signal's, or its field's
        let mut known: HashMap<String, (Option<u32>, Option<crate::trace::Names>, bool)> = HashMap::new();
        for r in &p.roles {
            match signals.iter().find(|s| s.0 == *r).map(|s| self.resolve(&s.1)) {
                Some(Ok(probe)) => {
                    let v = &self.trace.vars[probe.0];
                    known.insert(r.clone(), if probe.1.is_some() { (Some(1), None, false) } else { (v.width, v.names.clone(), v.signed) });
                }
                Some(Err(e)) => errors.push(Reason { on: vec![r.clone()], why: Why::In(e) }),
                None => errors.push(Reason { on: vec![r.clone()], why: Why::In("no signal; :decode puts the protocol on signals, the form checks with one".into()) }),
            }
        }
        if !errors.is_empty() {
            return Err(Reasons { name: None, list: errors });
        }
        let fpt = self.trace.fs_per_tick;
        let only = |known: &HashMap<_, (Option<u32>, Option<crate::trace::Names>, bool)>, name: &str| {
            let names: Vec<&String> = known.keys().collect();
            known.get(name).map(|k| ((0, None), k.0, k.1.clone(), k.2)).ok_or_else(|| format!("{name} is no role nor a field before it: {}", names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")))
        };
        for (f, e) in &p.fields {
            match search::parse_term(e, fpt, &mut |n| only(&known, n)) {
                Ok((t, _)) => _ = known.insert(f.clone(), (Some(t.shape().bits), None, t.shape().signed)),
                Err(err) => errors.push(Reason { on: vec![f.clone()], why: Why::In(err) }),
            }
        }
        if let Err(err) = search::parse_query(&p.when, fpt, &mut |n| only(&known, n)) {
            errors.push(Reason { on: vec!["when".into()], why: Why::In(err) });
        }
        match errors.is_empty() {
            true => Ok(()),
            false => Err(Reasons { name: None, list: errors }),
        }
    }

    /// Put a protocol on signals (or `frame` / `pair` on streams): the streams it makes, named after
    /// the instance (a built-in protocol's `rx.beat`, `rx.frame` ...). Every mistake in it is
    /// reported at once. Adding one again by name replaces it. Returns the first stream's var.
    pub fn add_decode(&mut self, mut d: Decode) -> Result<usize, String> {
        // kept by full path: a short name may only have been unique thanks to the rows shown, and
        // a session makes its instances before its rows
        if !matches!(d.protocol.as_str(), "frame" | "pair") {
            for (_, sig) in &mut d.signals {
                *sig = self.full_path(sig).unwrap_or(sig.clone());
            }
        }
        if !d.clock.is_empty() {
            d.clock = self.full_path(&d.clock).unwrap_or(d.clock);
        }
        // a prefix too, through its first role: `s_axi_` + `awvalid` is `top.s_axi_awvalid`
        if let (Some(b), Some((_, prefix))) = (protocol::builtin(&d.protocol), d.params.iter_mut().find(|p| p.0 == "prefix")) {
            // the first role there is: a read-only port has no awvalid
            let full = b.required.iter().find_map(|role| self.resolve(&format!("{prefix}{role}")).ok().map(|p| (role, self.trace.vars[p.0].path.clone())));
            if let Some(full) = full.and_then(|(role, f)| f.strip_suffix(role).map(String::from)) {
                *prefix = full;
            }
        }
        let var = if let Some(b) = protocol::builtin(&d.protocol) {
            self.decode_builtin(&d, b)
        } else if matches!(d.protocol.as_str(), "frame" | "pair") {
            self.decode_streams(&d)
        } else if d.protocol == "check" {
            self.decode_check(&d)
        } else {
            self.decode_user(&d)
        }
        .map_err(|e| format!("{}: {e}", d.name))?;
        match self.decodes.iter_mut().find(|x| x.name == d.name) {
            Some(old) => *old = d,
            None => self.decodes.push(d),
        }
        Ok(var)
    }

    /// The decodes that read decode `name`'s streams: their signals, clock or rules name one, or a
    /// stream of theirs is made from one.
    fn readers(&self, name: &str) -> Vec<String> {
        let mine = |p: &str| p == name || p.strip_prefix(name).is_some_and(|r| r.starts_with('.'));
        let named = self.decodes.iter().filter(|d| d.name != name).filter(|d| mine(&d.clock) || d.signals.iter().any(|(_, s)| mine(s)) || d.params.iter().any(|(_, v)| v.contains(&format!("{name}."))));
        let made = self.streams.iter().filter(|s| !mine(&s.name) && s.inputs.iter().any(|x| mine(x))).filter_map(|s| self.decodes.iter().find(|d| s.name == d.name || s.name.starts_with(&format!("{}.", d.name))));
        let mut out: Vec<String> = named.chain(made).map(|d| d.name.clone()).collect();
        out.dedup();
        out
    }

    /// The rows showing a stream of decode `name`, or a field of one, that isn't one now.
    fn drop_rows_of(&mut self, name: &str) {
        let mut now = HashSet::new();
        for i in self.streams_of_decode(name) {
            let s = &self.streams[i];
            now.insert(s.name.clone());
            now.extend(s.schema.iter().map(|f| format!("{}.{}", s.name, f.name)));
        }
        let gone = |p: &str| (p == name || p.starts_with(&format!("{name}."))) && !now.contains(p);
        let keep: Vec<bool> = self.rows.iter().map(|r| r.sig().is_none_or(|s| !gone(&self.trace.vars[s.var].path))).collect();
        let mut k = keep.into_iter();
        self.rows.retain(|_| k.next().expect("one per row"));
        rows::tidy(&mut self.rows);
        self.anchor = None;
        self.fix_sel();
    }

    /// Remove decode `name` (`:decode <name> off`, spec §16): the instance, its streams and the
    /// rows showing them. A decode another reads stays, naming the readers to remove first.
    pub fn remove_decode(&mut self, name: &str) -> Result<(), String> {
        let Some(i) = self.decodes.iter().position(|d| d.name == name) else {
            let all: Vec<&str> = self.decodes.iter().map(|d| d.name.as_str()).collect();
            return Err(match all.is_empty() {
                true => format!("no decode {name}: there are none yet"),
                false => format!("no decode {name} (decodes: {})", all.join(", ")),
            });
        };
        let readers = self.readers(name);
        if !readers.is_empty() {
            return Err(format!("{name} is read by {}: remove {} first", readers.join(", "), if readers.len() > 1 { "them" } else { "it" }));
        }
        self.decodes.remove(i);
        let mine: Vec<usize> = self.streams_of_decode(name);
        let mut k = 0;
        self.streams.retain(|_| {
            k += 1;
            !mine.contains(&(k - 1))
        });
        self.drop_rows_of(name);
        Ok(())
    }

    /// Decode `d` again under its name, as edited (the decode form, spec §5): the streams it
    /// makes now replace the old ones, rows showing the old ones it doesn't make go, and the
    /// decodes that read it are made again. If it fails, the old one stays.
    pub fn redecode(&mut self, d: Decode) -> Result<String, String> {
        let name = d.name.clone();
        let Some(at) = self.decodes.iter().position(|x| x.name == name) else { return self.add_decode(d).map(|_| String::new()) };
        let mine = self.streams_of_decode(&name);
        let (mut old, mut kept) = (Vec::new(), Vec::new());
        for (i, s) in std::mem::take(&mut self.streams).into_iter().enumerate() {
            if mine.contains(&i) { old.push(s) } else { kept.push(s) }
        }
        self.streams = kept;
        let was = self.decodes.remove(at);
        if let Err(e) = self.add_decode(d) {
            self.streams.retain(|s| !(s.name == name || s.name.starts_with(&format!("{name}."))));
            self.streams.extend(old);
            self.decodes.insert(at, was);
            return Err(e);
        }
        // made last by add_decode: back in its place, before the decodes that read it
        let d = self.decodes.pop().expect("just made");
        self.decodes.insert(at, d);
        self.drop_rows_of(&name);
        let again: Vec<Decode> = self.decodes[at + 1..].iter().filter(|x| self.readers(&name).contains(&x.name)).cloned().collect();
        let failed: Vec<String> = again.into_iter().filter_map(|x| self.add_decode(x).err()).collect();
        Ok(failed.join("; "))
    }

    /// The mistakes in an instance's names, without reading a value: its protocol, roles,
    /// prefix, params and clock, as `add_decode` reports them before it decodes (the decode form
    /// checks as it is filled in, spec §5 "Decode form").
    pub fn check_decode(&mut self, d: &Decode) -> Result<(), Reasons> {
        let mut errors: Vec<Reason> = Vec::new();
        let clocked = if let Some(b) = protocol::builtin(&d.protocol) {
            let required = self.required(d, &b);
            self.bind(d, &required, b.optional, b.params, &mut errors);
            b.clocked
        } else if let Some(p) = self.protocols.iter().find(|p| p.name == d.protocol).cloned() {
            let roles: Vec<&str> = p.roles.iter().map(String::as_str).collect();
            self.bind(d, &roles, &[], &[], &mut errors);
            true
        } else {
            errors.push(Reason { on: vec!["protocol".into()], why: Why::Said(protocol::unknown(&d.protocol, &self.protocols)) });
            false
        };
        self.roles.clear();
        if clocked && d.clock.is_empty() {
            errors.push(Reason { on: vec!["clock".into()], why: Why::NoClock });
        } else if clocked && let Err(e) = self.resolve(&d.clock) {
            errors.push(Reason { on: vec!["clock".into()], why: Why::In(e) });
        }
        match errors.is_empty() {
            true => Ok(()),
            false => Err(Reasons { name: Some(d.name.clone()), list: errors }),
        }
    }

    /// A built-in protocol's required roles for this instance: a side none of whose roles is
    /// given, explicitly or under the prefix, may be left out (spec §16).
    fn required(&mut self, d: &Decode, b: &protocol::Builtin) -> Vec<&'static str> {
        let prefix = d.params.iter().find(|p| p.0 == "prefix").map(|p| p.1.clone());
        b.needed(|r| d.signals.iter().any(|s| s.0 == r) || prefix.as_ref().is_some_and(|p| self.resolve(&format!("{p}{r}")).is_ok()))
    }

    /// An instance's signals for the roles: each role's width, and the mistakes (a role without a
    /// signal, a signal for no role, a signal that isn't one).
    /// The roles map to the signals' full paths, so names made by an earlier step can't change them.
    /// With the param `prefix` (`s_axi_`), a role not given is played by `<prefix><role>`, if there
    /// is one (the required roles must be).
    fn bind(&mut self, d: &Decode, required: &[&str], optional: &[&str], allowed: &[&str], errors: &mut Vec<Reason>) -> Vec<(String, Option<u32>)> {
        let mut signals = d.signals.clone();
        if let Some((_, prefix)) = d.params.iter().find(|p| p.0 == "prefix") {
            for r in required.iter().chain(optional) {
                let name = format!("{prefix}{r}");
                if !signals.iter().any(|s| s.0 == *r) && (required.contains(r) || self.resolve(&name).is_ok()) {
                    signals.push((r.to_string(), name));
                }
            }
        }
        let missing: Vec<&str> = required.iter().copied().filter(|r| !signals.iter().any(|(s, _)| s == r)).collect();
        if !missing.is_empty() {
            // where the file has this protocol's ports, the line that decodes them
            let found: Vec<String> = protocol::detect(&self.trace)
                .into_iter()
                .filter(|f| f.protocol == d.protocol)
                .map(|f| f.decode_line().replacen(&format!("decode {} ", f.name()), &format!("decode {} ", d.name), 1))
                .collect();
            errors.push(Reason { on: missing.iter().map(|r| r.to_string()).collect(), why: Why::NoSignal { found } });
        }
        let (mut out, mut roles) = (Vec::new(), HashMap::new());
        for (role, sig) in &signals {
            if !required.contains(&role.as_str()) && !optional.contains(&role.as_str()) {
                errors.push(Reason { on: vec![role.clone()], why: Why::NoRole { protocol: d.protocol.clone() } });
            } else {
                match self.resolve(sig) {
                    Ok(p) => {
                        roles.insert(role.clone(), self.trace.probe_path(p));
                        out.push((role.clone(), self.trace.probe_width(p)));
                    }
                    Err(e) => errors.push(Reason { on: vec![role.clone()], why: Why::In(e) }),
                }
            }
        }
        for (p, _) in d.params.iter().filter(|p| p.0 != "prefix" && !allowed.contains(&p.0.as_str())) {
            let others: Vec<String> = std::iter::once("prefix").chain(allowed.iter().copied()).map(str::to_string).collect();
            errors.push(Reason { on: vec![p.clone()], why: Why::NoParam { protocol: d.protocol.clone(), params: others } });
        }
        // set after all are named, so a role isn't taken for a signal of the same name
        self.roles = roles;
        out
    }

    /// The instance `signals` (vars) make for `protocol`, read from their names alone (spec §5
    /// "Decode form"): a signal plays a role its leaf ends in after `_` or `.` (`s_axi_wvalid`
    /// plays `wvalid`, `s_axi_arvalid` never `rvalid`), the clock one ending in `clk` / `clock`.
    /// A role or clock two signals could play is left empty, and so is the rest.
    pub fn plays(&self, protocol: &str, signals: &[usize]) -> Decode {
        let leaves: Vec<(String, &str)> = signals.iter().map(|&v| (rows::child_name(&self.trace.vars[v].path).to_lowercase(), self.trace.vars[v].path.as_str())).collect();
        let only = |hit: &dyn Fn(&str) -> bool| match leaves.iter().filter(|(l, _)| hit(l)).collect::<Vec<_>>()[..] {
            [(_, p)] => p.to_string(),
            _ => String::new(),
        };
        let (roles, clocked) = self.roles_of(protocol);
        let signals = roles
            .iter()
            .map(|r| {
                let low = r.to_lowercase();
                (r.clone(), only(&|l: &str| l == low || l.strip_suffix(low.as_str()).is_some_and(|s| s.ends_with(['_', '.']))))
            })
            .filter(|(_, s)| !s.is_empty())
            .collect();
        let clock = if clocked { only(&|l: &str| l.ends_with("clk") || l.ends_with("clock")) } else { String::new() };
        Decode { name: String::new(), protocol: protocol.into(), clock, signals, params: Vec::new() }
    }

    /// `base`, else `base1`, `base2`...: the first no protocol or decode is named.
    pub fn free_name(&self, base: &str) -> String {
        let taken = |n: &str| self.protocols.iter().any(|p| p.name == n) || self.decodes.iter().any(|d| d.name == n);
        std::iter::once(base.to_string()).chain((1..).map(|k| format!("{base}{k}"))).find(|n| !taken(n)).expect("names run out never")
    }

    /// A clocked search (`@clk when`) as a protocol named `protocol` and the decode `decode`
    /// putting it on the signals (spec §15 `find`): its roles are the signals `when` names (not
    /// functions, numbers or value names) then those `shown`, each named as its signal's leaf
    /// (two alike: `scope_leaf`, then `_k`); `when` is written on the roles, and each shown signal
    /// is a field, a slice kept (`lo=data[7:0]`).
    pub fn protocol_of(&mut self, protocol: &str, decode: &str, search: &str, shown: &[String]) -> Result<(Sampler, Decode), String> {
        self.query(search)?;
        let (clock, when) = search.trim().strip_prefix('@').and_then(|e| e.split_once(char::is_whitespace)).ok_or("not a clocked search: write it @<clock> <expr>")?;
        let ident = |c: char| c.is_alphanumeric() || "_.$".contains(c);
        let mut words: Vec<(usize, &str)> = Vec::new();
        let mut at = 0;
        for piece in when.split(|c: char| !ident(c) && c != '[') {
            let w = piece.split('[').next().unwrap_or("");
            let next = when[at + piece.len()..].trim_start();
            if !w.is_empty() && !w.starts_with(|c: char| c.is_ascii_digit()) && !next.starts_with('(') && !when[..at].ends_with('\'') {
                words.push((at, w));
            }
            at += piece.len() + 1;
        }
        let mut sigs: Vec<String> = Vec::new();
        for (_, w) in &words {
            if self.resolve(w).is_ok() && !sigs.iter().any(|s| s == w) {
                sigs.push(w.to_string());
            }
        }
        let base = |n: &str| n.split('[').next().unwrap_or(n).to_string();
        for n in shown {
            if !sigs.contains(&base(n)) {
                sigs.push(base(n));
            }
        }
        // a role is its signal's last name; two alike take their scope's (`cpu_state`), then a number
        let word = |s: &str| -> String { s.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect() };
        let mut roles: Vec<String> = sigs.iter().map(|n| word(n.rsplit('.').next().unwrap_or(n))).collect();
        let alike = |roles: &[String]| -> Vec<usize> { (0..roles.len()).filter(|&k| roles.iter().filter(|f| **f == roles[k]).count() > 1).collect() };
        for k in alike(&roles) {
            let mut parts = sigs[k].rsplit('.');
            let (last, scope) = (parts.next().unwrap_or(""), parts.next().map(word).unwrap_or_default());
            roles[k] = if scope.is_empty() { word(last) } else { format!("{scope}_{}", word(last)) };
        }
        for k in alike(&roles) {
            roles[k] = format!("{}_{k}", roles[k]);
        }
        let role = |sig: &str| sigs.iter().position(|s| s == sig).map(|k| roles[k].clone()).unwrap_or_default();
        // `when` with each signal's name its role's, from the end so the places stay
        let mut on_roles = when.to_string();
        for (at, w) in words.iter().rev().filter(|(_, w)| sigs.iter().any(|s| s == w)) {
            on_roles.replace_range(*at..at + w.len(), &role(w));
        }
        // a field is named as its signal's role; two alike (slices of one) take a number
        let mut names: Vec<String> = shown.iter().map(|n| role(&base(n))).collect();
        for k in alike(&names) {
            names[k] = format!("{}_{k}", names[k]);
        }
        let fields = shown.iter().zip(names).map(|(n, f)| (f, n.replacen(&base(n), &role(&base(n)), 1))).collect();
        let p = Sampler { name: protocol.into(), when: on_roles.trim().into(), fields, roles: roles.clone() };
        let d = Decode { name: decode.into(), protocol: protocol.into(), clock: clock.into(), signals: roles.into_iter().zip(sigs).collect(), params: Vec::new() };
        Ok((p, d))
    }

    /// The protocols, built-in then the session's, `signals` play a role of, with how many,
    /// most first.
    pub fn playable(&self, signals: &[usize]) -> Vec<(String, usize)> {
        let names = protocol::BUILTINS.iter().map(|s| s.to_string()).chain(self.protocols.iter().map(|p| p.name.clone()));
        let mut found: Vec<(String, usize)> = names.map(|n| { let k = self.plays(&n, signals).signals.len(); (n, k) }).filter(|p| p.1 > 0).collect();
        found.sort_by_key(|p| std::cmp::Reverse(p.1));
        found
    }

    /// A protocol's roles, required then optional, and whether it samples at a clock.
    fn roles_of(&self, protocol: &str) -> (Vec<String>, bool) {
        if let Some(b) = protocol::builtin(protocol) {
            (b.required.iter().chain(b.optional).map(|s| s.to_string()).collect(), b.clocked)
        } else if let Some(p) = self.protocols.iter().find(|p| p.name == protocol) {
            (p.roles.clone(), true)
        } else {
            (Vec::new(), false)
        }
    }

    /// The decodes a var is in, each with the role it plays there (spec §5 "Picker"): their
    /// clock, a role given it, or one their prefix gives it, or the stream a decoder takes.
    pub fn decodes_of(&self, var: usize) -> Vec<(&str, String)> {
        let path = &self.trace.vars[var].path;
        let role = |d: &Decode| {
            if d.clock == *path {
                return Some("clock".to_string());
            }
            if let Some((r, _)) = d.signals.iter().find(|(_, s)| s == path) {
                return Some(r.clone());
            }
            // kept as the full path's prefix (`add_decode`); a role given apart wins
            let b = protocol::builtin(&d.protocol)?;
            let prefix = &d.params.iter().find(|p| p.0 == "prefix")?.1;
            let r = path.strip_prefix(prefix.as_str())?;
            (b.required.iter().chain(b.optional).any(|x| *x == r) && !d.signals.iter().any(|(x, _)| x == r)).then(|| r.to_string())
        };
        self.decodes.iter().filter_map(|d| Some((d.name.as_str(), role(d)?))).collect()
    }

    /// Stream `i` is intermediate: another stream is made from it (spec §16 "Results").
    pub fn intermediate(&self, i: usize) -> bool {
        let name = &self.streams[i].name;
        self.streams.iter().any(|s| s.inputs.contains(name))
    }

    /// The streams a decode made (named it, or `<it>.<step>`), in the order made.
    pub fn streams_of_decode(&self, decode: &str) -> Vec<usize> {
        let prefix = format!("{decode}.");
        (0..self.streams.len()).filter(|&i| self.streams[i].name == decode || self.streams[i].name.starts_with(&prefix)).collect()
    }

    /// A decode's results then its violations: the streams nothing is made from.
    pub fn results(&self, decode: &str) -> Vec<usize> {
        let mut out: Vec<usize> = self.streams_of_decode(decode).into_iter().filter(|&i| !self.intermediate(i)).collect();
        out.sort_by_key(|&i| self.streams[i].violations);
        out
    }

    /// The streams named for `records` (spec §16 "Records of a set"), each name a stream, a
    /// decode's name (its results and violations) or a pattern in the workspace's syntax
    /// (`bus.*`, `/^bus\./`), separated by spaces.
    pub fn stream_set(&self, names: &str) -> Result<Vec<usize>, unknown::NoStream> {
        use crate::pattern::Pattern;
        let mut out: Vec<usize> = Vec::new();
        for name in names.split_whitespace() {
            let hit = |p: &Pattern, s: &str| match p {
                Pattern::Glob(g) => crate::command::glob(g, s),
                Pattern::Regex(re) => re.is_match(&crate::trace::canon(s)),
            };
            let found = match self.streams.iter().position(|s| s.name == name) {
                Some(i) => vec![i],
                None if self.decodes.iter().any(|d| d.name == name) => self.results(name),
                None => match self.patterns.pattern(name).map_err(unknown::NoStream::Pattern)? {
                    Some(p) => (0..self.streams.len()).filter(|&i| hit(&p, &self.streams[i].name)).collect(),
                    None => vec![],
                },
            };
            if found.is_empty() {
                // an interface the file has, no decode yet, names no stream
                if let Some(e) = self.undecoded(name) {
                    return Err(e);
                }
                return Err(match self.streams.is_empty() {
                    true => unknown::NoStream::NoneYet { name: name.to_string() },
                    false => unknown::NoStream::Unknown { name: name.to_string(), streams: self.streams.iter().map(|s| s.name.clone()).collect() },
                });
            }
            out.extend(found.into_iter().filter(|i| !out.contains(i)).collect::<Vec<_>>());
        }
        if out.is_empty() {
            return Err(unknown::NoStream::Empty);
        }
        Ok(out)
    }

    /// The interface `name` names but nothing decoded yet: the rest of the name, and its interfaces.
    /// A name that starts one (`s_axi.wr`, `top.s_axi.wr`) or is one (`s_axi`) does.
    fn undecoded(&self, name: &str) -> Option<unknown::NoStream> {
        let up_to_last = name.rsplit_once('.').map_or(name, |(b, _)| b);
        let matched = |f: &protocol::Found, n: &str| n == f.name() || n == f.prefix.trim_end_matches(['_', '.']);
        let (mut found, mut part) = (Vec::new(), String::new());
        for f in protocol::detect(&self.trace) {
            let Some(base) = [name, up_to_last].into_iter().find(|n| matched(&f, n)) else { continue };
            // decoded under that name, its streams are what it makes
            if self.decodes.iter().any(|d| d.name == f.name()) {
                return None;
            }
            if found.is_empty() {
                part = name.strip_prefix(base).unwrap_or("").trim_start_matches('.').to_string();
            }
            found.push(f);
        }
        (!found.is_empty()).then(|| unknown::NoStream::NotDecoded { name: name.to_string(), part, found })
    }

    /// The full path of a signal named as in a search.
    fn full_path(&mut self, name: &str) -> Result<String, String> {
        self.resolve(name).map(|p| self.trace.probe_path(p))
    }

    /// A protocol of the session's on signals: the stream of the instance's name.
    fn decode_user(&mut self, d: &Decode) -> Result<usize, String> {
        let p = self.protocols.iter().find(|p| p.name == d.protocol).cloned().ok_or_else(|| protocol::unknown(&d.protocol, &self.protocols))?;
        let mut errors: Vec<Reason> = Vec::new();
        let roles: Vec<&str> = p.roles.iter().map(String::as_str).collect();
        self.bind(d, &roles, &[], &[], &mut errors);
        if d.clock.is_empty() {
            errors.push(Reason { on: vec!["clock".into()], why: Why::NoClock });
        }
        if !errors.is_empty() {
            self.roles.clear();
            return Err(Reasons { name: None, list: errors }.to_string());
        }
        // its expressions name roles and its own fields, never a signal
        self.only_roles = Some((format!("{} names only its roles ({}) and fields", p.name, p.roles.join(", ")), d.clock.clone()));
        let var = self.make_stream(&d.name, &d.clock, &p);
        self.only_roles = None;
        self.roles.clear();
        var
    }

    /// A protocol written in Rust on signals: its steps' streams, `<instance>.<step>`.
    fn decode_builtin(&mut self, d: &Decode, b: protocol::Builtin) -> Result<usize, String> {
        let mut errors: Vec<Reason> = Vec::new();
        let required = self.required(d, &b);
        let given = self.bind(d, &required, b.optional, b.params, &mut errors);
        if d.clock.is_empty() && b.clocked {
            errors.push(Reason { on: vec!["clock".into()], why: Why::NoClock });
        }
        if !errors.is_empty() {
            self.roles.clear();
            return Err(Reasons { name: None, list: errors }.to_string());
        }
        let given: Vec<(&str, Option<u32>)> = given.iter().map(|(r, w)| (r.as_str(), *w)).collect();
        let params: Vec<(String, String)> = d.params.iter().filter(|p| p.0 != "prefix").cloned().collect();
        let (mut vars, mut scratch) = (Vec::new(), Vec::new());
        let mut r = Ok(());
        for step in (b.steps)(&given, &params) {
            let var = match step {
                protocol::Step::Changes { name, signal } => {
                    // kept for the decoders after it, not a stream of the instance's
                    let made = self.resolve(&signal).map(|p| {
                        self.trace.load(&[p.0]);
                        stream::changes(&self.trace, name, p)
                    });
                    match made {
                        Ok(s) => {
                            scratch.push(s);
                            continue;
                        }
                        Err(e) => Err(e),
                    }
                }
                protocol::Step::Sample { name, when, fields } => {
                    let p = Sampler { name: name.into(), when, fields, roles: vec![] };
                    self.make_stream(&format!("{}.{name}", d.name), &d.clock, &p)
                }
                protocol::Step::Check { name, rules } => self.check(&format!("{}.{name}", d.name), &d.clock, &rules).and_then(|s| self.add_signals(s)),
                protocol::Step::Join { name, requests, response, id } => {
                    let find = |x: &str| self.streams.iter().position(|s| s.name == format!("{}.{x}", d.name)).ok_or(format!("no stream {}.{x}", d.name));
                    let made = requests.iter().map(|&r| find(r).map(|i| (r, i))).collect::<Result<Vec<_>, _>>().and_then(|reqs| {
                        let reqs: Vec<(&str, &Stream)> = reqs.into_iter().map(|(r, i)| (r, &self.streams[i])).collect();
                        decode::join(&format!("{}.{name}", d.name), &reqs, &self.streams[find(response)?], id)
                    });
                    made.and_then(|(done, unmatched)| {
                        let v = self.add_signals(done)?;
                        self.add_signals(unmatched).map(|_| v)
                    })
                }
                protocol::Step::Decode { name, input, decoder, params } => {
                    let full = format!("{}.{input}", d.name);
                    let input = scratch.iter().find(|s| s.name == input).or_else(|| self.streams.iter().find(|s| s.name == full)).ok_or(format!("no stream {full}"));
                    let made = input.and_then(|i| {
                        let mut s = decode::run(&format!("{}.{name}", d.name), i, decode::make(decoder, &i.schema, &params, self.trace.fs_per_tick)?);
                        // a signal's changes are no stream of the workspace's
                        s.inputs.retain(|x| *x == full);
                        Ok(s)
                    });
                    made.and_then(|s| self.add_signals(s))
                }
            };
            match var {
                Ok(v) => vars.push(v),
                Err(e) => {
                    r = Err(e);
                    break;
                }
            }
        }
        self.roles.clear();
        r.map(|()| vars[0])
    }

    /// `frame` on a stream (role `input`), or `pair` on two (`request`, `response`), named in
    /// `signals`; the decoder's `params`.
    fn decode_streams(&mut self, d: &Decode) -> Result<usize, String> {
        let roles: &[&str] = if d.protocol == "frame" { &["input"] } else { &["request", "response"] };
        let mut errors = Vec::new();
        let mut inputs = Vec::new();
        for r in roles {
            match d.signals.iter().find(|s| s.0 == *r) {
                None => errors.push(format!("no stream for {r}")),
                Some((_, s)) => match self.streams.iter().position(|x| x.name == *s) {
                    Some(i) => inputs.push(i),
                    None => errors.push(format!("{r}: unknown stream: {s}")),
                },
            }
        }
        errors.extend(d.signals.iter().filter(|s| !roles.contains(&s.0.as_str())).map(|s| format!("{} has no role {}", d.protocol, s.0)));
        let fpt = self.trace.fs_per_tick;
        if d.protocol == "frame" {
            let decoder = inputs.first().map(|&i| decode::make("frame", &self.streams[i].schema, &d.params, fpt));
            match decoder {
                Some(Err(e)) => errors.push(e),
                Some(Ok(dec)) if errors.is_empty() => return self.add_signals(decode::run(&d.name, &self.streams[inputs[0]], dec)),
                _ => {}
            }
            return Err(errors.join("; "));
        }
        let (mut req, mut resp) = (None, None);
        for (k, v) in &d.params {
            match k.as_str() {
                "id" => (req, resp) = (Some(v.as_str()), Some(v.as_str())),
                "req_id" => req = Some(v.as_str()),
                "resp_id" => resp = Some(v.as_str()),
                _ => errors.push(format!("pair: no parameter {k} (id, req_id, resp_id)")),
            }
        }
        if req.is_some() != resp.is_some() {
            errors.push("pair: req_id and resp_id go together".into());
        }
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
        let (pairs, unmatched) = decode::pair(&d.name, &self.streams[inputs[0]], &self.streams[inputs[1]], req.zip(resp))?;
        let var = self.add_signals(pairs)?;
        self.add_signals(unmatched)?;
        Ok(var)
    }

    /// `check` as an instance: its params are the rules (name, expression), at its clock.
    fn decode_check(&mut self, d: &Decode) -> Result<usize, String> {
        let mut errors = Vec::new();
        if d.clock.is_empty() {
            errors.push("no clock".to_string());
        }
        if d.params.is_empty() {
            errors.push("check: no rules (params = { rule = \"expression\" })".into());
        }
        if !d.signals.is_empty() {
            errors.push("check takes no signals".into());
        }
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
        let s = self.check(&d.name, &d.clock, &d.params)?;
        self.add_signals(s)
    }

    /// Violations: a record per cycle of `clock` where a rule (name, expression) holds, its name
    /// the field `rule`; in time order, rules in their order at one time.
    pub fn check(&mut self, name: &str, clock: &str, rules: &[(String, String)]) -> Result<Stream, String> {
        let mut records = Vec::new();
        for (rule, when) in rules {
            let s = self.sample(name, clock, when, &[]).map_err(|e| format!("{rule}: {e}"))?;
            records.extend(s.records.into_iter().map(|r| stream::Record { fields: vec![Some(crate::trace::Val::Text(rule.clone()))], ..r }));
        }
        records.sort_by_key(|r| r.start);
        Ok(Stream { name: name.into(), schema: vec![stream::Field { name: "rule".into(), width: None, time: false, signed: false }], records, violations: true, inputs: vec![] })
    }

    /// Sample a protocol into the stream `name` and add it as signals. Returns the stream's var.
    fn make_stream(&mut self, name: &str, clock: &str, p: &Sampler) -> Result<usize, String> {
        let fields: Vec<(&str, &str)> = p.fields.iter().map(|(f, s)| (f.as_str(), s.as_str())).collect();
        let s = self.sample(name, clock, &p.when, &fields)?;
        self.add_signals(s)
    }

    /// Add a stream as signals (`Stream::signals`): `rx`, `rx.tdata` ... replacing a stream of that
    /// name. Returns the stream's own var.
    fn add_signals(&mut self, s: Stream) -> Result<usize, String> {
        let signals = s.signals(self.trace.start());
        // all or nothing: no signal of the file is taken
        if let Some((path, ..)) = signals.iter().find(|(p, ..)| self.trace.by_path.get(p).is_some_and(|&v| !self.trace.is_computed(v))) {
            return Err(format!("{path} is a signal of the file"));
        }
        // fields it had and hasn't now hold no records: their vars stay, as vars aren't removed
        if let Some(old) = self.streams.iter().find(|x| x.name == s.name) {
            let gone: Vec<(String, Option<u32>)> = old.schema.iter().filter(|f| !s.schema.iter().any(|n| n.name == f.name)).map(|f| (format!("{}.{}", s.name, f.name), f.width)).collect();
            let start = self.trace.start();
            for (path, w) in gone {
                let gap = w.map_or(Val::Text(String::new()), |w| Val::Bits("z".repeat(w as usize)));
                self.trace.add_computed(&path, w, vec![(start, gap)])?;
            }
        }
        let mut vars = Vec::new();
        for (path, width, values) in signals {
            vars.push(self.trace.add_computed(&path, width, values)?);
        }
        // shown by its whole name, the instance's with it (`bus.wr`, spec §4 "Name column")
        let decl = crate::trace::parts(&s.name).len();
        self.trace.vars[vars[0]].decl = decl;
        for (f, &v) in s.schema.iter().zip(&vars[1..]) {
            self.trace.vars[v].decl = decl + 1;
            self.trace.vars[v].signed = f.signed;
            self.trace.vars[v].time = self.trace.fs_per_tick.filter(|_| f.time);
        }
        match self.streams.iter_mut().find(|x| x.name == s.name) {
            Some(old) => *old = s,
            None => self.streams.push(s),
        }
        Ok(vars[0])
    }
}

/// A protocol's own names: its name, its roles' and fields', and that it has roles.
fn protocol_names(p: &Sampler) -> Result<(), Reason> {
    let said = |on: &str, text: String| Reason { on: vec![on.to_string()], why: Why::Said(text) };
    let name = &p.name;
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || "_.$".contains(c)) {
        return Err(said("name", format!("not a protocol name: {name}")));
    }
    if protocol::reserved(name) {
        return Err(said("name", format!("{name} is a built-in protocol")));
    }
    // a field is the last part of its signal's path; a role is a name in expressions
    let plain = |f: &String| !f.is_empty() && f.chars().all(|c| c.is_alphanumeric() || c == '_');
    if let Some((f, _)) = p.fields.iter().find(|(f, _)| !plain(f)) {
        return Err(said(f, format!("{name}: not a field name: {f}")));
    }
    if let Some(r) = p.roles.iter().find(|r| !plain(r)) {
        return Err(said(r, format!("{name}: not a role name: {r}")));
    }
    if p.roles.is_empty() {
        return Err(said("roles", format!("{name}: needs roles, the names its expressions use (roles=valid,ready,data); :decode puts them on signals")));
    }
    Ok(())
}
