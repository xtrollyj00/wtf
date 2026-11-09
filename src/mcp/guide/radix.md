Radixes: hex, bin, dec, sdec (signed), oct, ascii, float, fixed point like q4.12, and enum (the
names the file gives values, as `state` READ). The default is enum where the file names values,
otherwise hex; 1-bit signals always show 0 / 1 / X / Z.
- A call's `radix` sets it for all its signals; `data:dec` for one signal.
- Rows (`help workspace`) take a radix and a colour: `awaddr:dec`, `wvalid:red`,
  `awaddr:hex:red`. Colours: red orange yellow green cyan blue magenta gray (`off` removes it).
