//! What is wrong with a decode instance or a protocol (`check_decode`, `check_protocol`), worked
//! out apart from how it is said (`Display`): the fields each mistake is on, so the forms mark
//! them without reading the text (spec §5 "Decode form", "Protocol form").

use std::fmt;

/// The mistakes in an instance or protocol, said as `<name>: <a>; <b>` (no prefix without a name).
#[derive(Debug, Clone, PartialEq)]
pub struct Reasons {
    pub name: Option<String>,
    pub list: Vec<Reason>,
}

/// One mistake, and the fields it is on: roles, `clock`, `protocol`, `name`, `roles`, `when`,
/// a param or a protocol field, as the form labels them.
#[derive(Debug, Clone, PartialEq)]
pub struct Reason {
    pub on: Vec<String>,
    pub why: Why,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Why {
    /// Required roles without a signal (`on`), and the decode lines the file has for the protocol.
    NoSignal { found: Vec<String> },
    /// A signal given for a role the protocol hasn't (`on[0]`).
    NoRole { protocol: String },
    /// A param the protocol hasn't (`on[0]`), and the ones it has.
    NoParam { protocol: String, params: Vec<String> },
    NoClock,
    /// What is wrong in the field: its signal or expression's error.
    In(String),
    /// Said as it is (an unknown protocol, a protocol's own names).
    Said(String),
}

impl Reasons {
    /// The fields named, in order, each once.
    pub fn fields(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for on in self.list.iter().flat_map(|r| &r.on) {
            if !out.contains(&on.as_str()) {
                out.push(on);
            }
        }
        out
    }
}

impl fmt::Display for Reasons {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(name) = &self.name {
            write!(f, "{name}: ")?;
        }
        write!(f, "{}", self.list.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let on = |i: usize| self.on.get(i).map_or("", String::as_str);
        match &self.why {
            Why::NoSignal { found } => {
                write!(f, "no signal for {}", self.on.join(", "))?;
                match found.is_empty() {
                    true => Ok(()),
                    false => write!(f, " (the file has: {})", found.join("; ")),
                }
            }
            Why::NoRole { protocol } => write!(f, "{protocol} has no role {}", on(0)),
            Why::NoParam { protocol, params } => write!(f, "{protocol} has no param {} (params: {})", on(0), params.join(", ")),
            Why::NoClock => write!(f, "no clock"),
            Why::In(e) => write!(f, "{}: {e}", on(0)),
            Why::Said(text) => write!(f, "{text}"),
        }
    }
}
