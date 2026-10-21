## 9. Cursors

Cursors mark times on the waveform. They are numbered `1`–`9`, as in Vim's cursor and marks:

- **Cursor 1 is the cursor.** It always exists, and it is the only one that moves: `h` / `l`,
  `w` / `b`, `gg` / `G`, `/`, `n` / `N` and `:<time>` move it, and the value column shows values
  at its time.
- **Cursors 2–9 are marks.** They stay where they are put, so looking around never moves them;
  only `m`, `M` and `:cursor` change them. A mark placed by the agent stays where the agent put it.

| Keys | Action |
|---|---|
| `m` | put a mark at the cursor, with the lowest free number |
| `Nm` | put mark `N` at the cursor (moving it there if it exists), e.g. `3m` |
| `Tab` / `Shift-Tab` | jump to the next / previous mark in time, wrapping around |
| `'2`…`'9` | jump to that mark |
| `''` | back to where the cursor was before the last jump; again, forward to where it jumped |
| `M` | remove the mark under the cursor |
| `NM` | remove mark `N`, e.g. `3M` |

- **Jumps center.** A jump to a mark (`'N`, `Tab`, `:cursor N`) centers the screen on it, also
  when it is already on screen. A jump to a time (`:<time>`) or a search match (`/`, `n`, `N`)
  centers only when it lands off screen. Small moves (`h` / `l`, `w` / `b`) scroll just enough,
  and `gg` / `G` keep the start / end at the edge.
- **Jumps can be undone.** Each jump keeps where the cursor was, and `''` goes back there, so
  looking around from a mark never loses the place the agent (or you) put the cursor on. `present`
  forgets it: the agent's cursor 1 is the place to come back to.
- Measuring looks like this: put the cursor on the request edge, press `m`, move the cursor to
  the acknowledge with `w`, and read Δt in the ruler.
- A new mark takes the lowest free number, so numbers stay stable when others are removed.
- Errors on the status line: `M` with no mark under the cursor, `'N` or `NM` for a mark that
  doesn't exist, `m` with all 8 marks in use, `1m` / `1M` (cursor 1 is the cursor).
- `Tab` goes to marks without reaching for the number row; `'N` jumps to a specific one.

### Cursor ruler

When there are marks, a ruler line appears above the waveforms. It shows the cursor and each
mark by number at its position, the cursor highlighted, and the Δt between neighbours in time:

```
   1─────── 40ns ──────3── 20ns ─2
```

- Cursors outside the view are shown at the ruler's edge with an arrow (`◂1`, `3▸`),
  and the Δt to them is still shown.
- A Δt label that doesn't fit between two cursors is left out, like bus labels.
- Without marks the ruler is hidden.
- The ruler sits under the time axis. Marks' columns are drawn in a dim amber, the cursor's in
  full amber. `--snapshot` shows them as `╎` and `┃`.
