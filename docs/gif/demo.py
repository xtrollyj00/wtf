# Writes docs/demo.gif: add signals, decode an AXI4-Lite bus, find a stall, list the reads, open
# the failing one. Each frame is wtf's own screen after the keys so far (`--snapshot --ansi
# --keys`), drawn by docs/screenshot.py in a browser, so bus edges join up as in a terminal that
# draws the legacy computing glyphs itself. Run in docker, see docs/gif/Dockerfile.
import os, shutil, subprocess, tempfile

SIZE = "120x24"
TYPE = 0.05  # seconds per typed character
# (keys, seconds to hold the screen after them); ("type", text) is typed a character at a time
STEPS = [
    ("", 1.5),
    ("i", 1.0),
    (("type", "/aclk"), 0.4), ("<CR>", 0.5), ("<CR>", 0.8),
    (("type", "/s_axi_aw"), 0.4), ("<CR>", 0.5),
    ("<Space>", 0.3), ("n<Space>", 0.3), ("n<Space>", 0.8), ("<CR>", 1.0),
    ("<Esc><Esc>", 1.0),
    (("type", ":decode bus axilite clock=aclk prefix=s_axi_"), 0.5), ("<CR>", 2.0),
    (("type", ":add bus.wr bus.rd"), 0.4), ("<CR>", 1.0),
    (("type", ":fit"), 0.3), ("<CR>", 2.0),
    (("type", "/@aclk s_axi_awvalid && !s_axi_awready"), 0.6), ("<CR>", 1.5),
    ("m", 1.5),
    (("type", ":records bus.rd"), 0.4), ("<CR>", 2.5),
    ("j", 1.0), ("<CR>", 1.5),
    ("<Space>", 3.0),
    ("?", 3.0),
    ("<Esc>", 1.5),
]


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
    tmp = tempfile.mkdtemp()
    trace = os.path.join(tmp, "demo.vcd")  # without its saved session: the screen starts empty
    shutil.copy("docs/demo.vcd", trace)
    shots, shown = {}, []
    for keys, hold in frames():
        if keys not in shots:
            n = len(shots)
            ansi = subprocess.run(["wtf", trace, "--snapshot", SIZE, "--ansi", "--keys", keys], capture_output=True, check=True).stdout
            html = subprocess.run(["python3", "docs/screenshot.py"], input=ansi, capture_output=True, check=True).stdout
            page, png = os.path.join(tmp, f"{n}.html"), os.path.join(tmp, f"{n}.png")
            with open(page, "wb") as f:
                f.write(html)
            subprocess.run(["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars",
                            f"--screenshot={png}", "--window-size=1380,620", f"file://{page}"], capture_output=True, check=True)
            shots[keys] = png
        shown.append((shots[keys], hold))
    concat = os.path.join(tmp, "frames.txt")
    with open(concat, "w") as f:
        for png, hold in shown:
            f.write(f"file '{png}'\nduration {hold}\n")
        # the concat demuxer drops the last frame's duration unless the frame is repeated
        f.write(f"file '{shown[-1][0]}'\n")
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", concat,
                    "-vf", "split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=none",
                    "docs/demo.gif"], check=True)
    print(f"docs/demo.gif: {len(shots)} screens, {sum(h for _, h in shown):.1f} s")


main()
