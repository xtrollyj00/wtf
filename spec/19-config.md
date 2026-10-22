## 19. Configuration

How wtf looks on this machine, kept apart from sessions: a session is what to show of a trace and
travels with it; the config file is the user's terminal and taste, and stays on the machine.

### The file

`$XDG_CONFIG_HOME/wtf/config.toml`, or `~/.config/wtf/config.toml` without `XDG_CONFIG_HOME`. It is
optional; without it wtf uses the defaults.

```toml
glyphs = "box"          # unicode (default), box, ascii (section 14)
theme = "light"         # dark (default), light

[colors]                # change single colours of the theme
cursor = "#e5c07b"
grid = 236
red = "#ff5555"         # the shade of a signal colour (section 14)
```

- `[colors]` keys are the palette's: `line`, `fill`, `unknown`, `highz`, `cursor`, `cursor_text`,
  `cursor_dim`, `selected`, `hatch`, `dim`, `grid`, `background`, `text`, and the signal colours `red`, `orange`,
  `yellow`, `green`, `cyan`, `blue`, `magenta`, `gray`. A value is `"#rrggbb"` or a 256-colour
  index.
- Read once at start; a change shows in the next wtf started.
- A bad file never stops wtf. It starts with what is valid in the file and the defaults for the
  rest, and the status line says what was wrong and what is allowed: `config: unknown key
  colors.curser (keys: line, fill, …)`, `config: glyphs = "utf8"? (unicode, box, ascii)`. A file
  that isn't TOML is left out whole, with the parse error's line.

### Precedence

- `--glyphs` over `glyphs` in the file, over `unicode`.
- `NO_COLOR` over the theme and `[colors]` (section 14).
- `--snapshot` follows the glyphs too, so a snapshot can be ASCII. `wtf --mcp` reads no config:
  what agents get doesn't depend on the machine.
