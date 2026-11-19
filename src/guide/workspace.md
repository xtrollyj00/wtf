Showing the user evidence.

Form
- `workspace` sets everything at once, as one undo step, in the session's own shape (spec §11):
  `signal`, `protocol`, `decode`, `names`, `rows`, `cursors`, `view`, `scope`, `unit`. The
  session validator checks it, so a bad name, radix or time is worded as wtf words it for a
  session file; all the errors come back at once and a call that fails changes nothing.
- `view` describes it, `render` draws it as text, `present` shows it to the user.

Recipes
- Show a finding: declare the `workspace`, check it with `view` / `render`, then `present` it
  into the user's wtf windows on the trace (`:undo` takes it back).
- Point at an event: a mark, `{"2": {"time": "390ns", "name": "first bad write", "note": "AW
  before W, so the address latched is stale"}}`; the user jumps there with '2.
- Build on what the user has open: `user_view` `adopt`, then `command` steps.

Details
- `signal`: computed signals, `[{"name": "op", "expr": "cmd[7:0]"}]`.
- `protocol`: sampled protocols, `[{"name": "cmd", "roles": ["valid", "ready", "data"], "when":
  "valid && ready", "fields": [{"op": "data[7:0]"}]}]`, as `protocol` defines them (`fields` in
  order: an array of single-key tables; a table works for session parity but sorts); `decode`
  puts them on signals: `[{"name": "c0", "protocol": "cmd", "clock": "clk", "signals":
  [{"valid": "cmd_valid"}], "params": [{"prefix": "rx_"}]}]` (`help decode`). Streams can then
  be rows.
- `names`: value names, `[{"signal": "state", "values": "0=zero"}]`.
- `rows`, top to bottom: `{"signal": "awaddr", "radix": "hex", "color": "red"}`, a group
  `{"group": "write", "rows": [...], "collapsed": false}`, an open struct `{"signal": "pkts",
  "open": ["pkts", "pkts[1]"], "fields": [{"field": "pkt.pair.hi", "radix": "dec"}]}`. Groups
  hold signals only.
- `cursors` by number, `{"1": "80ns", "2": "390ns"}`: 1 is the cursor, where values are read;
  2 - 9 are marks on the events to show. A mark's name stands on the ruler, its note on the
  user's status line when they jump there.
- `view`: the visible range, `{"time": "60ns", "to": "480ns"}`; default the whole trace.
- `scope` (`help names`).
- `unit`: the time unit of the answers, `ns`, or `auto` for the one that fits (`help times`).
- A key the tool doesn't take, at the top, is an error naming those it does; inside an entry the
  validator names the key it didn't take. The session file's `version` and `trail` are not
  declared: the trail is your calls.
- A key of the wrong kind, a string where a list goes, is an error naming the kind it takes.
- With no window open, `present` writes a session file that `wtf <trace>` shows when the user
  opens the trace: tell them so. The user also sees each query and change you make as a step
  (`:trail`), with its time, so each query is part of what they see.
- `command` edits the workspace step by step instead (`help commands`).
