+++
title = "Configuration"
description = "The config file's keys and values, themes, the first-start questions, and every command line flag."
+++

wtf reads one file, `config.toml`, for how it looks and which keys it takes. Sessions don't depend on it. A session travels with its trace; the config stays on your machine.

## The config file

| Where | Notes |
|---|---|
| `$XDG_CONFIG_HOME/wtf/config.toml` | used when `XDG_CONFIG_HOME` is set |
| `~/.config/wtf/config.toml` | otherwise |
| `<config dir>/themes/<name>.toml` | your themes, next to `config.toml` |

The file is optional. wtf reads it once, at start, so a change shows the next time you start wtf.

A bad entry never stops wtf. It starts with the valid entries and the defaults for the rest. The
status line names the entry and lists what is allowed:

```
config: unknown key colors.curser (keys: line, fill, …)
config: glyphs = "utf8"? (unicode, box, ascii, nerd)
```

## Top-level keys

| Key | Values | Default | Sets |
|---|---|---|---|
| `glyphs` | `"unicode"`, `"box"`, `"ascii"`, `"nerd"` | `"unicode"` | the characters for waveforms, cards and menus |
| `theme` | `"dark"`, `"light"`, or a name from your themes | `"dark"`, or `"light"` on a light terminal | the palette |
| `which_key` | milliseconds, `0`-`10000` | `300` | how long keys wait before the menu of what can follow shows; `0` shows it at once |
| `name_width` | columns, `4`-`200` | `30` | the widest the name column gets |
| `value_width` | columns, `5`-`200` | `20` | the widest the value column gets |
| `colors` | a table of [colour keys](#colour-keys) | none | single colours, over the theme |
| `keys` | a table of [action = key](#keys-and-leader) | none | Normal mode's keys |
| `leader` | a table of [menu entries](#keys-and-leader) | none | the leader menu |

At start, wtf asks the terminal for its background colour. If the background is light and `theme`
is not set, wtf uses `light`. If the terminal does not answer, wtf uses `dark`.

A minimal file:

```toml
# ~/.config/wtf/config.toml
glyphs = "box"
which_key = 300
theme = "light"
```

## Colour keys

Put these under `[colors]` to change one colour of the theme:

```toml
[colors]
cursor = "#e5c07b"
panel = 235
red = "#ff5555"
```

| Key | Used for |
|---|---|
| `line` | 1-bit lines |
| `bus` | bus and stream outlines; their labels use `text` |
| `fill` | 1-bit signals that toggle faster than a column |
| `unknown` | X |
| `highz` | Z |
| `cursor` | the accent: the cursor's time, the selected name, edit mode |
| `cursor_text` | text on the accent |
| `column` | behind the cursor's column |
| `cursor_dim` | behind the marks' columns |
| `mark` | behind a mark's number and name on the ruler |
| `selected` | behind the selected row |
| `panel` | behind the signal panel, help and completions |
| `hatch` | busy bus columns |
| `dim` | scopes, time labels and headings |
| `background` | the screen |
| `text` | text |
| `red`, `orange`, `yellow`, `green`, `cyan`, `blue`, `magenta`, `gray` | the shades a signal's colour can take |

`background` and `text` take `"default"` to keep the terminal's own colours. The signal colours
are the names a signal's row can have, set with `:color`.

A colour value takes one of these forms:

| Form | Example | Meaning |
|---|---|---|
| hex | `"#e5c07b"` | a 24-bit colour. On a terminal without truecolor, the nearest of 256 colours |
| index | `235` | a 256-colour index, `0`-`255` |
| pair | `["#282828", 235]` | the hex colour with truecolor, the index without it. Use it when the nearest shade turns grey |
| default | `"default"` | the terminal's own colour |

## Themes

wtf has two built-in themes:

| Theme | Use it when |
|---|---|
| `dark` | your terminal has a dark background. It keeps the terminal's own background and text |
| `light` | your terminal has a light background. It paints its own background and text |

To write a theme, make a file in `<config dir>/themes/` named after it. It takes the same keys and
values as `[colors]`. A key left out takes its value from `dark`, so a theme can be a few lines:

```toml
# <config dir>/themes/gruvbox.toml
background = ["#282828", 235]
text       = ["#ebdbb2", 223]
line       = ["#83a598", 109]
cursor     = ["#fabd2f", 214]
```

Select it with `theme = "gruvbox"`. A theme file named as a built-in theme (`dark` or `light`)
replaces it.

A bad key in a theme file is skipped and named with the theme, for example
`config: theme gruvbox: unknown key lne (keys: …)`. An unknown theme name lists the themes there are.

## Keys and leader

### Keys

`[keys]` moves Normal mode's keys. Each entry is an action name and the key, written as for
[`--keys`](#command-line): `"x"`, `"gx"`, `"<C-x>"`, `"<CR>"`, `"<Space>"`, `"<Tab>"`, `"<S-Tab>"`.

```toml
[keys]
zoom_in = "i"
fold = "o"
start = "<C-g>"
```

The action names and their default keys are in [the keys page](@/keys.md#names-for-keys).

- A key is one key or two.
- An action you leave out keeps its key.
- Two actions on one key, or one key that starts another (`g` and `gg`), is an error. The error
  names the action that already has the key.
- A digit can't start a key, because digits are counts. `Esc` can't be a key, because it cancels.
- An unknown action name lists the names that exist. The entry is skipped.

### Leader

`[leader]` adds to the menu that opens with `Space` and changes entries in it. An entry is a key
and a list of a label and a command:

```toml
[leader]
b = ["bus reads", ":pick records bus.rd"]
n = ""                  # no messages in the menu
```

- A table is a group with a `name`. An entry inside it goes in the group.
- `""` removes the entry on that key.
- An entry replaces the one on its key. A group merges into the group on its key.
- A key is one key, but not `Esc` or `Backspace`, which close the menu.

A bad entry is skipped and named, for example `config: leader.x = ["x", "q"]? (…)`.

## First-start questions

The first time you open a trace in a terminal without a config file, wtf asks two questions:

1. Which glyph set the terminal draws cleanly. Each set is a card with a sample.
2. Dark or light. Each theme is a card with a sample, drawn in the glyph set you chose.

Answer with `1`-`4`, or with `j`, `k` and `Enter`. `Esc` takes the defaults (`unicode`, and the
theme that suits your background) for the questions you have not answered. `Ctrl-c` leaves without
writing, so the next start asks again.

The answers are saved as `glyphs` and `theme` in `config.toml`, which wtf creates if it is missing.

To ask again, run:

```sh
wtf --setup
wtf --setup trace.vcd
```

`--setup` changes only `glyphs` and `theme` in an existing file. Your other keys and comments stay.
Without a trace, wtf exits after it writes the file.

If the glyphs or colours look wrong on your terminal, see [troubleshooting](@/troubleshooting.md#boxes-or-question-marks-in-the-waveforms).

## Command line

```sh
wtf [OPTIONS] [TRACE]
```

`TRACE` is a VCD, FST or GHW file. Without it, wtf opens the start screen.

| Option | Meaning |
|---|---|
| `-S <session>` | load this session file instead of `<trace>.session.toml` |
| `--glyphs <set>` | draw with this glyph set: `unicode`, `box`, `ascii` or `nerd`. Overrides `glyphs` in the config file |
| `--snapshot <width>[x<height>]` | print one screen as plain text to stdout and exit. Without a height, the screen is as tall as its content |
| `--ansi` | with `--snapshot`: print the screen in colour, as SGR escape codes, with the configured theme |
| `--keys <keys>` | with `--snapshot`: press these keys before the screen is printed |
| `--setup` | ask the first-start questions again, then open `TRACE` if given |
| `--mcp` | serve on stdin and stdout instead of opening the screen. Reads no config file |
| `-h`, `--help` | print help |
| `-V`, `--version` | print the version |

### Snapshots

`--snapshot` prints the screen without the terminal interface. Use it in bug reports, CI logs and
scripts.

```sh
wtf trace.vcd --snapshot 100x20
wtf trace.vcd --snapshot 100x20 --glyphs ascii --keys ':add top<CR>'
```

- Snapshots have no colour, unless you add `--ansi`.
- The cursor is drawn as `┃` (`|` in `ascii`). Marks are drawn as `╎` (`:`).
- `--keys` takes Vim's notation: plain characters, `<Esc>`, `<CR>`, `<Tab>`, `<S-Tab>`, `<BS>`,
  `<Space>`, `<lt>` for `<`, and `<C-x>`. Another `<name>` is an error. A `<` that starts no name
  is typed as it is.
- A snapshot follows `--glyphs` and `glyphs`, so an `ascii` snapshot pastes cleanly into a
  report. Without `--ansi` it uses the default theme.

## Precedence

When two settings disagree, the first one here wins:

| Setting | Over |
|---|---|
| `NO_COLOR` | the theme and `[colors]` |
| `--glyphs` | `glyphs` in the config file |
| `glyphs` in the config file | the default, `unicode` |

## NO_COLOR

Set `NO_COLOR` to any non-empty value to turn off colour, as [no-color.org](https://no-color.org)
describes:

```sh
NO_COLOR=1 wtf trace.vcd
```

wtf then marks the cursor and the selected row by reversing them, and draws marks with a glyph.
Colours from the theme and `[colors]` are not drawn. The first-start question about dark or light
is not asked when `NO_COLOR` is set.

For colour and glyph problems in general, see [troubleshooting](@/troubleshooting.md#the-colours-look-wrong).
