"""Makes the site's screens from a wtf build: `site/screens/<name>.svg` for each screen in
`site/screens.toml`, and `site/screens/agent.html`, an agent's calls over `wtf --mcp` and the
answers. Run from anywhere:

    python3 site/tools/screens.py [--wtf target/release/wtf]
"""

import argparse
import html
import json
import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from ansi_svg import convert  # noqa: E402

SITE = Path(__file__).resolve().parent.parent
REPO = SITE.parent


def home_env(home):
    """An environment with an empty home, so the user's config and history don't show."""
    env = {k: v for k, v in os.environ.items() if not k.startswith('XDG_')}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'), XDG_STATE_HOME=str(home / '.state'),
               XDG_RUNTIME_DIR=str(home / '.run'), NO_COLOR='', COLORTERM='truecolor')
    del env['NO_COLOR']
    (home / '.run').mkdir(mode=0o700)
    return env


def screen(wtf, s, out):
    with tempfile.TemporaryDirectory() as tmp:
        home = Path(tmp)
        env = home_env(home)
        trace = REPO / s['trace']
        if 'copy' in s:
            # the trace alone, as one opened the first time
            dest = home / s['copy']
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(trace, dest)
            trace = dest
        cmd = [wtf, str(trace), '--snapshot', s['size'], '--ansi', '--glyphs', 'nerd']
        if 'keys' in s:
            cmd += ['--keys', s['keys']]
        r = subprocess.run(cmd, env=env, cwd=home, capture_output=True, text=True)
        if r.returncode != 0:
            sys.exit(f"{s['name']}: {' '.join(cmd)} failed:\n{r.stderr}")
        (out / f"{s['name']}.svg").write_text(convert(r.stdout, s['label']))


def agent(wtf, a, out):
    """The calls in order over `wtf --mcp`, each with its answer, as HTML lines for a <pre>."""
    with tempfile.TemporaryDirectory() as tmp:
        home = Path(tmp)
        sim = home / 'sim'
        sim.mkdir()
        shutil.copy(REPO / a['trace'], sim / Path(a['calls'][0]['args']['trace']).name)
        p = subprocess.Popen([wtf, '--mcp'], env=home_env(home), cwd=home, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        n = 0

        def rpc(method, params, notify=False):
            nonlocal n
            msg = {'jsonrpc': '2.0', 'method': method, 'params': params}
            if not notify:
                n += 1
                msg['id'] = n
            p.stdin.write(json.dumps(msg) + '\n')
            p.stdin.flush()
            while not notify:
                r = json.loads(p.stdout.readline())
                if r.get('id') == n:
                    return r

        rpc('initialize', {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 'wtf-site', 'version': '0'}})
        rpc('notifications/initialized', {}, notify=True)
        lines = []
        for c in a['calls']:
            r = rpc('tools/call', {'name': c['tool'], 'arguments': c['args']})
            if 'result' not in r or r['result'].get('isError'):
                sys.exit(f"agent: {c['tool']} {c['args']} failed: {r}")
            answer = r['result']['content'][0]['text'].splitlines()
            # the workspace's id changes every run; the hint after a decode is for agents
            answer = [l for l in answer if not l.startswith('workspace ') and not l.startswith('`')]
            if '\t' in ''.join(answer):
                answer = columns(answer)
            lines.append(f'<span class="call">{c["tool"]}</span> <span class="arg">{html.escape(json.dumps(c["args"]))}</span>')
            for l in answer:
                # a violation's record in red
                cls = 'err' if any(w in l.split(' ')[0] for w in ('errors', 'unmatched')) else 'ans'
                lines.append(f'<span class="{cls}">{html.escape(l)}</span>')
            lines.append('')
        p.stdin.close()
        p.wait()
        (out / 'agent.html').write_text('\n'.join(lines).rstrip() + '\n')


def columns(lines):
    """Tab-separated lines with their columns lined up."""
    cells = [l.split('\t') for l in lines]
    widths = [max(len(r[k]) for r in cells if k < len(r)) for k in range(max(len(r) for r in cells))]
    return [' '.join(v.ljust(widths[k]) for k, v in enumerate(r)).rstrip() for r in cells]


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--wtf', default=str(REPO / 'target/release/wtf'))
    wtf = os.path.abspath(ap.parse_args().wtf)
    if not os.access(wtf, os.X_OK):
        sys.exit(f'no wtf at {wtf}: build it (cargo build --release) or give --wtf')
    spec = tomllib.loads((SITE / 'screens.toml').read_text())
    out = SITE / 'screens'
    # made afresh, so a screen dropped from screens.toml leaves no file behind
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir()
    for s in spec['screen']:
        screen(wtf, s, out)
    agent(wtf, spec['agent'], out)
    # the agent benchmark's chart, from the same script as the README's
    subprocess.run([sys.executable, str(REPO / 'docs/agents.py'), '--site', str(out)], check=True)
    print(f"{len(spec['screen'])} screens and the agent's session in {out.relative_to(REPO)}")


if __name__ == '__main__':
    main()
