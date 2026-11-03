# The wtf site

A [Zola](https://www.getzola.org) site. Its screens are wtf's own output: `tools/screens.py` runs
each screen in `screens.toml` (`wtf --snapshot --ansi --glyphs nerd`, in an empty home of its
own) and draws it as SVG with `tools/ansi_svg.py`; it also records an agent's session over
`wtf --mcp`. They go to `screens/`, which is not committed.

```sh
cargo build --release
python3 site/tools/screens.py      # site/screens/, from target/release/wtf (or --wtf <path>)
cd site && zola serve              # http://127.0.0.1:1111; zola build writes public/
```

A new screen is an entry in `screens.toml`, shown by a `load_data` of its SVG in a template, or
in content by the `screen` component (`templates/components.html`): `{{ <screen name="..." caption="..." /> }}`.
Zola 0.23 runs content through Tera, so a page that shows `{{` wraps it in `{% raw %}`.

The terminal font, wtf Term, is JetBrains Mono Nerd Font Mono (SIL OFL 1.1, `static/fonts/OFL.txt`)
cut to what wtf draws, with the bus ends wtf draws (U+1FBA0-3, 6, 7) drawn in, as no Nerd Font but
Iosevka has them. It is committed; `tools/font.py` makes it again from the Nerd Font's files, for
a new icon in `src/draw.rs` (it says which are missing).
