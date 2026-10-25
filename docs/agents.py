# Writes docs/agents.svg and docs/agents-dark.svg: the agent benchmark with and without wtf, from
# the numbers in spec §18 "Agents with and without wtf" (wtf-bench agent report).
# Run: python3 docs/agents.py docs
import sys

# task: (runs with every answer right, of 5; median wall s; median cost $), with wtf / without
RUNS = 5
DATA = {
    "eth-parser-headers": ((5, 116, 0.029), (3, 205, 0.048)),
    "eth-parser-stall": ((3, 77, 0.014), (4, 127, 0.018)),
    "head-trimmer-boundary": ((4, 80, 0.014), (0, 138, 0.023)),
    "axil2mi-stale-addr": ((4, 44, 0.013), (2, 115, 0.026)),
}
PANELS = [  # title, field, axis maximum, value label
    ("Every answer right (runs of 5)", 0, RUNS, lambda v: f"{v}"),
    ("Time to the fix, median", 1, 210, lambda v: f"{v} s"),
    ("Cost per run, median", 2, 0.05, lambda v: f"${v:.3f}"),
]
THEMES = {  # the dataviz reference palette: series 1 blue, the rest gray
    "agents.svg": dict(surface="#fcfcfb", ink="#0b0b0b", ink2="#52514e", muted="#898781", axis="#c3c2b7", wtf="#2a78d6", bare="#898781"),
    "agents-dark.svg": dict(surface="#1a1a19", ink="#f0efec", ink2="#c3c2b7", muted="#898781", axis="#383835", wtf="#3987e5", bare="#898781"),
}
W, LABELS, PANEL, GAP = 820, 170, 190, 20
BAR, PAIR_GAP, GROUP = 12, 2, 18
TOP = 118  # the first bar


def bar(x, y, w, h, color):
    """A bar anchored at x, its data end rounded (4px)."""
    r = min(4, w / 2, h / 2)
    if w <= 0:
        return ""
    return (f'<path d="M{x:.1f},{y:.1f} h{w - r:.1f} a{r},{r} 0 0 1 {r},{r} v{h - 2 * r:.1f} '
            f'a{r},{r} 0 0 1 {-r},{r} h{-(w - r):.1f} z" fill="{color}"/>')


def svg(c):
    font = 'font-family="system-ui, -apple-system, Segoe UI, sans-serif"'
    rows = len(DATA) * (2 * BAR + PAIR_GAP + GROUP) - GROUP
    H = TOP + rows + 58
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" {font} role="img" aria-labelledby="t d">',
           '<title id="t">Haiku fixing three failing RTL testbenches, with and without wtf</title>',
           '<desc id="d">' + "; ".join(f"{t}: with wtf {a[0]} of {RUNS} right, {a[1]} s, ${a[2]}; without {b[0]} of {RUNS}, {b[1]} s, ${b[2]}" for t, (a, b) in DATA.items()) + '</desc>',
           f'<rect width="{W}" height="{H}" rx="8" fill="{c["surface"]}"/>',
           f'<text x="20" y="32" font-size="17" font-weight="600" fill="{c["ink"]}">An agent fixing failing RTL testbenches, with and without wtf</text>',
           f'<text x="20" y="54" font-size="13" fill="{c["ink2"]}">Every run fixed its bug either way; with wtf, 16 of 20 had every answer right (9 without), sooner and cheaper.</text>']
    # legend
    lx = 20
    for name, color in (("with wtf", c["wtf"]), ("without wtf (logs, prints, reruns)", c["bare"])):
        out.append(f'<rect x="{lx}" y="70" width="12" height="12" rx="3" fill="{color}"/>')
        out.append(f'<text x="{lx + 18}" y="80.5" font-size="12.5" fill="{c["ink2"]}">{name}</text>')
        lx += 18 + 7.2 * len(name) + 24
    # task labels, one per group of two bars
    for i, task in enumerate(DATA):
        y = TOP + i * (2 * BAR + PAIR_GAP + GROUP)
        out.append(f'<text x="20" y="{y + BAR + PAIR_GAP / 2 + 4:.1f}" font-size="12.5" fill="{c["ink"]}">{task}</text>')
    for p, (title, field, top, label) in enumerate(PANELS):
        x0 = LABELS + p * (PANEL + GAP)
        out.append(f'<text x="{x0}" y="{TOP - 14}" font-size="12.5" font-weight="600" fill="{c["ink2"]}">{title}</text>')
        out.append(f'<line x1="{x0}" y1="{TOP - 4}" x2="{x0}" y2="{TOP + rows + 4}" stroke="{c["axis"]}" stroke-width="1"/>')
        span = PANEL - 52  # room for the value after the longest bar
        for i, (a, b) in enumerate(DATA.values()):
            y = TOP + i * (2 * BAR + PAIR_GAP + GROUP)
            for k, (v, color) in enumerate(((a[field], c["wtf"]), (b[field], c["bare"]))):
                yy = y + k * (BAR + PAIR_GAP)
                w = span * v / top
                out.append(bar(x0, yy, w, BAR, color))
                out.append(f'<text x="{x0 + max(w, 0) + 5:.1f}" y="{yy + BAR - 2:.1f}" font-size="11.5" fill="{c["ink2"]}">{label(v)}</text>')
    out.append(f'<text x="20" y="{H - 18}" font-size="11.5" fill="{c["muted"]}">claude-haiku-5-5, {RUNS} runs per task and arm, at most 3 simulations each; wtf-bench agent, spec §18 has the table (2026-10-25)</text>')
    out.append("</svg>")
    return "\n".join(out) + "\n"


for name, colors in THEMES.items():
    with open(f"{sys.argv[1]}/{name}", "w") as f:
        f.write(svg(colors))
