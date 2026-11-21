//! The session file as one serde model (spec §11).
//!
//! Every entry struct is permissive: scalars the reader must warn about (a non-string name, a
//! bad time) stay `Option<toml::Value>`, so parsing the file never fails. `apply` checks entry by
//! entry: a bad entry is a warning, unknown keys (each struct's `rest`) are listed, only a broken
//! file is an `Err`. The writer only builds valid values.

use serde::{Deserialize, Serialize};
use toml::{Table, Value};

fn version_default() -> i64 {
    super::VERSION
}

/// The whole session file (spec §11). Field order is the file's key order.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct File {
    #[serde(default = "version_default")]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub version: i64,
    /// The scope names resolve under first (`help names`); not given: it stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// The workspace's time unit (`help times`); not given: it stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The visible range, `{"time": "60ns", "to": "480ns"}`; default the whole trace.
    #[serde(default, skip_serializing_if = "no_view")]
    pub view: View,
    /// The cursor and marks by number, `{"1": "80ns", "2": {"time": "390ns", "name": "first bad write", "note": "..."}}`: 1 is the cursor, a bare time, never named; marks 2 - 9 take a time or a name and note.
    #[serde(default, skip_serializing_if = "Table::is_empty")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub cursors: Table,
    /// Computed signals, `[{"name": "op", "expr": "cmd[7:0]"}]`.
    #[serde(
        default,
        deserialize_with = "de_entry_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub signal: Vec<SignalEntry>,
    /// Value names, `[{"signal": "state", "values": "0=zero"}]`.
    #[serde(
        default,
        deserialize_with = "de_entry_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub names: Vec<NamesEntry>,
    /// Sampled protocols, as `protocol` makes them (`help decode`).
    #[serde(
        default,
        deserialize_with = "de_entry_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub protocol: Vec<Protocol>,
    /// Decodes, as `decode` makes them (`help decode`).
    #[serde(
        default,
        deserialize_with = "de_entry_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub decode: Vec<Decode>,
    /// The rows, top to bottom: `{"signal": "awaddr", "radix": "hex", "color": "red"}`, a group or a struct's open levels (`help workspace`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Row>,
    #[serde(
        default,
        deserialize_with = "de_entry_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub trail: Vec<TrailStep>,
    #[serde(default, skip_serializing_if = "Table::is_empty")]
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub rest: Table,
}

impl Default for File {
    fn default() -> Self {
        File {
            version: super::VERSION,
            scope: None,
            unit: None,
            view: View::default(),
            cursors: Table::new(),
            signal: Vec::new(),
            names: Vec::new(),
            protocol: Vec::new(),
            decode: Vec::new(),
            rows: Vec::new(),
            trail: Vec::new(),
            rest: Table::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct View {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub zoom: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub time: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub to: Option<Value>,
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub rest: Table,
}

/// An empty view writes nothing: the layout's text then holds no `[view]` (`[+]` compares it).
fn no_view(v: &View) -> bool {
    v.zoom.is_none() && v.time.is_none() && v.to.is_none() && v.rest.is_empty()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct SignalEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub name: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub expr: Option<Value>,
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub rest: Table,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct NamesEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub signal: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub values: Option<Value>,
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub rest: Table,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Protocol {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub name: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub when: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub roles: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub fields: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub wasm: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    // read so the declaration can say it takes none, never offered (`schemars(skip)`)
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub clock: Option<Value>,
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub rest: Table,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Decode {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub name: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub protocol: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub clock: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub signals: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub params: Option<Value>,
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub rest: Table,
}

// One `[[rows]]` entry: a signal row, a group, or anything else (which `apply` warns about as
// `rows: entry without signal or group`). `Signal`/`Group` keep their discriminator required so
// dispatch falls through to `Other`, which the schema leaves out: an agent is offered only signal
// and group rows. A `//` comment: a doc comment would be the schema's text.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum Row {
    Signal(SignalRow),
    Group(GroupRow),
    #[cfg_attr(feature = "schema", schemars(skip))]
    Other(#[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))] Value),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SignalRow {
    pub signal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub radix: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub color: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub open: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub fields: Option<Value>,
    #[serde(default, skip_serializing_if = "Table::is_empty")]
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub rest: Table,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GroupRow {
    pub group: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub collapsed: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub rows: Option<Value>,
    #[serde(default, skip_serializing_if = "Table::is_empty")]
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub rest: Table,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct TrailStep {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub tool: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub ask: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub answer: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_value"))]
    pub time: Option<Value>,
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub rest: Table,
}

/// Any TOML value; the workspace checks it when the declaration applies.
#[cfg(feature = "schema")]
fn any_value(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({})
}

/// Elements of the wrong shape read as missing entries, which `apply` warns about (spec §11):
/// only a broken file is an `Err`.
fn de_entry_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default,
{
    let vs = Vec::<Value>::deserialize(d)?;
    Ok(vs
        .into_iter()
        .map(|v| T::deserialize(v).unwrap_or_default())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_shaped_entries_read_as_missing() {
        let f: File = toml::from_str("signal = [\"x\"]\n").unwrap();
        assert_eq!(f.signal.len(), 1);
        assert!(f.signal[0].name.is_none());
    }

    #[test]
    fn unknown_keys_land_in_rest() {
        let f: File = toml::from_str("mystery = 1\n").unwrap();
        assert_eq!(f.rest.get("mystery"), Some(&Value::Integer(1)));
    }

    #[test]
    fn file_round_trips() {
        let mut f = File {
            scope: Some("top".into()),
            ..Default::default()
        };
        f.view.time = Some(Value::String("0ns".into()));
        f.rows.push(Row::Group(GroupRow {
            group: "cpu".into(),
            ..Default::default()
        }));
        let text = toml::to_string(&f).unwrap();
        assert_eq!(
            toml::to_string(&toml::from_str::<File>(&text).unwrap()).unwrap(),
            text
        );
    }
}
