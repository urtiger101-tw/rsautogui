//! Local action log (SPEC §22): one JSON line per MCP tool call or CLI
//! command in `%LOCALAPPDATA%\AutoGui\logs\actions-YYYY-MM-DD.jsonl`
//! (local date). It is on by default and switched by
//! [`crate::user_config`]. Nothing leaves the machine.
//!
//! Text the agent types or pastes is never written: `type_text`,
//! `paste_text` and `ui_set_value` text (also inside `run` steps), and the
//! CLI `type`/`paste`/`ui set-value` text, become `"[N chars]"`. Other
//! strings are cut to 300 characters. Files older than 30 days are deleted
//! once per process. A failure to write the log never fails the action.
use serde_json::{Map, Value, json};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

const MAX_STRING: usize = 300;
const KEEP_DAYS: i64 = 30;

/// Tools whose `text` argument is typed or pasted into an application.
const TEXT_TOOLS: &[&str] = &["type_text", "paste_text", "ui_set_value"];

/// Keys that only select a window or element. For text tools, unknown
/// tools and malformed `run` steps every other string is reduced to a
/// character count, so text can never slip through under another key.
const SELECTOR_KEYS: &[&str] = &[
    "target",
    "element",
    "name",
    "name_match",
    "role",
    "automation_id",
];

/// The serialized `args` above this size are replaced by their length.
const MAX_ARGS_BYTES: usize = 8 * 1024;

/// CLI commands whose arguments are logged (text commands redacted).
const CLI_COMMANDS: &[&str] = &[
    "list",
    "wait",
    "capture",
    "locate",
    "click",
    "type",
    "paste",
    "screensaver",
    "window",
    "display",
    "ui",
    "session",
];

fn chars(text: &str) -> Value {
    Value::String(format!("[{} chars]", text.chars().count()))
}

fn cut_str(text: &str) -> String {
    if text.chars().count() <= MAX_STRING {
        text.to_owned()
    } else {
        let head: String = text.chars().take(MAX_STRING).collect();
        format!("{head}…")
    }
}

fn cut(text: &str) -> Value {
    Value::String(cut_str(text))
}

/// Cuts long strings everywhere in a JSON value.
fn cut_all(value: &Value) -> Value {
    match value {
        Value::String(s) => cut(s),
        Value::Array(items) => Value::Array(items.iter().map(cut_all).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), cut_all(v)))
                .collect::<Map<_, _>>(),
        ),
        other => other.clone(),
    }
}

/// Every string becomes `"[N chars]"`, at any depth.
fn hide_all(value: &Value) -> Value {
    match value {
        Value::String(s) => chars(s),
        Value::Array(items) => Value::Array(items.iter().map(hide_all).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), hide_all(v)))
                .collect::<Map<_, _>>(),
        ),
        other => other.clone(),
    }
}

/// Selector keys stay readable; everything else is hidden.
fn hide_except_selectors(args: &Value) -> Value {
    match args {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let shown = if SELECTOR_KEYS.contains(&k.as_str()) {
                        cut_all(v)
                    } else {
                        hide_all(v)
                    };
                    (k.clone(), shown)
                })
                .collect::<Map<_, _>>(),
        ),
        other => hide_all(other),
    }
}

fn redact_inner(tool: &str, args: &Value) -> Value {
    let known = autogui_protocol::tools::NAMES.contains(&tool);
    if !known || TEXT_TOOLS.contains(&tool) {
        return hide_except_selectors(args);
    }
    let mut logged = cut_all(args);
    match tool {
        // Program arguments may carry tokens: keep only their number.
        "app_launch" => {
            if let (Some(map), Some(list)) = (
                logged.as_object_mut(),
                args.get("args").and_then(Value::as_array),
            ) {
                map.insert("args".into(), json!(format!("[{} args]", list.len())));
            }
        }
        "run" => {
            if let (Some(steps), Some(original)) = (
                logged.get_mut("steps").and_then(Value::as_array_mut),
                args.get("steps").and_then(Value::as_array),
            ) {
                for (step, source) in steps.iter_mut().zip(original) {
                    *step = match (
                        source.get("tool").and_then(Value::as_str),
                        source.get("arguments"),
                    ) {
                        (Some(name), Some(arguments)) => {
                            json!({"tool": cut_str(name), "arguments": redact_inner(name, arguments)})
                        }
                        // A malformed step may hold text under any key.
                        _ => hide_all(source),
                    };
                }
            }
        }
        _ => {}
    }
    logged
}

/// MCP arguments as they are logged.
pub(crate) fn redact_tool_args(tool: &str, args: &Value) -> Value {
    let logged = redact_inner(tool, args);
    let size = serde_json::to_string(&logged).map_or(usize::MAX, |s| s.len());
    if size > MAX_ARGS_BYTES {
        json!({"truncated_bytes": size})
    } else {
        logged
    }
}

/// CLI arguments as they are logged.
pub(crate) fn redact_cli_args(args: &[String]) -> Vec<String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Vec::new();
    };
    if !CLI_COMMANDS.contains(&command) {
        // A mistyped command may carry text: keep only its shape.
        return vec![cut_str(command), format!("[{} args]", args.len() - 1)];
    }
    let mut logged: Vec<String> = args.iter().map(|a| cut_str(a)).collect();
    if matches!(command, "type" | "paste") {
        for (slot, original) in logged.iter_mut().zip(args).skip(2) {
            *slot = format!("[{} chars]", original.chars().count());
        }
    }
    if command == "ui" && args.get(1).is_some_and(|a| a == "set-value") {
        let mut redact_next = false;
        for (slot, original) in logged.iter_mut().zip(args) {
            if redact_next || original.starts_with("--text=") {
                *slot = format!("[{} chars]", original.chars().count());
            }
            redact_next = original == "--text";
        }
    }
    logged
}

// ---- time -----------------------------------------------------------------

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// (year, month, day) for days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if month <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        month,
        day,
    )
}

/// `YYYY-MM-DD` and an RFC 3339 timestamp with offset, from UTC
/// milliseconds and the local offset in minutes.
fn stamp(utc_ms: i64, offset_min: i64) -> (String, String) {
    let local = utc_ms + offset_min * 60_000;
    let days = local.div_euclid(86_400_000);
    let ms_of_day = local.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss, ms) = (
        ms_of_day / 3_600_000,
        ms_of_day / 60_000 % 60,
        ms_of_day / 1000 % 60,
        ms_of_day % 1000,
    );
    let sign = if offset_min < 0 { '-' } else { '+' };
    let off = offset_min.abs();
    let date = format!("{y:04}-{m:02}-{d:02}");
    let ts = format!(
        "{date}T{hh:02}:{mm:02}:{ss:02}.{ms:03}{sign}{:02}:{:02}",
        off / 60,
        off % 60
    );
    (date, ts)
}

#[cfg(windows)]
fn local_offset_minutes() -> i64 {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    fn ms(t: &SYSTEMTIME) -> i64 {
        let days = days_from_civil(i64::from(t.wYear), i64::from(t.wMonth), i64::from(t.wDay));
        days * 86_400_000
            + i64::from(t.wHour) * 3_600_000
            + i64::from(t.wMinute) * 60_000
            + i64::from(t.wSecond) * 1000
            + i64::from(t.wMilliseconds)
    }
    let zero = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    let (mut utc, mut local) = (zero, zero);
    // SAFETY: both functions only write the SYSTEMTIME they are given.
    unsafe {
        GetSystemTime(&mut utc);
        GetLocalTime(&mut local);
    }
    // Offsets are whole quarter hours; rounding absorbs the gap between calls.
    let minutes = (ms(&local) - ms(&utc)) as f64 / 60_000.0;
    ((minutes / 15.0).round() * 15.0) as i64
}

#[cfg(not(windows))]
fn local_offset_minutes() -> i64 {
    0
}

fn now() -> (String, String) {
    let utc_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    stamp(utc_ms, local_offset_minutes())
}

// ---- writing --------------------------------------------------------------

/// `actions-YYYY-MM-DD.jsonl` files dated before `today_days - KEEP_DAYS`.
fn expired(names: &[String], today_days: i64) -> Vec<String> {
    names
        .iter()
        .filter(|name| {
            let Some(date) = name
                .strip_prefix("actions-")
                .and_then(|n| n.strip_suffix(".jsonl"))
            else {
                return false;
            };
            parse_date(date)
                .is_some_and(|(y, m, d)| days_from_civil(y, m, d) < today_days - KEEP_DAYS)
        })
        .cloned()
        .collect()
}

/// `YYYY-MM-DD` with exactly three numeric parts.
fn parse_date(text: &str) -> Option<(i64, i64, i64)> {
    let mut parts = text.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((year, month, day))
}

fn prune(dir: &Path, today: &str) {
    let Some((y, m, d)) = parse_date(today) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let names: Vec<String> = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    for name in expired(&names, days_from_civil(y, m, d)) {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

fn append(entry: Value) {
    let Some(dir) = crate::user_config::log_dir() else {
        return;
    };
    let (date, ts) = now();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    // Prune once per process and again whenever the local date changes.
    static PRUNED: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    if let Ok(mut last) = PRUNED.lock()
        && *last != date
    {
        prune(&dir, &date);
        last.clone_from(&date);
    }
    let Some(mut line) = line_with_time_first(&ts, std::process::id(), &entry) else {
        return;
    };
    line.push(b'\n');
    // One write per line in append mode, so concurrent servers do not interleave.
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("actions-{date}.jsonl")))
    {
        let _ = file.write_all(&line);
    }
}

fn failure(code: String, message: &str) -> Value {
    json!({"ok": false, "code": code, "error": cut(message)})
}

/// `{"ts":…,"pid":…,<entry fields>}` so each line starts with its time.
fn line_with_time_first(ts: &str, pid: u32, entry: &Value) -> Option<Vec<u8>> {
    let rest = serde_json::to_string(entry).ok()?;
    let head = format!("{{\"ts\":{},\"pid\":{pid}", serde_json::to_string(ts).ok()?);
    let body = rest.strip_prefix('{')?;
    let line = if body == "}" {
        format!("{head}}}")
    } else {
        format!("{head},{body}")
    };
    Some(line.into_bytes())
}

/// The logged outcome of an MCP call: `ok`, plus `code` and `error` on
/// failure (also for `run`, which reports a failed step as `isError`).
fn outcome_of(result: &autogui::Result<Value>, code: impl Fn(&autogui::Error) -> String) -> Value {
    match result {
        Ok(value) if value.get("isError").and_then(Value::as_bool) == Some(true) => {
            let content = value.get("structuredContent");
            json!({
                "ok": false,
                "code": content.and_then(|c| c.get("code")).cloned().unwrap_or(Value::Null),
                "error": content
                    .and_then(|c| c.get("message"))
                    .or_else(|| value.pointer("/content/0/text"))
                    .map(cut_all)
                    .unwrap_or(Value::Null),
            })
        }
        Ok(_) => json!({"ok": true}),
        Err(e) => failure(code(e), &e.to_string()),
    }
}

/// One MCP tool call. `window` is the target window's title when known;
/// `in_run` marks a step executed by the `run` tool.
#[cfg(feature = "agent")]
pub(crate) fn record_tool(
    tool: &str,
    window: Option<String>,
    args: Value,
    result: &autogui::Result<Value>,
    elapsed: Duration,
    in_run: bool,
) {
    let mut entry = json!({
        "source": "mcp",
        "tool": tool,
        "args": args,
        "ms": u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
    });
    if let Some(map) = entry.as_object_mut() {
        if let Some(title) = window {
            map.insert("window".into(), cut(&title));
        }
        if in_run {
            map.insert("in_run".into(), Value::Bool(true));
        }
        if let Value::Object(outcome) = outcome_of(result, |e| {
            crate::mcp::errors::report(e).code.as_str().to_owned()
        }) {
            map.extend(outcome);
        }
    }
    append(entry);
}

/// A change of the settings themselves, written before it applies, so
/// switching the log off always leaves a trace.
pub(crate) fn record_settings_change(
    before: &crate::user_config::UserConfig,
    after: &crate::user_config::UserConfig,
) {
    if !(crate::user_config::load().action_log || after.action_log) {
        return;
    }
    append(json!({
        "source": "cli",
        "command": "settings",
        "before": {"overlay": before.overlay, "action_log": before.action_log},
        "after": {"overlay": after.overlay, "action_log": after.action_log},
        "ok": true,
    }));
}

/// One CLI command (not `settings`, `mcp` or internal commands).
pub(crate) fn record_cli(args: &[String], result: &autogui::Result<()>, elapsed: Duration) {
    let Some(command) = args.first() else {
        return;
    };
    if matches!(
        command.as_str(),
        "settings" | "mcp" | "__runtime-version" | "--help" | "-h"
    ) || !crate::user_config::load().action_log
    {
        return;
    }
    let mut entry = json!({
        "source": "cli",
        "command": command,
        "args": redact_cli_args(args),
        "ms": u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
    });
    let outcome = match result {
        Ok(()) => json!({"ok": true}),
        Err(e) => failure(crate::cli::errors::kind(e).to_owned(), &e.to_string()),
    };
    if let (Some(map), Value::Object(outcome)) = (entry.as_object_mut(), outcome) {
        map.extend(outcome);
    }
    append(entry);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_and_pasted_text_is_never_logged() {
        for tool in TEXT_TOOLS {
            let logged = redact_tool_args(tool, &json!({"target":"w1-0","text":"hunter2 密碼"}));
            assert_eq!(logged["text"], "[10 chars]", "{tool}");
            assert_eq!(logged["target"], "w1-0");
        }
        // Search text is not typed into anything and stays readable.
        let logged = redact_tool_args("ui_wait", &json!({"text":"Saved"}));
        assert_eq!(logged["text"], "Saved");
    }

    #[test]
    fn malformed_calls_never_leak_text() {
        // Misnamed tool carrying text.
        let logged = redact_tool_args("type", &json!({"target":"w1-0","text":"secret"}));
        assert_eq!(logged["text"], "[6 chars]");
        assert_eq!(logged["target"], "w1-0");
        // Text under the wrong key of a text tool.
        let logged = redact_tool_args("ui_set_value", &json!({"name":"Pwd","value":"hunter2"}));
        assert_eq!(logged["value"], "[7 chars]");
        assert_eq!(logged["name"], "Pwd");
        // Malformed run step.
        let logged = redact_tool_args(
            "run",
            &json!({"steps":[{"name":"type_text","args":{"text":"secret"}}]}),
        );
        assert_eq!(logged["steps"][0]["name"], "[9 chars]");
        assert_eq!(logged["steps"][0]["args"]["text"], "[6 chars]");
        // Nested text inside an otherwise known text tool.
        let logged = redact_tool_args("paste_text", &json!({"extra":{"deep":["abc"]}}));
        assert_eq!(logged["extra"]["deep"][0], "[3 chars]");
    }

    #[test]
    fn launch_arguments_are_counted_and_big_args_truncated() {
        let logged = redact_tool_args(
            "app_launch",
            &json!({"path":"C:\\a.exe","args":["--token","abc"]}),
        );
        assert_eq!(logged["args"], "[2 args]");
        assert_eq!(logged["path"], "C:\\a.exe");
        let many: Vec<Value> = (0..2000).map(|i| json!(i)).collect();
        let logged = redact_tool_args("windows_list", &json!({"list": many}));
        assert!(logged["truncated_bytes"].as_u64().unwrap() > MAX_ARGS_BYTES as u64);
    }

    #[test]
    fn dates_need_exactly_three_parts() {
        assert_eq!(parse_date("2026-11-28"), Some((2026, 11, 28)));
        assert_eq!(parse_date("2026-x-10-28"), None);
        assert_eq!(parse_date("2026-10-28-1"), None);
        assert_eq!(parse_date("2026-10"), None);
    }

    #[test]
    fn run_steps_are_redacted_per_tool() {
        let logged = redact_tool_args(
            "run",
            &json!({"steps":[
                {"tool":"type_text","arguments":{"target":"w1-0","text":"secret"}},
                {"tool":"hotkey","arguments":{"target":"w1-0","keys":["ctrl","s"]}}
            ]}),
        );
        assert_eq!(logged["steps"][0]["arguments"]["text"], "[6 chars]");
        assert_eq!(
            logged["steps"][1]["arguments"]["keys"],
            json!(["ctrl", "s"])
        );
    }

    #[test]
    fn long_strings_are_cut() {
        let long = "x".repeat(MAX_STRING + 50);
        let logged = redact_tool_args("windows_list", &json!({"title": long}));
        let title = logged["title"].as_str().unwrap();
        assert_eq!(title.chars().count(), MAX_STRING + 1);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn cli_text_is_redacted() {
        let args = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            redact_cli_args(&args(&["type", "Notepad", "secret"])),
            args(&["type", "Notepad", "[6 chars]"])
        );
        assert_eq!(
            redact_cli_args(&args(&["paste", "Notepad", "多語言"])),
            args(&["paste", "Notepad", "[3 chars]"])
        );
        assert_eq!(
            redact_cli_args(&args(&[
                "ui",
                "set-value",
                "App",
                "--name",
                "Pwd",
                "--text",
                "abc"
            ])),
            args(&[
                "ui",
                "set-value",
                "App",
                "--name",
                "Pwd",
                "--text",
                "[3 chars]"
            ])
        );
        assert_eq!(
            redact_cli_args(&args(&["click", "Notepad", "ok.png"])),
            args(&["click", "Notepad", "ok.png"])
        );
        assert_eq!(
            redact_cli_args(&args(&["tpye", "Notepad", "secret"])),
            args(&["tpye", "[2 args]"])
        );
        assert_eq!(
            redact_cli_args(&args(&["ui", "set-value", "App", "--text=abc"])),
            args(&["ui", "set-value", "App", "[10 chars]"])
        );
    }

    #[test]
    fn civil_dates_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for days in [-1, 59, 60, 10_957, 20_000, 20_785, 100_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
    }

    #[test]
    fn stamps_use_the_local_offset() {
        // 2026-11-27T16:30:05.007Z
        let utc = days_from_civil(2026, 11, 27) * 86_400_000 + 59_405_007;
        assert_eq!(
            stamp(utc, 0),
            ("2026-11-27".into(), "2026-11-27T16:30:05.007+00:00".into())
        );
        // UTC+8 crosses midnight into the next local date.
        assert_eq!(
            stamp(utc, 480),
            ("2026-11-28".into(), "2026-11-28T00:30:05.007+08:00".into())
        );
        assert_eq!(stamp(utc, -210).1, "2026-11-27T13:00:05.007-03:30");
    }

    #[test]
    fn old_logs_expire_after_30_days() {
        let today = days_from_civil(2026, 11, 28);
        let names: Vec<String> = [
            "actions-2026-10-28.jsonl", // 31 days: expired
            "actions-2026-10-29.jsonl", // 30 days: kept
            "actions-2026-11-28.jsonl",
            "notes.txt",
            "actions-bad.jsonl",
            "actions-2026-x-10-28.jsonl",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        assert_eq!(expired(&names, today), vec!["actions-2026-10-28.jsonl"]);
    }

    #[test]
    fn lines_start_with_the_time() {
        let line = line_with_time_first("T", 7, &json!({"tool":"x","ok":true})).unwrap();
        let text = String::from_utf8(line).unwrap();
        assert!(text.starts_with(r#"{"ts":"T","pid":7,"#), "{text}");
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["tool"], "x");
        let empty = line_with_time_first("T", 7, &json!({})).unwrap();
        assert_eq!(String::from_utf8(empty).unwrap(), r#"{"ts":"T","pid":7}"#);
    }

    #[test]
    fn outcomes() {
        let ok: autogui::Result<Value> = Ok(json!({"content":[]}));
        assert_eq!(outcome_of(&ok, |_| String::new()), json!({"ok": true}));
        let batch: autogui::Result<Value> = Ok(json!({
            "isError": true,
            "structuredContent": {"code": "stopped", "message": "stopped"}
        }));
        assert_eq!(outcome_of(&batch, |_| String::new())["code"], "stopped");
        let err: autogui::Result<Value> = Err(autogui::Error::WindowNotFound);
        let outcome = outcome_of(&err, |_| "window_not_found".into());
        assert_eq!(outcome["ok"], false);
        assert_eq!(outcome["code"], "window_not_found");
    }
}
