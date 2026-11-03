## 4. Screen layout

wtf is started like Vim: `wtf <path to file>`. Without a path it shows the start screen.

### Start screen

`wtf` without a trace shows, as one block centred on the screen:

```
                                                            ┃
                                                ▄▄          ┃         ▄▄▄▄▄▄
                                               ▀▀▀▄▄        ┃       ▄▀▀▀▀▀▀▀▄▄
                      ▀▀▀         ▀▀▀      ▀▀▀▀▀▀▀▀▀▀▀▀     ┃   ▀▀▀▀▀▀▀▀▀▀▀▀
                       …                    (the logo, 11 lines)
                      ──────────────────🮦───────────────────🮦━━━━━━━━━━━━━━━
                      ────── 0x77 ──────🮧────── 0x74 ───────🮧━━━━ 0xXX ━━━━━

                       Open a trace                                         o
                       Join 4123-1 on axilite.vcd                           a
                       Keys                                                 ?
                       Quit                                                 q

                       Recent                last 7 days
                       axilite.vcd           ·············━    now          1
                       axilite.vcd bug.toml  ···········━··    1d ago       2
                       eth_parser.fst        ···━···━·━····    2d ago       3

                                wtf 0.2.0   1 agent on this machine
```

- **The logo** is a trace at the wtf moment: `w t f` in a pixel font drawn with `▀` / `▄` (two
  pixels a cell), the cursor's column in the gap between `t` and `f`, the letters before it in
  the accent and `f` after it in the X colour (`unknown`), with a shadow (`cursor_dim`) two pixels
  right and down. Under it a bus carries the letters' codes, `0x77`, `0x74`, then `0xXX` drawn as
  an X. In `ascii`, without colour and in a plain `--snapshot` the logo is plain characters
  (`#`, `"` for an upper half, `,` for a lower one) without the shadow.
- **Actions**, a column with the key at its right: `o` open a trace (its path is typed on the
  footer's line, `Enter` opens, `Esc` cancels), `a` join an agent's workspace (shown only when
  an agent has one open: with one, its id and trace; with several, `a` lists them, numbered, in
  Recent's place), `?` the keys (in Recent's place; `?` or `Esc` back), `q` (or `Ctrl-c`) quit.
- **Recent**: the traces opened on this machine (below), the latest first, by file name, each with the session
  it was opened with (`-S`) if not its own; `1`-`9` open one. A strip shows the last 7 days,
  half a day a cell, oldest on the left: `━` where the trace was opened, in the bus colour by
  the user and in cyan by an agent (the latest open in the half day decides; without colour an
  agent's is `─`); then how long ago it was last opened. Traces that are gone are left out.
- **Joining** opens the agent workspace's trace and follows it (`:follow <id>`, section 15). The
  agents are looked up again after a second without a key.
- **The footer**: the version, and how many agents (`wtf --mcp` servers) have a workspace open on
  this machine. A trace that doesn't open says why there, and the start screen stays.
- **Sizes**: the block is centred; Recent takes the lines left, up to 9. Under 26 lines, or 58
  columns, the logo is one line, `w t ┃ f`, without the bus.
- Nerd Font icons go before the actions and Recent's traces with the `nerd` glyphs only (section
  14).

The history is `$XDG_STATE_HOME/wtf/history` (`~/.local/state/wtf/history` without it): one line
per trace a window opens or an agent's `open` (section 15), `<unix seconds>\t<you|agent>\t<trace>`
then `\t<session>` for a `-S`, absolute paths. The newest 1000 lines are kept. A `--snapshot`
isn't an open; one that can't be written changes nothing else.

### At launch

No signals are shown yet. The screen is empty except for a short guide in the middle: a small
wordmark in the accent colour, the file's name in bold, what is in it (dim), and the keys to
start with, in the accent.

```
                             w t f
                   waveform terminal frontend

                           trace.fst
                    42 signals  ·  0ns–1.2us

                         a  add signals
                         :  commands
                         ?  help
                         q  quit

 NORMAL   signals ▸                              0ns  2ns/col  trace.fst
```

The guide disappears once a signal is added, and comes back if all signals are removed. A click
anywhere on it opens the signal panel, as `a` does.
With no rows, row keys (`j`, `k`, `w`, `b`, `/`, `n`, `N`, `Enter`) do nothing; time keys still move
the cursor.

### Normal mode

Only the waveforms, a name column, a value column and the status line. No panels, no borders;
a dim line between the columns, as an editor's splits.

```
Name  │ Value │ 60ns      80ns      100ns     120ns
      │       │ 1─────── 40ns ──────3── 20ns ─2
      │       │    ┌───┐   ┌───┐   ┌───┐   ┌───┐   ┌───
clk   │ 1     │ ───┘   └───┘   └───┘   └───┘   └───┘
      │       │ ────────🮦──────────────🮦────────────────
state │ 0x4   │ ─ 0x1 ──🮧──── 0x2 ─────🮧──── 0x4 ──────
      │       │ ────────🮦───────🮦──────🮦────────🮦──────
data  │ 0x3A  │ ─ 0x43 ─🮧 0x96 ─🮧 0xE1 🮧── 0x3A ─🮧 0x7F
 NORMAL   signals ▸  cpu.state              120ns  2ns/col  trace.fst
```

- **Rows**: each signal takes two terminal lines (section 14). The name and value sit on the lower line.
- **Name column**: the leaf name of each signal (`clk`, `state`); inside a struct, record or array,
  its whole name in the HDL (`pkts[1].pair.lo`, `mem[3]`, not `lo`, `[3]`; a VCD from Verilator
  writes a struct as a module, so there it is the leaf). If two shown signals share a name, enough
  of their scope is added to tell them apart (`cpu.state`, `dma.state`). That scope
  is greyed, so the leaf names stand out.
  With the `nerd` glyphs an icon before the name (after a `▸`) tells what the row is (section 14, "Icons").
  It is as wide as the longest name, up to a limit. The selected signal's name is bold in the accent colour,
  and both of its lines get a background across the whole width.
- **Time axis**: the top line shows times at round steps above the waveforms, and the cursor's
  time in the accent colour at its column (kept on screen at the right edge); labels it would
  touch are left out. No grid runs down from them: in the gaps of the waveforms it read as specks.
- **Columns**: the names, the values and the waveforms, a dim `│` between them (section 14's
  glyphs) from the time axis down, with a space on each side. On the time axis' line, dim, the headers `Name`
  and `Value`; the columns are at least as wide as them.
- **Value column**: each signal's value at the cursor, so all values can be read at once
  even where a segment is too narrow for its label. Empty for group headers.
- **Cursor ruler**: a line above the waveforms, shown only when there is more than one cursor (section 9).
- **Cursor and marks**: columns down to the last row. The cursor's column is tinted warm, so the
  waveform under it stays as it is; the marks' columns are a fainter, neutral tint. Without colour
  the waveform stays too, and the column's blank cells show `┃` (marks `╎`).
- **Status line** (bottom), in segments, each on its own background:
  - on the left the mode in a badge on the accent (`NORMAL`, `SIGNALS`, `PICK`), the keys typed
    so far (`5`, `Space a`), then a button, `signals ▸` (`signals ◂` while the signal panel is
    open), that opens and closes the panel with a click, as `a` does;
  - in the middle, dim: the mark at the cursor (its name or note), else the record of a stream
    selected there (section 14), else the full path of the selected signal; a message only when
    no card shows it (below, "Notifications");
  - on the right: `following 41-2` when following an agent (section 15), the cursor's time in
    the accent, the zoom (time per column), `unit ns` if an agent set the unit of its answers
    (section 15), and the file name, with `[+]` in the accent if the session has unsaved changes.
  When the line is too narrow, the segments least needed go first: the unit, the zoom, the
  following, the file name. The visible time range is on the time axis. With the `nerd` glyphs
  (section 14) the middle, the time, the following and the file name get an icon.

### Signal panel

To add signals, `a` or a click on the status line's `signals ▸` opens a signal panel on the left;
the rows and their waveforms stay visible on the right, with a line marking where new signals go.
`a`, the button or `Esc` closes the panel again.

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
 SIGNALS  signals ◂  cpu.state   0ns  2ns/col  trace.fst
```

See section 5 for the keys.

### Notifications

What wtf says (an error, `copied: 0x96`, an agent joining, each of its steps) shows in a card in
the top right, so several stack and none is missed:

```
                                    ╭─ error ──────────────────────────────╮
                                    │ bad time: nope                       │
                                    ╰──────────────────────────────────────╯
                                    ╭─ 41-2 ──────────────────────── find ─╮
                                    │ @clk busy                            │
                                    │ 3 matches, the first at 20ns         │
                                    ╰──────────────────────────────────────╯
```

- The newest on top, up to three, two fifths of the width (24 to 49 columns), the text in up to
  three lines, the last cut with `…`.
- An error's border is red with `error` in it. An agent's step is cyan, with the agent's id as
  the title and the tool at the right: what it asked, then its answer in green. The user's own
  is dim; a message `word: rest` (`copied: 0x96`) has `word` as its title.
- A card shows 4 seconds, an error 8, and fades (all of it dim) over its last second. `Esc` in
  Normal mode clears them.
- They hide under a box, a menu, the leader menu and the picker, which have the keys. When no
  card can show (under a box, or a screen too small), the status line shows the message instead,
  red for an error.
- What a drag's status line says while it moves (section 2, "Drags") is not a message.
- `:messages` opens a panel across the bottom with every message (the last 200), the newest
  highlighted, each with the clock time it came and who caused it, `you` or the agent's id:
  `14:03:05  you      unknown command: bad`. `j` / `k` (or the arrows, the wheel) move,
  `g` / `G` go to the first / last, `y` copies the message, `Enter` on an agent's step opens its
  trail (`:pick trail`, section 15), `Esc` or `q` (or a click outside) closes it.

### Command line

`:` opens the command line at the bottom, in place of the status line, as in Vim, with a badge
before it (`CMD`; `SEARCH` for `/`) where the screen is 40 columns or wider.
Above it, a card lists what can be typed there: titled with what it lists (`command`,
`protocol`, `signal`), at the word being completed, each item with a short description dim after
it, the first at the top and highlighted, up to ten, scrolled to the highlight; `Tab accept` in
its bottom border. With the `nerd` glyphs each item has an icon: a command's own, a protocol's
plug, a check for an interface found in the trace.

```
         ╭─ protocol ──────────────────────────────────╮╭─ axilite  top.s_axi_ ───────────────╮
         │▌✓ axilite  top.s_axi_  found here: clock=…  ││ This trace: clock=top.aclk prefix=…  │
         │   axis                 AXI4-Stream: beats…  ││                                      │
         │   axilite              AXI4-Lite: write a…  ││ decode <name> axilite clock=<clk>    │
         │   axi4                 AXI4: write and re…  ││ ...                                  │
         ╰─────────────────────────────── Tab accept ─╯╰──────────────────────────────────────╯
 CMD  :decode bus axilite clock=top.aclk prefix=top.s_axi_
```

- Typing filters the list by fuzzy match on the command name. Once a command and a space are
  typed, the list completes its argument at the end of the line instead:

  | Command | Completes |
  |---|---|
  | `:add`, `:rm` | signal paths |
  | `:signal <name> =`, `:names` | signal paths |
  | `:protocol <name>` | `clock=`, `when=`, `roles=`; after `clock=` or a field's `=` a signal path, and in `when=`'s expression the signal at the end, after any `!` or `(` |
  | `:radix`, `:color`, `:help` | radix names, colours, topics |
  | `:records` | streams, then `where`, then in its expression the stream's fields |
  | `:pick` | sources, and after `records` the streams |
  | `:scope` | scopes |
  | `:select` | the rows' names |
  | `:cursor` | marks 1-9, each with its time and name; then `off`, `name=`, `note=` |
  | `:fit` | the marks put, as `'2` |
  | `:follow` | the agents on this trace, and `off` |
  | `:w` | session files (`.toml`) and directories |
  | `:decode <name>` | protocols, each with what it makes; first the interfaces found in the trace (section 15, `info`), each with its whole line (`axilite  top.s_axi_  found here: clock=top.aclk prefix=top.s_axi_`) |
  | `:decode <name> <protocol>` | its roles and params, as `role=`; after `prefix=` the prefixes found, after another role a signal path |

- `↓` / `Ctrl-n` move the highlight down to the next item, `↑` / `Ctrl-p` up, `Tab` puts the highlighted item in place of
  the word being typed. On an empty line they go through earlier commands instead.
- **Ghost text**: with the cursor at the end of the line, what the highlighted item would add
  shows dim after it, then what is still to type: `:zo` shows `om <time>`, `:decode bus ax`
  shows `ilite clock=<clk> prefix=<prefix>`.
- **Docs**: a card beside the list, where the screen leaves it 40 columns, says more about the
  highlighted item, briefly: for a protocol its `decode` line, what it makes, its roles and
  streams, then `Tab takes it.  :help decode has the rest.` dim; for an interface found in the
  trace its arguments first, in green (`This trace: clock=top.aclk prefix=top.s_axi_`); for a
  command its description where the list cuts it.
- Editing works as on Vim's command line: `←` / `→` move the cursor, `Home` / `End` or `Ctrl-b` / `Ctrl-e`
  go to the start / end, `Backspace` / `Delete` remove the character before / under the cursor,
  `Ctrl-w` the word before it and `Ctrl-u` everything before it. The value search line (`/`) edits the same way.
- `Enter` runs what is typed. `Esc` closes the command line.
- The mouse: a click on a command puts it on the line as `Tab`, a double click runs it (section 2,
  "Mouse").
- An unknown command or a bad argument shows an error (above, "Notifications").
