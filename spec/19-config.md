## 19. Configuration

How wtf looks on this machine, kept apart from sessions: a session is what to show of a trace and
travels with it; the config file is the user's terminal and taste, and stays on the machine.

### The file

`$XDG_CONFIG_HOME/wtf/config.toml`, or `~/.config/wtf/config.toml` without `XDG_CONFIG_HOME`. It is
optional; without it wtf uses the defaults.

```toml
glyphs = "box"          # unicode (default), box, ascii (section 14)
theme = "light"         # dark, light, or one of the user's (below); default: the one that
                        # suits the terminal's background

[colors]                # change single colours of the theme
cursor = "#e5c07b"
panel = 235
red = "#ff5555"         # the shade of a signal colour (section 14)
```

- `[colors]` keys are the palette's: `line`, `bus`, `fill`, `unknown`, `highz`, `cursor`, `cursor_text`,
  `column`, `cursor_dim`, `selected`, `panel`, `hatch`, `dim`, `background`, `text`, and the signal colours `red`, `orange`,
  `yellow`, `green`, `cyan`, `blue`, `magenta`, `gray`. A value is `"#rrggbb"` (the nearest
  256-colour shade on terminals without truecolor), a 256-colour index, `["#rrggbb", index]` (that
  index without truecolor, for colours the nearest shade turns gray) or `"default"` (the
  terminal's own).
- Read once at start; a change shows in the next wtf started.
- A bad file never stops wtf. It starts with what is valid in the file and the defaults for the
  rest, and the status line says what was wrong and what is allowed: `config: unknown key
  colors.curser (keys: line, fill, …)`, `config: glyphs = "utf8"? (unicode, box, ascii)`. A file
  that isn't TOML is left out whole, with the parse error's line.

### First start

When wtf opens a window in a terminal (stdin and stdout both one; not `--snapshot`, `--mcp` or
`--attach` listing) and there is no config file, it first asks two questions, each with samples
of a clock and a bus drawn as wtf draws them:

1. Which glyph set the terminal draws cleanly: `unicode`, `box` and `ascii` side by side. Fonts
   without the legacy computing glyphs show boxes in the first; lines that don't join up mean
   box drawing is missing too.
2. Dark or light: the samples in each theme, in the chosen glyphs. Not asked with `NO_COLOR`.

`1`-`3` (or `j` / `k` and `Enter`) answer; `Esc` takes the defaults (`unicode`, and the theme
that suits the terminal's background, below) for what is left; `Ctrl-c` leaves without writing, so the next start asks again. The answers are written
as `glyphs` and `theme` to the config file, which is made if missing; then the trace opens. A
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
the selected row (`selected`) and the cursor columns (`cursor_dim`, `column`) can fall too close
to a background they weren't made for. A window (as for the first start) asks the terminal for
its background colour once at start (OSC 11; most terminals and tmux 3.3+ answer):

- `dark`'s surfaces are steps from that colour: `panel`, `selected` and `cursor_dim` 0.07, 0.10
  and 0.14 lighter in OKLab lightness on a dark background, darker on a light one, at least 0.21
  from black; `column` a quarter of the way toward the accent's yellow. A theme or `[colors]`
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
