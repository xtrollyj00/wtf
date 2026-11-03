"""Makes the site's terminal font, wtf Term: JetBrains Mono Nerd Font Mono (SIL OFL 1.1, see
`static/fonts/OFL.txt`) cut to what wtf draws, with the six bus ends from Symbols for Legacy
Computing (U+1FBA0-3, 6, 7) drawn in from the font's own box lines, as no Nerd Font but Iosevka
has them. Needs fontTools and brotli (`pip install fonttools brotli`):

    python3 site/tools/font.py JetBrainsMonoNerdFontMono-Regular.ttf JetBrainsMonoNerdFontMono-Bold.ttf

from the JetBrainsMono.zip of https://www.nerdfonts.com/font-downloads (3.4 or newer). Writes
`site/static/fonts/wtf-term-400.woff2` and `-700.woff2`.
"""

import io
import math
import re
import sys
from pathlib import Path

from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.subset import Options, Subsetter
from fontTools.ttLib import TTFont

SITE = Path(__file__).resolve().parent.parent
FAMILY = 'wtf Term'

# what wtf and the site draw: ASCII and Latin-1, punctuation, arrows, box drawing, blocks, shapes,
# the bus ends; the icons are read from src/draw.rs
RANGES = [(0x20, 0x7E), (0xA0, 0xFF), (0x2010, 0x2027), (0x2190, 0x21FF), (0x2500, 0x259F), (0x25A0, 0x25FF), (0x1FBA0, 0x1FBAF)]


def icons():
    src = (SITE.parent / 'src/draw.rs').read_text()
    return {int(h, 16) for h in re.findall(r"'\\u\{([0-9a-fA-F]+)\}'", src)}


def bus_ends(f):
    """Draws U+1FBA0-3, 6, 7 (diagonal box lines) with the stroke of ─, between the cell's middle
    left / right and the top / bottom of │, a little past the ends so they meet the next cell."""
    cmap, gs = f.getBestCmap(), f.getGlyphSet()

    def bounds(cp):
        p = BoundsPen(gs)
        gs[cmap[cp]].draw(p)
        return p.bounds

    adv = f['hmtx'][cmap[0x2500]][0]
    _, hy0, _, hy1 = bounds(0x2500)
    vx0, vy0, vx1, vy1 = bounds(0x2502)
    t, ym, xc = hy1 - hy0, (hy0 + hy1) / 2, (vx0 + vx1) / 2
    L, R, U, D = (0, ym), (adv, ym), (xc, vy1), (xc, vy0)

    def stroke(pen, a, b):
        (x0, y0), (x1, y1) = a, b
        n = math.hypot(x1 - x0, y1 - y0)
        ux, uy = (x1 - x0) / n * t / 2, (y1 - y0) / n * t / 2
        pts = [(x0 - ux - uy, y0 - uy + ux), (x1 + ux - uy, y1 + uy + ux), (x1 + ux + uy, y1 + uy - ux), (x0 - ux + uy, y0 - uy - ux)]
        # clockwise, as TrueType fills
        if (pts[1][0] - pts[0][0]) * (pts[2][1] - pts[0][1]) - (pts[1][1] - pts[0][1]) * (pts[2][0] - pts[0][0]) > 0:
            pts.reverse()
        pen.moveTo(tuple(map(round, pts[0])))
        for p in pts[1:]:
            pen.lineTo(tuple(map(round, p)))
        pen.closePath()

    shapes = {0x1FBA0: [(U, L)], 0x1FBA1: [(U, R)], 0x1FBA2: [(L, D)], 0x1FBA3: [(R, D)], 0x1FBA6: [(L, D), (D, R)], 0x1FBA7: [(L, U), (U, R)]}
    for cp, lines in shapes.items():
        if cp in cmap:
            continue
        name = f'uni{cp:X}'
        pen = TTGlyphPen(None)
        for a, b in lines:
            stroke(pen, a, b)
        g = pen.glyph()
        g.recalcBounds(f['glyf'])
        f['glyf'][name] = g
        f['hmtx'][name] = (adv, g.xMin)
        for tab in f['cmap'].tables:
            if tab.isUnicode() and (cp <= 0xFFFF or tab.format in (12, 13)):
                tab.cmap[cp] = name
    f['maxp'].numGlyphs = len(f.getGlyphOrder())


def rename(f, weight):
    style = 'Bold' if weight == 700 else 'Regular'
    for rec in f['name'].names:
        if rec.nameID in (1, 16):
            rec.string = FAMILY
        elif rec.nameID in (2, 17):
            rec.string = style
        elif rec.nameID == 4:
            rec.string = f'{FAMILY} {style}'
        elif rec.nameID == 6:
            rec.string = f"{FAMILY.replace(' ', '')}-{style}"


def make(src, weight):
    f = TTFont(src)
    bus_ends(f)
    rename(f, weight)
    # the cut works on the font as saved, with the drawn glyphs in it
    buf = io.BytesIO()
    f.save(buf)
    buf.seek(0)
    f = TTFont(buf)
    want = {c for a, b in RANGES for c in range(a, b + 1)} | icons()
    missing = sorted(icons() - set(f.getBestCmap()))
    if missing:
        sys.exit(f"{src}: no icons {' '.join(f'U+{c:04X}' for c in missing)}: a Nerd Font 3.4 or newer has them")
    opts = Options()
    opts.flavor, opts.layout_features, opts.name_IDs, opts.drop_tables = 'woff2', [], ['*'], ['PfEd']
    sub = Subsetter(opts)
    sub.populate(unicodes=want)
    sub.subset(f)
    out = SITE / 'static/fonts' / f'wtf-term-{weight}.woff2'
    f.flavor = 'woff2'
    f.save(out)
    print(f'{out.relative_to(SITE.parent)}: {out.stat().st_size} bytes')


if __name__ == '__main__':
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    make(sys.argv[1], 400)
    make(sys.argv[2], 700)
