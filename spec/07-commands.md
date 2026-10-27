## 7. Commands

| Command | Description |
|---|---|
| `:<time>` | move the time cursor to that time, e.g. `:2000ns`, `:1.5us` |
| `:zoom <time>` | set the time per column, e.g. `:zoom 1ns` |
| `:add <path\|pattern>...` | add signals below the selected row; a scope becomes a group, e.g. `:add top.cpu.*`; several are added in order, and those that match nothing are reported; an expression in parentheses or braces is one, `:add (pc + 4) {a, b}` |
| `:rm [path\|pattern]...` | remove matching rows, or the selected rows if no argument; `:rm **` removes all |
| `:signal <name> = <signal>` | name a computed signal (section 16), e.g. `:signal op = cmd[7:0]`, `:signal hdr = {tuser, tdata[63:32]}`, `:signal next = pc + 4` (any expression of section 8 without `past` and the like); it is saved in the session and made again on `:e`; defining a name again replaces it |
| `:protocol <name> clock=<clk> when=<expr> <field>=<signal>...` | a stateless protocol (section 16), as `[[protocol]]`: a stream sampled where `when` holds, with its fields in order; `roles=a,b,c` makes it a type for `:decode`. Values run to the next `key=`, as in `:decode` |
| `:decode <name> <protocol> <role>=<signal>...` | decode a protocol into streams (section 16), as a `[[decode]]` instance: `clock=` is its clock, `prefix=s_axi_` plays each role of a built-in protocol not given by `<prefix><role>`, a built-in's own params are params (`mode=0`, `baud=115200`), and for `frame`, `pair` and `check` the keys that aren't roles are params; a value runs to the next `key=`, e.g. `:decode rx axis clock=clk tvalid=rx_tvalid tready=rx_tready tdata=rx_tdata tlast=rx_tlast`, `:decode stall check clock=clk bad=valid && !ready` |
| `:group <name>` | make a group from the selected rows |
| `:rename <name>` | rename the group the selected row is in |
| `:ungroup` | dissolve the group the selected row is in; its signals stay where they are |
| `:select <row> [<last row>]` | select a signal (named as in `:add`) or group, or the rows from one to another, as `V` does |
| `:cursor <n> [time\|off]` | put cursor `n` at a time (`:cursor 1 <time>` moves the cursor, others place a mark), remove mark `n`, or without a time jump to mark `n` (`:cursor 1` centers the screen on the cursor) |
| `:scope [path]` | names resolve under this scope first, wherever signals are named (search, `:add`, `:select`, the agent's tools): with `:scope top.cpu`, `state` is `top.cpu.state` though `top.dma.state` exists; saved in the session; without a path, no scope |
| `:radix <name>` | set the radix of the selected row or group, e.g. `:radix dec` |
| `:color <name\|off>` | set the colour of the selected row or group (section 14): `red`, `orange`, `yellow`, `green`, `cyan`, `blue`, `magenta`, `gray`; `off` for the theme's |
| `:undo` / `:redo` | undo / redo a change to the session layout |
| `:fit [<from> <to>]` | zoom so the whole simulation fits on screen; with two cursors or times (`:fit '2 '3`, `:fit 10ns 20ns`, either order), zoom so the range between them fits. A cursor is `'N` as in marks (`'1` is the cursor). Zoom goes in steps, so a little more than the range may show, never less |
| `:e` | reload the file, keeping signals, groups, cursor and zoom |
| `:w [file]` | save the session (section 11) |
| `:wq` | save the session and quit |
| `:records <stream> [where <expr>]` | a box listing a stream's records (section 16), `#n start–end` and every field, as the `records` tool does; `where` takes the same expression. In the box `j` / `k` move, `Ctrl-f` / `Ctrl-b` a box of records, `g` / `G` go to the first / last, `Enter` jumps the cursor to the record's start (a jump, so `''` comes back) and selects the stream's row if shown, `Esc` or `q` closes it. No `group_by` or `count_only`: those are for agents |
| `:help [<topic>]` | show the keys of the current mode, like `?`; with a topic, a box with its text from the agents' `help` tool (section 15), e.g. `:help records`. In the box `j` / `k` scroll, `Ctrl-f` / `Ctrl-b` a box, `g` / `G` go to the top / end, `Esc` or `q` closes it |
| `:q` | quit; refuses if the session has unsaved changes |
| `:q!` | quit without saving |

The command line shows the description column of this table next to each matching command.

Times accept `fs`, `ps`, `ns`, `us`, `ms`, `s`. A number without a unit means file ticks.

- Layout commands (`:add`, `:rm`, `:group`, `:rename`, `:ungroup`, `:radix`, `:color`) work from View and
  Edit mode and are undoable: each command that changes the rows is one undo step.
  "Selected rows" means the `V` selection in Edit mode, otherwise the selected row.
- Patterns match full signal paths: `*` matches within one scope level, `**` across levels.

- `:zoom` and `:fit` keep the cursor on its time, like `z` / `x`. A zoom finer than one tick per
  column is rounded to the nearest zoom wtf can show.
- `:e` is meant for rerunning the simulation and looking at the new result without rebuilding the view.
  It re-applies the current layout and view with the same rules as loading a session (section 11):
  missing signals are dropped and listed, cursors and the visible time past the new end move to the end.
  Dropping signals marks the session as modified. The undo history is cleared.
  If the file can't be read, the old data stays and the error is shown.
