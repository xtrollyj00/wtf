# Roadmap

Where wtf is going. The details are designed in [`spec/`](spec/README.md) as each piece starts;
nothing here is promised, and issues and pull requests can move it.

wtf is built around one workflow: a simulation fails, an agent opens the trace, finds the cause,
shows you the evidence in your window, fixes the design and checks the fix. The roadmap follows
it.

## Done

A Vim-like viewer for VCD and FST, with sessions, search, cursors, computed signals, protocol
decoders and composite types (structs, records and arrays as one row), and an MCP server through
which agents debug a trace and present what they found. Themes, glyph sets for any font (asked on
the first start), the mouse, and a [user guide](docs/guide.md). Release binaries for Linux and
macOS with an install script, and an agent benchmark (`wtf-bench agent`) that measures wtf against
debugging without it.

## Next

1. **Finding the cause.** Agents get from the failing time to the root cause in fewer calls:
   better overviews of a trace, richer queries, shorter answers.
2. **Showing the evidence.** Cheaper ways for an agent to check what it is about to show you, and
   views focused on the events that matter.
3. **Easy to start, measured.** One step sets up the server and a debugging workflow for the
   agent; harder benchmark tasks, from designs not covered yet, so every change made for agents
   is judged.
4. **Checking the fix.** Comparing a run with the one before it, to confirm the bug is gone.
5. **Big traces.** Comfortable with the gigabyte dumps of real regressions.

## Later

- **Remote traces**, for traces on a machine reached over ssh, where drawing remotely lags. Until
  then: mount the directory with sshfs and run wtf locally, or use mosh. Sketch: `wtf -r
  host:path/run.fst` starts `ssh host wtf --serve path/run.fst` and talks to it over stdin /
  stdout (no ports, no daemon). The seam is `Trace`: the rest of wtf only asks it for values and
  changes, thousands of times per frame, so those can't be remote calls. The client fetches the
  hierarchy once and each signal's changes when it is added, and answers queries locally. Open
  points: a wire format for signal data, version checks between client and server, fetching
  huge signals by time window, and `:e` and errors over the link.
- **A demo in the browser.**
- **Subgroups** (nested groups), and groups made from the file's scopes.
- **Cursors**: names, more than 9.
- **Config** (spec §19): a default radix, key remapping.

Shortcuts taken so far, each short of a spec section, are the
[`known-limit` issues](https://github.com/xtrollyj00/wtf/issues?q=is%3Aissue+label%3Aknown-limit).
