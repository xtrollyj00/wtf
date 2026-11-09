`command` runs one of wtf's `:` commands, without the colon, on the workspace.
- Rows: `add <path|pattern>...` (below the selected row, joining its group; a scope becomes a
  group of its direct signals, a struct or array one row), `rm <path|pattern>...` (`rm {all}` clears; patterns: `help names`), `select <row> [<last row>]` (the rows the next
  commands act on), `group <name>`, `rename <name>`, `ungroup`, `radix <radix>`, `color
  <colour|off>` (`help radix`), `names <signal> <value>=<name>...` (names for its values, as an
  FST enum table gives them; `off` removes them), `undo`, `redo`. The answer lists the rows added and removed.
- Cursors and view: `cursor <1-9> [time|off] [name=<text>] [note=<text>]` (without a time or
  keys, jump to that mark; `name=` / `note=` say what a mark shows), `<time>`
  (moves the cursor), `zoom <time per column>`, `fit [<from> <to>]` (all, or between cursors
  `'2 '3` or times).
- Names and times: `scope <path>` (`help names`), `unit <ns|ps|...|auto>` (`help times`).
- Decoding: `decode`, `protocol`, `signal` (`help decode`).
