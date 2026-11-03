## 19. Configuration

How wtf looks and which keys it takes on this machine, kept apart from sessions: a session is what to show of a trace and
travels with it; the config file is the user's terminal and taste, and stays on the machine.

### The file

`$XDG_CONFIG_HOME/wtf/config.toml`, or `~/.config/wtf/config.toml` without `XDG_CONFIG_HOME`. It is
optional; without it wtf uses the defaults.

```toml
glyphs = "box"          # unicode (default), box, ascii, nerd (section 14)
which_key = 300         # ms keys wait before which-key shows what can follow (section 2); 0 at once
name_width = 30         # the name column's widest, 4-200 (section 4)
value_width = 20        # the value column's widest, 5-200 (section 4)
theme = "light"         # dark, light, or one of the user's (below); default: the one that
                        # suits the terminal's background

[keys]                  # move Normal mode's keys (below)
zoom_in = "i"

[colors]                # change single colours of the theme
cursor = "#e5c07b"
panel = 235
red = "#ff5555"         # the shade of a signal colour (section 14)
```

- `[colors]` keys are the palette's: `line`, `bus`, `fill`, `unknown`, `highz`, `cursor`, `cursor_text`,
  `column`, `cursor_dim`, `mark`, `selected`, `panel`, `hatch`, `dim`, `background`, `text`, and the signal colours `red`, `orange`,
  `yellow`, `green`, `cyan`, `blue`, `magenta`, `gray`. A value is `"#rrggbb"` (the nearest
  256-colour shade on terminals without truecolor), a 256-colour index, `["#rrggbb", index]` (that
  index without truecolor, for colours the nearest shade turns gray) or `"default"` (the
  terminal's own).
- Read once at start; a change shows in the next wtf started.
- A bad file never stops wtf. It starts with what is valid in the file and the defaults for the
  rest, and the status line says what was wrong and what is allowed: `config: unknown key
  colors.curser (keys: line, fill, …)`, `config: glyphs = "utf8"? (unicode, box, ascii, nerd)`. A file
  that isn't TOML is left out whole, with the parse error's line.

### Keys

`[keys]` moves Normal mode's keys (section 3): each entry is an action and its key, written as for
`--keys` (section 11): `"x"`, `"gx"`, `"<C-x>"`, `"<CR>"`, `"<Space>"`, `"<Tab>"`, `"<S-Tab>"`.

```toml
[keys]
zoom_in = "i"
fold = "<Space>"
start = "<C-g>"
```

| Action | Key | Action | Key | Action | Key |
|---|---|---|---|---|---|
| `left` | `h` | `page_up` | `<C-b>` | `search` | `/` |
| `right` | `l` | `zoom_in` | `z` | `next_match` | `n` |
| `next_change` | `w` | `zoom_out` | `x` | `prev_match` | `N` |
| `prev_change` | `b` | `panel` | `a` | `mark` | `m` |
| `down` | `j` | `remove` | `dd` | `unmark` | `M` |
| `up` | `k` | `move_down` | `J` | `next_mark` | `<Tab>` |
| `fold` | `<CR>` | `move_up` | `K` | `prev_mark` | `<S-Tab>` |
| `examine` | `e` | `range` | `V` | `jump` | `'` |
| `start` | `gg` | `radix` | `r` | `jump_back` | `''` |
| `end` | `G` | `undo` | `u` | `command` | `:` |
| `half_right` | `<C-d>` | `redo` | `<C-r>` | `help` | `?` |
| `half_left` | `<C-u>` | `copy_value` | `yy` | `quit` | `q` |
| `screen_left` | `H` | `copy_time` | `yt` | | |
| `screen_right` | `L` | `center` | `c` | `leader` | `<Space>` |
| `page_down` | `<C-f>` | | | | |

- A key is one key or two. `jump` is followed by the cursor's number, as `'2` is.
- An action left out keeps its key. A moved action's old key does nothing, unless another action
  takes it.
- Every key must be typeable: two actions on one key, or one action's key the start of another's
  (`g` and `gg`), is an error naming the action that has it: `config: keys.quit = "x"? x is
  zoom_out's key`. A digit can't start a key (digits are counts), nor can `Esc` (it cancels).
- An unknown action lists the actions. A bad entry is skipped, as any other in the file, and
  the action keeps its key.

### Leader

`[leader]` adds to and changes the leader menu (section 2, "Leader"). An entry is a key and
`["what it does", ":command"]`, with a third string saying what happens if that helps; a table is a group, with `name`; `""` removes the entry on a key.
An entry replaces the one on its key, and a group merges into the group there:

```toml
[leader]
b = ["bus reads", ":pick records bus.rd"]
n = ""                  # no messages in the menu

[leader.a]              # add to the agent group
t = ["steps", ":trail"]
```

The defaults:

```toml
f = ["find a signal", ":pick signals"]
m = ["marks", ":pick marks"]
r = ["records", ":pick records"]
":" = ["commands", ":pick commands"]
n = ["messages", ":messages"]
"?" = ["keys", ":help"]

[v]
name = "view"
g = ["group the selection…", ":group "]
r = ["rename the group…", ":rename "]
u = ["ungroup", ":ungroup"]
c = ["colour…", ":color "]
x = ["radix…", ":radix "]

[t]
name = "time"
f = ["zoom to fit", ":fit"]
z = ["zoom to…", ":zoom "]
m = ["mark at a time…", ":cursor "]

[d]
name = "decode"
d = ["decode…", ":decode "]
p = ["protocol…", ":protocol "]
s = ["computed signal…", ":signal "]
n = ["name values…", ":names "]
r = ["records where…", ":records "]

[a]
name = "agent"
f = ["follow…", ":follow ", "the window shows its workspace as it works"]
F = ["stop following", ":follow off", "your own view back"]
t = ["trail", ":pick trail", "its steps; Enter jumps to one's time"]
j = ["join a session…", ":pick sessions", "the agents working on this machine"]
s = ["the agent's session", ":agent", "what it left while no window was open"]

[q]
name = "session"
w = ["save", ":w", "rows, marks and decodes, beside the trace"]
e = ["reload the trace", ":e", "after the simulation ran again"]
q = ["quit", ":q", "refused with unsaved changes"]
x = ["save and quit", ":wq"]
Q = ["quit without saving", ":q!", "unsaved changes are lost"]
```

- A key is one key, as for `[keys]`, but not `Esc` or `Backspace` (they close and go up).
- A bad entry is skipped and named: `config: leader.x = ["x", "q"]? (["what it does",
  ":command"] and maybe what happens, a table for a group, or "" to remove)`.

### First start

When wtf opens a window in a terminal (stdin and stdout both one; not `--snapshot` or `--mcp`) and there is no config file, it first asks two questions, each with samples
of a clock and a bus drawn as wtf draws them:

1. Which glyph set the terminal draws cleanly: `unicode`, `box`, `ascii` and `nerd`, each a
   card (section 14) titled with its key and name, the sample inside, the card's frame too in
   that set. Fonts without the legacy computing glyphs show boxes in the first; lines that don't
   join up mean box drawing is missing too; `nerd` shows an icon by its name, a box without a
   Nerd Font.
2. Dark or light: a card per theme, the sample in it, in the chosen glyphs. Not asked with
   `NO_COLOR`.

The cards go two a row where each gets 34 columns, one a row else, without blank lines between
rows where they wouldn't all fit; the highlighted one's border is in the accent (`▌` before its
title without colour). The top line says which question it is (`1 of 2`), the bottom the keys.

`1`-`4` (or `j` / `k` and `Enter`) answer; `Esc` takes the defaults (`unicode`, and the theme
that suits the terminal's background, below) for what is left; `Ctrl-c` leaves without writing, so the next start asks again. The answers are written
as `glyphs` and `theme` to the config file, which is made if missing; then the trace opens (or
the start screen, section 4). A
file that can't be written is said on the status line.

`wtf --setup` asks again, setting only those two keys in an existing file: its other keys and its
comments stay. Without a trace it exits after writing.

### Themes

A theme is a TOML file of colours, with the same keys and values as `[colors]`:

```toml
# ~/.config/wtf/themes/gruvbox.toml
background = ["#282828", 235]
text       = ["#ebdbb2", 223]
line       = ["#83a598", 109]
cursor     = ["#fabd2f", 214]
```

- The keys a theme leaves out are `dark`'s, so a theme can be a few lines.
- The built-in themes are files in the repository's `themes/`, compiled into wtf: `dark` sets
  every key, as the base of the others. A new theme is its file there and its name in the list
  in `src/theme.rs`.
- A user's themes are `themes/<name>.toml` next to `config.toml`, picked by `theme = "<name>"`.
  One named as a built-in theme replaces it.
- A bad entry in a theme is skipped and named with the theme (`config: theme gruvbox: unknown
  key lne (keys: …)`); an unknown theme lists the ones there are.

### The terminal's background

`dark` keeps the terminal's own background and text, so its fixed shades behind cards (`panel`),
the selected row (`selected`), the marks' tabs on the ruler (`mark`) and the cursor columns
(`cursor_dim`, `column`) can fall too close to a background they weren't made for. A window (as for the first start) asks the terminal for
its background and text colours once at start (OSC 11 and 10; most terminals and tmux 3.3+
answer). The text colour is what the terminal's own text fades from in a notification (section
4). With the background:

- `dark`'s surfaces are steps from that colour: `panel`, `selected`, `mark` and `cursor_dim`
  0.07, 0.10, 0.12 and 0.14 lighter in OKLab lightness on a dark background, darker on a light
  one, at least 0.21 from black; `column` a quarter of the way toward the accent's yellow. A theme or `[colors]`
  that sets them still wins, and `light`, which sets its own background, is left as it is.
- Without a `theme` in the file, a light background gets `light`; the first start selects it.
- A terminal that doesn't answer, `NO_COLOR`, `--snapshot` and `--mcp` keep the fixed shades, so
  what they draw doesn't depend on the terminal.

### Precedence

- `--glyphs` over `glyphs` in the file, over `unicode`.
- `NO_COLOR` over the theme and `[colors]` (section 14).
- `--snapshot` follows the glyphs too, so a snapshot can be ASCII, and with `--ansi` the theme and
  `NO_COLOR`; plain text keeps the default theme. `wtf --mcp` reads no config:
  what agents get doesn't depend on the machine.
