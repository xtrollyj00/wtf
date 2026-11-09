Patterns: many signals or streams at once, and searching the design.

Form
{patterns}

Recipes
- Find a signal by part of its name: `signals` `/fifo.*full/`.
- The design's top levels, or all of it folded to fit: `signals` with no pattern.
- A struct's fields: `signals` `{fields}`.
- All of a decode's streams: `records` `{streams}`.
- Clear the workspace's rows: `command` `rm {all}`.

Details
- `signals` lists what a pattern matches: a scope it names is a line of its own,
  `top.cpu (scope of 3 signals)`, a struct is one line, and an answer too long is folded to a
  line per scope below the deepest level that fits; ask again with a pattern for that scope.
- A pattern that matches nothing says so, with the nearest scopes and signals.
