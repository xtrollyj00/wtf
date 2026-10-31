## 5. Edit mode

Edit mode works on the **rows** (the signals being shown). Adding happens in the **signal panel**
(all signals in the file), which opens on the left with `a`, or with `/` already searching, and has
the keys while it is open. `Esc` closes it and the keys go back to the rows. With no rows yet,
`i` opens the panel straight away. The selected row is the same in View and Edit mode.

### Signal panel

The panel shows the file's scope tree, like Zed's project panel. Signals already shown are marked `●`.
Searching works as in vim: `/` types a search, and the highlight jumps to the first match as you
type, opening the scopes around it. `Enter` ends the typing and `n` / `N` go to the next / previous
match in tree order, wrapping around. A search matches a case-insensitive part of the full path,
scopes included: `cpu.st` matches `top.cpu.state`. The search line shows the last search and which
match the highlight is on (`2/5`).

| Keys | Action |
|---|---|
| `j` / `k` | move |
| `gg` / `G` | first / last line |
| `l` / `h` | expand / collapse a scope; `h` on a signal or a collapsed scope goes to the parent scope |
| `Space` | mark / unmark the signal (a struct, record or array is one), or all signals directly in a scope; with a `V` range, its signals |
| `V`, then `j` / `k` / `n` / `N` | select a range of lines, as in vim; a closed scope in it brings the signals directly in it |
| `Enter` | add the marked signals and the `V` range as rows; without marks, the highlighted item: a signal (a struct, record or array too) as a row, a scope as a group named after it |
| `Ctrl-g` | add the marked signals and the `V` range (or the highlighted signal or scope) as a new group, then name it |
| `/` | search; while typing, `Enter` ends it and `Esc` (or `Backspace` on an empty search) cancels it |
| `n` / `N` | next / previous match of the last search |
| `Ctrl-a` | mark every signal the search matches (also while typing) |
| `u` / `Ctrl-r` | undo / redo |
| `Esc` | end a `V` range, then clear the marks, then close the panel and go back to the rows |

The mouse, in the panel (a click outside it closes the panel, as `Esc`; a drag from the panel drops
there instead):

| Mouse | Action |
|---|---|
| click on a scope (or a struct, record or array) | highlight it and expand / collapse it, as `l` / `h` |
| click on a signal | highlight it |
| double click on a signal (a struct, record or array too) | add it as a row, as `Enter` on it without marks; the marks stay |
| `Ctrl` + click | highlight the line and mark / unmark it, as `Space` (on a scope, the signals directly in it) |
| `Shift` + click | mark the lines from the highlight to the clicked one, as `V` there then `Space`; the highlight moves to it |
| drag from one line to another | mark the lines between, as `V` then `Space`, shown as a `V` range while dragging; the highlight ends on the last |
| drag into the rows | add what is dragged below the row it is dropped on: the lines dragged over (just the pressed one when dragged straight out), or all the marks when pressed on a marked line |
| right click | highlight the line and open its menu (section 2, "Menu") |
| wheel | move the highlight, as `j` / `k` |

A click acts when the button is released on the line it was pressed on; released on another line, it
is a drag. While a drag is over the rows, the insertion line (`▸ new signals go here`) follows the
pointer; a drop selects the row under it and adds as `Enter` does (a scope as a group, into a group
if the row is in one). Dropped anywhere else (the status line, the panel's border), nothing is
added. The drag inside the panel is the mouse-only range: `Shift` + click often never reaches wtf
(section 18, "Known limits"). A double click is a second click on the same line within 500 ms; on a
scope it opens or closes it once, not twice. A plain click keeps the marks (`Esc` clears them, as
with the keys).

While dragging, the lines dragged over show as a `V` range, a label at the pointer says what a
drop adds (`+ state`, `+ 3`), and the status line what the drop does (`mark 3 · drag out to add ·
Esc cancels`, `drop to add 3 below clk · Esc cancels`, or `release to cancel` where nothing can be
dropped). `Esc` cancels a drag: nothing is marked or added. The pointer's shape stays the
terminal's (OSC 22 isn't in every terminal).

```
┌ signals ───────────┐                ┌───┐   ┌───┐   ┌───
│ / _                │clk    0     ───┘   └───┘   └───┘
│ ▾ top              │▸ new signals go here ── + 3 ────────
│   ▾ cpu            │state  0x1   ─ 0x1 ─🮧─── 0x2 ──────
│     ● clk          │
│       state        │
│       busy         │
│       irq          │
└────────────────────┘
 EDIT  drop to add 3 below clk · Esc cancels
```

A drag scrolls what it is over while the pointer rests at an edge, so it reaches signals and rows
off screen: on the panel's first or last line (or past it, on the border), the tree scrolls a line
every 50 ms; on the rows' first or last row, the rows scroll a row every 50 ms; with the pointer two
or more lines past the edge, four at a time. The wheel scrolls during a drag as well. Scrolling
stops at the ends, and when the pointer leaves the edge or the button is released.

The tree lists each scope's subscopes first, then its signals. Top-level scopes start expanded.
An array's elements nest under it, one level per index (`grid` > `[1]` > `[2]`).
Adding several signals: mark them (`/state`, `Enter`, `Space`, `n`, `Space`), then `Enter` or `Ctrl-g`.
There is no prompt; `u` takes a wrong add back. Marked signals show `+`, and the panel's lower border
counts them. They are added in the order they were marked. A group from `Ctrl-g` goes below the group the selected row is in, if any.
It is created at once with the name of the scope its signals share (`group` if none), and the command
line opens with `:rename <that name>`: `Enter` keeps or applies the name, `Esc` keeps the suggestion.
The panel stays open after adding.

- New rows go below the selected row, which a line in the rows marks while the panel is open
  (`▸ new signals go here`). Each added signal goes below the previous one, so they keep the order
  they were added in. To add elsewhere, close the panel, select another row and open it again.
- If the selected row is inside a group, a new signal joins that group, and a new group
  (from a scope) goes below the whole group. If the selected row is a group's header, new rows
  go below the whole group, outside it.
- A signal that is already shown is never added twice: `Enter` on a `●` signal does nothing,
  and it can't be marked; adding a scope, `Space` on a scope and `Ctrl-a` skip shown signals. Removing is done on the rows side with `dd`.
- Adding a scope (a module, an instance) takes only the signals directly in it; subscopes are
  skipped until subgroups exist. A struct, record or array is one signal in the HDL, so it is
  added as one row (section 6, "Composite rows"), marked `●` when shown; a signal in it can
  still be added as a row of its own. Its fields are listed as declared, not subscopes first.
  A scope without direct signals, or whose signals are all shown already, adds nothing and says so
  on the status line.

### Rows

| Keys | Action |
|---|---|
| `j` / `k` | select the next / previous row |
| `a` | open the signal panel to add signals below the selected row |
| `/` | open the signal panel, searching |
| `dd` | remove the selected row |
| `J` / `K` | move the selected row down / up |
| `r` | cycle the radix of the selected line (a row, or a field under it), or of all signals in a group or `V` range (section 10) |
| `u` / `Ctrl-r` | undo / redo |
| `Space` | collapse / expand the selected group, or open / fold a bus, a stream or a composite (one level) |
| `e` | examine the selected signal, as in View mode |
| `Esc` | clear a `V` selection if there is one, otherwise back to View mode |

### Undo

`u` / `:undo` undo and `Ctrl-r` / `:redo` redo changes to the rows: adding, removing, moving, grouping
and radix. The keys work in Edit mode, also while the panel is open (not while typing a search), the commands from
any mode. The history covers the whole session.
View-mode actions, such as moving the cursor or zooming, are not part of the undo history.
Undo and redo return the selection to the row the change was made on.
Undoing back to the last saved state
clears `[+]`.

## 6. Signal groups

A group is a named set of rows with a header row. Groups have one level (no nesting yet).

```

▌▾ cpu
                       ┌───┐   ┌───┐   ┌───
    clk        0    ───┘   └───┘   └───┘
                    ───────🮦──────────────
    state      0x1  ─ 0x1 ─🮧─── 0x2 ──────
                       ▄▄  ▄▄▄▄   ▄
 ▸ stream (4)       ───▀▀──▀▀▀▀───▀───     ← collapsed: shows activity
```

- **Collapse / expand**: `Space` on a group header, in View and Edit mode.
- **Bus bits**: `Space` on a bus shows one line per bit under it, MSB first (`data[7]` … `data[0]`),
  drawn as 1-bit signals; `Space` on the bus or one of its bits folds them again. Bit lines can be
  selected (`j` / `k`, `w` / `b` jump between that bit's changes, the value column shows the bit),
  but they move and are removed with their bus. Like collapsing, expanding is not an undo step.
  Combining bits or signals into new ones is done by computed signals (`:signal`, section 16).
- **Composite rows**: a struct, record, union or array the trace dumps as a scope is one row,
  named as in the HDL (`grid`, `pkts`, `pkt`). See "Composite rows" below.
- **Activity**: a collapsed group draws one row in place of its signals. A column is marked active
  when any signal in the group has a transition in it (filled between the two lines, `▄` over `▀`),
  and drawn as a dim idle line on the lower line otherwise.
  So you can see where something happens in the group without expanding it.
  An expanded group's header shows only its name.
- `j` / `k` and `w` / `b` treat a collapsed group as one row. On a collapsed group, `w` / `b`
  jump to the next / previous column with activity.
- A collapsed header shows the number of signals in it: `▸ stream (4)`.
- With a header selected, the status line shows the group's name and `/` shows an error.

In Edit mode, on the rows side:

| Keys | Action |
|---|---|
| `V`, then `j` / `k` | select a range of rows, then `:group <name>` groups them |
| `dd` on a header | remove the group and its signals |
| `dd` with rows selected | remove all selected rows |
| `J` / `K` on a header | move the whole group down / up |

- `J` / `K` on a signal next to a group boundary moves it into or out of the group.
- `:group` on a selection that includes a header shows an error, since groups don't nest yet.

### Composite rows

A struct, record or array is one signal in the HDL, so it is one row. `Space` opens it one level,
the way a bus opens into its bits and a stream into its fields: **every index and every field is
a level**. `pkts` opens into `pkts[0]`, `pkts[1]`; `pkts[0]` into `valid`, `state`, `pair`;
`pair` into `hi`, `lo`. `grid` opens into `grid[0]`, `grid[1]`, and `grid[1]` into `grid[1][0]`
.. `grid[1][2]`.

```
▌▸ pkt         0x5A5     ── 0x0 ──🮦──── 0x5A5 ─────
 ▾ pkts        0x000007  ── 0x0 ──🮦─── 0x000007 ───
   ▸ pkts[0]   0x000     ───────── 0x000 ──────────
   ▾ pkts[1]   0x007     ── 0x0 ──🮦──── 0x007 ─────
       valid   0         ───────────────────────────
       state   IDLE      ───────── IDLE ───────────
     ▸ pair    0x07      ── 0x00 ─🮦──── 0x07 ──────
```

(Each row is two lines on screen; one is drawn here. The values are `tests/data/types.sv`'s at
10ns.)

- **Value**: a composite is a signal whose value is its fields' values joined in declaration
  order (an array's elements in the order the trace lists them), the first the most significant
  (`pkt` is `{valid, state, pair}`: `0x5A5`; `pkts` is `{pkts[0], pkts[1]}`), drawn as a bus.
  When a field is text (nvc's enum is a string), the value is text instead, written as an SV
  aggregate: `'{valid:1, state:busy, pair:'{hi:A, lo:5}}`, bits fields in hex without `0x`, an
  array's elements without names (`'{8, 7}`). It changes whenever a field changes.
- **Lines**: each open level shows its direct children under it, indented two columns a
  level; a child is a composite (folded or open, `▸` / `▾`) or a signal (drawn as any signal, a
  bus may open into its bits). A child is named by its field (`valid`, `pair`) or by its array
  and index as the HDL writes it (`pkts[0]`, `grid[1][2]`); the status line shows its full path.
- **Like bit lines**: child lines are selected with `j` / `k` (`w` / `b` jump between that
  child's changes, the value column shows it, `e` examines it), but they move and are removed
  with their row: `dd` on one removes the whole row. Opening and folding is not an undo step.
  `Space` on an open level folds it and every level under it.
- **Each line its own look**: a composite row's radix applies to its whole value; a child line
  starts in the default radix for its signal (an enum its names), a stream's field in its row's,
  and each can be given its own radix and colour (`r`, `:radix`, `:color`, the menu), kept with
  the row. A child without a colour of its own takes its row's.
- **Groups are not composites**: a group is the user's collection of rows; a composite is the
  trace's structure. Modules and instances stay groups (`:add top.cpu`), and a group can hold
  composite rows.
