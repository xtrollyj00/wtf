#!/bin/sh
# Records an AI agent debugging docs/demo.vcd in your wtf window, for the README:
#   docs/agent/record.sh [out.cast]
# wtf on top, Claude Code below, in a tmux of their own, recorded by asciinema. Claude Code gets
# only wtf's MCP server and may call its tools without asking. When the agent has presented its
# evidence, `:undo` in the wtf pane (focused from the start) gives the view back; Ctrl-b d ends
# the take, with both panes still open.
#
# Needs tmux, asciinema, claude (logged in) and wtf (or WTF=path/to/wtf). MODEL=haiku|sonnet|...
# picks the model. Claude Code runs in $DIR, away from the repository, so the agent sees only the
# trace; the first take there asks whether to trust the folder (Ctrl-b o to answer), later ones don't.
set -eu

out=${1:-agent.cast}
wtf=$(command -v "${WTF:-wtf}") || { echo "record.sh: wtf not found; build it or set WTF" >&2; exit 1; }
# absolute: the panes start in $dir
wtf=$(cd "$(dirname "$wtf")" && pwd)/$(basename "$wtf")
model=${MODEL:-sonnet}
# wide: GitHub shows a video at most 640px high, so a taller one is narrower than the page
cols=160 rows=40
dir=${DIR:-${TMPDIR:-/tmp}/wtf-agent-demo}
repo=$(cd "$(dirname "$0")/../.." && pwd)

if [ "$(tput cols)" -lt $cols ] || [ "$(tput lines)" -lt $rows ]; then
    echo "record.sh: make the terminal at least ${cols}x$rows (it is $(tput cols)x$(tput lines))" >&2
    exit 1
fi

# a fresh copy of the trace, without its saved session, so the window starts empty
mkdir -p "$dir/config/wtf"
rm -f "$dir"/demo.vcd*
cp "$repo/docs/demo.vcd" "$dir/demo.vcd"
# glyphs and theme set, so the window doesn't ask on its first start
printf 'glyphs = "unicode"\ntheme = "dark"\n' > "$dir/config/wtf/config.toml"
printf '{"mcpServers": {"wtf": {"command": "%s", "args": ["--mcp"]}}}\n' "$wtf" > "$dir/mcp.json"
# your own status line, hooks and plugins (bands above the prompt) stay out of the recording
printf '{"statusLine": {"type": "command", "command": "true"}, "disableAllHooks": true, "enabledPlugins": {"ponytail@ponytail": false}}\n' > "$dir/settings.json"

prompt="In $dir/demo.vcd one AXI4-Lite read fails. Which address, what response, and how long did it wait for it? Show me the evidence in my wtf window."
# the prompt first: --mcp-config and --allowedTools take lists, and would take it as theirs
agent="claude \"$prompt\" --model $model --strict-mcp-config --mcp-config $dir/mcp.json --allowedTools mcp__wtf --permission-mode default --settings $dir/settings.json"

t="tmux -L wtf-record -f /dev/null"
$t kill-server 2>/dev/null || true
$t new-session -d -s take -x $cols -y $rows -c "$dir" "XDG_CONFIG_HOME=$dir/config $wtf demo.vcd"
sleep 1
$t has-session -t take 2>/dev/null || {
    echo "record.sh: wtf exited at once; run it to see why: XDG_CONFIG_HOME=$dir/config $wtf $dir/demo.vcd" >&2
    exit 1
}
$t set -g status off
$t set -g focus-events on
$t set -g window-size manual
$t split-window -v -l 19 -c "$dir" "$agent"
# the agent needs no keys; yours go to wtf
$t select-pane -t take:0.0

asciinema rec --overwrite --idle-time-limit 2 --window-size ${cols}x$rows -c "$t attach -t take" "$out"
$t kill-server 2>/dev/null || true
echo "recorded $out; asciinema play $out to watch it"
