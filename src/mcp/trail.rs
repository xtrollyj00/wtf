//! The agent's trail (spec §15 "Trail"): each query and change as one step the user's window
//! shows, with the time it is about.

use rmcp::handler::server::tool::IntoCallToolResult;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, MetaObject};
use rmcp::serde_json::Value;
use wtf_core::time::Tick;
use wtf_core::workspace::Step;

/// The tools whose calls are steps: queries of the trace and changes to the workspace. Lookups
/// (`help`, `info`, `signals`, `view`, ...) show nothing about the trace.
const STEPS: [&str; 7] = ["find", "records", "values", "value_at", "render", "command", "workspace"];

/// The `_meta` key a tool's time rides under from the tool to `call_tool`, which takes it out.
const TIME: &str = "wtf/time";

/// A step tool's answer: its text, and the time it is about, if any (spec §15 "Trail").
pub struct Answer {
    pub text: String,
    pub time: Option<Tick>,
}

impl IntoCallToolResult for Answer {
    fn into_call_tool_result(self) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let mut res = CallToolResult::success(vec![ContentBlock::text(self.text)]);
        if let Some(t) = self.time {
            let mut meta = JsonObject::new();
            meta.insert(TIME.into(), Value::from(t));
            res.meta = Some(MetaObject(meta));
        }
        Ok(res.into())
    }
}

/// The time an answer carried, taken out so the client never sees it.
pub fn take_time(res: &mut CallToolResult) -> Option<Tick> {
    let mut meta = res.meta.take()?.0;
    let time = meta.remove(TIME)?.as_u64();
    // no empty `_meta` on the wire
    if !meta.is_empty() {
        res.meta = Some(MetaObject(meta));
    }
    time
}

/// The step a call of `tool` with `args` that answered `answer` makes, if it makes one.
pub fn step(tool: &str, args: Option<&JsonObject>, answer: &str, error: bool, time: Option<Tick>) -> Option<Step> {
    if !STEPS.contains(&tool) {
        return None;
    }
    let arg = |k: &str| args.and_then(|a| a.get(k)).and_then(Value::as_str).unwrap_or_default();
    let list = |k: &str| args.and_then(|a| a.get(k)).and_then(Value::as_array).map_or(String::new(), |l| l.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "));
    let count = |k: &str| args.and_then(|a| a.get(k)).and_then(Value::as_array).map_or(0, Vec::len);
    let ask = match tool {
        "command" => arg("line").to_string(),
        "find" => arg("expr").to_string(),
        "records" if arg("where").is_empty() => arg("stream").to_string(),
        "records" => format!("{} where {}", arg("stream"), arg("where")),
        "values" => format!("{} from {} to {}", list("signals"), arg("from"), arg("to")),
        "value_at" => format!("{} at {}", list("signals"), arg("time")),
        "render" if count("signals") == 0 => "the workspace".into(),
        "render" => list("signals"),
        _ => format!("{} rows, {} cursors", count("rows"), args.and_then(|a| a.get("cursors")).and_then(Value::as_object).map_or(0, |c| c.len())),
    };
    let time = if error { None } else { time };
    let first = answer.lines().next().unwrap_or_default();
    let answer = match (error, tool) {
        (true, _) => format!("error: {first}"),
        // its answer is the `view` that follows; the step says what was asked
        (false, "workspace") => "declared".into(),
        _ => first.to_string(),
    };
    Some(Step { tool: tool.into(), ask, answer, time })
}
