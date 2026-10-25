//! The tools' arguments, whose doc comments are the agents' documentation (spec §15).

use rmcp::schemars;
use serde::Deserialize;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct OpenArgs {
    /// Path of a VCD or FST file, best absolute.
    pub trace: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct UseArgs {
    /// The id of one of this server's workspaces, from `sessions`.
    pub session: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SignalsArgs {
    /// `*` within a scope level, `**` across (`**.s_axi_*`); without `*`, any path
    /// containing the text. Empty lists all.
    #[serde(default)]
    pub pattern: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct HelpArgs {
    /// A topic; none lists them.
    pub topic: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ProtocolsArgs {
    /// A protocol to describe (`axilite`, `frame`, ...); none lists them all.
    pub name: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ValuesArgs {
    /// Signals by path or path end (`help names`).
    pub signals: Vec<String>,
    /// Start time with a unit (`help times`).
    pub from: String,
    /// End time, inclusive.
    pub to: String,
    /// hex, bin, dec, ... (`help radix`); `data:dec` for one signal.
    pub radix: Option<String>,
    /// The unit of the answer's times (`help times`).
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
    /// A search expression, `@clk valid && ready` (`help expressions`).
    pub expr: String,
    /// Search after (or with `backward`, before) this time; default the start.
    pub from: Option<String>,
    /// Stop at this time (with `backward`, down to it); default the end.
    pub to: Option<String>,
    #[serde(default)]
    pub backward: bool,
    /// How many lines to list; default 20.
    pub count: Option<usize>,
    /// Signals whose values to list on each match: a table of the transfers.
    #[serde(default)]
    pub show: Vec<String>,
    /// Only count the matches; no list.
    #[serde(default)]
    pub count_only: bool,
    /// As for `values`.
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RenderArgs {
    /// Signals drawn top to bottom; none: the workspace.
    #[serde(default)]
    pub signals: Vec<String>,
    /// Start of the range; default the start, or the workspace's range.
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
    /// The scope names resolve under first (`help names`); not given: it stays.
    pub scope: Option<String>,
    /// Computed signals, `["op = cmd[7:0]"]`.
    #[serde(default)]
    pub signals: Vec<String>,
    /// Sampled protocols, as `protocol` makes them (`help decode`).
    #[serde(default)]
    pub protocols: Vec<ProtocolSpec>,
    /// Decodes, as `decode` makes them (`help decode`).
    #[serde(default)]
    pub decode: Vec<DecodeSpec>,
    /// The rows, top to bottom: `"name:radix:colour"` or a group (`help workspace`).
    pub rows: Vec<RowSpec>,
    /// Cursors by number, `{"1": "80ns", "2": "390ns"}`: 1 the cursor, 2 - 9 marks.
    #[serde(default)]
    pub cursors: std::collections::BTreeMap<String, String>,
    /// The visible range, `["60ns", "480ns"]`; default the whole trace.
    pub range: Option<[String; 2]>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ProtocolSpec {
    /// The stream's name, e.g. `rx`.
    pub name: String,
    /// The clock; with roles, each instance may give it.
    #[serde(default)]
    pub clock: String,
    /// When to make a record: an expression without the clock, `tvalid && tready`.
    pub when: String,
    /// Fields in order, `["tdata = tdata", "op = cmd[7:0]"]`.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Role names, making it a type for `decode`.
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DecodeSpec {
    /// The stream it makes, `rx` (a built-in makes several: `rx.frame`).
    pub name: String,
    /// The protocol: `axis`, `axilite`, `frame`, `pair`, `check`, ... (`protocols`).
    pub protocol: String,
    /// The clock, if not the protocol's.
    #[serde(default)]
    pub clock: String,
    /// Each role's signal, `["tvalid = rx_tvalid"]`; streams for `frame` / `pair`.
    #[serde(default)]
    pub signals: Vec<String>,
    /// Params, `["prefix = rx_"]`, `["last = tlast"]`, a check's rules.
    #[serde(default)]
    pub params: Vec<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RecordsArgs {
    /// A stream, e.g. `bus.wr` (`help decode`).
    pub stream: String,
    /// Records starting at or after this time; default the start.
    pub from: Option<String>,
    /// Records starting at or before this time; default the end.
    pub to: Option<String>,
    /// Only the records this holds for: `resp != 0`, `n == 138`, `req_addr != resp_addr`
    /// (`help records`).
    #[serde(rename = "where")]
    pub filter: Option<String>,
    /// Fields to group by: a line per distinct value, with its count.
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
    /// A `:` command without the colon: `decode ...`, `add ...`, `cursor 2 80ns` (`help commands`).
    pub line: String,
}
