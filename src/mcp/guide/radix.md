How values are shown, and the colours of rows.

Form
- hex, bin, dec, sdec (signed), oct, ascii, float, fixed point like q4.12, and enum (the names
  the file gives values, as `state` READ).
- Colours: red orange yellow green cyan blue magenta gray; `off` removes one.

Recipes
- Every signal of a call in decimal: the call's `radix` `dec`.
- One signal in another radix: `data:dec`, wherever tools take signals.
- A row with a radix and a colour: `awaddr:hex:red`, `wvalid:red` (`help workspace`).

Details
- The default is enum where the file names values, otherwise hex; 1-bit signals always show
  0 / 1 / X / Z.
