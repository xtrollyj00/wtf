//! The agent's trail (spec §15 "Trail"): each query and change as one step the user's window
//! shows, with the time it is about.

use rmcp::handler::server::tool::IntoCallToolResult;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, MetaObject};
use rmcp::serde_json::Value;
use wtf_core::time::Tick;
use wtf_core::workspace::Step;

/// The `_meta` keys a step's time and ask ride under from the tool to `call_tool`, which takes
/// them out: the client never sees them.
const TIME: &str = "wtf/time";
const ASK: &str = "wtf/ask";

/// The tools whose calls are steps (spec §15 "Trail"): those answering with an `Answer`. A call
/// whose arguments don't parse never reaches its tool, so `call_tool` knows it by name.
pub const STEPS: [&str; 6] = [
    "find",
    "records",
    "values",
    "render",
    "command",
    "workspace",
];

/// A step tool's answer: its text, the time it is about, and what it was asked in its own words
/// (spec §15 "Trail"). A look-up answers with a plain `String`: the type says which calls make a
/// step, the ask says what it was.
#[derive(Debug)]
pub struct Answer {
    pub text: String,
    pub time: Option<Tick>,
    pub ask: Option<String>,
}

impl Answer {
    pub fn new(text: impl Into<String>, time: Option<Tick>) -> Answer {
        Answer {
            text: text.into(),
            time,
            ask: None,
        }
    }

    /// Run `f` as the step asked `ask`: the ask rides with the answer, or with the error, so the
    /// trail shows what was asked either way (spec §15 "Trail").
    pub fn asked(
        ask: impl Into<String>,
        f: impl FnOnce() -> Result<Answer, String>,
    ) -> Result<Answer, Answer> {
        let ask = Some(ask.into());
        match f() {
            Ok(mut a) => {
                a.ask = ask;
                Ok(a)
            }
            Err(text) => Err(Answer {
                text,
                time: None,
                ask,
            }),
        }
    }
}

impl IntoCallToolResult for Answer {
    fn into_call_tool_result(self) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        // a step tool's own failures answer through the `Err` side, which rmcp marks as the
        // call's error and this conversion still carries the ask out of
        let mut res = CallToolResult::success(vec![ContentBlock::text(self.text)]);
        let mut meta = JsonObject::new();
        if let Some(t) = self.time {
            meta.insert(TIME.into(), Value::from(t));
        }
        if let Some(a) = self.ask {
            meta.insert(ASK.into(), Value::from(a));
        }
        if !meta.is_empty() {
            res.meta = Some(MetaObject(meta));
        }
        Ok(res.into())
    }
}

/// The ask and time an answer carried, taken out so the client never sees them. `None` for a
/// look-up, which makes no step.
pub fn take_meta(res: &mut CallToolResult) -> Option<(String, Option<Tick>)> {
    let mut meta = res.meta.take()?.0;
    let ask = meta.remove(ASK)?.as_str()?.to_string();
    Some((ask, meta.get(TIME).and_then(Value::as_u64)))
}

/// The step a call of `tool` that asked `ask` and answered `answer` makes (spec §15 "Trail").
pub fn step(tool: &str, ask: &str, answer: &str, error: bool, time: Option<Tick>) -> Step {
    // `name:` alone says nothing: with its first value, `data: 40ns 0x2A`
    let first = match tool {
        "values" => answer
            .lines()
            .take(2)
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" "),
        _ => answer.lines().next().unwrap_or_default().to_string(),
    };
    let answer = match (error, tool) {
        (true, _) => format!("error: {first}"),
        // its answer is the `view` that follows; the step says what was asked
        (false, "workspace") => "declared".into(),
        _ => first,
    };
    Step {
        tool: tool.into(),
        ask: ask.into(),
        answer,
        time: if error { None } else { time },
    }
}
