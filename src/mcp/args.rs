//! The tools' arguments, whose doc comments are the agents' documentation (spec §15).

use rmcp::schemars;
use serde::Deserialize;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct OpenArgs {
    /// Path of a VCD or FST file; best absolute (e.g. a trace from `sessions`), as the server's
    /// working directory may not be the user's.
    pub trace: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct UseArgs {
    /// The id of one of this server's workspaces, from `sessions`.
    pub session: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SignalsArgs {
    /// Full-path pattern: `*` within one scope level, `**` across levels (`**.s_axi_*`);
    /// without `*`, any path containing the text (case-insensitive). Empty lists all.
    #[serde(default)]
    pub pattern: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ProtocolsArgs {
    /// A protocol to describe (`axilite`, `frame`, ...); none lists them all.
    pub name: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ValuesArgs {
    /// Signals by full path, or by the end of a path if only one signal ends that way
    /// (`s_axi_wdata`); `data[3]` is one bit.
    pub signals: Vec<String>,
    /// Start time with a unit, e.g. `0ns`, `1.5us`; a plain number is file ticks.
    pub from: String,
    /// End time, inclusive.
    pub to: String,
    /// hex, bin, dec, sdec, oct, ascii, float, fixed point like q4.12, or enum (the names the
    /// file gives values, as `state` READ); default enum where the file names values, otherwise
    /// hex. 1-bit signals always show 0 / 1 / X / Z. One signal's own: `data:dec`.
    pub radix: Option<String>,
    /// The unit for every time in the answer (s, ms, us, ns, ps, fs); default the workspace's
    /// (`command` `unit`, or the fastest clock searched so far). Times are rounded to a thousandth of
    /// it (`2595ns` for 2595.000001ns); `fs` gives them exactly.
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ValueAtArgs {
    /// Signals, named as for `values`.
    pub signals: Vec<String>,
    /// The time, e.g. `120ns`.
    pub time: String,
    /// As for `values`.
    pub radix: Option<String>,
    /// As for `values`.
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct FindArgs {
    /// A search expression: `valid && ready && data == 0x40`, `state == 3 || state == 5`, `!rst`,
    /// with ==, !=, &&, ||, ! and parentheses; values as 0x40, 64, 0b1000000, -12, x (unknown),
    /// z (high impedance). A bare signal is true when 1. A match is where the expression becomes
    /// true. Prefixed with `@clk ` (a clock signal), a match is each clock cycle whose ending
    /// rising edge samples it true, reported at the cycle's starting edge. Over time: `past(x)` is
    /// x one cycle earlier (without a clock, one tick), `rose(x)`, `fell(x)`, `stable(x)` compare
    /// with it, so a protocol check is a search for its violations: `@clk fell(valid) && !past(ready)`.
    /// Slices `tdata[63:32]` and concatenations `{a, b}` are signals too.
    pub expr: String,
    /// Search after (or with `backward`, before) this time; default the start.
    pub from: Option<String>,
    /// Stop at this time (with `backward`, down to it); default the end.
    pub to: Option<String>,
    #[serde(default)]
    pub backward: bool,
    /// How many lines to list; default 20.
    pub count: Option<usize>,
    /// Signals (named as for `values`, `name:radix` for a radix) whose values to list on each
    /// match, e.g. the payload of a handshake: `@clk awvalid && awready` with show
    /// `["awaddr", "wdata", "wstrb:bin"]` is a table of the writes. With a clock, each cycle is
    /// then a row of its own, with the values its ending edge samples.
    #[serde(default)]
    pub show: Vec<String>,
    /// Only count the matches (and with a clock, their runs of consecutive cycles), up to
    /// 1000000; no list.
    #[serde(default)]
    pub count_only: bool,
    /// As for `values`.
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RenderArgs {
    /// Signals, named as for `values`, drawn top to bottom. Empty: draw the workspace (the rows and
    /// cursors prepared with `command`).
    #[serde(default)]
    pub signals: Vec<String>,
    /// Start of the time range shown; default the start of the trace, or for the workspace its
    /// current range. Given for the workspace, the range stays.
    pub from: Option<String>,
    /// End of the time range shown.
    pub to: Option<String>,
    /// Width in characters; default 120.
    pub width: Option<u16>,
    /// The unit of the range in the heading; as for `values`.
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ViewArgs {
    /// As for `values`.
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct WorkspaceArgs {
    /// The scope names resolve under first, here and in every later call (as `command` `scope`):
    /// with `dut`, `s_axi_aclk` is `dut.s_axi_aclk`. Names not under it are looked up
    /// as for `values`. Not given: the workspace's scope stays.
    pub scope: Option<String>,
    /// Named computed signals, defined before the rows, as `:signal` defines them:
    /// `["op = cmd[7:0]", "hdr = {tuser, tdata[63:32]}"]`. A row can name them.
    #[serde(default)]
    pub signals: Vec<String>,
    /// Protocols, defined after the signals: one record per cycle of `clock` where `when` holds,
    /// with `fields` sampled with it. Without `roles` a protocol is a stream of its own name; with
    /// roles it is a type for `decode`. A stream is a signal (`rx`, its record's number) and so is
    /// each field (`rx.tdata`, `z` between records); list its records with `records`.
    #[serde(default)]
    pub protocols: Vec<ProtocolSpec>,
    /// Protocols put on signals, after the protocols: each makes the stream `name` (or several,
    /// `rx.beat`, `rx.frame` ...). Streams and their fields are signals for the rows and every other
    /// tool, and `records` lists them.
    #[serde(default)]
    pub decode: Vec<DecodeSpec>,
    /// The rows, top to bottom: a signal as `"name"` or `"name:radix"` (`"s_axi_awaddr:dec"`), or
    /// a group `{"group": "write", "rows": ["s_axi_awaddr", "s_axi_awvalid"]}` (optionally
    /// `"collapsed": true`). Groups hold signals only.
    pub rows: Vec<RowSpec>,
    /// Cursors by number, e.g. `{"1": "80ns", "2": "390ns"}`: 1 is the cursor (the values shown are
    /// at its time; default the start), 2 - 9 are marks, which stay where they are put: the events to
    /// show. The user jumps to a mark with 'N and back to cursor 1 with ''.
    #[serde(default)]
    pub cursors: std::collections::BTreeMap<String, String>,
    /// The visible range, `["60ns", "480ns"]`; default the whole trace.
    pub range: Option<[String; 2]>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ProtocolSpec {
    /// The stream's name, e.g. `rx`.
    pub name: String,
    /// The clock signal, e.g. `aclk`; with roles it may be left to each instance.
    #[serde(default)]
    pub clock: String,
    /// A search expression without the clock, e.g. `tvalid && tready`.
    pub when: String,
    /// Fields in order, as `"name = signal"`: `["tdata = tdata", "op = cmd[7:0]"]`.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Names `when` and `fields` use for signals each instance gives, e.g. `["valid", "ready",
    /// "data"]`.
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DecodeSpec {
    /// The stream it makes, e.g. `rx`; a built-in protocol makes several (`rx.beat`, `rx.frame`,
    /// `rx.errors`).
    pub name: String,
    /// A protocol with roles; built in: `axis` (roles tvalid, tready, tdata, and tlast, tkeep if
    /// there are), `axilite` (the AXI4-Lite channels' signals, awvalid ... rresp; makes `wr` and `rd`
    /// transactions with their delay, and `errors`), `axi4` (as axilite with bursts and IDs: awid,
    /// awlen, wlast, rid, rlast ...), `spi` (roles sclk, cs, mosi, miso; no clock; params `mode`,
    /// `bits`, `order`; makes `bit`, `word`, `transfer`), `uart` (role `line`; no clock; params `baud`,
    /// `bits`, `parity`, `stop`; makes `byte`, `errors`); `protocols <name>` says more. Or a protocol
    /// on streams: `frame` (role `input`; params `last`, `gap`, `select`, `key`, `bytes`), `pair` (roles `request`, `response`; params `id` or `req_id` and `resp_id`). Or
    /// `check`: params are rules, `"name = expression"`, a violation record per cycle of `clock`
    /// where one holds (`["dropped = fell(valid) && !past(ready)"]`), field `rule`.
    pub protocol: String,
    /// The clock, if not the protocol's.
    #[serde(default)]
    pub clock: String,
    /// Every role's signal, as `"role = signal"`: `["valid = s_axis_tvalid", "data = s_axis_tdata"]`;
    /// for `frame` and `pair`, streams: `["input = rx.beat"]`.
    #[serde(default)]
    pub signals: Vec<String>,
    /// For `frame`, `pair` and `check`, as `"name = value"`: `["last = tlast", "gap = 0"]`. For a
    /// built-in protocol, `["prefix = s_axi_"]` plays each role not in `signals` by `<prefix><role>`,
    /// and its own params: `["baud = 115200"]`.
    #[serde(default)]
    pub params: Vec<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RecordsArgs {
    /// A stream made by `decode` or `protocol` (with `command` or in `workspace`), e.g. `rx.frame`.
    pub stream: String,
    /// Records starting at or after this time; default the start.
    pub from: Option<String>,
    /// Records starting at or before this time; default the end.
    pub to: Option<String>,
    /// Only the records for which this holds: a search expression over the stream's fields, named
    /// bare, evaluated on each record (`addr != 0x1C && resp != 0`, `rule == dropped`, `past(x)` is x
    /// of the record before).
    #[serde(rename = "where")]
    pub filter: Option<String>,
    /// Fields to group by: one line per distinct value of them, with how many records have it and
    /// the first and last start (`4 × addr=0x1C data=0x3 (260ns .. 350ns)`), so repeats are one line.
    #[serde(default)]
    pub group_by: Vec<String>,
    /// How many records to list; default 20.
    pub count: Option<usize>,
    /// Only count the records; no list.
    #[serde(default)]
    pub count_only: bool,
    /// As for `values`.
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum RowSpec {
    Signal(String),
    Group {
        group: String,
        rows: Vec<String>,
        #[serde(default)]
        collapsed: bool,
    },
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct UserViewArgs {
    /// Also copy the user's rows, cursors and visible range into the workspace, to build on them.
    #[serde(default)]
    pub adopt: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct CommandArgs {
    /// One of wtf's `:` commands, without the colon, run on the workspace: `add <path|pattern>...`
    /// (below the selected row, joining its group; after the group if its header is selected; a
    /// scope becomes a group), `rm <path|pattern>...` (`rm **` clears the workspace),
    /// `select <row> [<last row>]` (the rows `group`, `rename`, `ungroup` and `radix` act on),
    /// `group <name>`, `rename <name>`, `ungroup`, `radix <hex|bin|dec|sdec|oct|ascii|float|q4.12|enum>`,
    /// `cursor <1-9> [time|off]` (1 is the cursor, where values are read; 2 - 9 are marks, which stay
    /// where they are put; without a time the cursor jumps to that mark), `<time>` (moves the
    /// cursor), `zoom <time per column>`, `fit`, `undo`, `redo`, `unit <ns|ps|...|auto>` (the time
    /// unit of every answer, to a thousandth of it; auto: the fastest clock searched), `scope <path>`
    /// (names resolve under it first in every tool: `s_axi_awvalid` is `<path>.s_axi_awvalid`; empty:
    /// none). The answer lists the
    /// rows added and removed. To set up the whole workspace at once, `workspace` is simpler.
    /// Decoding, without touching the rows: `signal <name> = <expr>` (a computed signal),
    /// `decode <name> <protocol> clock=<clk> <role>=<signal>...` (streams `<name>.<step>`; built in:
    /// `axis`, `axilite`, `axi4`, and without a clock `spi` (`mode=0`), `uart` (`baud=115200`);
    /// `prefix=s_axi_` plays each role by `<prefix><role>`; on streams: `frame`
    /// `input=<stream>`, `pair` `request=<s> response=<s>`, `check` `<rule>=<expr>`), and
    /// `protocol <name> clock=<clk> when=<expr> <field>=<signal>...` (a stream sampled where `when`
    /// holds; with `roles=a,b` a type for `decode`). A value runs to the next `key=`, so it can have
    /// spaces. List the records with `records`.
    pub line: String,
}
