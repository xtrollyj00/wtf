## 14. Display rules

### Time

- The file's timescale (VCD `$timescale`, FST header) converts ticks into real time.
- Times are shown in the largest unit that keeps the value at 1 or more, with up to 3 decimals
  and trailing zeros dropped: `12.5ns`, `1.234us`, `0ps`. Files without a timescale show raw ticks.
- Answers to agents follow their own rule, a time unit per workspace (section 15).

### Zoom

- Zoom is time per column, changed by a factor of 2 per step.
- Below one tick per column, zoom continues as columns per tick, up to 1024.
- Zooming out stops once the whole simulation fits the waveforms' width, as `:fit` shows it
  (at the step that first holds it all).
- Zoom keeps the cursor on the same time. Near time 0 the view can't scroll further left,
  so the cursor moves on screen instead.

### Waveforms

- Each signal takes two terminal lines, so its drawing can connect.
- 1-bit signals are drawn as a high line on the upper line and a low line on the lower one,
  joined at each edge by corners (`┌┐└┘`). `X` and `Z` draw both lines, labelled where they fit (section "Colours").
- Buses are drawn as hexagon outlines (`─` on both lines, as thin as 1-bit lines) with the value
  centred in the lower edge. Neighbouring values meet in a
  one-column crossing (`🮦` over `🮧`, Unicode 13 legacy computing glyphs that end mid-cell).
- A column that can't show one value is drawn as busy: a 1-bit signal is filled between its high and
  low lines, a bus shows crossings (`╳`). A column is busy when it holds two or more transitions,
  or it's part of a run of segments narrower than two columns. A single short pulse between long
  segments is still drawn normally.
- Bus labels are shown whole if they fit with a spare column on each side, not counting the crossings;
  otherwise cut with `…` if three characters of them still fit (`0x4…`, `a0 00 …`), and if not only
  the outline is drawn.
- A stream (section 16) draws a segment per record, labelled `#3 64B`, and nothing
  between records, nor do its fields (whose value there is `z`, section 15), in the value column too; a segment opens with `🮣` over `🮡` and closes with `🮢` over `🮠`. Violations are red. A stream row's name has `▸` / `▾` before it, as a group's, since `Space` folds and unfolds its fields.
  With a stream selected, the status line shows its record at the cursor (`rx.frame #2 65ns–85ns
  beats=0x2 len=0x8 data=c0 00 …`).
- `X`, `U` and `x` are drawn red (red outline for buses), `Z` and `z` green, and each with a line
  of its own, so the shape tells them from a value and from each other where colour can't (a
  colour-blind eye, `NO_COLOR`) and a stretch is too short for its label: X heavy (`━`), Z dotted
  (`┄`).
- The glyphs above are the `unicode` set; the others are below.

### Glyphs

Three glyph sets, chosen with `glyphs` in the config file (section 19) or `--glyphs` (section 12):

- `unicode` (the default): everything above, including the Unicode 13 legacy computing glyphs
  whose lines end mid-cell, so crossings join the outlines.
- `box`: box drawing and block elements only, which nearly every font has. For terminals and fonts
  that show the legacy computing glyphs as boxes or question marks.
- `ascii`: 7-bit only, for plain terminals, logs and pasting a snapshot into a mail or an issue.
  Every cell is one ASCII character.

Each set keeps every distinction the others make, so a drawing reads the same in any of them.

| Drawn | `unicode` | `box` | `ascii` |
|---|---|---|---|
| line (1-bit high / low, bus outline) | `─` | `─` | `-` |
| the line of an X, of a Z | `━`, `┄` | as `unicode` | `=`, `.` |
| 1-bit edge, upper / lower line | `┌┐┬╷` / `└┘┴╵` | as `unicode` | `+` / `+` |
| bus crossing, upper / lower | `🮦` / `🮧` | `┬` / `┴` | `+` / `+` |
| segment opens, upper / lower | `🮣` / `🮡` | `┌` / `└` | `+` / `+` |
| segment closes, upper / lower | `🮢` / `🮠` | `┐` / `┘` | `+` / `+` |
| busy 1-bit, a collapsed group's activity | `▄` over `▀` | as `unicode` | `#` over `#` |
| busy bus | `╳` | `╳` | `#` |
| label cut | `…` | `…` | `~` |
| cursor, marks (where there is no colour) | `┃`, `╎` | as `unicode` | `\|`, `:` |
| cursor off screen | `◂2`, `2▸` | as `unicode` | `<2`, `2>` |
| group open / closed, signal shown (panel) | `▾`, `▸`, `●` | as `unicode` | `v`, `>`, `*` |
| highlighted completion, insertion point | `▌`, `▸` | as `unicode` | `>`, `>` |
| panel and help borders (without colour) | `┌─┐│└┘` | as `unicode` | `+-+\|++` |
| in text (`help · keys`, `65ns–85ns`) | `·`, `–` | as `unicode` | `-`, `-` |

- In `ascii`, an edge and a bus crossing are both `+` on both lines: a 1-bit signal has one line
  on each side of it, a bus two, so they don't mix up. `X` is never a glyph, only a value.
- Busy columns (`####`) and a stream's labels (`#3 64B`) both use `#`; a label always has a
  number after it.

### Colours

- 1-bit lines and bus outlines have hues of their own (`line`, `bus`), and a bus's labels are
  in the text's colour, so the values read first.
- The palette (lines, bus outlines, busy fill, X, Z, cursor, marks, selection, panels, dimmed text) comes from a
  theme: `dark` (the default) or `light`. `dark` keeps the terminal's background and text; `light`
  paints its own over the whole screen, whatever the terminal's are. Single colours can be changed
  in the config file (section 19), `background` and `text` too.
- Colours are given as `#rrggbb` or a 256-colour index. On a terminal without truecolor (section
  4's detection), an `#rrggbb` colour is drawn as the nearest of the 256.
- **A signal's colour**: a row can have its own colour, from a fixed set of names: `red`,
  `orange`, `yellow`, `green`, `cyan`, `blue`, `magenta`, `gray`. The theme gives each name its
  shade, so a colour suits dark and light terminals alike. It colours the row's line or outline
  and its labels; X, Z and violations keep their own colours, and the name column keeps its own.
  Set with `:color` (section 7), saved in the session (section 11).
- **Boxes over the waveforms** (the signal panel, the `?` and `e` boxes, records, the command
  completions) are cards: a background of their own (`panel`), the title in bold with how to
  close it dim across from it, two columns cleared on each side so no stub of a waveform is left
  beside a card, and no border. Without colour they get a border instead.
- **Without colour** (`NO_COLOR` set to anything but empty, as no-color.org says): the cursor's
  column is reversed, marks are drawn with their glyph where a cell is empty, the selected row's
  name is reversed. Snapshots and the agents' `render` have no colours either.
- A 1-bit signal's X and Z stretches are labelled like a bus value (`━━━ X ━━━`) where they fit,
  and drawn on both lines, each with its own line (above).

### Values

- Shown in the row's radix (section 10), hex by default.
- Leading zero digits are dropped, keeping at least one digit: `0x00004000` → `0x4000`.
