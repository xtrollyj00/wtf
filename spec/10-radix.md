## 10. Radix

Each bus row shows its value in a radix. A new row starts in hex, whatever its width, or in `enum` if the file names
its values.

| Name | Example | Notes |
|---|---|---|
| `hex` | `0x4000` | leading zero digits dropped |
| `bin` | `0b1010` | leading zero digits dropped |
| `dec` | `16384` | unsigned, any width |
| `sdec` | `-12` | two's complement over the signal's width |
| `oct` | `0o17` | leading zero digits dropped |
| `ascii` | `Hi` | one character per byte; `.` for bytes that aren't printable, `?` for unknown ones; leading zero bytes dropped |
| `float` | `0.1`, `1e300` | IEEE 754 half, single or double, for 16-, 32- and 64-bit signals |
| `q<m>.<n>` | `-1.25` | signed fixed point: `m` integer bits including the sign, `n` fraction bits (up to 38), `m + n` = the signal's width; exact |
| `uq<m>.<n>` | `1.25` | unsigned fixed point |
| `enum` | `READ` | the name the file gives the value (an FST enum table, as Verilator writes SystemVerilog enums; enums written as strings, as nvc writes VHDL ones, are text already), or one given with `:names` (section 7); a value without a name in hex; only for signals with names |

- **Changing**: `r` on the rows side of Edit mode cycles the selected row through the list above,
  skipping the ones that don't fit its width (`enum`: a signal without names) and fixed point, which needs its numbers. On a group
  header it sets every signal in the group that the radix fits. `:radix <name>` sets one directly
  (`:radix q4.12`). Both are undoable.
- **Unknown bits**: hex, binary and octal show `X` (unknown), `U` (uninitialized) and `Z` (high impedance)
  per digit. A decimal, float or fixed-point value with any unknown or uninitialized bit shows as `x`, with any
  high-impedance bit as `z`.
- 1-bit signals are drawn as lines and have no radix.
- **Search is independent of the radix**: `/0x40` finds the value however the row shows it, and
  on a signal with names `/READ` finds the value named so, in any radix (section 8).
  A negative number (`/-12`) matches the two's complement value of the signal's width.

### Extending

Radixes must be easy to add. A radix is one self-contained definition:

- a **name**, used by `:radix` and shown in the command line's completion,
- a **format** rule from a value (including unknown bits) to the label text,
- optionally, a **parse** rule from search text to a value, if search should accept that notation.

Adding a radix means adding one definition. `r`, `:radix`, completion, the value column and
search pick it up without other changes. The order of the list is the order `r` cycles through.
