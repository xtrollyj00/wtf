## 2. Modes

| Mode | Purpose | Entered by | Left by |
|---|---|---|---|
| Normal | browse time and signals, and arrange the rows; the default | start | — |
| Command | type a `:` command | `:` | `Enter` (runs), `Esc` (cancels) |
| Search | type a value to search for | `/` | `Enter` (jumps), `Esc` (cancels) |

- Command and Search return to Normal mode.
- There is one mode for keys: time keys, row keys and the keys that change the rows (`dd`,
  `J` / `K`, `r`, `V`, `u`) all work at once. Their keys don't overlap, and `u` takes back any
  change to the rows, so a mode to guard them would only cost round trips.
- Every key that changes the rows is also a `:` command (section 7); the keys are shortcuts.
- The signal panel (section 5) is no mode: `a` or the status line's `signals ▸` opens and closes
  it. It has the keys while open; `Esc` also closes it. Searching in it is a filter inside the
  panel, not Search mode.
- The mouse (below) needs no mode and changes none.
- The current mode is always shown on screen: the command or search line; Normal mode by nothing.

## 3. Normal mode keys

| Keys | Action |
|---|---|
| `h` / `l` | move the time cursor one column left / right |
| `w` / `b` | next / previous transition of the selected signal |
| `j` / `k` | select the next / previous signal |
| `Space` | collapse / expand the selected group (section 6), or expand a bus into its bits, a stream into its fields, or a struct, record or array one level into its fields or elements (section 6, "Composite rows") / fold them |
| `e` | examine: a box with the selected signal's whole value at the cursor; on a stream, its record there (times and every field, a frame's `data` 16 bytes a line); on a struct, record or array, without opening it, one line per field, nested fields indented, an array's elements by index (`pkt`: `valid 1`, `state BUSY`, `pair`, `  hi 0xA`, `  lo 0x5`; `pkts`: `[0]` and `[1]`, each with its fields); `Esc`, `q` or `e` closes it. A box longer than the screen takes its whole width and scrolls: `j` / `k`, `Ctrl-d` / `Ctrl-u`, `g` / `G`, its last line saying which lines show |
| `gg` / `G` | time cursor to the start / end of the simulation |
| `Ctrl-d` / `Ctrl-u` | move the screen half its width right / left, the cursor the same distance (at the start or end of the simulation the cursor still moves) |
| `H` / `L` | move the screen half its width left / right; the cursor keeps its time, even off screen, and the next cursor move scrolls back to it |
| `c` | center the screen on the cursor |
| `Ctrl-f` / `Ctrl-b` | select the signal a screen of rows down / up |
| `z` / `x` | zoom in / out by a factor of 2 around the cursor |
| `a` | open / close the signal panel, to add signals below the selected row (section 5) |
| `dd` | remove the selected row; on a group header the group and its signals; with a `V` range, its rows |
| `J` / `K` | move the selected row (on a header, the whole group) down / up; next to a group's edge, into or out of it |
| `V` | start / end a range of rows: `j` / `k` extend it, then `dd`, `r`, `:group <name>`, `:radix`, `:color` take it |
| `r` | cycle the radix of the selected line (a row, or a field under it), or of all signals in a group or `V` range (section 10) |
| `u` / `Ctrl-r` | undo / redo a change to the rows (section 5, "Undo") |
| `Esc` | end a `V` range |
| `yy` | copy the selected signal's value at the cursor, as the value column shows it in its radix; on a stream's row, its record there. Copied to the terminal's clipboard with OSC 52 (a terminal that doesn't support it ignores it; over ssh it reaches the local clipboard); the status line shows `copied: 0x96` |
| `yt` | copy the cursor's time, e.g. `25ns` |
| `/` | search for a value on the selected signal (section 8) |
| `n` / `N` | next / previous match |
| `m` / `Nm` | put a mark at the cursor, numbered `N` or the lowest free (section 9) |
| `Tab` / `Shift-Tab` | jump to the next / previous mark in time |
| `'1`…`'9` | jump to that cursor: `'1` centers the screen on the cursor, `'2`…`'9` jump to that mark |
| `''` | back to where the cursor was before the last jump |
| `M` / `NM` | remove the mark under the cursor, or mark `N` |
| `:` | command |
| `?` | help: a box with the keys (of the signal panel while it is open), in the accent colour under dim headings (Move, Rows, Marks...), in as many columns as fit, and below them a few things to type (searches, commands), each with what it does; `Esc`, `q` or `?` closes it; it scrolls as the `e` box does |
| `q` | quit, same as `:q` |

A count before a key repeats it, as in Vim: `5l`, `3w`, `3x`. Counts apply to `h` `l` `w` `b`
`j` `k` `J` `K` `z` `x` `n` `N` `Ctrl-d` `Ctrl-u` `H` `L` `Ctrl-f` `Ctrl-b`.
Other keys ignore a count.
A bare `0` does nothing, and so do other `Ctrl` keys.

Multi-key sequences (`gg`, `dd`, `'1`) wait for their second key without a timeout.
`Esc` or an invalid second key cancels them.

`z` / `x` sit under the left hand, so one hand zooms while the other stays on `hjkl`.

### Mouse

The keys do everything; the mouse is for what GTKWave and Surfer users reach for first, and
enough on its own to open a trace, add signals and look at them. On the rows:

| Mouse | Action |
|---|---|
| click on a row | select it; on its waveform, also put the cursor at that column's time |
| click on a `▸` / `▾` marker | select the row and open / fold it, as `Space` (a group, a stream, a struct, record or array, each level) |
| click on the selected row's name | open / fold it, as `Space`: also a bus into its bits, and a bit line folds its bus |
| click on the time axis | put the cursor there |
| wheel | move the selection down / up, as `j` / `k` |
| `Ctrl` + wheel | zoom out / in, as `x` / `z` |
| `Shift` + wheel, or sideways | scroll the time by an eighth of the screen, the cursor staying |
| right click on a row | select it (inside a `V` range, keep the range) and open its menu at the pointer |
| click below the rows, with no rows yet | open the signal panel, as `a` |
| click on the status line's `signals ▸` / `signals ◂` | open / close the signal panel, as `a` |
| right click below the rows | a menu: `Add signals… a`, `Undo u`, `Redo Ctrl-r` |

Over the rows, what is on top takes the mouse, and a click outside it closes it, as `Esc` does:

| Over the rows | Mouse |
|---|---|
| the signal panel | as section 5, "Signal panel"; a click on the rows side closes the panel (a drag from the panel drops there instead) |
| the `e` and `?` boxes | the wheel scrolls, as `j` / `k` |
| the `:records` box (section 7) | a click highlights a record, a double click jumps to it as `Enter`, the wheel moves the highlight |
| the command completions (section 4) | a click puts the command on the command line as `Tab`, a double click runs it as `Enter`, the wheel moves the highlight; a click outside closes the command line |
| a menu | as "Menu" below |

#### Menu

A right click opens a menu of what can be done to the row (or panel line) under the pointer, each
item with its key or command, so the menu teaches the keys. It runs the same action as the key.

```
 pkt          ▸ 0x5A5     ──┌─────────────────────┐
                            │▌Open           Space│
 pkts[0]      ▸ 0x000     ──│ Examine            e│
                            │ Radix              ▸│┌─────────┐
                            │ Colour             ▸││ hex    ●│
                            │ Copy value        yy││ bin     │
                            │ Remove            dd││ dec     │
                            └─────────────────────┘└─────────┘
```

- It opens with its corner at the pointer, moved left or up to fit the screen. It is a card, as the
  other boxes (section 14). No item is highlighted until the pointer is over one or `j` / `k`
  moves.
- Pointer over an item highlights it; a click runs it and closes the menu. `j` / `k` move, `Enter`
  runs. `Esc`, a click outside or `q` closes it; a right click outside opens the menu there
  instead. Other keys do nothing while it is open.
- An item ending in `▸` opens a second menu beside it (left if there is no room on the right), by a
  click, `Enter` or `l`; `h` or `Esc` closes only the second menu. Its current choice is marked `●`.
- Only items that apply are listed:
  - a row: `Open` / `Fold` (`Space`, where it opens), `Examine` (`e`), `Radix` ▸ (the radixes the
    signal can take, section 10; `q<m>.<n>` opens the command line with `:radix q`), `Colour` ▸
    (the eight colours each drawn in it, and `off`), `Copy value` (`yy`), `Remove` (`dd`);
  - on a row's waveform also `Mark here` (`m`) and `Copy time` (`yt`), for the column clicked;
  - a group header: `Open` / `Fold`, `Radix` ▸, `Colour` ▸, `Rename…` (opens `:rename <name>`),
    `Ungroup` (`:ungroup`), `Remove` (`dd`);
  - a `V` range: `Group…` (opens `:group `), `Radix` ▸, `Colour` ▸, `Remove` (`dd`);
  - below the rows: `Add signals…` (`a`), `Undo` (`u`) and `Redo` (`Ctrl-r`) when there is one;
  - a panel line (section 5): `Add` (`Enter`), `Add as group` (`Ctrl-g`), `Mark` / `Unmark`
    (`Space`); a scope also `Expand` / `Collapse` (`l` / `h`). With marks, `Add` and `Add as group`
    take the marks, as the keys do.
