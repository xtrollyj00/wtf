To show the user evidence, declare the workspace, check it with `view` / `render`, then `present`
it into the user's wtf windows on the trace (`:undo` takes it back). With none open, present
writes a session file that `wtf <trace>` shows when the user opens the trace: tell them so. The
user also sees each query and change you make as a step (`:trail`), with its time, so each query
is part of what they see. `workspace` replaces everything at once, as one undo step:
- `rows`, top to bottom: `"name"`, `"name:radix"`, `"name:radix:colour"` (`help radix`), or a
  group `{"group": "write", "rows": ["awaddr", "awvalid"], "collapsed": false}`. Groups
  hold signals only. A struct or array is one row (`"pkts"`); `{"signal": "pkts", "open":
  ["pkts", "pkts[1]"]}` opens levels of it, each named from the row's name; `"fields":
  ["pkts[1].pair.hi:dec:red"]` gives lines under a row a radix and colour of their own.
- `cursors` by number, `{"1": "80ns", "2": "390ns"}`: 1 is the cursor, where values are
  read; 2 - 9 are marks on the events to show. The user jumps to a mark with 'N. Say what a mark
  shows with `{"time": "390ns", "name": "first bad write", "note": "AW before W, so the
  address latched is stale"}`: the name stands on the ruler, the note on the user's status line
  when they jump there.
- `range`: the visible range, `["60ns", "480ns"]`; default the whole trace.
- `scope` (`help names`); `signals`: computed signals, `["op = cmd[7:0]"]`.
- `protocols`: `{name, roles, when, fields: ["op = data[7:0]"]}`, as `protocol` defines them;
  `decode`: `{name, protocol, clock, signals: ["tvalid = rx_tvalid"], params: ["prefix =
  rx_"]}`, as `decode` does (`help decode`). Streams can then be rows.
Every name, radix and time is checked first and all the errors come back at once. `command`
edits the workspace step by step instead (`help commands`).
