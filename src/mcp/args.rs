//! The tools' arguments, whose doc comments are the agents' documentation (spec §15).

use rmcp::schemars;
use serde::Deserialize;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct OpenArgs {
    /// Path of a VCD, FST or GHW file, best absolute: a new workspace on it.
    pub trace: Option<String>,
    /// The id of one of this server's workspaces, from `sessions`: make it current again.
    pub session: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SignalsArgs {
    /// A regex in slashes, found anywhere in the path in any case (no flags), indices in brackets
    /// (`/s_axi_.*valid/`, `/^top\.cpu\./`, `/gen\[0\]/`); none lists all.
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
    /// Signals by path or path end (`help names`); `signals` finds them.
    pub signals: Vec<String>,
    /// Start time with its unit, any unit: `120ns`, `3.4ms` (`help times`).
    pub from: String,
    /// End time, inclusive; omit for the value at `from` alone.
    pub to: Option<String>,
    /// hex, bin, dec, ... (`help radix`); `data:dec` for one signal.
    pub radix: Option<String>,
    /// The unit of the answer's times (`help times`).
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct FindArgs {
    /// A search expression as in SystemVerilog: `==` `!=` `<` `&&` `||` `!`; only `rose(x)` `fell(x)`
    /// `stable(x)` `past(x)` (SVA's, no `$`); `@<clock> ` first samples each cycle:
    /// `@top.clk valid && ready`, `rose(irq) && !past(busy)` (`help expressions`).
    pub expr: String,
    /// Search after (or with `backward`, before) this time; default the start.
    pub from: Option<String>,
    /// Stop at this time (with `backward`, down to it); default the end.
    pub to: Option<String>,
    /// Search toward the start: the latest matches first.
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
    /// The unit of the answer's times, `ns`, `us` (`help times`).
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RenderArgs {
    /// Signals drawn top to bottom; none: the workspace.
    #[serde(default)]
    pub signals: Vec<String>,
    /// Start of the range; default the start, or the workspace's range.
    pub from: Option<String>,
    /// End of the range; default the end, or the workspace's range.
    pub to: Option<String>,
    /// Width in characters; default 120.
    pub width: Option<u16>,
    /// The unit of the range in the heading, `ns`, `us` (`help times`).
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ViewArgs {
    /// The unit of the answer's times, `ns`, `us` (`help times`).
    pub unit: Option<String>,
}

/// The workspace in the session's own shape (spec §11): `signal`, `protocol`, `decode`, `names`,
/// `rows`, `cursors`, `view`, `scope`, `unit`. Every name, radix, time and key is checked first
/// and all the errors come back at once.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct WorkspaceArgs {
    #[serde(flatten)]
    pub file: wtf_core::session::File,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RecordsArgs {
    /// A stream, e.g. `bus.wr`; a decode's name for its results, `bus`; a pattern, `/^bus\./`;
    /// or several, space-separated (`help records`).
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
    /// hex, dec, ... for every field (`help radix`); `len:dec` for one, several with commas.
    pub radix: Option<String>,
    /// The unit of the answer's times, `ns`, `us` (`help times`).
    pub unit: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct UserViewArgs {
    /// Also copy the user's rows, cursors and visible range into the workspace, to build on them;
    /// they replace the workspace's (one `undo` step).
    #[serde(default)]
    pub adopt: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct CommandArgs {
    /// A `:` command without the colon: `decode ...`, `add top.cpu /_valid$/`, `cursor 2 80ns`
    /// (`help commands`).
    pub line: String,
}
