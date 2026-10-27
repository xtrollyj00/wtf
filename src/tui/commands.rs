//! The `:` commands as the command line lists and completes them (spec §4 "Command line", §7):
//! the core's, and the window's own (`:w`, `:q`, `:help`).

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

pub struct Command {
    pub name: &'static str,
    pub usage: &'static str,
    pub desc: &'static str,
    pub args: bool,
}

const TIME: &str = "<time>";

pub const COMMANDS: &[Command] = &[
    Command { name: TIME, usage: ":<time>", desc: "Move the cursor to a time, e.g. :2000ns", args: false },
    Command { name: "zoom", usage: ":zoom <time>", desc: "Set the time per column, e.g. :zoom 1ns", args: true },
    Command { name: "add", usage: ":add <path|pattern>...", desc: "Add signals below the selected row (after a selected group); a scope becomes a group, e.g. :add top.cpu.*", args: true },
    Command { name: "rm", usage: ":rm [path|pattern]...", desc: "Remove matching rows, or the selected row; :rm ** removes all", args: true },
    // shortcut: the radix names are listed by hand here; complete arguments from RADIXES once commands take completable arguments.
    Command { name: "signal", usage: ":signal <name> = <signal>", desc: "Name a computed signal, e.g. :signal op = cmd[7:0] or :signal hdr = {a, b}", args: true },
    Command { name: "decode", usage: ":decode <name> <protocol> <role>=<signal>...", desc: "Decode a protocol into streams, e.g. :decode rx axis clock=clk tvalid=rx_tvalid tready=rx_tready tdata=rx_tdata tlast=rx_tlast", args: true },
    Command { name: "protocol", usage: ":protocol <name> clock=<clk> when=<expr> <field>=<signal>...", desc: "Sample a stream where when holds, e.g. :protocol cmd clock=clk when=valid && ready op=cmd[7:0]; roles=a,b makes a type for :decode", args: true },
    Command { name: "names", usage: ":names <signal> <value>=<name>...", desc: "Name a signal's values, shown by the enum radix and searched by name, e.g. :names state 0=IDLE 1=BUSY; off removes them", args: true },
    Command { name: "records", usage: ":records <stream> [where <expr>]", desc: "List a stream's records, e.g. :records bus.wr where resp != 0; Enter jumps to one", args: true },
    Command { name: "scope", usage: ":scope [path]", desc: "Resolve names under a scope first, e.g. :scope top.cpu makes state top.cpu.state; none without a path", args: true },
    Command { name: "radix", usage: ":radix <name>", desc: "Set the radix: hex bin dec sdec oct ascii float enum, q4.12 / uq4.12 fixed point", args: true },
    Command { name: "color", usage: ":color <name|off>", desc: "Colour the selected row or group: red orange yellow green cyan blue magenta gray; off for the theme's", args: true },
    Command { name: "group", usage: ":group <name>", desc: "Group the selected rows (V in Edit mode)", args: true },
    Command { name: "rename", usage: ":rename <name>", desc: "Rename the group the selected row is in", args: true },
    Command { name: "ungroup", usage: ":ungroup", desc: "Dissolve the group the selected row is in, keeping its signals", args: false },
    Command { name: "select", usage: ":select <row> [<last row>]", desc: "Select a signal or group by name, or the rows from one to another", args: true },
    Command { name: "cursor", usage: ":cursor <n> [time|off]", desc: "Put mark n at a time (1: the cursor), remove it, or jump to it, e.g. :cursor 2 120ns", args: true },
    Command { name: "undo", usage: ":undo", desc: "Undo the last change to the rows", args: false },
    Command { name: "redo", usage: ":redo", desc: "Redo a change undone with :undo", args: false },
    Command { name: "fit", usage: ":fit [<from> <to>]", desc: "Zoom to the whole simulation, or between cursors or times, e.g. :fit '2 '3 or :fit 10ns 20ns", args: true },
    Command { name: "follow", usage: ":follow [off]", desc: "In a window opened with wtf --attach: show the agent's changes as it makes them, or stop", args: true },
    Command { name: "e", usage: ":e", desc: "Reload the trace, keeping signals, groups, cursor and zoom", args: false },
    Command { name: "w", usage: ":w [file]", desc: "Save the session", args: true },
    Command { name: "wq", usage: ":wq", desc: "Save the session and quit", args: false },
    Command { name: "help", usage: ":help [topic]", desc: "Show the keys of the current mode (also ?), or a topic: names times radix expressions decode records workspace commands", args: true },
    Command { name: "q", usage: ":q", desc: "Quit; refuses if the session has unsaved changes", args: false },
    Command { name: "q!", usage: ":q!", desc: "Quit without saving", args: false },
];

/// Commands matching what is typed so far, best first.
pub fn completions(line: &str) -> Vec<&'static Command> {
    let word = line.split_whitespace().next().unwrap_or("");
    if word.is_empty() {
        return COMMANDS.iter().collect();
    }
    if word.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return vec![&COMMANDS[0]];
    }
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).filter(|n| *n != TIME).collect();
    Pattern::parse(word, CaseMatching::Ignore, Normalization::Smart)
        .match_list(names, &mut Matcher::new(Config::DEFAULT))
        .into_iter()
        .filter_map(|(n, _)| COMMANDS.iter().find(|c| c.name == n))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes() {
        assert_eq!(completions("").len(), COMMANDS.len());
        assert_eq!(completions("20")[0].name, TIME);
        assert_eq!(completions("zo")[0].name, "zoom");
        assert_eq!(completions("ad top")[0].name, "add");
    }
}
