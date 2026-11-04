# Writes docs/demo.gif: the site's tour on docs/demo.vcd. Open it, decode the bus from the Space
# menu, add the address handshake with the finder, find the cycles where the address waits for
# ready, list the records, filter to the error, jump to it and unfold it into its fields. Each
# frame is wtf's own screen after the keys so far (`--snapshot --ansi --glyphs nerd`, in an empty
# home), drawn as the site draws its screens (site/tools/ansi_svg.py, in the wtf Term font) and
# photographed by headless Firefox; ffmpeg joins them. Run from the repository root after
# `cargo build --release`:
#   python3 docs/gif/demo.py
import os, shutil, subprocess, sys, tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "site/tools"))
from ansi_svg import convert, CW, CH  # noqa: E402
from screens import home_env  # noqa: E402

WTF = str(REPO / "target/release/wtf")
COLS, ROWS = 110, 27  # 27 rows: the smallest that draws the big logo
SCALE = 2  # pixels per SVG unit: a 1320 px wide screen
PAD = 16
TYPE = 0.05  # seconds per typed character
# (keys, seconds to hold the screen after them); ("type", text) is typed a character at a time
STEPS = [
    ("", 2.0),
    ("<Esc>", 0.8),
    ("<Space>", 1.2), ("d", 0.8), ("d", 1.8), ("<CR>", 1.8), ("<Esc>", 0.5),
    ("<Space>", 0.4), ("f", 0.4), (("type", "s_axi_aw"), 0.8), ("<Tab>", 0.3), ("<Tab>", 0.3), ("<Tab>", 0.4), ("<CR>", 1.5),
    (("type", "/@aclk s_axi_awvalid && !s_axi_awready"), 0.6), ("<CR>", 2.5),
    ("<Esc>", 0.5),
    (("type", ":records s_axi"), 0.4), ("<CR>", 2.0),
    ("/", 0.3), (("type", "resp != 0"), 0.5), ("<CR>", 1.8),
    ("<CR>", 1.5), ("<CR>", 3.5),
]
PAGE = """<!doctype html><meta charset="utf-8"><style>
@font-face {{ font-family: "wtf Term"; font-weight: 400; src: url("{fonts}/wtf-term-400.woff2"); }}
@font-face {{ font-family: "wtf Term"; font-weight: 700; src: url("{fonts}/wtf-term-700.woff2"); }}
html, body {{ margin: 0; background: #1b1d23; overflow: hidden; }}
/* named, as Firefox won't fall back to them by itself: what a recorded agent draws that wtf Term
   lacks (Claude Code's prompt and spinner), from fonts installed for it (docs/agent/cast2gif.py) */
body {{ padding: {pad}px; font-family: "wtf Term", "JetBrainsMono Nerd Font Mono", "Noto Sans Symbols 2"; }}
svg {{ display: block; width: {w}px; height: {h}px; }}
</style>{svg}"""


def photo(ansi, cols, rows, png, tmp):
    """Draws a screen of ANSI text as the site does and saves it as `png`; `tmp` holds the page and
    Firefox's profile. docs/agent/cast2gif.py draws its frames with it too."""
    w, h = cols * CW * SCALE, rows * CH * SCALE
    page = png.with_suffix(".html")
    page.write_text(PAGE.format(fonts=REPO / "site/static/fonts", pad=PAD, w=w, h=h, svg=convert(ansi, "")))
    (tmp / "profile").mkdir(exist_ok=True)
    browser = shutil.which("firefox") or shutil.which("firefox-esr")
    subprocess.run([browser, "--headless", "--profile", str(tmp / "profile"), "--screenshot", str(png),
                    f"--window-size={w + 2 * PAD},{h + 2 * PAD}", page.as_uri()], capture_output=True, check=True)


def frames():
    """(keys so far, seconds) for each frame."""
    keys, out = "", []
    for step, hold in STEPS:
        if isinstance(step, tuple):
            text = step[1]
            for i, c in enumerate(text):
                keys += {" ": "<Space>", "<": "<lt>"}.get(c, c)
                out.append((keys, hold if i == len(text) - 1 else TYPE))
        else:
            keys += step
            out.append((keys, hold))
    return out


def main():
    tmp = Path(tempfile.mkdtemp())
    home = tmp / "home"
    home.mkdir()
    env = home_env(home)
    shutil.copy(REPO / "docs/demo.vcd", home / "tour.vcd")  # without its saved session
    shots, shown = {}, []
    for keys, hold in frames():
        if keys not in shots:
            n = len(shots)
            ansi = subprocess.run([WTF, "tour.vcd", "--snapshot", f"{COLS}x{ROWS}", "--ansi", "--glyphs", "nerd", "--keys", keys],
                                  env=env, cwd=home, capture_output=True, text=True, check=True).stdout
            png = tmp / f"{n}.png"
            photo(ansi, COLS, ROWS, png, tmp)
            shots[keys] = png
        shown.append((shots[keys], hold))
    concat = tmp / "frames.txt"
    with open(concat, "w") as f:
        for png, hold in shown:
            f.write(f"file '{png}'\nduration {hold}\n")
        # the concat demuxer drops the last frame's duration unless the frame is repeated
        f.write(f"file '{shown[-1][0]}'\n")
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", str(concat),
                    "-vf", "split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=none",
                    str(REPO / "docs/demo.gif")], check=True)
    print(f"docs/demo.gif: {len(shots)} screens, {sum(h for _, h in shown):.1f} s")


if __name__ == "__main__":
    main()
