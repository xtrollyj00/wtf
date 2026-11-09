//! Patterns naming many signals or streams (spec §7, §15). Each interface writes them its own
//! way: the window globs (`top.cpu.*`, `**.state`), an agent regexes in slashes (`/^top\.cpu\./`),
//! so an agent thinks in one syntax only. A bare word is a name in both.

/// How the workspace's interface writes patterns.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Syntax {
    /// `*` within a level of the path, `**` across: the window.
    #[default]
    Glob,
    /// A regex in slashes, found anywhere in the path in canonical form (`canon`: lowercase,
    /// indices in brackets), in any case: the agent.
    Regex,
}

/// A pattern, as `Syntax::pattern` read it.
pub enum Pattern {
    Glob(String),
    Regex(regex::Regex),
}

impl Syntax {
    /// The pattern `word` is, or None for a name: a glob has a `*` outside an expression, a regex
    /// is in slashes. A regex that doesn't parse, or a glob where patterns are regexes, is an
    /// error giving the form meant.
    pub fn pattern(self, word: &str) -> Result<Option<Pattern>, String> {
        let w = word.trim();
        let glob = w.contains('*') && crate::computed::form(w).is_none();
        match self {
            Syntax::Glob => Ok(glob.then(|| Pattern::Glob(w.to_string()))),
            Syntax::Regex => match w.strip_prefix('/').and_then(|r| r.strip_suffix('/')).filter(|_| w.len() >= 2) {
                Some(re) => regex(re).map(|r| Some(Pattern::Regex(r))),
                None if glob => Err(format!("{w} is a glob; a pattern is a regex in slashes: /{}/", glob_regex(w))),
                None => Ok(None),
            },
        }
    }
}

/// A regex, in any case. Its error says that it is one, not a glob.
pub fn regex(re: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(re).case_insensitive(true).build().map_err(|e| {
        let why = e.to_string().lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().trim_start_matches("error: ").to_string();
        match re.contains('*') {
            true => format!("not a regex: /{re}/ ({why}); `/{}/` for that glob", glob_regex(re)),
            false => format!("not a regex: /{re}/ ({why})"),
        }
    })
}

/// A glob as the regex meant (`*uart*` is `uart`, `**.{a,b}` is `(a|b)`, `top.cpu.*` is
/// `top\.cpu\.`): agents write globs out of habit.
pub fn glob_regex(glob: &str) -> String {
    let core = glob.trim_start_matches("**.").trim_matches('*');
    let mut out = String::new();
    let mut chars = core.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                chars.next_if_eq(&'*');
                out += ".*";
            }
            '{' => out.push('('),
            '}' => out.push(')'),
            ',' => out.push('|'),
            '?' => out.push('.'),
            c => out += &regex::escape(&c.to_string()),
        }
    }
    if out.is_empty() { ".*".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_in_each_syntax() {
        let is = |s: Syntax, w: &str| s.pattern(w).map(|p| p.is_some());
        assert_eq!(is(Syntax::Glob, "top.cpu.*"), Ok(true));
        assert_eq!(is(Syntax::Glob, "top.cpu"), Ok(false));
        assert_eq!(is(Syntax::Glob, "(a * b)"), Ok(false));
        assert_eq!(is(Syntax::Regex, "/^top\\.cpu\\./"), Ok(true));
        assert_eq!(is(Syntax::Regex, "top.cpu"), Ok(false));
        assert_eq!(is(Syntax::Regex, "(a * b)"), Ok(false));
        assert_eq!(is(Syntax::Regex, "/"), Ok(false));
        assert_eq!(is(Syntax::Regex, "top.cpu.*"), Err("top.cpu.* is a glob; a pattern is a regex in slashes: /top\\.cpu\\./".into()));
        assert_eq!(is(Syntax::Regex, "**"), Err("** is a glob; a pattern is a regex in slashes: /.*/".into()));
        assert_eq!(is(Syntax::Regex, "/*uart*/"), Err("not a regex: /*uart*/ (repetition operator missing expression); `/uart/` for that glob".into()));
        // without a `*`, no glob to offer
        let e = regex("(").unwrap_err();
        assert!(e.starts_with("not a regex: /(/ (") && !e.contains("glob"), "{e}");
    }

    #[test]
    fn globs_as_regexes() {
        assert_eq!(glob_regex("top.*.state"), "top\\..*\\.state");
        assert_eq!(glob_regex("**.a**b"), "a.*b");
        assert_eq!(glob_regex("rx?"), "rx.");
        assert_eq!(glob_regex("**"), ".*");
    }
}
