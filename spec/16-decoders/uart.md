## 16.5 UART (uart)

UART samples by time too, and takes no `clock`: its bit times come from the trace's timescale, so
a trace without one is an error that says so. Its settings are params of the instance: `baud`,
`bits`, `parity` (`none`, `even`, `odd`), `stop`, `idle` (`high`, the default, or `low`, which
reads the line inverted as RS-232 levels do). It makes both streams always: `byte` with `data`,
and `errors` with `rule`, `detail` and `severity`. A wrong stop bit (`framing`) or `parity` bit is
an error; the line low for a whole frame is a `break`, a warning, with no byte.

```
decode console uart line=uart_tx baud=115200
```

| role | required | it is |
| --- | --- | --- |
| `line` | yes | the serial line, idle high (`idle=low`: inverted, as RS-232 levels are) |

Params: `baud` (the bit rate), `bits` (`8`), `parity` (`none`, or `even`, `odd`), `stop` (`1`),
`idle` (`high`, or `low`).

Streams:

| stream | it is |
| --- | --- |
| `<name>.byte` | each frame's data bits, sampled in the middle of each bit at `baud` |
| `<name>.errors` | a stop bit that isn't 1 (framing), a wrong parity bit (parity), the line low for a whole frame (break, no byte); `rule` names it, `detail` says what it saw, `severity` is how bad |
