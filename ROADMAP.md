# Roadmap

Where wtf is going. The details are designed in [`spec/`](spec/README.md) as each piece starts;
nothing here is promised, and issues and pull requests can move it.

wtf is built around one workflow: a simulation fails, an agent opens the trace, finds the cause,
shows you the evidence in your window, fixes the design and checks the fix. The roadmap follows
it.

## Done

A Vim-like viewer for VCD and FST, with sessions, search, cursors, computed signals and protocol
decoders, and an MCP server through which agents debug a trace and present what they found. Themes,
glyph sets for any font (asked on the first start), the mouse, and a [user guide](docs/guide.md).

## Next

1. **Finding the cause.** Agents get from the failing time to the root cause in fewer calls:
   better overviews of a trace, richer queries, shorter answers.
2. **Showing the evidence.** Cheaper ways for an agent to check what it is about to show you, and
   views focused on the events that matter.
3. **Easy to start, measured.** Release binaries, so no Rust is needed to install; one step sets
   up the server and a debugging workflow for the agent; a benchmark of real debugging tasks
   judges every change made for agents.
4. **Checking the fix.** Comparing a run with the one before it, to confirm the bug is gone.
5. **Big traces.** Comfortable with the gigabyte dumps of real regressions.

## Later

Remote traces, a demo in the browser, and the [known limits](spec/18-deferred.md#known-limits) as they
get in the way.
