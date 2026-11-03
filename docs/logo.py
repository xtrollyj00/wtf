# Draws wtf's logo for the README, docs/logo.svg and docs/logo-dark.svg, from the start screen of a
# wtf build in each theme: the pixel letters and their shadow as rectangles, the cursor, and the bus
# under them as lines, so it needs no font but a monospace one for the bus's labels.
#   cargo build --release && python3 docs/logo.py
import os
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / 'site/tools'))
from ansi_svg import cells  # noqa: E402

CW, CH = 6, 13  # a cell, as ansi_svg's
HALF = {'▀': (0, .5), '▄': (.5, .5), '█': (0, 1)}  # top and height of the cell a block fills


def screen(theme):
    """The start screen in a theme, as cells."""
    with tempfile.TemporaryDirectory() as tmp:
        home = Path(tmp)
        (home / 'c/wtf').mkdir(parents=True)
        (home / 'c/wtf/config.toml').write_text(f'theme = "{theme}"\nglyphs = "unicode"\n')
        (home / 'run').mkdir(mode=0o700)
        env = {k: v for k, v in os.environ.items() if not k.startswith('XDG_') and k != 'NO_COLOR'}
        env.update(HOME=tmp, XDG_CONFIG_HOME=str(home / 'c'), XDG_STATE_HOME=str(home / 's'),
                   XDG_RUNTIME_DIR=str(home / 'run'), COLORTERM='truecolor')
        out = subprocess.run([str(REPO / 'target/release/wtf'), '--snapshot', '100x30', '--ansi'],
                             env=env, cwd=tmp, capture_output=True, text=True, check=True).stdout
    return cells(out)


def logo(rows):
    """The logo's rows and columns: from the first row with a block to the bus's labels."""
    first = next(y for y, r in enumerate(rows) if any(c[0] in HALF for c in r))
    last = next(y for y, r in enumerate(rows) if y > first and '0x77' in ''.join(c[0] for c in r))
    rows = rows[first - 1:last + 1]  # the cursor starts a row above the letters
    used = [x for r in rows for x, c in enumerate(r) if c[0] != ' ']
    return [r[min(used):max(used) + 1] for r in rows]


def svg(rows, label):
    # the screen's own background is left out, so the logo sits on the page's
    counts = {}
    for r in rows:
        for c in r:
            counts[c[2]] = counts.get(c[2], 0) + 1
    page = max(counts, key=counts.get)
    w, h = max(len(r) for r in rows) * CW, len(rows) * CH
    out = []
    for y, row in enumerate(rows):
        for x, (ch, fg, bg, bold, *_) in enumerate(row):
            X, Y = x * CW, y * CH
            if bg and bg != page:
                out.append(f'<rect x="{X}" y="{Y}" width="{CW}" height="{CH}" fill="{bg}"/>')
            if ch in HALF:
                top, hh = HALF[ch]
                out.append(f'<rect x="{X}" y="{Y + top * CH:g}" width="{CW}" height="{hh * CH:g}" fill="{fg}"/>')
            elif ch == '┃':
                out.append(f'<rect x="{X + CW / 2 - 1:g}" y="{Y}" width="2" height="{CH}" fill="{fg}"/>')
            elif ch in '─━':
                t = 1 if ch == '─' else 2
                out.append(f'<rect x="{X}" y="{Y + CH / 2 - t / 2:g}" width="{CW}" height="{t}" fill="{fg}"/>')
            elif ch in '🮦🮧':
                # the bus's crossing: from the middle of the cell's sides to its bottom (🮦) or top (🮧)
                ey = CH if ch == '🮦' else 0
                out.append(f'<polyline points="{X},{Y + CH / 2:g} {X + CW / 2:g},{Y + ey} {X + CW},{Y + CH / 2:g}" '
                           f'fill="none" stroke="{fg}" stroke-width="1" shape-rendering="geometricPrecision"/>')
        # the bus's labels, each a run of text stretched to its cells
        x = 0
        while x < len(row):
            if row[x][0] in '0x' and row[x][0] != ' ' and (x == 0 or row[x - 1][0] == ' '):
                e = x
                while e < len(row) and row[e][0] not in ' ─━🮦🮧':
                    e += 1
                s, fg = ''.join(c[0] for c in row[x:e]), row[x][1]
                out.append(f'<text x="{x * CW}" y="{y * CH + 10}" textLength="{(e - x) * CW}" lengthAdjust="spacingAndGlyphs" '
                           f'fill="{fg}">{s}</text>')
                x = e
            x += 1
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w * 2}" height="{h * 2}" '
            f'role="img" aria-label="{label}"><g shape-rendering="crispEdges">{"".join(o for o in out if "<text" not in o)}</g>'
            f'<g font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace" font-size="10">'
            f'{"".join(o for o in out if "<text" in o)}</g></svg>\n')


if __name__ == '__main__':
    label = 'wtf: the letters w and t in amber, the cursor, then f in red, over a bus carrying 0x77, 0x74 and 0xXX'
    for name, theme in (('logo.svg', 'light'), ('logo-dark.svg', 'dark')):
        (REPO / 'docs' / name).write_text(svg(logo(screen(theme)), label))
        print(f'docs/{name}')
