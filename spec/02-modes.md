## 2. Modes

| Mode | Purpose | Entered by | Left by |
|---|---|---|---|
| View | browse time and signals; the default | start, `Esc` | — |
| Edit | change what is shown: add/remove signals, groups, configuration | `i` | `Esc` |
| Command | type a `:` command | `:` from View or Edit | `Enter` (runs), `Esc` (cancels) |
| Search | type a value to search for | `/` from View | `Enter` (jumps), `Esc` (cancels) |

- Command and Search return to the mode they were opened from.
- Everything Edit mode changes is session state, so every Edit action is also a `:` command
  (section 7). The Edit-mode keys are shortcuts for those commands.
- Searching in the signal panel (`/` in Edit mode, section 5) is a filter inside Edit mode,
  not Search mode.
- Time navigation (`h` / `l`, `w` / `b`, `gg` / `G`, `z` / `x`, value search) works in View mode only.
- The current mode is always shown on screen.

## 3. View mode keys

| Keys | Action |
|---|---|
| `h` / `l` | move the time cursor one column left / right |
| `w` / `b` | next / previous transition of the selected signal |
| `j` / `k` | select the next / previous signal |
| `Space` | collapse / expand the selected group (section 6), or expand a bus into its bits or a stream into its fields / fold them |
| `e` | examine: a box with the selected signal's whole value at the cursor; on a stream, its record there (times and every field, a frame's `data` 16 bytes a line); `Esc`, `q` or `e` closes it |
| `gg` / `G` | time cursor to the start / end of the simulation |
| `z` / `x` | zoom in / out by a factor of 2 around the cursor |
| `i` | enter Edit mode |
| `/` | search for a value on the selected signal (section 8) |
| `n` / `N` | next / previous match |
| `m` / `Nm` | put a mark at the cursor, numbered `N` or the lowest free (section 9) |
| `Tab` / `Shift-Tab` | jump to the next / previous mark in time |
| `'2`…`'9` | jump to that mark |
| `''` | back to where the cursor was before the last jump |
| `M` / `NM` | remove the mark under the cursor, or mark `N` |
| `:` | command |
| `?` | help: a box with the keys of the current mode (View, Edit, or the signal panel); `Esc`, `q` or `?` closes it |
| `q` | quit, same as `:q` |

A count before a key repeats it, as in Vim: `5l`, `3w`, `3x`. Counts apply to `h` `l` `w` `b`
`j` `k` `z` `x` `n` `N`, and in Edit mode to `j` `k` `J` `K`. Other keys ignore a count.
A bare `0` does nothing.

Multi-key sequences (`gg`, `dd`, `'1`) wait for their second key without a timeout.
`Esc` or an invalid second key cancels them.

`z` / `x` sit under the left hand, so one hand zooms while the other stays on `hjkl`.
