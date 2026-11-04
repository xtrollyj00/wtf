//! The site's keys and commands page (`site/content/keys.md`), written from the tables the window
//! reads: the `?` box's sections, the actions and their `[keys]` names, the leader menu's
//! defaults and the `:` commands. A test keeps the page in step with them.

use crate::tui::app::App;
use crate::tui::commands::COMMANDS;
use crate::tui::keys::{ACTIONS, Entry, Key, Keymap};

/// A table cell: `|` escaped, as Markdown tables need.
fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Keys as `<kbd>`s, each word of `j k` or `Ctrl-d/u` its own.
fn kbd(keys: &str) -> String {
    keys.split(' ').filter(|k| !k.is_empty()).map(|k| format!("<kbd>{}</kbd>", cell(&k.replace('<', "&lt;")))).collect::<Vec<_>>().join(" ")
}

/// The `?` box's sections as tables, a heading each.
fn sections(out: &mut String, app: &App) {
    let (_, sections, heading, examples) = crate::tui::render::help_text(app);
    // side by side on a wide page, as the `?` box puts them (markdown inside, between blank lines)
    out.push_str("<div class=\"keys\">\n\n");
    for (head, keys) in sections {
        out.push_str(&format!("<section>\n\n### {head}\n\n| Keys | Does |\n|---|---|\n"));
        for (k, d) in keys {
            out.push_str(&format!("| {} | {} |\n", kbd(&k), cell(d)));
        }
        out.push_str("\n</section>\n\n");
    }
    out.push_str("</div>\n\n");
    // help_text always has examples
    if !heading.is_empty() {
        out.push_str(&format!("### {heading}\n\n| Type | Finds |\n|---|---|\n"));
        for (e, d) in examples {
            out.push_str(&format!("| `{}` | {} |\n", cell(e), cell(d)));
        }
    } else {
        for (_, d) in examples {
            out.push_str(&format!("{d}\n"));
        }
    }
    out.push('\n');
}

/// The leader menu: the top's entries, then a table per group.
fn leader(out: &mut String, items: &[(Key, Entry)]) {
    let run = |out: &mut String, path: &str, k: &Key, label: &str, line: &str, why: &str| {
        let what = if why.is_empty() { cell(label) } else { format!("{}: {}", cell(label), cell(why)) };
        out.push_str(&format!("| {} | {what} | `{}` |\n", kbd(&format!("{path}{}", k.show())), cell(line.trim_end())));
    };
    out.push_str("| Keys | Does | Runs |\n|---|---|---|\n");
    for (k, e) in items {
        if let Entry::Run { label, line, why } = e {
            run(out, "Space ", k, label, line, why);
        }
    }
    out.push('\n');
    for (k, e) in items {
        if let Entry::Group { name, items } = e {
            out.push_str(&format!("### {} {name}\n\n| Keys | Does | Runs |\n|---|---|---|\n", kbd(&format!("Space {}", k.show()))));
            for (k2, e2) in items {
                if let Entry::Run { label, line, why } = e2 {
                    run(out, &format!("Space {} ", k.show()), k2, label, line, why);
                }
            }
            out.push('\n');
        }
    }
}

/// The whole page, front matter first.
pub fn keys_page(app: &mut App) -> String {
    let mut out = String::from(
        "+++\ntitle = \"Keys and commands\"\ndescription = \"Every key and command of wtf's window, with the names to change them.\"\n+++\n\n\
         <!-- Written by `UPDATE_SITE=1 cargo test site_keys` from the tables wtf reads; don't edit. -->\n\n\
         These are wtf's defaults. `?` in the window shows the keys of where you are; `[keys]` and \
         `[leader]` in the config file change them (see [the names](#names-for-keys) below).\n\n\
         ## In the waveforms\n\n",
    );
    sections(&mut out, app);
    out.push_str("## In the signal panel\n\n`a` opens the panel: the trace's scopes and signals as a tree.\n\n");
    let adding = std::mem::replace(&mut app.adding, true);
    sections(&mut out, app);
    app.adding = adding;
    for (head, intro, keys) in [
        ("In the records table", "`:records` opens it on a set of streams. The keys are fixed.", crate::tui::app::App::BROWSER_KEYS),
        ("In the picker", "`:pick <source>` opens it: typing edits the query. The keys are fixed.", crate::tui::app::App::PICKER_KEYS),
    ] {
        out.push_str(&format!("## {head}\n\n{intro}\n\n| Keys | Does |\n|---|---|\n"));
        for (k, d) in keys {
            out.push_str(&format!("| {} | {} |\n", kbd(k), cell(d)));
        }
        out.push('\n');
    }
    out.push_str("## The Space menu\n\n`Space` opens a menu of everything that has no key of its own. Groups open with their key; `Backspace` goes back up.\n\n");
    if let Entry::Group { items, .. } = &Keymap::default().leader {
        leader(&mut out, items);
    }
    out.push_str("## Commands\n\n`:` opens the command line; `Tab` completes, and the card above it says what each command does.\n\n| Command | Does |\n|---|---|\n");
    for c in COMMANDS {
        out.push_str(&format!("| `{}` | {} |\n", cell(c.usage), cell(c.desc)));
    }
    out.push_str("\n## Names for `[keys]`\n\nEach key's name in the config file's `[keys]` table, with its default. A count before a key marked *count* repeats it (`5l`), or numbers a mark (`3m`).\n\n```toml\n[keys]\nzoom_in = \"i\"       # z\nstart = \"<C-g>\"     # gg\n```\n\n| Name | Default | Does |\n|---|---|---|\n");
    for &(_, name, key, desc, count) in ACTIONS {
        out.push_str(&format!("| `{name}` | `{}` | {}{} |\n", cell(key), cell(desc), if count { ", *count*" } else { "" }));
    }
    out
}
