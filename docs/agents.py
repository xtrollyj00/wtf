# Draws the agent benchmark, with and without wtf, from the numbers in bench/RESULTS.md (wtf-bench
# agent report): docs/agents.svg and docs/agents-dark.svg for the README, and with --site the
# site's bench.svg, which takes its colours and fonts from the page (site/static/site.css).
#   python3 docs/agents.py docs
#   python3 docs/agents.py --site site/screens
import sys

RUNS = 5
# task: (runs with every answer right, of 5; median wall s; median cost $), with wtf / without
DATA = {
    "eth-parser-headers": ((5, 134, 0.029), (2, 270, 0.058)),
    "eth-parser-stall": ((5, 76, 0.014), (5, 136, 0.024)),
    "head-trimmer-boundary": ((5, 85, 0.012), (3, 203, 0.024)),
    "axil2mi-stale-addr": ((2, 62, 0.015), (1, 57, 0.015)),
}
RIGHT = (sum(a[0] for a, _ in DATA.values()), sum(b[0] for _, b in DATA.values()))
TITLE = "An agent fixing failing RTL testbenches, with and without wtf"
SUB = (f"Every run fixed its bug either way. With wtf, {RIGHT[0]} of {RUNS * len(DATA)} runs also had "
       f"every answer right, against {RIGHT[1]} without.")
NOTE = (f"claude-haiku-5-5, wtf d0ebd30, {RUNS} runs per task and arm, at most 3 simulations each; "
        "wtf-bench agent, 2026-10-25. bench/RESULTS.md has the table.")
PANELS = [  # title, field, axis maximum, value label
    ("Every answer right, runs of 5", 0, RUNS, lambda v: f"{v}"),
    ("Time to the fix, median", 1, 280, lambda v: f"{v} s"),
    ("Cost per run, median", 2, 0.06, lambda v: f"${v:.3f}"),
]
# colours checked with the dataviz validator against each surface: wtf in the site's amber, the
# runs without it in blue
THEMES = {
    "agents.svg": dict(surface="#eef0f3", ink="#14161b", ink2="#576070", axis="#c3c8d0", wtf="#a86e00", bare="#4a78c8",
                       font="system-ui, -apple-system, Segoe UI, sans-serif"),
    "agents-dark.svg": dict(surface="#111317", ink="#e5e7eb", ink2="#9aa2b0", axis="#30343d", wtf="#c08518", bare="#5a86d8",
                            font="system-ui, -apple-system, Segoe UI, sans-serif"),
}
SITE = dict(surface=None, ink="var(--ink)", ink2="var(--muted)", axis="var(--rule)", wtf="var(--bench-wtf)",
            bare="var(--bench-bare)", font="var(--body)")
GEOM = 196, 194, 20  # the README's: task labels, panel, gap
BAR, PAIR_GAP, GROUP = 12, 2, 18
ARMS = (("with wtf", "wtf"), ("without wtf: logs, prints, reruns", "bare"))


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;")


def bar(x, y, w, h, color, tip):
    """A bar anchored at x, its data end rounded (4px), its value as a tooltip."""
    if w <= 0:
        return ""
    r = min(4, w / 2, h / 2)
    return (f'<path d="M{x:.1f},{y:.1f} h{w - r:.1f} a{r},{r} 0 0 1 {r},{r} v{h - 2 * r:.1f} '
            f'a{r},{r} 0 0 1 {-r},{r} h{-(w - r):.1f} z" fill="{color}"><title>{esc(tip)}</title></path>')


def svg(c, site=False):
    # the site shows the chart in a column narrower than W: its type a quarter larger, the panels
    # wider for it, and their titles in two lines, broken at the comma
    F, LABELS, PANEL, GAP = (1.25, 236, 204, 24) if site else (1, *GEOM)
    W = LABELS + 3 * PANEL + 2 * GAP + (0 if site else 32)
    top = 90 if site else 118  # the site's page carries the title and the line under it
    rows = len(DATA) * (2 * BAR + PAIR_GAP + GROUP) - GROUP
    H = top + rows + (12 if site else 58)
    size = f'width="100%" style="max-width:{W}px;height:auto"' if site else f'width="{W}" height="{H}"'
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" {size} viewBox="0 0 {W} {H}" font-family="{c["font"]}" role="img" aria-labelledby="bt bd">',
           f'<title id="bt">{TITLE}</title>',
           '<desc id="bd">' + "; ".join(f"{t}: with wtf {a[0]} of {RUNS} runs with every answer right, {a[1]} s, ${a[2]}; "
                                        f"without {b[0]} of {RUNS}, {b[1]} s, ${b[2]}" for t, (a, b) in DATA.items()) + '</desc>']
    y_legend = 6 if site else 70
    if not site:
        out += [f'<rect width="{W}" height="{H}" rx="8" fill="{c["surface"]}"/>',
                f'<text x="20" y="32" font-size="17" font-weight="600" fill="{c["ink"]}">{TITLE}</text>',
                f'<text x="20" y="54" font-size="13" fill="{c["ink2"]}">{esc(SUB)}</text>']
    x0 = 0 if site else 20
    lx = x0
    for name, key in ARMS:
        out.append(f'<rect x="{lx}" y="{y_legend}" width="12" height="12" rx="3" fill="{c[key]}"/>')
        out.append(f'<text x="{lx + 18}" y="{y_legend + 10.5}" font-size="{13 * F:g}" fill="{c["ink2"]}">{esc(name)}</text>')
        lx += 18 + 7.2 * F * len(name) + 24
    for i, task in enumerate(DATA):
        y = top + i * (2 * BAR + PAIR_GAP + GROUP)
        out.append(f'<text x="{x0}" y="{y + BAR + PAIR_GAP / 2 + 4:.1f}" font-size="{13 * F:g}" fill="{c["ink"]}">{task}</text>')
    labels = LABELS - (20 if site else 0)
    for p, (title, field, axis, label) in enumerate(PANELS):
        px = labels + p * (PANEL + GAP) + (20 if site else 0)
        lines = title.split(", ") if site else [title]
        for j, t in enumerate(lines):
            out.append(f'<text x="{px}" y="{top - 14 - 20 * (len(lines) - 1 - j)}" font-size="{13 * F:g}" font-weight="600" fill="{c["ink2"]}">{t}</text>')
        out.append(f'<line x1="{px}" y1="{top - 4}" x2="{px}" y2="{top + rows + 4}" stroke="{c["axis"]}" stroke-width="1"/>')
        span = PANEL - 52 * F  # room for the value after the longest bar
        for i, (task, arms) in enumerate(DATA.items()):
            y = top + i * (2 * BAR + PAIR_GAP + GROUP)
            for k, ((arm, key), v) in enumerate(zip(ARMS, (arms[0][field], arms[1][field]))):
                yy = y + k * (BAR + PAIR_GAP)
                w = span * v / axis
                out.append(bar(px, yy, w, BAR, c[key], f"{task}, {arm.split(':')[0]}: {label(v)}"))
                out.append(f'<text x="{px + w + 5:.1f}" y="{yy + BAR - 2:.1f}" font-size="{12 * F:g}" fill="{c["ink2"]}">{label(v)}</text>')
    if not site:
        out.append(f'<text x="20" y="{H - 18}" font-size="11.5" fill="{c["ink2"]}">{esc(NOTE)}</text>')
    out.append("</svg>")
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    if sys.argv[1:2] == ["--site"]:
        with open(f"{sys.argv[2]}/bench.svg", "w") as f:
            f.write(svg(SITE, site=True))
    else:
        for name, colors in THEMES.items():
            with open(f"{sys.argv[1]}/{name}", "w") as f:
                f.write(svg(colors))
