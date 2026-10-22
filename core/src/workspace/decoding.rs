//! Decoding in a workspace: protocols and decode instances made into streams, whose records and
//! fields become vars (spec §16).

use super::*;

impl Workspace {
    /// A stream sampled at `clock` where `when` holds, with fields (name, signal) named as in a
    /// search (`tdata[63:32]`, `{a, b}`).
    pub fn sample(&mut self, name: &str, clock: &str, when: &str, fields: &[(&str, &str)]) -> Result<Stream, String> {
        let q = self.query(&format!("@{clock} {when}"))?;
        let mut probes = Vec::new();
        for &(field, sig) in fields {
            let p = self.resolve(sig).map_err(|e| format!("{name}.{field}: {e}"))?;
            probes.push((field.to_string(), p, if p.1.is_some() { Some(1) } else { self.trace.vars[p.0].width }));
        }
        self.trace.load(&probes.iter().map(|p| p.1.0).collect::<Vec<_>>());
        stream::sample(&self.trace, name, &q, &probes)
    }

    /// Add a protocol, replacing one of that name. Without roles it is sampled into a stream of
    /// its own name, whose var is returned; with roles it waits for instances (`add_decode`).
    pub fn add_protocol(&mut self, mut p: Sampler) -> Result<Option<usize>, String> {
        // by full path, as an instance's (`add_decode`)
        if !p.clock.is_empty() {
            p.clock = self.full_path(&p.clock).unwrap_or(p.clock);
        }
        let name = &p.name;
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || "_.$".contains(c)) {
            return Err(format!("not a protocol name: {name}"));
        }
        if protocol::reserved(name) {
            return Err(format!("{name} is a built-in protocol"));
        }
        // a field is the last part of its signal's path; a role is a name in expressions
        let plain = |f: &String| !f.is_empty() && f.chars().all(|c| c.is_alphanumeric() || c == '_');
        if let Some((f, _)) = p.fields.iter().find(|(f, _)| !plain(f)) {
            return Err(format!("{name}: not a field name: {f}"));
        }
        if let Some(r) = p.roles.iter().find(|r| !plain(r)) {
            return Err(format!("{name}: not a role name: {r}"));
        }
        let var = match p.roles.is_empty() {
            true => Some(self.make_stream(name, &p.clock, &p)?),
            false => None,
        };
        match self.protocols.iter_mut().find(|x| x.name == p.name) {
            Some(old) => *old = p,
            None => self.protocols.push(p),
        }
        Ok(var)
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
            let role = b.required[0];
            let full = self.resolve(&format!("{prefix}{role}")).ok().map(|p| self.trace.vars[p.0].path.clone());
            if let Some(full) = full.and_then(|f| f.strip_suffix(role).map(String::from)) {
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

    /// An instance's signals for the roles: each role's width, and the mistakes (a role without a
    /// signal, a signal for no role, a signal that isn't one).
    /// The roles map to the signals' full paths, so names made by an earlier step can't change them.
    /// With the param `prefix` (`s_axi_`), a role not given is played by `<prefix><role>`, if there
    /// is one (the required roles must be).
    fn bind(&mut self, d: &Decode, required: &[&str], optional: &[&str], allowed: &[&str], errors: &mut Vec<String>) -> Vec<(String, Option<u32>)> {
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
            errors.push(format!("no signal for {}", missing.join(", ")));
        }
        let (mut out, mut roles) = (Vec::new(), HashMap::new());
        for (role, sig) in &signals {
            if !required.contains(&role.as_str()) && !optional.contains(&role.as_str()) {
                errors.push(format!("{} has no role {role}", d.protocol));
            } else {
                match self.resolve(sig) {
                    Ok(p) => {
                        roles.insert(role.clone(), self.path_of(p));
                        out.push((role.clone(), if p.1.is_some() { Some(1) } else { self.trace.vars[p.0].width }));
                    }
                    Err(e) => errors.push(format!("{role}: {e}")),
                }
            }
        }
        for (p, _) in d.params.iter().filter(|p| p.0 != "prefix" && !allowed.contains(&p.0.as_str())) {
            let others: Vec<&str> = std::iter::once("prefix").chain(allowed.iter().copied()).collect();
            errors.push(format!("{} has no param {p} (params: {})", d.protocol, others.join(", ")));
        }
        // set after all are named, so a role isn't taken for a signal of the same name
        self.roles = roles;
        out
    }

    /// A signal's full path, a bit of a bus as `data[3]`.
    fn path_of(&self, (var, bit): search::Probe) -> String {
        let path = &self.trace.vars[var].path;
        bit.map_or(path.clone(), |b| format!("{path}[{b}]"))
    }

    /// The full path of a signal named as in a search.
    fn full_path(&mut self, name: &str) -> Result<String, String> {
        self.resolve(name).map(|p| self.path_of(p))
    }

    /// A protocol of the session's on signals: the stream of the instance's name.
    fn decode_user(&mut self, d: &Decode) -> Result<usize, String> {
        let p = self.protocols.iter().find(|p| p.name == d.protocol).cloned().ok_or_else(|| protocol::unknown(&d.protocol, &self.protocols))?;
        let mut errors = Vec::new();
        if p.roles.is_empty() {
            errors.push(format!("protocol {} has no roles to put on signals", p.name));
        }
        let roles: Vec<&str> = p.roles.iter().map(String::as_str).collect();
        self.bind(d, &roles, &[], &[], &mut errors);
        let clock = if d.clock.is_empty() { &p.clock } else { &d.clock };
        if clock.is_empty() {
            errors.push("no clock (the protocol has none either)".into());
        }
        if !errors.is_empty() {
            self.roles.clear();
            return Err(errors.join("; "));
        }
        let var = self.make_stream(&d.name, &clock.clone(), &p);
        self.roles.clear();
        var
    }

    /// A protocol written in Rust on signals: its steps' streams, `<instance>.<step>`.
    fn decode_builtin(&mut self, d: &Decode, b: protocol::Builtin) -> Result<usize, String> {
        let mut errors = Vec::new();
        let given = self.bind(d, b.required, b.optional, b.params, &mut errors);
        if d.clock.is_empty() && b.clocked {
            errors.push("no clock".into());
        }
        if !errors.is_empty() {
            self.roles.clear();
            return Err(errors.join("; "));
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
                        let width = if p.1.is_some() { Some(1) } else { self.trace.vars[p.0].width };
                        self.trace.load(&[p.0]);
                        let start = self.trace.start();
                        let mut changes = vec![(start, self.trace.value_of(p.0, p.1, start))];
                        changes.extend(self.trace.walk(p.0, p.1, start, true));
                        let end = self.trace.end();
                        let ends: Vec<Tick> = changes.iter().skip(1).map(|c| c.0).collect();
                        let records = changes
                            .into_iter()
                            .enumerate()
                            .map(|(i, (t, v))| Record { start: t, end: ends.get(i).copied().unwrap_or(end.max(t + 1)), fields: vec![v] })
                            .collect();
                        Stream { name: name.into(), schema: vec![stream::Field { name: "value".into(), width, time: false }], records, violations: false }
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
                    let p = Sampler { name: name.into(), clock: d.clock.clone(), when, fields, roles: vec![] };
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
                    let made = input.and_then(|i| Ok(decode::run(&format!("{}.{name}", d.name), i, decode::make(decoder, &i.schema, &params, self.trace.fs_per_tick)?)));
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
        Ok(Stream { name: name.into(), schema: vec![stream::Field { name: "rule".into(), width: None, time: false }], records, violations: true })
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
        let mut vars = Vec::new();
        for (path, width, values) in signals {
            vars.push(self.trace.add_computed(&path, width, values)?);
        }
        match self.streams.iter_mut().find(|x| x.name == s.name) {
            Some(old) => *old = s,
            None => self.streams.push(s),
        }
        Ok(vars[0])
    }
}
