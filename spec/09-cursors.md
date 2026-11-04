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
| `'1`…`'9` | jump to that cursor: `'1` centers the screen on cursor 1 (where it is), `'2`…`'9` jump to that mark |
| `''` | back to where the cursor was before the last jump; again, forward to where it jumped |
| `M` | remove the mark under the cursor |
| `NM` | remove mark `N`, e.g. `3M` |

With the mouse (section 2), a click on a number in the ruler jumps to that cursor, a drag moves a
mark, and a right click on one has `Jump to` and `Remove mark`.

- **Jumps center.** A jump to a mark (`'N`, `Tab`, `:cursor N`), or to cursor 1 (`'1`, `:cursor 1`),
  centers the screen on it, also when it is already on screen. A jump to a time (`:<time>`) or a search match (`/`, `n`, `N`)
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

### Names and notes

A mark can say what happens there, so the evidence an agent presents explains itself: a **name**
(`first bad write`) and a **note** (`AW came before W, so the address latched is stale`).

- `:cursor 2 390ns name=first bad write note=AW came before W` places a mark with both;
  `:cursor 2 name=...` or `note=...` changes them on a mark that exists, and an empty one clears
  it. The agent gives them in `workspace` cursors (section 15); sessions keep them (section 11).
- The ruler shows the name on the mark's tab, in place of the Δt to the next cursor ("Cursor
  ruler" below).
- While the cursor is on a mark with a name or note (after `'N`, `Tab` or a click on its
  number), the status line shows `2 first bad write: AW came before W` in the cursor's colour,
  cut at the end if long. A message on the status line comes first.
- A mark moved (`Nm`, `:cursor N <time>`, dragging it) keeps its name and note; a new mark has
  none, and removing a mark removes them. Cursor 1 moves all the time, so it has neither.
- Placing, moving, naming and removing a mark can be undone (`u`, section 5 "Undo"), and the
  marks are part of `[+]`; moving cursor 1 isn't a change.
- Names and notes are one line each: line breaks and runs of spaces become one space.

### Cursor ruler

When there are marks, a ruler line appears above the waveforms. It shows the cursor and each
mark by number at its position, the cursor highlighted, and the Δt between neighbours in time:

```
   1─────── 40ns ──────3── 20ns ─2
```

- A mark is a tab: its number, then its name with a space on each side, on a tint of its own
  (the theme's `mark`, section 19): `3 first write ───── 2 SLVERR on 0x20`.
- A name runs up to the next cursor's number. A longer one is cut with `…` at its end; one with
  room for fewer than three of its columns isn't shown.
- The cursor on a mark leaves the mark's number in place, drawn on the accent, and its name in the
  accent on the cursor column's tint. The cursor's `1` shows only where no mark is.
- Cursors outside the view are shown at the ruler's edge with an arrow (`◂1`, `3▸`), and the Δt
  to them is still shown. A mark there keeps its name, beside the arrow on the left (`◂3 first
  write`) and before the number on the right (`first write 3▸`); a name on the right gets the room
  left after the names on its left.
- Cursors in one place (one column, or off one edge) show one number: a mark over the cursor,
  and of several marks the one nearest the view (the last in time on the left and in a column, the
  first on the right). The cursor there gives it the accent.
- Without colour (section 14) a tab has no tint: the number is bold, and under the cursor
  reversed, its name bold.
- A Δt label that doesn't fit between two cursors is left out, like bus labels.
- Without marks the ruler is hidden.
- The ruler sits under the time axis. Marks' columns are tinted a faint gray, the cursor's
  amber, with its time on the time axis in full amber. `--snapshot` shows them as `╎` and
  `┃`.
