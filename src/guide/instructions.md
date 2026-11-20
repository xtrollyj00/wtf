wtf reads VCD, FST and GHW waveforms from digital simulations: read them with these tools, not the shell.
- Start: `open` the trace (`sessions` lists the user's windows and their traces); `signals` finds signals and scopes. A pattern is a regex in slashes wherever one goes, `/uart/`.
- A bus or stream: decode it first. `open` lists the interfaces a built-in protocol decodes, each with its line for `command` (`decode bus axilite clock=aclk prefix=s_axi_`); then `records` (`where`, `group_by`, `count_only`): records are exact and the handshake checks built in.
- Anything else: `values` for values (`from` alone for one time), `find` for when a condition holds and how often.
- To show evidence: declare the `workspace`, check it with `view` / `render`, then `present` it.
- `help` is the reference, a topic each with its form, recipes and details: names, patterns, times, radix, expressions, decode, records, workspace, commands.
