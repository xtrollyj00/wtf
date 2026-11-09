+++
title = "Keys and commands"
description = "Every key and command of wtf's window, with the names to change them."
+++

<!-- Written by `UPDATE_SITE=1 cargo test site_keys` from the tables wtf reads; don't edit. -->

These are wtf's defaults. `?` in the window shows the keys of where you are; `[keys]` and `[leader]` in the config file change them (see [the names](#names-for-keys) below).

## In the waveforms

<div class="keys">

<section>

### Move

| Keys | Does |
|---|---|
| <kbd>h</kbd> <kbd>l</kbd> | cursor left / right |
| <kbd>w</kbd> <kbd>b</kbd> | next / prev change |
| <kbd>gg</kbd> <kbd>G</kbd> | start / end |
| <kbd>z</kbd> <kbd>x</kbd> | zoom in / out |
| <kbd>c</kbd> | center the cursor |
| <kbd>H</kbd> <kbd>L</kbd> | screen left / right |
| <kbd>Ctrl-d/u</kbd> | half a screen |

</section>

<section>

### Rows

| Keys | Does |
|---|---|
| <kbd>j</kbd> <kbd>k</kbd> | select a row |
| <kbd>Ctrl-f/b</kbd> | a screen of rows |
| <kbd>Enter</kbd> | fold a group or bus |
| <kbd>e</kbd> | the whole value |
| <kbd>yy</kbd> <kbd>yt</kbd> | copy value / time |

</section>

<section>

### Change

| Keys | Does |
|---|---|
| <kbd>a</kbd> | add signals |
| <kbd>dd</kbd> | remove |
| <kbd>p</kbd> <kbd>P</kbd> | put below / above |
| <kbd>J</kbd> <kbd>K</kbd> | move down / up |
| <kbd>></kbd> <kbd>&lt;</kbd> | group / ungroup |
| <kbd>R</kbd> | rename the group |
| <kbd>V</kbd> | select rows; Esc ends |
| <kbd>r</kbd> | next radix |
| <kbd>u</kbd> <kbd>Ctrl-r</kbd> | undo / redo |

</section>

<section>

### Search

| Keys | Does |
|---|---|
| <kbd>/</kbd> | search |
| <kbd>n</kbd> <kbd>N</kbd> | next / previous match |

</section>

<section>

### Marks

| Keys | Does |
|---|---|
| <kbd>m</kbd> <kbd>M</kbd> | put / remove a mark |
| <kbd>Tab/S-Tab</kbd> | next / previous mark |
| <kbd>'2..'9</kbd> | jump to a mark |
| <kbd>''</kbd> | back before the jump |

</section>

<section>

### Other

| Keys | Does |
|---|---|
| <kbd>:</kbd> | command |
| <kbd>Space</kbd> | leader menu |
| <kbd>5l</kbd> <kbd>3w</kbd> | a count repeats a key |
| <kbd>q</kbd> | quit |

</section>

</div>

### Search expressions

| Type | Finds |
|---|---|
| `/0x40` | the selected row's value, in any radix |
| `/x  /z` | unknown, high impedance |
| `/valid && ready && data == 0x40` | several signals at once |
| `/@clk fell(valid) && !past(ready)` | sampled at clk; also rose() stable() |

## In the signal panel

`a` opens the panel: the trace's scopes and signals as a tree.

<div class="keys">

<section>

### Move

| Keys | Does |
|---|---|
| <kbd>j</kbd> <kbd>k</kbd> | move |
| <kbd>l</kbd> <kbd>h</kbd> | open / close a scope |
| <kbd>gg</kbd> <kbd>G</kbd> | first / last line |
| <kbd>/</kbd> | search paths |
| <kbd>n</kbd> <kbd>N</kbd> | next / previous match |

</section>

<section>

### Mark

| Keys | Does |
|---|---|
| <kbd>Space</kbd> | mark a signal or scope |
| <kbd>V</kbd> | mark a range |
| <kbd>Ctrl-a</kbd> | mark all matches |

</section>

<section>

### Add

| Keys | Does |
|---|---|
| <kbd>Enter</kbd> | add as rows |
| <kbd>Ctrl-g</kbd> | add as a group |
| <kbd>u</kbd> <kbd>Ctrl-r</kbd> | undo / redo |
| <kbd>a</kbd> | close |
| <kbd>Esc</kbd> | clear marks / close |

</section>

</div>

New signals go below the selected row, at the accent line.

## In the records table

`:records` opens it on a set of streams. The keys are fixed.

| Keys | Does |
|---|---|
| <kbd>j</kbd> <kbd>k</kbd> <kbd>Down</kbd> <kbd>Up</kbd> | move |
| <kbd>Ctrl-f</kbd> <kbd>Ctrl-b</kbd> | a screen of records |
| <kbd>g</kbd> <kbd>G</kbd> | first / last record |
| <kbd>h</kbd> <kbd>l</kbd> <kbd>Left</kbd> <kbd>Right</kbd> | scroll the columns |
| <kbd>/</kbd> | filter; Enter or Esc stops, Ctrl-u clears |
| <kbd>s</kbd> <kbd>S</kbd> | sort by the next column / reverse |
| <kbd>=</kbd> | group by the sorted field; again to ungroup |
| <kbd>Tab</kbd> <kbd>Shift-Tab</kbd> | next / previous set |
| <kbd>Enter</kbd> | jump to a record; on a group, keep its records |
| <kbd>Esc</kbd> <kbd>q</kbd> | close |

## In the picker

`:pick <source>` opens it: typing edits the query. The keys are fixed.

| Keys | Does |
|---|---|
| <kbd>Ctrl-n</kbd> <kbd>Ctrl-p</kbd> <kbd>Down</kbd> <kbd>Up</kbd> | move |
| <kbd>PageDown</kbd> <kbd>PageUp</kbd> | a box of entries |
| <kbd>Backspace</kbd> | delete a character of the query |
| <kbd>Tab</kbd> | mark the signal and move on |
| <kbd>Ctrl-s</kbd> | next source |
| <kbd>Enter</kbd> | pick and close |
| <kbd>Esc</kbd> | close |

## In the decode form

`:decode` alone, or `Space d D`, opens it: typing edits the field. The keys are fixed.

| Keys | Does |
|---|---|
| <kbd>Tab</kbd> <kbd>Down</kbd> <kbd>Shift-Tab</kbd> <kbd>Up</kbd> | next / previous field |
| <kbd>Right</kbd> | take the suggestion |
| <kbd>Ctrl-n</kbd> <kbd>Ctrl-p</kbd> | the next / previous suggestion |
| <kbd>Ctrl-Space</kbd> | pick a signal; on the protocol, the next one |
| <kbd>Backspace</kbd> | delete a character |
| <kbd>Ctrl-u</kbd> | clear the field |
| <kbd>Enter</kbd> | run the line; a field it names keeps the form open |
| <kbd>Esc</kbd> | close |

## In the decodes list

`:decodes`, or `Space d l`, opens it. The keys are fixed.

| Keys | Does |
|---|---|
| <kbd>j</kbd> <kbd>k</kbd> <kbd>Down</kbd> <kbd>Up</kbd> | move |
| <kbd>Enter</kbd> | its records |
| <kbd>a</kbd> | add its results as a group |
| <kbd>e</kbd> | edit it in the decode form |
| <kbd>d</kbd> | dd: remove it, its streams and their rows |
| <kbd>Esc</kbd> <kbd>q</kbd> | close |

## The Space menu

`Space` opens a menu of everything that has no key of its own. Groups open with their key; `Backspace` goes back up.

| Keys | Does | Runs |
|---|---|---|
| <kbd>Space</kbd> <kbd>f</kbd> | find a signal | `:pick signals` |
| <kbd>Space</kbd> <kbd>m</kbd> | marks | `:pick marks` |
| <kbd>Space</kbd> <kbd>r</kbd> | records | `:pick records` |
| <kbd>Space</kbd> <kbd>:</kbd> | commands | `:pick commands` |
| <kbd>Space</kbd> <kbd>n</kbd> | messages | `:messages` |
| <kbd>Space</kbd> <kbd>?</kbd> | keys | `:help` |

### <kbd>Space</kbd> <kbd>v</kbd> view

| Keys | Does | Runs |
|---|---|---|
| <kbd>Space</kbd> <kbd>v</kbd> <kbd>g</kbd> | group the selection… | `:group` |
| <kbd>Space</kbd> <kbd>v</kbd> <kbd>r</kbd> | rename the group… | `:rename` |
| <kbd>Space</kbd> <kbd>v</kbd> <kbd>u</kbd> | ungroup | `:ungroup` |
| <kbd>Space</kbd> <kbd>v</kbd> <kbd>c</kbd> | colour… | `:color` |
| <kbd>Space</kbd> <kbd>v</kbd> <kbd>x</kbd> | radix… | `:radix` |

### <kbd>Space</kbd> <kbd>t</kbd> time

| Keys | Does | Runs |
|---|---|---|
| <kbd>Space</kbd> <kbd>t</kbd> <kbd>f</kbd> | zoom to fit | `:fit` |
| <kbd>Space</kbd> <kbd>t</kbd> <kbd>z</kbd> | zoom to… | `:zoom` |
| <kbd>Space</kbd> <kbd>t</kbd> <kbd>m</kbd> | mark at a time… | `:cursor` |

### <kbd>Space</kbd> <kbd>d</kbd> decode

| Keys | Does | Runs |
|---|---|---|
| <kbd>Space</kbd> <kbd>d</kbd> <kbd>d</kbd> | decode an interface: the ones found in the trace, each with its line | `:pick interfaces` |
| <kbd>Space</kbd> <kbd>d</kbd> <kbd>l</kbd> | decodes: the ones made: records, rows, edit, remove | `:decodes` |
| <kbd>Space</kbd> <kbd>d</kbd> <kbd>D</kbd> | decode with a form…: name, protocol, clock and roles as fields, checked as you fill them in; on a V range, its rows play the roles | `:decode` |
| <kbd>Space</kbd> <kbd>d</kbd> <kbd>p</kbd> | protocol with a form…: roles from the selected rows, when and the fields checked as you type | `:protocol` |
| <kbd>Space</kbd> <kbd>d</kbd> <kbd>m</kbd> | records of the matches…: the last clocked search as a protocol: its when, the shown signals as fields | `:protocol search` |
| <kbd>Space</kbd> <kbd>d</kbd> <kbd>s</kbd> | computed signal… | `:signal` |
| <kbd>Space</kbd> <kbd>d</kbd> <kbd>n</kbd> | name values… | `:names` |
| <kbd>Space</kbd> <kbd>d</kbd> <kbd>r</kbd> | records where… | `:records` |

### <kbd>Space</kbd> <kbd>a</kbd> agent

| Keys | Does | Runs |
|---|---|---|
| <kbd>Space</kbd> <kbd>a</kbd> <kbd>f</kbd> | follow…: the window shows its workspace as it works | `:follow` |
| <kbd>Space</kbd> <kbd>a</kbd> <kbd>F</kbd> | stop following: your own view back | `:follow off` |
| <kbd>Space</kbd> <kbd>a</kbd> <kbd>t</kbd> | trail: its steps; Enter jumps to one's time | `:pick trail` |
| <kbd>Space</kbd> <kbd>a</kbd> <kbd>j</kbd> | join a session…: the agents working on this machine | `:pick sessions` |
| <kbd>Space</kbd> <kbd>a</kbd> <kbd>s</kbd> | the agent's session: what it left while no window was open | `:agent` |

### <kbd>Space</kbd> <kbd>q</kbd> session

| Keys | Does | Runs |
|---|---|---|
| <kbd>Space</kbd> <kbd>q</kbd> <kbd>w</kbd> | save: rows, marks and decodes, beside the trace | `:w` |
| <kbd>Space</kbd> <kbd>q</kbd> <kbd>e</kbd> | reload the trace: after the simulation ran again | `:e` |
| <kbd>Space</kbd> <kbd>q</kbd> <kbd>q</kbd> | quit: refused with unsaved changes | `:q` |
| <kbd>Space</kbd> <kbd>q</kbd> <kbd>x</kbd> | save and quit | `:wq` |
| <kbd>Space</kbd> <kbd>q</kbd> <kbd>Q</kbd> | quit without saving: unsaved changes are lost | `:q!` |

## Commands

`:` opens the command line; `Tab` completes, and the card above it says what each command does.

| Command | Does |
|---|---|
| `:<time>` | Move the cursor to a time, e.g. :2000ns |
| `:zoom <time>` | Set the time per column, e.g. :zoom 1ns |
| `:add <path\|pattern>...` | Add signals below the selected row (after a selected group); a scope becomes a group, e.g. :add top.cpu.* |
| `:rm [path\|pattern]...` | Remove matching rows, or the selected row; :rm ** removes all |
| `:signal <name> = <signal>` | Name a computed signal, e.g. :signal op = cmd[7:0] or :signal hdr = {a, b} |
| `:decode <name> <protocol> <role>=<signal>...` | Decode a protocol into streams, e.g. :decode rx axis clock=clk tvalid=rx_tvalid tready=rx_tready tdata=rx_tdata tlast=rx_tlast; alone, a form that fills it in |
| `:decodes` | List the decodes made: Enter opens records, a adds rows, e edits, dd removes |
| `:protocol <name> roles=<a,b> when=<expr> <field>=<expr>...` | Define a protocol on its roles, a record per cycle where when holds, e.g. :protocol cmd roles=valid,ready,data when=valid && ready op=data[7:0]; :decode puts it on signals; alone, a form with the selected rows as roles; :protocol search, one filled in from the last clocked search |
| `:names <signal> <value>=<name>...` | Name a signal's values, shown by the enum radix and searched by name, e.g. :names state 0=IDLE 1=BUSY; off removes them |
| `:records <streams> [where <expr>]` | Browse records as a table: a stream, a decode's results (:records bus), a glob; / filters, s sorts, = groups |
| `:scope [path]` | Resolve names under a scope first, e.g. :scope top.cpu makes state top.cpu.state; none without a path |
| `:radix <name>` | Set the radix: hex bin dec sdec oct ascii float enum, q4.12 / uq4.12 fixed point |
| `:color <name\|off>` | Colour the selected row or group: red orange yellow green cyan blue magenta gray; off for the theme's |
| `:group <name>` | Group the selected rows (V selects them) |
| `:rename <name>` | Rename the group the selected row is in |
| `:ungroup` | Dissolve the group the selected row is in, keeping its signals |
| `:select <row> [<last row>]` | Select a signal or group by name, or the rows from one to another |
| `:pick <source>` | A fuzzy list with a preview: signals, marks, records <stream>, commands, sessions, trail, interfaces; Ctrl-s goes to the next |
| `:messages` | Every message so far, with its time and who caused it: you or an agent |
| `:trail` | The agent's steps; Enter jumps to a step's time |
| `:cursor <n> [time\|off] [name=] [note=]` | Put mark n at a time (1: the cursor), name it, remove it, or jump to it, e.g. :cursor 2 120ns name=bad write |
| `:undo` | Undo the last change to the rows |
| `:redo` | Redo a change undone with :undo |
| `:fit [<from> <to>]` | Zoom to the whole simulation, or between cursors or times, e.g. :fit '2 '3 or :fit 10ns 20ns |
| `:agent` | Show the session an agent left for this trace when no window was open |
| `:follow [<id>\|off]` | Follow an agent working on this trace: show its workspace and each change it makes |
| `:e` | Reload the trace, keeping signals, groups, cursor and zoom |
| `:w [file]` | Save the session |
| `:wq` | Save the session and quit |
| `:help [topic]` | Show the keys of the current mode (also ?), or a topic: names times radix expressions decode records workspace commands |
| `:q` | Quit; refuses if the session has unsaved changes |
| `:q!` | Quit without saving |

## Names for `[keys]`

Each key's name in the config file's `[keys]` table, with its default. A count before a key marked *count* repeats it (`5l`), or numbers a mark (`3m`).

```toml
[keys]
zoom_in = "i"       # z
start = "<C-g>"     # gg
```

| Name | Default | Does |
|---|---|---|
| `left` | `h` | cursor left, *count* |
| `right` | `l` | cursor right, *count* |
| `next_change` | `w` | next change, *count* |
| `prev_change` | `b` | previous change, *count* |
| `down` | `j` | next row, *count* |
| `up` | `k` | previous row, *count* |
| `fold` | `<CR>` | open / fold the row |
| `examine` | `e` | the whole value |
| `start` | `gg` | start of the trace |
| `end` | `G` | end of the trace |
| `half_right` | `<C-d>` | half a screen right, *count* |
| `half_left` | `<C-u>` | half a screen left, *count* |
| `screen_left` | `H` | screen left, *count* |
| `screen_right` | `L` | screen right, *count* |
| `center` | `c` | center the cursor |
| `page_down` | `<C-f>` | a screen of rows down, *count* |
| `page_up` | `<C-b>` | a screen of rows up, *count* |
| `zoom_in` | `z` | zoom in, *count* |
| `zoom_out` | `x` | zoom out, *count* |
| `panel` | `a` | add signals |
| `remove` | `dd` | remove the row |
| `move_down` | `J` | move the row down, *count* |
| `move_up` | `K` | move the row up, *count* |
| `range` | `V` | select rows |
| `radix` | `r` | next radix |
| `group` | `>` | group the rows |
| `ungroup` | `<` | out of the group |
| `rename` | `R` | rename the group |
| `put` | `p` | put removed rows below |
| `put_above` | `P` | put removed rows above |
| `undo` | `u` | undo |
| `redo` | `<C-r>` | redo |
| `copy_value` | `yy` | copy the value |
| `copy_time` | `yt` | copy the cursor's time |
| `search` | `/` | search a value |
| `next_match` | `n` | next match, *count* |
| `prev_match` | `N` | previous match, *count* |
| `mark` | `m` | put mark N, *count* |
| `unmark` | `M` | remove mark N, *count* |
| `next_mark` | `<Tab>` | next mark |
| `prev_mark` | `<S-Tab>` | previous mark |
| `jump` | `'` | jump to a cursor |
| `jump_back` | `''` | back before the jump |
| `command` | `:` | command |
| `help` | `?` | keys |
| `quit` | `q` | quit |
| `leader` | `<Space>` | the leader menu |
