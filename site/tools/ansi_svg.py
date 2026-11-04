"""A terminal screen, as `wtf --snapshot --ansi` prints it, drawn as SVG on its cell grid.

Backgrounds and the block characters wtf draws its logo with (`▀ ▄ ▌`) become rectangles with
crisp edges, so neighbours snap to the same pixel and no seam shows at any size; text is placed run
by run at its column, so a font's rounding can't drift along the line. The SVG is meant to be
inlined in a page that sets `font-family` (a monospace font whose advance is 0.6 em).
"""

import html
import re

SGR = re.compile(r'\x1b\[([0-9;]*)m')
# a cell 0.6 x 1.3 em at font-size 10, the baseline 10 down
CW, CH, BASE, SIZE = 6, 13, 10, 10
FG, BG = '#abb2bf', '#1b1d23'
# the part of the cell each block character fills: x, y, width, height
BLOCKS = {'▀': (0, 0, 1, .5), '▄': (0, .5, 1, .5), '▌': (0, 0, .5, 1)}
# the theme's dim grey is 3:1 on the screen's background; lifted to 4.5:1 for text on the web
LIFT = {'#5c6370': '#7f848e'}


def cells(text):
    """The screen as rows of cells: (char, fg, bg, bold, dim, italic, underline)."""
    rows, st = [], {}
    for line in text.rstrip('\n').split('\n'):
        row, pos = [], 0

        def put(s):
            fg, bg = st.get('fg'), st.get('bg')
            if st.get('rev'):
                fg, bg = bg or BG, fg or FG
            row.extend((ch, fg or FG, bg, st.get('b'), st.get('d'), st.get('i'), st.get('u')) for ch in s)

        for m in SGR.finditer(line):
            put(line[pos:m.start()])
            pos = m.end()
            codes = [int(c) if c else 0 for c in m.group(1).split(';')]
            i = 0
            while i < len(codes):
                c = codes[i]
                if c == 0:
                    st.clear()
                elif c in (1, 2, 3, 4, 7):
                    st[{1: 'b', 2: 'd', 3: 'i', 4: 'u', 7: 'rev'}[c]] = 1
                elif c == 22:
                    st.pop('b', None)
                    st.pop('d', None)
                elif c in (23, 24, 27):
                    st.pop({23: 'i', 24: 'u', 27: 'rev'}[c], None)
                elif c in (38, 48) and codes[i + 1:i + 2] == [2]:
                    rgb = '#%02x%02x%02x' % tuple(codes[i + 2:i + 5])
                    st['fg' if c == 38 else 'bg'] = LIFT.get(rgb, rgb) if c == 38 else rgb
                    i += 4
                elif c in (38, 48):
                    raise ValueError(f'colour {codes[i:i + 3]}: only truecolor (38;2) is read; snapshot with a truecolor theme')
                elif c == 39:
                    st.pop('fg', None)
                elif c == 49:
                    st.pop('bg', None)
                i += 1
        put(line[pos:])
        rows.append(row)
    return rows


def convert(text, label):
    """The SVG of a screen; `label` says what it shows, for screen readers."""
    rows = cells(text)
    cols = max(len(r) for r in rows)
    rects, texts = [], []

    def rect(x, y, w, h, fill):
        rects.append(f'<rect x="{x:g}" y="{y:g}" width="{w:g}" height="{h:g}" fill="{fill}"/>')

    for y, row in enumerate(rows):
        # backgrounds, a rectangle per run of one colour
        x = 0
        while x < len(row):
            bg, e = row[x][2], x
            while e < len(row) and row[e][2] == bg:
                e += 1
            if bg:
                rect(x * CW, y * CH, (e - x) * CW, CH, bg)
            x = e
        for x, c in enumerate(row):
            if c[0] in BLOCKS:
                bx, by, bw, bh = BLOCKS[c[0]]
                rect((x + bx) * CW, (y + by) * CH, bw * CW, bh * CH, c[1])
        # text, a run per style from its column
        x = 0
        while x < len(row):
            style = row[x][1:2] + row[x][3:]
            e = x
            while e < len(row) and row[e][1:2] + row[e][3:] == style and row[e][0] not in BLOCKS:
                e += 1
            if e == x:
                x += 1
                continue
            s = ''.join(c[0] for c in row[x:e])
            if s.strip():
                fg, b, d, i, u = style
                attrs = f' fill="{fg}"' + (' font-weight="700"' if b else '') + (' opacity=".6"' if d else '') \
                    + (' font-style="italic"' if i else '') + (' text-decoration="underline"' if u else '')
                lead = len(s) - len(s.lstrip(' '))
                texts.append(f'<text x="{(x + lead) * CW}" y="{y * CH + BASE}"{attrs}>{html.escape(s.strip(" "))}</text>')
            x = e
    return (f'<svg class="tsvg" viewBox="0 0 {cols * CW} {len(rows) * CH}" style="--cols:{cols}" role="img" aria-label="{html.escape(label)}">'
            f'<g shape-rendering="crispEdges">{"".join(rects)}</g>'
            f'<g font-size="{SIZE}" xml:space="preserve">{"".join(texts)}</g></svg>\n')


if __name__ == '__main__':
    # a quick check: a background run, a block, and text placed by column
    svg = convert('\x1b[48;2;1;2;3mab\x1b[0m \x1b[38;2;9;9;9m▀\x1b[0m c', 'test')
    assert '<rect x="0" y="0" width="12" height="13" fill="#010203"/>' in svg, svg
    assert '<rect x="18" y="0" width="6" height="6.5" fill="#090909"/>' in svg, svg
    assert '<text x="30" y="10" fill="#abb2bf">c</text>' in svg, svg
    assert 'fill="#7f848e"' in convert('\x1b[38;2;92;99;112mdim', 'test')
    print('ok')
