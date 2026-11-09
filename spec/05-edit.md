## 5. Editing rows

The **rows** (the signals being shown) are changed with Normal mode keys (section 3): `dd`,
`p` / `P`, `J` / `K`, `>` / `<`, `R`, `r`, `V`, `u`. Adding happens in the **signal panel** (all signals in the file), which
`a` or a click on the status line's `signals ▸` opens on the left, and `a`, the button or `Esc`
closes. It has the keys while it is open; when it closes, they go back to the rows.

### Signal panel

The panel shows the file's scope tree, like Zed's project panel. Signals already shown are marked `●`.
Searching works as in vim: `/` types a search, and the highlight jumps to the first match as you
type, opening the scopes around it. `Enter` ends the typing and `n` / `N` go to the next / previous
match in tree order, wrapping around. A search matches a case-insensitive part of the full path,
scopes included: `cpu.st` matches `top.cpu.state`. The search line shows the last search and which
match the highlight is on (`2/5`).

When computed signals appear or go (section 16), the panel keeps its highlighted signal or scope, open scopes,
marks, range and search. New signals join the tree and the current search results.

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
| `a` | close the panel, as the button does: a `V` range ends, the marks stay for when it opens again |
| `Esc` | end a `V` range, then clear the marks, then close the panel |

The mouse, in the panel (a click outside it closes the panel, as `Esc`):

| Mouse | Action |
|---|---|
| click on a scope (or a struct, record or array) | highlight it and expand / collapse it, as `l` / `h` |
| click on a signal | highlight it |
| double click on a signal (a struct, record or array too) | add it as a row, as `Enter` on it without marks; the marks stay |
| `Ctrl` + click | highlight the line and mark / unmark it, as `Space` (on a scope, the signals directly in it) |
| `Shift` + click | mark the lines from the highlight to the clicked one, as `V` there then `Space`; the highlight moves to it |
| drag into the rows | add the pressed line, or all the marks when pressed on a marked line, below the row it is dropped on |
| right click | highlight the line and open its menu (section 2, "Menu") |
| wheel | move the highlight, as `j` / `k` |

A click acts when the button is released on the line it was pressed on; a plain click keeps the
marks. A double click on a scope opens or closes it once. To add several signals with the mouse,
mark them with `Ctrl` + click and drag one of them into the rows; a drag takes the pressed line
or the marks, never the lines it passes over.

While a drag is over the rows, the insertion line (`▸ new signals go here`) follows the pointer,
a label at the pointer says what a drop adds (`+ state`, `+ 3`), and the status line what the
drop does (`drop to add 3 below clk · Esc cancels`). A drop selects the row under it and adds as
`Enter` does (a scope as a group, into a group if the row is in one). Elsewhere (the panel, the
status line) the status line says `release to cancel`, and letting go adds nothing. `Esc` cancels
a drag. The pointer's shape stays the terminal's (OSC 22 isn't in every terminal).

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
 signals ◂  drop to add 3 below clk · Esc cancels
```

A drag scrolls the rows while the pointer rests at their edge, so it reaches rows off screen: on
the rows' first or last row, the rows scroll a row every 50 ms; with the pointer two or more
lines past the edge, four at a time. The wheel scrolls during a drag as well. Scrolling stops at
the ends, and when the pointer leaves the edge or the button is released.

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

### Picker

The **picker** is another way in: one floating box over the rows that finds things by name.
`:pick <source>` (section 7) opens it. A query line is on top, the entries it matches under it,
best first, the matched characters in the accent (bold and underlined without colour, section 14),
and a preview of the highlighted entry on the right. The sources are named in the box's top
border, the current one highlighted.

```
┌─ signals  marks  records  commands  sessions  trail ─────────────────────────────┐
│ ▸ cpust                            1/6 │ top.cpu.state                           │
│────────────────────────────────────────│ 3 bits                                  │
│▌  top.cpu.state                  [2:0] │                                         │
│                                        │ at 30ns  0x2                            │
│                                        │                                         │
│                                        │ ───🮦─────────🮦────────┃────────🮦────── │
│                                        │ ───🮧── 0x1 ──🮧────── 0x2 ─────🮧─ 0x4 ─ │
│                                        │ 7ns                                53ns │
│                                        │ Enter adds it below top.clk             │
└────────────────────────────────────────┴── Enter add  Tab mark  C-s source  Esc ─┘
```

| Source | Entries | Preview | `Enter` |
|---|---|---|---|
| `signals` | every signal by full path, computed ones and streams too; `●` shown, `+` marked | its width, the decodes it is in, under a dim `decodes` heading, a line each with the decode, its protocol and the signal's role there (`bus  axilite  awvalid`: its records may be what is wanted), its value at the cursor and its waveform around the cursor, drawn as the rows draw it; a signal not read from the file yet is read once the highlight has rested on it 120 ms (`…` till then), so holding `j` over a big trace doesn't read every signal it passes | adds the marked signals, or the highlighted one, below the selected row as the signal panel's `Enter` does; a shown signal is not added twice |
| `marks` | the cursor and each mark, with its name and time | its name, time and note | jumps to it, as `'n` |
| `records` | a stream's records, `#n` and every field; every stream's, in time order and named by their stream (`bus.rd #3`), unless one is named or selected | its start, end and fields | jumps to its start and selects the stream's row, as the records browser does |
| `commands` | the `:` commands | the usage and what it does | puts it on the command line, to finish and run |
| `sessions` | the agents' workspaces on this machine (section 15) | its trace, and whether it is this one | follows it, as `:follow <id>`; an agent on another trace says to open that trace |
| `trail` | the agent's steps, newest highlighted | what it asked and the answer | jumps to its time, as `:trail` |
| `interfaces` | the interfaces built-in protocols decode in the trace (section 16, as the agents' `open` lists them), each its name and prefix, the protocol at the right (`→ bus` once decoded) | its `:decode` line, each role and the signal it takes, the clock (or that none was found) | decodes it, named after its prefix (`s_axi`, a number after a name taken), and adds its results and violations (section 16 "Results") below the selected row as a group of that name; without a clock found, opens the decode form on the clock, the rest filled in; decoded already, opens the records browser on the decode |

The picker's keys are fixed: typing edits the query, `Backspace` deletes, `Ctrl-n` / `Ctrl-p`
or the arrows move, `PageDown` / `PageUp` a box of entries, `Tab` marks the highlighted signal
and moves on, `Ctrl-s` goes to the next source (an empty one says why), `Enter` picks and closes,
`Esc` closes. Adding is one undo step. A query matches its characters in order, not
necessarily together (`cpust` matches `top.cpu.state`); a tie goes to the shorter entry.

### Decode form

`:decode` without arguments (`Space d D`) opens the **decode form**: a card with a new instance's
fields, for a person who doesn't know the protocol's roles and params, so `:decode` is learnt by
using it.

```
╭─ decode ───────────────────────────────────────────────────────────────╮
│ name       s                                                           │
│ protocol   axilite  AXI4-Lite: write and read transactions with their  │
│ clock      top.aclk                                                    │
│▌prefix     s_axi_   plays 17 of 19 roles                               │
│ awvalid    top.s_axi_awvalid                                           │
│ …                                                                      │
│────────────────────────────────────────────────────────────────────────│
│ the names match: Enter decodes                                         │
│ :decode s axilite clock=top.aclk prefix=s_axi_                         │
╰─────────────────── Tab next  → take  C-Space pick  Enter run  Esc close ─╯
```

- **Fields**: `name`, `protocol`, then the protocol's: `clock` (a clocked one), `prefix` (a
  built-in), its required roles, its optional roles and params dim. The built-in protocols and the
  session's sampled ones (section 16); `frame`, `pair` and `check` work on streams and rules, and
  are typed as `:decode` lines. Changing the protocol keeps the values of the fields it still has.
- **Filling in**: typing edits the field; each field suggests, `→` takes the suggestion and
  `Ctrl-n` / `Ctrl-p` go through the others (`2/5` beside it), so a form is mostly filled in
  without typing a name. Left empty, a field suggests what fits it: the protocol, those of the
  interfaces found (section 4, as `Space d d` lists them); the prefix, those interfaces' prefixes
  for the protocol; the clock, the one found with that prefix, then the file's signals ending in
  `clk` or `clock`; a role, the file's signals whose name ends in it, shortest first; the name, the
  prefix's last part (`s_axi`). Typed, the text matches as the picker's query does (`awv` matches
  `top.s_axi_awvalid`), signals ending in the role first. Nothing is filled in without `→`, and
  nothing is guessed beyond a name's exact ending. `Ctrl-Space` opens the signals picker on the
  role's name, its `Enter` fills the field in; on the protocol, it goes to the next one.
- **From a selection**: opened on a `V` range (`Space d D`, or the menu's `Decode as` ▸, which
  names the protocol), the range's signals fill the empty roles they play, and the clock: a
  signal plays a role its name ends in after `_` or `.` (`s_axi_wvalid` plays `wvalid`,
  `s_axi_arvalid` never `rvalid`), the clock the one ending in `clk` or `clock`. A role two of
  them could play is left empty, as are the roles none plays; nothing is guessed beyond that.
  Changing the protocol fills its roles again; the protocol, left empty, suggests those the
  signals play roles of, most first. The form opens even when every role is filled: `Enter`
  decodes.
- **The prefix** plays each role left empty by `<prefix><role>`, as in `:decode`: the role shows
  that signal dim, the prefix how many roles it plays. Nothing is guessed beyond that.
- **Checked as filled in**: after each key the form checks the names as `:decode` would before
  decoding (the protocol, each role's signal, the prefix, the params, the clock, and that the
  instance's name is new) and says what is wrong, the fields it names in red, or that the names
  match. It reads no signal's values, so it is as quick on a 1 GB trace as on a small one; the
  decode, which walks every clock cycle, runs on `Enter` only.
- **The line** under it is the `:decode` line the fields make, `Enter` runs it: the instance's
  results and violations are added as a group of its name, as the interfaces picker does. An error
  keeps the form open on the first field it names. A name taken by another instance is refused:
  the form makes new instances, it doesn't replace one.
- **Keys** are fixed (section 5 "Records"): `Tab` / `Down` and `Shift-Tab` / `Up` go between the
  fields, `Backspace` deletes, `Ctrl-u` clears the field, `Esc` closes. A click on a field fills
  that one in.

### Protocol form

`:protocol` without arguments (`Space d p`) opens the **protocol form**: the same card for a
sampled protocol (section 16), so `:protocol` is learnt by using it.

```
╭─ protocol ─────────────────────────────────────────────────────────────╮
│ name       w                                                           │
│ roles      wvalid, wready, wdata                                       │
│ wvalid     top.s_axi_wvalid                                            │
│ …                                                                      │
│ when       wvalid && wready                                            │
│ field      lo = wdata[7:0]                                             │
│▌field                                                                  │
│────────────────────────────────────────────────────────────────────────│
│ the names match: Enter defines it, then decodes it                     │
│ :protocol w roles=wvalid,wready,wdata when=wvalid && wready lo=wdata[7:0] │
╰─────────────────────────────────────────── Tab next  Enter run  Esc close ─╯
```

- **Roles** come from the selected rows: a role per signal, named after it without the start the
  names share (`rx_valid`, `rx_data`: `valid`, `data`), with the signal filled in. `roles` is a
  comma list to edit; a row follows each role, its signal suggested as in the decode form.
- **`when` and the fields** are expressions on the roles; a field is `name = expression` and may
  name the fields before it. The word typed is completed to a role or a field, `→` takes it. An
  empty field row stays last to add the next.
- **Checked as filled in**: with a signal for each role, the form reads the expressions against
  those signals' widths (a slice past a role's width, a name no role nor field before it) and
  says what is wrong, the field it is in red. It reads no value. Without them, the expressions
  are not checked and `Enter` defines the protocol as it is.
- **`Enter`** runs the `:protocol` line under it, then opens the decode form on the new protocol
  with each role's signal filled in. A name taken by a protocol is refused. Keys are the decode
  form's.
- **From a search**: `:protocol search` (`Space d m`, or `Records of the matches` on a waveform)
  after a clocked search (`/@aclk wvalid && wready`) opens the form filled in as MCP's `find`
  offers it (section 15): named `ev` (`ev1`, ... when taken), its roles the signals the search
  names then the signal rows but the clock, `when` the search's on the roles, a field per signal
  row. `Enter` defines it; the decode form follows with the search's clock and named `s`, and its
  `Enter` decodes and opens the records (section 5, "Records").

### Value-names card

`:names` without arguments (`Space d n`) opens the **value-names card** on the selected signal, so
`:names` is learnt by using it: a line per value the signal takes in the trace, x and z left out,
in value order, as its row shows it (`0x2`), with how many times it is taken; the name typed beside
it, those it has filled in. More than 256 values is an error: `:names` names some. The `:names`
line is under them (`:names top.cpu.state 0x0=IDLE 0x2=BUSY`, `off` with none named); `Enter`
runs it, and an error stays on the card. Keys are the decode form's.

```
╭─ names top.cpu.state ──────────────────────────────────────────╮
│ 0x0        IDLE                                       1 time   │
│ 0x1                                                   1 time   │
│▌0x2        BUSY                                       1 time   │
│────────────────────────────────────────────────────────────────│
│ each value its name, or none: Enter names them                 │
│ :names top.cpu.state 0x0=IDLE 0x2=BUSY                         │
╰─────────────────────────────── Tab next  Enter run  Esc close ─╯
```

### Decodes

`:decodes` (`Space d l`) lists the decodes made, a line each: its name, protocol, where it is (its
prefix, else its clock, else the signals or streams it takes), and its records, results then
violations that hold any (`wr 2  rd 8  · errors 2`).

- `Enter` opens its records (section 5 "Records"); `a` adds its results and violations as a group
  of its name, as the interfaces picker does, or says they are shown already.
- `e` opens the decode form on it, titled `edit decode <name>`, its name not taken; `Enter` there
  decodes it again in place (section 16): rows of streams it still makes stay.
- `dd` removes it, as `:decode <name> off`: after the first `d` the footer says `d again removes
  it`. One another decode reads stays, the status line naming the readers.
- `j` / `k` or the arrows move, `Esc` / `q` close; the keys are fixed (section 5 "Records"). The
  mouse: the wheel moves, a click highlights, a double click opens the records, a click outside
  closes it. With no decodes, `:decodes` says how to make one.

### Records

`:records <streams> [where <expr>]` (section 7) opens the records browser: a card over the
waveforms with the records of a set of streams (section 16 "Records of a set": a stream, a
decode's name, a glob, several) as one table, as the agents' `records` tool gives them, for a
person to look through.

```
╭─ records · s_axi ───────────────────────────────────────────────── 13 records ─╮
│ all  wr  rd  errors  wr.unmatched  rd.unmatched  aw  w  b  ar  r               │
│ ▸ where / filters on delay, addr, data, strb, resp, rule, kind                 │
│────────────────────────────────────────────────────────────────────────────────│
│   stream  n    start    length    delay    addr    data        strb    resp    │
│▌  wr      #0   30ns     20ns      10ns     0x10    0x11111111  0xF     0x0     │
│   rd      #0   130ns    30ns      20ns     0x10    0x11111111          0x0     │
│   errors  #0   180ns    10ns                                                   │
╰──────── Enter jump  / filter  s S sort  = group  h l scroll  Tab set  Esc close ─╯
```

- **Sets**: when the streams are a decode's, the line under the title lists its sets, `all`
  (its results and violations, named one by one when a stream has the decode's name, as a
  `pair`'s) then each stream, results first and the streams they are made
  from last, named without the decode's name. `Tab` / `Shift-Tab` go to the next / previous, its
  filter and sort fresh.
- **Columns**: `stream` (a set of several), `n`, `start`, `length`, then a column per field
  (section 16), each as wide as its widest value up to 24 characters (a longer value is cut,
  `…`). `h` / `l` scroll the columns after `stream` and `n`, which stay. A record of a
  violations stream is red.
- **Filter**: `/` edits the `where` expression (section 16, the `records` tool's), applied as
  typed; empty, the line names the set's fields. `Enter` or `Esc` stops editing, `Ctrl-u` clears it. An expression that doesn't parse
  keeps the records shown and says why at the right of its line. The top border counts the
  records kept of all.
- **Sort**: in order of start at first; `s` sorts by the next column, `S` reverses; the sorted
  column's name is in the accent. Fields sort by value (a signed field as signed); values with
  `x` or `z` bits and text after numbers, by their text; a record without the field last.
- **Group**: `=`, sorted by a field, groups the records kept by its value: a line per value with
  its count and first and last start, the commonest first. `Enter` on a group keeps its records
  (adds `field == value` to the filter; a value with spaces can't be, and says so); `=` again
  ungroups. Sorted by `stream`, `n`, `start` or `length`, `=` says to sort by a field first.
- **Keys**: `j` / `k` or the arrows, `Ctrl-f` / `Ctrl-b`, `g` / `G` move; `Enter` on a record jumps the cursor
  to its start (a jump, so `''` comes back), selects its stream's row if shown and closes; `Esc`
  or `q` closes. The mouse: section 2. The browser's and the picker's keys are fixed, one table
  each that the keys page prints; a key not in it does nothing (in the picker, a character types
  into the query).

### Rows

The keys that change the rows are in section 3 with the others: `dd` removes and `p` / `P` put
what it removed elsewhere, `J` / `K` move, `>` / `<` group and ungroup, `R` renames a group, `r`
cycles the radix, `V` selects a range of rows for them, `u` / `Ctrl-r` undo and redo. In the panel, `/` searches the signals; on the rows it searches values (section 8), so
adding by name is `a` then `/`.

### Undo

`u` / `:undo` undo and `Ctrl-r` / `:redo` redo changes to the rows (adding, removing, moving,
grouping, radix and colour) and to the marks (placing, moving, naming and removing them, section 9;
a drag is one step), and an agent's `present` as a whole. The keys work on the rows and in the
panel (not while typing a search), the commands from the command line. The history covers the
whole session. Moving the cursor, zooming and other time keys are looking around, not changes,
and not part of the undo history. Undo and redo return the selection to the row the change was
made on. `[+]` compares the rows and the marks with the last saved state, so undoing back to it
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

- **Collapse / expand**: `Enter` on a group header.
- **Bus bits**: `Enter` on a bus shows one line per bit under it, MSB first (`data[7]` … `data[0]`),
  drawn as 1-bit signals; `Enter` on the bus or one of its bits folds them again. Bit lines can be
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

On the rows (section 3), as in vim's indenting:

- `>` groups the selected row or `V` range under a name it gives (the start the signals' names
  share, `s_axi_aw` for `s_axi_awvalid` and `s_axi_awready`; else `group`), and the name is typed
  in place on the header: the status line shows `NAME` and what the keys do, the first character
  replaces the name, `Backspace` and `Ctrl-u` edit it, `Enter` keeps it and `Esc` leaves the
  name it had. `R` types a new name for the group the selected row is in the same way. Grouping
  and naming are an undo step each. `:group <name>` and `:rename <name>` do it in one line.
- `<` takes the selected signals out of their group, placed right after it (a group left empty
  goes); on a header, it dissolves the group, its signals staying where they are, as `:ungroup`.
- `dd` on a header removes the group and its signals; `p` / `P` put them back elsewhere as a
  group. `J` / `K` on a header move the whole group, and on a signal next to a group's edge move
  it into or out of the group.

- `>` or `:group` on a selection that includes a header shows an error, since groups don't nest yet.
- `:group` on signals all inside one group takes them out of it into the new group, placed right
  after it, so a group is split without rebuilding it; a group left empty goes. A selection
  partly inside a group shows an error naming the group.

### Composite rows

A struct, record or array is one signal in the HDL, so it is one row. `Enter` opens it one level,
the way a bus opens into its bits and a stream into its fields (`bus.rd` into `delay`, `addr`,
named as a struct's are): **every index and every field is
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
  `Enter` on an open level folds it and every level under it.
- **Each line its own look**: a composite row's radix applies to its whole value; a child line
  starts in the default radix for its signal (an enum its names), a stream's field in its row's,
  and each can be given its own radix and colour (`r`, `:radix`, `:color`, the menu), kept with
  the row. A child without a colour of its own takes its row's.
- **Groups are not composites**: a group is the user's collection of rows; a composite is the
  trace's structure. Modules and instances stay groups (`:add top.cpu`), and a group can hold
  composite rows.
