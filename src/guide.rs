//! The `help` tool's topics: the reference the tool descriptions point to, each concept explained
//! once (spec §15), one Markdown file each in `guide/`: a line on what it covers, then Form (how
//! to write it), Recipes (a task and the call doing it, each from a real session) and Details.
//! Kept in step with the spec, as the descriptions are. The workspace in words for `view` and
//! the link's `GET` is here too, so the window and the agent word it the same way.

use wtf_core::pattern::Syntax;
use wtf_core::time::{self, Tick};
use wtf_core::workspace::Workspace;

/// (topic, what it covers, the reference): each reference is `guide/<topic>.md`, compiled in.
pub const TOPICS: &[(&str, &str, &str)] = &[
    (
        "names",
        "naming signals: paths, bits, slices, VHDL forms, scope, structs, streams",
        include_str!("guide/names.md"),
    ),
    (
        "patterns",
        "many signals at once, and searching the design with `signals`",
        include_str!("guide/patterns.md"),
    ),
    (
        "times",
        "times you give and get back: units, the workspace's unit, rounding",
        include_str!("guide/times.md"),
    ),
    (
        "radix",
        "radixes and colours of values and rows",
        include_str!("guide/radix.md"),
    ),
    (
        "expressions",
        "search expressions for `find`: @clk, rose / fell / past / stable, operators, values",
        include_str!("guide/expressions.md"),
    ),
    (
        "decode",
        "decoding interfaces into records: built-in protocols, protocol, frame, pair, check",
        include_str!("guide/decode.md"),
    ),
    (
        "wasm",
        "protocols in the config dir's protocols/, written as WebAssembly components",
        include_str!("guide/wasm.md"),
    ),
    (
        "records",
        "reading streams: where, group_by, count_only, n",
        include_str!("guide/records.md"),
    ),
    (
        "workspace",
        "showing evidence: the `workspace` declaration, `view`, `render`, `present`",
        include_str!("guide/workspace.md"),
    ),
    (
        "commands",
        "the `:` commands `command` runs",
        include_str!("guide/commands.md"),
    ),
];

/// The topics with what each covers, or one topic's reference.
pub fn answer(topic: Option<&str>, syntax: Syntax) -> Result<String, String> {
    let list = || {
        TOPICS
            .iter()
            .map(|(t, what, _)| format!("{t}: {what}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    match topic {
        None => Ok(format!("help <topic>:\n{}", list())),
        Some(t) => TOPICS
            .iter()
            .find(|(name, ..)| name.eq_ignore_ascii_case(t.trim()))
            .map(|(.., text)| written(text.trim_end(), syntax))
            .ok_or_else(|| format!("no help topic {t}; topics:\n{}", list())),
    }
}

/// A topic as its interface writes patterns: the window globs, an agent regexes in slashes, so
/// an agent reads of one syntax only (spec §15).
fn written(text: &str, syntax: Syntax) -> String {
    let parts: [(&str, &str); 4] = match syntax {
        Syntax::Regex => [
            (
                "{patterns}",
                "- A pattern is a regex in slashes, found anywhere in the path in any case, the path\n  \
                 lowercase with indices in brackets whichever HDL wrote it: `/uart/`, `/_valid$/`,\n  \
                 `/^top\\.cpu\\./` (a scope's signals), `/mem\\[0\\]/`. `signals`, `add`, `rm` and\n  \
                 `records` take them; a bare word is always a name.",
            ),
            ("{fields}", "/pkt\\./"),
            ("{streams}", "/^bus\\./"),
            ("{all}", "/.*/"),
        ],
        Syntax::Glob => [
            (
                "{patterns}",
                "- A pattern is a glob over the whole path: `*` within a level, `**` across (`top.cpu.*`,\n  \
                 `**.state`, `**.*valid*`). `add`, `rm` and `:records` take them; a word without `*` is a name.",
            ),
            ("{fields}", "pkt.**"),
            ("{streams}", "bus.*"),
            ("{all}", "**"),
        ],
    };
    parts
        .iter()
        .fold(text.to_string(), |t, (k, v)| t.replace(k, v))
}

/// The unit of an answer's times, and their precision.
#[derive(Clone, Copy)]
pub(crate) struct Unit {
    pub(crate) index: usize,
    /// None: exact.
    pub(crate) decimals: Option<u32>,
}

pub(crate) fn show(app: &Workspace, t: Tick, u: Unit) -> String {
    time::format_as(t, app.trace.fs_per_tick, Some(u.index), u.decimals)
}

pub(crate) fn unit_of(
    app: &Workspace,
    asked: Option<usize>,
    ts: impl IntoIterator<Item = Tick>,
) -> Unit {
    match asked.or(app.time_unit()) {
        Some(index) => Unit {
            index,
            decimals: Some(3),
        },
        None => Unit {
            index: time::unit_for(ts, app.trace.fs_per_tick),
            decimals: None,
        },
    }
}

/// The workspace in words, for `view`, its times in `unit` (default: the one that fits them).
pub(crate) fn describe(app: &Workspace, unit: Option<usize>) -> String {
    let names = wtf_core::rows::display_names(&app.trace, &app.rows);
    // the text just written always reads; describing the file keeps one description of
    // the layout. Built after `present`, it matches `app.rows`, so names
    // stay aligned even when missing signals were skipped.
    let file = wtf_core::session::read(&wtf_core::session::to_shared_toml(app)).unwrap_or_default();
    let mut cursors = app.cursor_list();
    cursors.sort_by_key(|&(i, t)| (t, i));
    let end = app.view.last_visible().min(app.trace.end());
    let u = unit_of(
        app,
        unit,
        cursors.iter().map(|c| c.1).chain([app.view.start, end]),
    );
    // the end is where a zoom step happens to stop: rounded, unlike the times the agent set
    let rounded = Unit {
        decimals: Some(3),
        ..u
    };
    let last = show(app, end, rounded);
    let show = |t: Tick| show(app, t, u);
    wtf_core::session::describe(
        &file,
        &wtf_core::session::Describe {
            names: &names,
            sel: app.sel,
            show: &show,
            visible: (show(app.view.start), last),
            trace: &app.trace,
        },
    )
}

#[cfg(test)]
mod syntax_tests {
    use super::*;

    /// Each interface reads its own pattern syntax only, in every topic: the window globs, an
    /// agent regexes; every placeholder is filled.
    #[test]
    fn topics_speak_the_interfaces_patterns() {
        let all = |s| {
            TOPICS
                .iter()
                .map(|(t, ..)| answer(Some(t), s).unwrap())
                .collect::<String>()
        };
        let (agent, window) = (all(Syntax::Regex), all(Syntax::Glob));
        assert!(
            agent.contains("`rm /.*/` clears")
                && agent.contains("a regex in slashes")
                && agent.contains("`/^bus\\./`"),
            "{agent}"
        );
        assert!(!agent.contains("**") && !agent.contains("bus.*"), "{agent}");
        assert!(
            ["{patterns}", "{fields}", "{streams}", "{all}"]
                .iter()
                .all(|k| !agent.contains(k) && !window.contains(k))
        );
        assert!(
            window.contains("`rm **` clears") && window.contains("A pattern is a glob"),
            "{window}"
        );
    }

    /// `help commands` names every command the core runs, so a reader has one list (spec §7).
    #[test]
    fn commands_topic_names_every_command() {
        let text = include_str!("guide/commands.md");
        let named = |c: &str| text.contains(&format!("`{c}`")) || text.contains(&format!("`{c} "));
        for c in wtf_core::command::COMMANDS {
            assert!(named(c.name), "commands.md never says `{}`", c.name);
        }
    }

    /// Every `help <topic>` the topics and the server's instructions point to exists (the tool
    /// descriptions' are checked over MCP, tests/mcp.rs).
    #[test]
    fn pointers_name_topics() {
        let texts = TOPICS
            .iter()
            .map(|(.., t)| *t)
            .chain([include_str!("guide/instructions.md")]);
        for text in texts {
            for p in text.split("`help ").skip(1) {
                let topic = p.split('`').next().unwrap();
                assert!(
                    answer(Some(topic), Syntax::Regex).is_ok(),
                    "no topic {topic}"
                );
            }
        }
    }

    /// Every topic has the same shape and stays a page: Form, Recipes, Details, in that order.
    #[test]
    fn topics_have_one_shape() {
        for (name, _, text) in TOPICS {
            let at = |h: &str| {
                text.lines()
                    .position(|l| l == h)
                    .unwrap_or_else(|| panic!("{name}: no {h}"))
            };
            assert!(
                at("Form") < at("Recipes") && at("Recipes") < at("Details"),
                "{name}: Form, Recipes, Details"
            );
            assert!(
                text.lines().count() <= 50,
                "{name}: {} lines, past 50",
                text.lines().count()
            );
            assert!(
                text.lines().all(|l| l.chars().count() <= 100),
                "{name}: a line past 100 characters"
            );
        }
    }
}
