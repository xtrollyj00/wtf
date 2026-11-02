//! The agent's trail (spec §15 "Trail"): each query and change as one step the user's window
//! shows, with the time it is about.

use rmcp::model::JsonObject;
use rmcp::serde_json::Value;
use wtf_core::time::{self, Tick};
use wtf_core::workspace::{Step, Workspace};

/// The tools whose calls are steps: queries of the trace and changes to the workspace. Lookups
/// (`help`, `info`, `signals`, `view`, ...) show nothing about the trace.
const STEPS: [&str; 7] = ["find", "records", "values", "value_at", "render", "command", "workspace"];

/// The step a call of `tool` with `args` that answered `answer` makes, if it makes one.
pub fn step(app: &Workspace, tool: &str, args: Option<&JsonObject>, answer: &str, error: bool) -> Option<Step> {
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
    let fpt = app.trace.fs_per_tick;
    let at = |s: &str| time::parse(s.trim(), fpt).ok().map(|t| (t.max(0.0).round() as Tick).min(app.trace.end()));
    let time = match tool {
        _ if error => None,
        "value_at" => at(arg("time")),
        "values" | "render" if !arg("from").is_empty() => at(arg("from")),
        _ => first_time(answer, at),
    };
    let first = answer.lines().next().unwrap_or_default();
    let answer = match (error, tool) {
        (true, _) => format!("error: {first}"),
        // its answer is the `view` that follows; the step says what was asked
        (false, "workspace") => "declared".into(),
        _ => first.to_string(),
    };
    Some(Step { tool: tool.into(), ask, answer, time })
}

/// The first time an answer lists: `first <time>` in a count, or the first value under a `time` or
/// `start` column (`find`, `records`).
fn first_time(answer: &str, at: impl Fn(&str) -> Option<Tick>) -> Option<Tick> {
    if let Some(after) = answer.lines().next()?.split_once("first ") {
        return at(after.1.split(',').next()?);
    }
    let mut lines = answer.lines();
    while let Some(head) = lines.next() {
        if let Some(k) = head.split('\t').position(|c| c == "time" || c == "start") {
            return at(lines.next()?.split('\t').nth(k)?);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(answer: &str) -> Option<Tick> {
        first_time(answer, |s| s.trim().strip_suffix("ns")?.parse().ok())
    }

    #[test]
    fn first_times() {
        assert_eq!(times("3 matches, first 10ns, last 60ns"), Some(10));
        assert_eq!(times("time\n20ns\n40ns"), Some(20));
        assert_eq!(times("7 of 7\nn\tstart\tend\td\n0\t5ns\t15ns\t0x43"), Some(5));
        assert_eq!(times("no matches"), None);
    }
}
