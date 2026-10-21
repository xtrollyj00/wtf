## 14. Display rules

### Time

- The file's timescale (VCD `$timescale`, FST header) converts ticks into real time.
- Times are shown in the largest unit that keeps the value at 1 or more, with up to 3 decimals
  and trailing zeros dropped: `12.5ns`, `1.234us`, `0ps`. Files without a timescale show raw ticks.
- Answers to agents follow their own rule, a time unit per workspace (section 15).

### Zoom

- Zoom is time per column, changed by a factor of 2 per step.
- Below one tick per column, zoom continues as columns per tick, up to 1024.
- Zooming out stops once one column spans the whole simulation.
- Zoom keeps the cursor on the same time. Near time 0 the view can't scroll further left,
  so the cursor moves on screen instead.

### Waveforms

- Each signal takes two terminal lines, so its drawing can connect.
- 1-bit signals are drawn as a high line on the upper line and a low line on the lower one,
  joined at each edge by corners (`┌┐└┘`). `X` and `Z` draw both lines.
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
  between records; a segment opens with `🮣` over `🮡` and closes with `🮢` over `🮠`. Violations are red.
  With a stream selected, the status line shows its record at the cursor (`rx.frame #2 65ns–85ns
  beats=0x2 len=0x8 data=c0 00 …`).
- `X`, `U` and `x` are drawn red (red outline for buses), `Z` and `z` green.

### Values

- Shown in the row's radix (section 10), hex by default.
- Leading zero digits are dropped, keeping at least one digit: `0x00004000` → `0x4000`.
