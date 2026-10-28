## 19. Configuration

How wtf looks on this machine, kept apart from sessions: a session is what to show of a trace and
travels with it; the config file is the user's terminal and taste, and stays on the machine.

### The file

`$XDG_CONFIG_HOME/wtf/config.toml`, or `~/.config/wtf/config.toml` without `XDG_CONFIG_HOME`. It is
optional; without it wtf uses the defaults.

```toml
glyphs = "box"          # unicode (default), box, ascii (section 14)
theme = "light"         # dark (default), light, or one of the user's (below)

[colors]                # change single colours of the theme
cursor = "#e5c07b"
panel = 235
red = "#ff5555"         # the shade of a signal colour (section 14)
```

- `[colors]` keys are the palette's: `line`, `fill`, `unknown`, `highz`, `cursor`, `cursor_text`,
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

### Precedence

- `--glyphs` over `glyphs` in the file, over `unicode`.
- `NO_COLOR` over the theme and `[colors]` (section 14).
- `--snapshot` follows the glyphs too, so a snapshot can be ASCII, and with `--ansi` the theme and
  `NO_COLOR`; plain text keeps the default theme. `wtf --mcp` reads no config:
  what agents get doesn't depend on the machine.
