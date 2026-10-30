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
| `Space` | mark / unmark the signal, or all signals directly in a scope; with a `V` range, its signals |
| `V`, then `j` / `k` / `n` / `N` | select a range of lines, as in vim; a closed scope in it brings the signals directly in it |
| `Enter` | add the marked signals and the `V` range as rows; without marks, the highlighted item: a signal as a row, a scope as a group named after it |
| `Ctrl-g` | add the marked signals and the `V` range (or the highlighted signal or scope) as a new group, then name it |
| `/` | search; while typing, `Enter` ends it and `Esc` (or `Backspace` on an empty search) cancels it |
| `n` / `N` | next / previous match of the last search |
| `Ctrl-a` | mark every signal the search matches (also while typing) |
| `u` / `Ctrl-r` | undo / redo |
| `Esc` | end a `V` range, then clear the marks, then close the panel and go back to the rows |

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
- Adding a scope takes only the signals directly in it. Subscopes are skipped until subgroups exist.
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
| `r` | cycle the radix of the selected row, or of all signals in a group (section 10) |
| `u` / `Ctrl-r` | undo / redo |
| `Space` | collapse / expand the selected group, or expand / fold a bus or a stream |
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
