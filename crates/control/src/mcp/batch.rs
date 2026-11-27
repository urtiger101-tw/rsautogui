//! Batched step execution (SPEC section 21): the `run` tool.
//!
//! Every step goes through `Session::call`, so capture proofs, the corner
//! fail-safe and cancellation behave exactly as for single calls. The batch
//! stops at the first failed step; a failed step is reported in the result,
//! it is not a tool error.
use super::errors::{invalid_arguments, report};
use super::input::sleep_sliced;
use super::session::Session;
use autogui::{Error, Result};
use autogui_protocol::NAMES;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::time::Duration;

const MAX_STEPS: usize = 8;
const FORBIDDEN: &[&str] = &["run", "app_launch", "screensaver"];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    steps: Vec<Step>,
    settle_ms: Option<u64>,
    image: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    tool: String,
    #[serde(default)]
    arguments: Option<Value>,
}

struct Batch {
    steps: Vec<(String, Value)>,
    settle: Duration,
    image_last: bool,
}

fn parse(args: Value) -> Result<Batch> {
    let a: Args = serde_json::from_value(args).map_err(invalid_arguments)?;
    if a.steps.is_empty() || a.steps.len() > MAX_STEPS {
        return Err(Error::InvalidArgument("steps must contain 1..8 entries"));
    }
    let settle = a.settle_ms.unwrap_or(100);
    if settle > 2000 {
        return Err(Error::InvalidArgument("settle_ms must be 0..2000"));
    }
    let image_last = match a.image.as_deref() {
        None | Some("none") => false,
        Some("last") => true,
        Some(_) => return Err(Error::InvalidArgument("image must be none or last")),
    };
    let mut steps = Vec::new();
    for step in a.steps {
        if FORBIDDEN.contains(&step.tool.as_str()) {
            return Err(Error::InvalidArgument(
                "run cannot contain run, app_launch or screensaver",
            ));
        }
        if !NAMES.contains(&step.tool.as_str()) {
            return Err(Error::InvalidArgument("unknown tool in steps"));
        }
        let arguments = match step.arguments {
            None => Value::Object(Map::new()),
            Some(v @ Value::Object(_)) => v,
            Some(_) => return Err(Error::InvalidArgument("step arguments must be an object")),
        };
        steps.push((step.tool, arguments));
    }
    Ok(Batch {
        steps,
        settle: Duration::from_millis(settle),
        image_last,
    })
}

/// The capture_id in a tool result (structured content first, then the JSON
/// text block).
fn capture_id(result: &Value) -> Option<String> {
    let from = |v: &Value| v.get("capture_id")?.as_str().map(str::to_owned);
    if let Some(id) = result.get("structuredContent").and_then(from) {
        return Some(id);
    }
    result
        .get("content")?
        .as_array()?
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| serde_json::from_str::<Value>(b["text"].as_str()?).ok())
        .find_map(|v| from(&v))
}

fn is_image(block: &Value) -> bool {
    block["type"] == "image"
}

/// The step result without image blocks: structured content if present,
/// otherwise the text blocks.
fn plain(result: &Value) -> Value {
    if let Some(s) = result.get("structuredContent") {
        return s.clone();
    }
    let texts: Vec<&str> = result["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| !is_image(b))
                .filter_map(|b| b["text"].as_str())
                .collect()
        })
        .unwrap_or_default();
    let joined = texts.join("\n");
    serde_json::from_str(&joined).unwrap_or(Value::String(joined))
}

fn failure(tool: &str, error: &Error) -> Value {
    json!({"tool": tool, "ok": false, "error": report(error).structured()})
}

/// Runs `steps` in order. `check` runs before every step and inside the
/// `settle` waits. With `image_last`, the final step's image block (if the
/// batch completed) is kept under `"_image"` in its entry; all other images
/// are dropped.
fn execute(
    steps: &[(String, Value)],
    settle: Duration,
    image_last: bool,
    exec: &mut dyn FnMut(&str, Value) -> Result<Value>,
    check: &dyn Fn() -> Result<()>,
) -> Value {
    let mut results: Vec<Value> = Vec::new();
    let mut completed = 0usize;
    let mut stopped_at = Value::Null;
    let mut last_capture: Option<String> = None;
    for (index, (tool, arguments)) in steps.iter().enumerate() {
        let gate = check().and_then(|()| {
            if index > 0 {
                sleep_sliced(settle, check)
            } else {
                Ok(())
            }
        });
        let mut arguments = arguments.clone();
        let outcome = gate.and_then(|()| {
            if arguments.get("capture_id").and_then(Value::as_str) == Some("$last") {
                let id = last_capture.clone().ok_or(Error::InvalidArgument(
                    "$last used before any capture in this batch",
                ))?;
                arguments["capture_id"] = Value::String(id);
            }
            exec(tool, arguments)
        });
        match outcome {
            Ok(result) => {
                if let Some(id) = capture_id(&result) {
                    last_capture = Some(id);
                }
                let mut entry = json!({"tool": tool, "ok": true, "result": plain(&result)});
                if image_last
                    && index + 1 == steps.len()
                    && let Some(image) = result["content"]
                        .as_array()
                        .and_then(|b| b.iter().find(|b| is_image(b)))
                {
                    entry["_image"] = image.clone();
                }
                results.push(entry);
                completed += 1;
            }
            Err(error) => {
                results.push(failure(tool, &error));
                stopped_at = json!(index);
                break;
            }
        }
    }
    json!({"completed": completed, "stopped_at": stopped_at, "results": results})
}

/// Runs the MCP tool `name` (run).
pub(super) fn call(
    session: &mut Session,
    _name: &str,
    args: Value,
    check: &dyn Fn() -> Result<()>,
) -> Result<Value> {
    let batch = parse(args)?;
    let mut exec = |tool: &str, arguments: Value| session.call(tool, arguments, check);
    let mut summary = execute(
        &batch.steps,
        batch.settle,
        batch.image_last,
        &mut exec,
        check,
    );
    let mut content = Vec::new();
    if let Some(results) = summary["results"].as_array_mut() {
        for entry in results {
            if let Some(obj) = entry.as_object_mut()
                && let Some(image) = obj.remove("_image")
            {
                content.push(image);
            }
        }
    }
    content.insert(0, json!({"type": "text", "text": summary.to_string()}));
    Ok(json!({"content": content, "structuredContent": summary, "isError": false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn steps(names: &[&str]) -> Value {
        json!({"steps": names.iter().map(|n| json!({"tool": n})).collect::<Vec<_>>()})
    }

    fn mcp(structured: Value, with_image: bool) -> Value {
        let mut content = vec![json!({"type":"text","text":structured.to_string()})];
        if with_image {
            content.push(json!({"type":"image","mimeType":"image/png","data":"AAAA"}));
        }
        json!({"content":content,"structuredContent":structured,"isError":false})
    }

    fn run(
        batch: &Batch,
        exec: &mut dyn FnMut(&str, Value) -> Result<Value>,
        check: &dyn Fn() -> Result<()>,
    ) -> Value {
        execute(&batch.steps, Duration::ZERO, batch.image_last, exec, check)
    }

    #[test]
    fn validates_step_count_and_names() {
        assert!(parse(steps(&[])).is_err());
        assert!(parse(steps(&["cursor_position"; 9])).is_err());
        assert!(parse(steps(&["cursor_position"; 8])).is_ok());
        assert!(parse(steps(&["app_launch"])).is_err());
        assert!(parse(steps(&["screensaver"])).is_err());
        assert!(parse(steps(&["run"])).is_err());
        assert!(parse(steps(&["nope"])).is_err());
        assert!(parse(steps(&["ui_click", "paste_text", "wait_change"])).is_ok());
        assert!(parse(json!({"steps":[{"tool":"capture","arguments":3}]})).is_err());
        assert!(parse(json!({"steps":[{"tool":"capture"}],"image":"all"})).is_err());
        assert!(parse(json!({"steps":[{"tool":"capture"}],"settle_ms":5000})).is_err());
    }

    #[test]
    fn last_capture_id_is_substituted() {
        let batch = parse(json!({"steps":[
            {"tool":"capture","arguments":{"target":"w1-0"}},
            {"tool":"click_at","arguments":{"capture_id":"$last","x":1,"y":2}}]}))
        .unwrap();
        let seen = RefCell::new(Vec::new());
        let mut exec = |tool: &str, args: Value| {
            seen.borrow_mut().push((tool.to_owned(), args.clone()));
            Ok(if tool == "capture" {
                mcp(json!({"capture_id":"c7"}), true)
            } else {
                mcp(json!({}), false)
            })
        };
        let out = run(&batch, &mut exec, &|| Ok(()));
        assert_eq!(out["completed"], 2);
        assert_eq!(out["stopped_at"], Value::Null);
        assert_eq!(seen.borrow()[1].1["capture_id"], "c7");
    }

    #[test]
    fn capture_id_is_found_in_text_block_too() {
        let r = json!({"content":[{"type":"text","text":"{\"capture_id\":\"c3\"}"}]});
        assert_eq!(capture_id(&r).as_deref(), Some("c3"));
    }

    #[test]
    fn last_without_capture_fails_the_step() {
        let batch =
            parse(json!({"steps":[{"tool":"click_at","arguments":{"capture_id":"$last"}}]}))
                .unwrap();
        let mut exec = |_: &str, _: Value| Ok(mcp(json!({}), false));
        let out = run(&batch, &mut exec, &|| Ok(()));
        assert_eq!(out["stopped_at"], 0);
        assert_eq!(out["results"][0]["ok"], false);
        assert_eq!(out["results"][0]["error"]["code"], "invalid_argument");
    }

    #[test]
    fn stops_at_first_failure() {
        let batch = parse(steps(&["capture", "click_at", "scroll"])).unwrap();
        let calls = Cell::new(0);
        let mut exec = |tool: &str, _: Value| {
            calls.set(calls.get() + 1);
            if tool == "click_at" {
                Err(Error::Stopped)
            } else {
                Ok(mcp(json!({"ok":true}), false))
            }
        };
        let out = run(&batch, &mut exec, &|| Ok(()));
        assert_eq!(calls.get(), 2);
        assert_eq!(out["completed"], 1);
        assert_eq!(out["stopped_at"], 1);
        assert_eq!(out["results"].as_array().map(Vec::len), Some(2));
        assert_eq!(out["results"][1]["error"]["code"], "stopped");
    }

    #[test]
    fn check_runs_before_every_step_and_can_stop() {
        let batch = parse(steps(&["capture", "capture", "capture"])).unwrap();
        let checks = Cell::new(0);
        let check = || {
            checks.set(checks.get() + 1);
            Ok(())
        };
        let mut exec = |_: &str, _: Value| Ok(mcp(json!({}), false));
        run(&batch, &mut exec, &check);
        assert!(checks.get() >= 3);

        let n = Cell::new(0);
        let stop = || {
            n.set(n.get() + 1);
            if n.get() > 1 {
                Err(Error::Stopped)
            } else {
                Ok(())
            }
        };
        let ran = Cell::new(0);
        let mut exec = |_: &str, _: Value| {
            ran.set(ran.get() + 1);
            Ok(mcp(json!({}), false))
        };
        let out = run(&batch, &mut exec, &stop);
        assert_eq!(ran.get(), 1);
        assert_eq!(out["stopped_at"], 1);
        assert_eq!(out["results"][1]["error"]["code"], "stopped");
    }

    #[test]
    fn images_are_stripped_unless_last() {
        let mut exec = |_: &str, _: Value| Ok(mcp(json!({"capture_id":"c1"}), true));
        let none = parse(steps(&["capture", "capture"])).unwrap();
        let out = run(&none, &mut exec, &|| Ok(()));
        assert!(out.to_string().find("_image").is_none());
        assert!(!out.to_string().contains("AAAA"));

        let last =
            parse(json!({"steps":[{"tool":"capture"},{"tool":"capture"}],"image":"last"})).unwrap();
        let out = run(&last, &mut exec, &|| Ok(()));
        assert!(out["results"][0].get("_image").is_none());
        assert_eq!(out["results"][1]["_image"]["type"], "image");
    }
}
