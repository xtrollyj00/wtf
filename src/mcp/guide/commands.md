The `:` commands `command` runs on the workspace.

Form
- A command without the colon, as the window takes it: `add top.cpu`, `cursor 2 80ns`.

Recipes
- Add rows: `add <path|pattern>...`, below the selected row, joining its group; a scope becomes
  a group of its direct signals, a struct or array one row. Remove: `rm <path|pattern>...`;
  `rm {all}` clears (`help patterns`).
- Act on rows: `select <row> [<last row>]`, then `group <name>`, `rename <name>`, `ungroup`,
  `radix <radix>`, `color <colour|off>` (`help radix`).
- Name a state machine's values: `names <signal> <value>=<name>...`, as an FST enum table gives
  them; `off` removes them.
- Mark an event: `cursor <1-9> [time|off] [name=<text>] [note=<text>]`; without a time or keys,
  jump to that mark.
- Look at a stretch: `fit [<from> <to>]` (all, or between cursors `'2 '3` or times);
  `zoom <time per column>`; `<time>` moves the cursor.

Details
- Names and times: `scope <path>` (`help names`), `unit <ns|ps|...|auto>` (`help times`).
- Decoding: `decode`, `protocol`, `signal` (`help decode`).
- `undo`, `redo`. The answer lists the rows added and removed.
