# Turns an asciinema recording (record.sh) into a GIF and an MP4, drawn as docs/demo.gif is:
#   python3 docs/agent/cast2gif.py agent.cast target/agent [end]
# writes target/agent/agent.gif and .mp4, cut at `end` seconds of the cast if given. Each screen is replayed by pyte, a terminal emulator,
# then drawn as the site draws its screens, in headless Firefox (docs/gif/demo.py `photo`). Needs
# pyte, Firefox and ffmpeg, and for what wtf's font lacks, JetBrains Mono Nerd Font and Noto Sans
# Symbols 2 installed (in ~/.local/share/fonts, then fc-cache).
import json, os, subprocess, sys, tempfile
from pathlib import Path
import pyte

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "gif"))
from demo import photo  # noqa: E402

NAMES = ["black", "red", "green", "brown", "blue", "magenta", "cyan", "white"]
MIN_FRAME = 0.05  # changes closer than this show as one frame


def colour(c, palette, bright):
    """An SGR colour for pyte's colour name or hex, or None for the default."""
    if c == "default":
        return None
    if c in NAMES:
        return palette[NAMES.index(c) + (8 if bright else 0)]
    if c.startswith("bright") and c[6:] in NAMES:
        return palette[NAMES.index(c[6:]) + 8]
    return "#" + c if len(c) == 6 else None


def ansi(screen, palette):
    """The screen as lines of SGR-coloured text, as `wtf --snapshot --ansi` prints them."""
    out = []
    for y in range(screen.lines):
        row = screen.buffer[y]
        line = ""
        for x in range(screen.columns):
            ch = row[x]
            sgr = ["0"]
            for c, base, bright in ((ch.fg, 38, ch.bold), (ch.bg, 48, False)):
                rgb = colour(c, palette, bright)
                if rgb:
                    sgr.append(f"{base};2;{int(rgb[1:3], 16)};{int(rgb[3:5], 16)};{int(rgb[5:7], 16)}")
            sgr += ["1"] * ch.bold + ["3"] * ch.italics + ["7"] * ch.reverse
            line += f"\x1b[{';'.join(sgr)}m{ch.data or ' '}"
        out.append(line + "\x1b[0m")
    return "\n".join(out)


def main(cast, out_dir, end=None):
    lines = open(cast).read().splitlines()
    head = json.loads(lines[0])
    cols, rows = head["term"]["cols"], head["term"]["rows"]
    palette = head["term"].get("theme", {}).get("palette", "").split(":")
    if len(palette) < 16:
        palette = ["#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5"] * 2
    screen = pyte.Screen(cols, rows)
    stream = pyte.Stream(screen)
    # (time the screen shows from, its text); version 3 stores the time since the last event
    shown, t = [], 0.0
    for line in lines[1:]:
        if not line.strip() or line.startswith("#"):
            continue
        dt, kind, data = json.loads(line)[:3]
        t = t + dt if head["version"] >= 3 else dt
        if kind != "o":
            continue
        if end is not None and t >= end:
            break
        # tmux leaving the alternate screen: the take is over (what follows is the shell's)
        if "\x1b[?1049l" in data:
            break
        stream.feed(data)
        text = ansi(screen, palette)
        blank = not "".join(screen.display).strip()
        if shown and t - shown[-1][0] < MIN_FRAME:
            shown[-1] = (shown[-1][0], text, blank)
        elif not shown or shown[-1][1] != text:
            shown.append((t, text, blank))
    # tmux clears the screen as its last pane ends
    while shown and shown[-1][2]:
        shown.pop()
    tmp = Path(tempfile.mkdtemp())
    concat = tmp / "frames.txt"
    with open(concat, "w") as f:
        for n, (start, text, _) in enumerate(shown):
            png = tmp / f"{n}.png"
            photo(text, cols, rows, png, tmp)
            end = shown[n + 1][0] if n + 1 < len(shown) else start + 3  # the last screen stays 3 s
            f.write(f"file '{png}'\nduration {max(end - start, MIN_FRAME):.3f}\n")
        f.write(f"file '{png}'\n")
    name = os.path.splitext(os.path.basename(cast))[0]
    os.makedirs(out_dir, exist_ok=True)
    gif, mp4 = os.path.join(out_dir, name + ".gif"), os.path.join(out_dir, name + ".mp4")
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", concat,
                    "-vf", "split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=none", gif], check=True)
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", concat,
                    "-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2,format=yuv420p", "-c:v", "libx264", "-r", "30", mp4], check=True)
    print(f"{gif}, {mp4}: {len(shown)} screens, {shown[-1][0] + 3:.1f} s")


main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else ".", float(sys.argv[3]) if len(sys.argv) > 3 else None)
