+++
title = "Troubleshooting"
description = "Boxes instead of lines, missing icons, odd colours, clicks that don't arrive, and big traces."
+++

## Boxes or question marks in the waveforms

A bus's outline uses six characters from Unicode 13's "legacy computing" block, so its segments
close in a point. Few fonts have them; Iosevka does, and some terminals draw them themselves.
If yours shows boxes, use the `box` glyph set, which draws the same thing with box drawing only:

```screen
unicode   ─🮦────🮦─────────🮦────────     box   ─┬────┬─────────┬────────
          ─🮧────🮧── 0x2 ──🮧── 0x3 ─           ─┴────┴── 0x2 ──┴── 0x3 ─
           🮣────🮦────🮢    🮣────🮢               ┌────┬────┐    ┌────┐
           🮡 #0 🮧 #1 🮠    🮡 #2 🮠               └ #0 ┴ #1 ┘    └ #2 ┘
```

```toml
# ~/.config/wtf/config.toml
glyphs = "box"
```

The first time wtf starts without a config file, it shows a sample in each set and asks which one
your terminal draws cleanly, so you rarely need to set this by hand. `--glyphs box` tries a set
for one run.

If even the box lines don't join up, the font lacks box drawing too: use `ascii`, which also suits
logs, and pasting a snapshot into an issue.

## Icons show as boxes

The `nerd` set puts an icon before names and in menus, from a [Nerd Font](https://www.nerdfonts.com/font-downloads)
3.4 or newer. Set the terminal's font to one, or pick `unicode`, which draws everything but the
icons. A Nerd Font older than 3.4 lacks a few of them.

## The colours look wrong

### Washed out, or the wrong shades

wtf draws in 24-bit colour when the terminal says it can
(`COLORTERM=truecolor`, or a terminal it knows by `TERM`: kitty, WezTerm, Alacritty, foot,
Ghostty), and in the nearest of 256 colours otherwise. Over ssh `COLORTERM` is often lost: set
it again on the remote side.

### Dark on dark, or light on light

Pick the other theme: `theme = "light"` or `"dark"`. `dark`
keeps your terminal's background and text; `light` paints its own.

### One colour bothers you

Change it alone under `[colors]`, as `cursor = "#e5c07b"` or a
256-colour index (`panel = 235`). A wrong key is said on the status line, with the keys allowed.

### No colour at all

`NO_COLOR` is set. wtf then marks the cursor and the selected row by
reversing them, and draws marks with a glyph.

## Clicks with Shift or Cmd do nothing

The terminal decides which clicks reach wtf. Most keep `Shift` + click for selecting text, and
macOS terminals send no `Cmd`: use `Ctrl` + click instead, or `V` to select rows from the keyboard.
kitty (`terminal_select_modifiers`) and WezTerm (`bypass_mouse_reporting_modifiers`) can pass
`Shift` on.

## A big trace is slow to open

### Prefer FST

A VCD has no index, so wtf reads it whole when it opens it, and its memory grows
with the file: about 13.5 GB for a 10 GB VCD. An FST opens by its hierarchy alone and reads a
signal's values when you add it. Most simulators write FST directly (Verilator `--trace-fst`,
Icarus `-fst`), and GTKWave's `vcd2fst` converts.

### Add what you look at

Memory grows with the signals shown, not the hierarchy: start from a
few, or from a decode, which reads only its interface's signals.

### Dump less

A window of simulated time around the failure keeps every file small.

How fast wtf is on 10 GB traces, and what is still being made faster, is measured in
[`bench/RESULTS.md`](https://github.com/xtrollyj00/wtf/blob/main/bench/RESULTS.md).

## Still stuck

[Open an issue](https://github.com/xtrollyj00/wtf/issues) with what you ran and what you saw.
`wtf <trace> --snapshot 120x30 --glyphs ascii` prints the screen as text, ready to paste.
