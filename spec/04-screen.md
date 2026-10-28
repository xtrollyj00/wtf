## 4. Screen layout

wtf is started like Vim: `wtf <path to file>`. The file path is required; without it
wtf prints usage and exits.

### At launch

No signals are shown yet. The screen is empty except for a short guide in the middle:

```
                                                              
                            trace.fst                         
                                                              
                    i      add signals                        
                    :      commands                           
                    ?      help                               
                    q      quit                               
                                                              
 VIEW  trace.fst                                      0ns     
```

The guide disappears once a signal is added, and comes back if all signals are removed.
With no rows, row keys (`j`, `k`, `w`, `b`, `/`, `n`, `N`, `Space`) do nothing; time keys still move
the cursor.

### View mode

Only the waveforms, a name column, a value column and the status line. No panels, no borders,
no lines between the columns.

```
             60ns      80ns      100ns     120ns
             1─────── 40ns ──────3── 20ns ─2
                ┌───┐   ┌───┐   ┌───┐   ┌───┐   ┌───
clk    1     ───┘   └───┘   └───┘   └───┘   └───┘
             ────────🮦──────────────🮦────────────────
state  0x4   ─ 0x1 ──🮧──── 0x2 ─────🮧──── 0x4 ──────
             ────────🮦───────🮦──────🮦────────🮦──────
data   0x3A  ─ 0x43 ─🮧 0x96 ─🮧 0xE1 🮧── 0x3A ─🮧 0x7F
 VIEW  cpu.state                                   120ns   2ns/col   trace.fst
```

- **Rows**: each signal takes two terminal lines (section 14). The name and value sit on the lower line.
- **Name column**: the leaf name of each signal (`clk`, `state`). If two shown signals share a leaf
  name, enough of their scope is added to tell them apart (`cpu.state`, `dma.state`). That scope
  is greyed, so the leaf names stand out.
  It is as wide as the longest name, up to a limit. The selected signal's name is bold in the accent colour,
  and both of its lines get a background across the whole width.
- **Time axis**: the top line shows times at round steps above the waveforms, and the cursor's
  time in the accent colour at its column (kept on screen at the right edge); labels it would
  touch are left out. No grid runs down from them: in the gaps of the waveforms it read as specks.
- **Columns**: the names, the values and the waveforms, two spaces apart.
- **Value column**: each signal's value at the cursor, so all values can be read at once
  even where a segment is too narrow for its label. Empty for group headers.
- **Cursor ruler**: a line above the waveforms, shown only when there is more than one cursor (section 9).
- **Cursor and marks**: columns down to the last row. The cursor's column is tinted warm, so the
  waveform under it stays as it is; the marks' columns are a fainter, neutral tint. Without colour
  the waveform stays too, and the column's blank cells show `┃` (marks `╎`).
- **Status line** (bottom): on the left the mode (`VIEW` reversed, `EDIT` in the accent colour, so
  it shows when keys change the layout) and the full path of the selected signal (or a
  message); on the right the cursor's time, the zoom (time per column) and the file
  name with `[+]` if the session has unsaved changes. The visible time range is on the time axis.

### Edit mode

Edit mode edits the rows in place. To add signals, `a` (or `/`) opens a signal panel on the left;
the rows and their waveforms stay visible on the right, with a line marking where new signals go.
`Esc` closes the panel again.

```
┌ signals ───────────┐                ┌───┐   ┌───┐   ┌───
│ / _                │clk    0     ───┘   └───┘   └───┘
│ ▾ top              │             ───────🮦──────────────
│   ▾ cpu            │state  0x1   ─ 0x1 ─🮧─── 0x2 ──────
│     ● clk          │▸ new signals go here ───────────────
│     ● state        │
│       busy         │
│   ▸ stream         │
└────────────────────┘
 EDIT  cpu.state                     0ns   2ns/col   trace.fst
```

See section 5 for the keys.

### Command line

`:` opens the command line at the bottom, in place of the status line, as in Vim.
Above it, a list of matching commands grows upward, each with a one-line description, as in Zed.

```
clk      ▁▁▁▁▔▔▔▔▁▁▁▁▔▔▔▔
   :<time>          Move the cursor to a time, e.g. :2000ns
 ▌ :q               Quit
:q_
```

- Typing filters the list by fuzzy match on the command name.
- `↑` / `↓` or `Ctrl-p` / `Ctrl-n` move the highlight, `Tab` completes the highlighted command.
  On an empty line they go through earlier commands instead.
- Editing works as on Vim's command line: `←` / `→` move the cursor, `Home` / `End` or `Ctrl-b` / `Ctrl-e`
  go to the start / end, `Backspace` / `Delete` remove the character before / under the cursor,
  `Ctrl-w` the word before it and `Ctrl-u` everything before it. The value search line (`/`) edits the same way.
- `Enter` runs what is typed. `Esc` closes the command line.
- An unknown command or a bad argument shows an error on the status line.
