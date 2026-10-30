## 11. Sessions

A session is everything needed to reopen a trace exactly as you left it:

- **layout**: rows and their order, groups and whether they are collapsed, radixes, colours, expanded buses,
- **view**: the cursor and the marks, zoom, visible time.

### Saving and loading

- `:w` saves to the session file in use. If there is none, it saves to `<trace>.session.toml` next to
  the trace (`run1.fst` → `run1.fst.session.toml`). `:w <file>` saves elsewhere, and that file is in use from then on.
- On open, `<trace>.session.toml` is loaded automatically if it exists.
- `wtf -S <file> <trace>` loads a specific session instead, e.g. one session shared by many runs
  of the same testbench. That file becomes the session file in use. If it doesn't exist yet,
  wtf starts with an empty session and `:w` creates it.
- When loading, signals that don't exist in the trace are skipped and listed on the status line.
  Cursors and the visible time past the end of the simulation move to the end. A group nested
  inside a group is flattened, with a warning.
- A failed save never leaves a half-written session file; the old file stays and the error is shown.

### Unsaved changes

- Changes to the layout mark the session as modified, including collapsing a group or expanding a bus.
  `[+]` compares the layout with the one last saved or loaded, so undoing back to it clears `[+]`.
  The status line then shows `[+]`, as in Vim.
- Cursors (adding, removing, moving), zooming and scrolling are saved by `:w` but don't mark the
  session as modified, so browsing and measuring never block quitting.
- `q` / `:q` refuse while the session is modified: `unsaved session (:w to save, :q! to quit)`.
  `:wq` saves and quits, `:q!` quits without saving.

### File format

TOML, so it is easy to read, edit, diff and generate from testbench scripts, and standard
libraries parse it.

```toml
# session
version = 1
scope = "top"           # optional: names resolve under it first (:scope)
unit = "ns"             # optional: the time unit an agent sets (command unit ns); absent when it is automatic

[view]
zoom = "8ns"
time = "0ns"            # left edge of the view

[cursors]
1 = "120ns"
2 = "160ns"

[[signal]]
name = "op"
expr = "top.cpu.cmd[7:0]"

[[rows]]
signal = "top.cpu.clk"

[[rows]]
group = "cpu"
rows = [
  { signal = "top.cpu.state", radix = "dec", color = "orange", open = ["state"] },
  { signal = "top.cpu.pc" },
  { signal = "top.cpu.pkts", open = ["pkts", "pkts[1]", "pkts[1].pair"] },
]

[[rows]]
group = "stream"
collapsed = true
rows = [
  { signal = "top.stream.data" },
]
```

- `scope` is the `:scope` names resolve under first, set before anything else is read.
- `unit` is the time unit an agent sets with `command` `unit ns` (the window refuses `:unit`),
  applied with the session; a name wtf doesn't know is a warning. The unit the clocks give is not
  saved: it comes back as decodes run again.
- `version` is the format version. A file with a newer version than wtf knows is refused
  with an error instead of being misread.
- `[[protocol]]` entries (after `[[signal]]`, before the rows) sample streams: `name`, `clock`,
  `when` and `fields` in their order (section 16). Each stream is a signal, and so is
  each of its fields (`rx.tdata`), so rows can show them; they are sampled again on `:e`. One that
  can't be is reported and kept, to try again on the next `:e`. Named signals, protocols and
  instances are made in passes, so one that uses one listed after it is made too.
  A protocol with `roles = ["valid", "ready", "cmd"]` is a type instead: its `when` and `fields`
  name the roles, its `clock` may be left out, and `[[decode]]` entries (`name`, `protocol`,
  optional `clock`, `signals = { valid = "top.cmd_valid", ... }`) put it on signals, each making
  the stream of its name. All of an instance's mistakes are reported at once.
  `[[decode]]` also puts built-in protocols on signals (`protocol = "axis"`, making
  `rx.beat`, `rx.frame`, `rx.errors`), and the protocols on streams, with
  `params`: `frame` (`signals = { input = "c0" }`, `params = { last = "last" }`, or `gap`,
  `select`, `bytes`), `pair` (`signals = { request = "req", response = "resp" }`, `params = { id =
  "id" }`) and `check` (no signals, a `clock`, `params = { stall = "valid && !ready" }`: a rule's
  name and the expression that is a violation).
- `[[names]]` entries give a signal's value names (`:names`, for the `enum` radix): `signal` and
  `values`, in the form `:names` takes (`values = "0=IDLE 1=BUSY"`), written before the rows.
- `[[signal]]` entries name computed signals (`:signal`), made before the rows, which can show
  them. Rows can also show unnamed slices and concatenations by their full form
  (`signal = "top.cpu.data[7:4]"`, `signal = "{top.a,top.b}"`).
- `[[rows]]` entries are shown in file order. An entry is either a signal (`signal`, optional `radix`,
  `color` and `open`, written only where they differ from the defaults)
  or a group (`group`, optional `collapsed`, and its own `rows`). The same shape inside groups
  leaves room for subgroups later.
- `open` lists what is open under a row (section 6): the row's own name opens a bus into its
  bits, a stream into its fields, a struct, record or array one level (`open = ["state"]`); its
  levels are named from the row's name on (`pkts`, `pkts[1]`, `pkts[1].pair`), parents before
  their children, and a bus in it named there shows its bits. A name in `open` that isn't in the
  row is skipped with a warning, so a trace whose types changed still loads.
- Times are strings with units, as in commands. A plain number means file ticks. wtf writes times
  exactly: in the largest unit that keeps them at 1 or more, with every decimal needed
  (`16.432us`, `2.595000001us`), read back to the same tick. A zoom finer than one tick per column
  is written as a fraction of a tick.
- Unknown keys are skipped with a warning, so files with extra settings still load.

## 12. Command line

As few options as possible. Everything that describes what to show (signals, groups, radixes,
cursors, zoom, time) lives in the session file (section 11), not on the command line.

```
wtf [-S <session>] [--glyphs <set>] [--snapshot <width>[x<height>] [--ansi] [--keys <keys>]] <trace>
wtf --attach [<id>] [--glyphs <set>] [--snapshot <width>[x<height>] [--ansi] [--keys <keys>]]
wtf --setup [<trace>]
wtf --mcp
wtf --help
wtf --version
```

| Option | Meaning |
|---|---|
| `<trace>` | the VCD or FST file to open; required |
| `-S <session>` | load this session instead of `<trace>.session.toml` |
| `--glyphs <set>` | draw with this glyph set: `unicode`, `box` or `ascii` (section 14), over the config file's (section 19) |
| `--snapshot <width>[x<height>]` | render one screen as plain text to stdout and exit, without the terminal UI; without a height, as tall as the content |
| `--ansi` | with `--snapshot`: print the screen in its colours, as SGR escape codes, with the configured theme (section 19); `NO_COLOR` gives the reversed cells of a terminal without colour |
| `--keys <keys>` | with `--snapshot`: press these keys first, the screen drawn before each as the window draws it, so a mode or an overlay can be shown (`?`, `i`, `:` with its completions, `e`). Written as in Vim's mappings: plain characters, `<Esc>`, `<CR>`, `<Tab>`, `<BS>`, `<Space>`, `<lt>` for `<`, `<C-x>`; another `<name>` is an error, a `<` that starts no name is typed as it is |
| `--attach [<id>]` | without an id, list the open sessions (windows and agent workspaces); with one, open a window on that session's trace showing its rows and cursors (section 15) |
| `--setup` | ask again which glyphs and theme to use, as on the first start (section 19), then open `<trace>` if given |
| `--mcp` | serve AI agents over MCP on stdin / stdout instead of opening the screen (section 15) |

- `--snapshot` is for CI logs, bug reports and wtf's own tests. Each screen cell becomes exactly
  one character, the same glyph the terminal UI draws, so columns stay aligned. What it shows comes
  from the session.
- Snapshots have no colours, unless `--ansi`; the cursor's column is drawn as `┃` (`|` in `ascii`), marks as `╎`
  (`:`). The terminal UI uses colours unless `NO_COLOR` is set (section 14).

## 13. Formats

- VCD and FST. No other formats are planned.
- The format is detected from the file's content, not its extension.
