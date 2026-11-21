//! The 19 MCP tool names and their embedded JSON schema catalogue.
use serde_json::Value;
use std::sync::OnceLock;

/// Tool names accepted by `tools/call`, in catalogue order.
pub const NAMES: &[&str] = &[
    "session_status",
    "displays_list",
    "display_capture",
    "window_to_display",
    "windows_list",
    "window_control",
    "capture",
    "locate",
    "click_image",
    "click_at",
    "pointer_move",
    "drag",
    "scroll",
    "cursor_position",
    "window_wait",
    "app_launch",
    "type_text",
    "hotkey",
    "screensaver",
];

/// The `tools/list` array, parsed once from the embedded `tools.json`.
///
/// Every tool carries a human `title`, an `inputSchema` and a permissive
/// `outputSchema` (no `required` fields, `additionalProperties: true`), so the
/// error object `{code, message, hint, retryable}` also validates against it.
pub fn definitions() -> &'static Value {
    static DEFINITIONS: OnceLock<Value> = OnceLock::new();
    // The catalogue is compiled in and covered by tests; an unparsable file
    // would yield an empty list rather than abort the server.
    DEFINITIONS.get_or_init(|| {
        serde_json::from_str(include_str!("tools.json")).unwrap_or(Value::Array(Vec::new()))
    })
}

/// The serialized `tools/list` result object (`{"tools":[...]}`), built once.
pub fn list_result_json() -> &'static str {
    static LIST: OnceLock<String> = OnceLock::new();
    LIST.get_or_init(|| {
        serde_json::to_string(&serde_json::json!({ "tools": definitions() }))
            .unwrap_or_else(|_| String::from(r#"{"tools":[]}"#))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_matches_names_in_order() {
        let tools = definitions().as_array().expect("tools array");
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(names, NAMES);
        assert_eq!(names.len(), 19);
        for tool in tools {
            assert!(tool["description"].is_string(), "{tool}");
            assert!(
                tool["title"].as_str().is_some_and(|t| !t.is_empty()),
                "{tool}"
            );
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
            let output = &tool["outputSchema"];
            assert_eq!(output["type"], "object", "{tool}");
            assert_eq!(output["additionalProperties"], true, "{tool}");
            assert!(output["properties"].is_object(), "{tool}");
            // Error results reuse structuredContent, so nothing may be required.
            assert!(output.get("required").is_none(), "{tool}");
            assert!(tool["annotations"]["readOnlyHint"].is_boolean(), "{tool}");
        }
    }

    #[test]
    fn cached_list_result_round_trips() {
        let parsed: Value = serde_json::from_str(list_result_json()).expect("json");
        assert_eq!(&parsed["tools"], definitions());
        assert!(std::ptr::eq(list_result_json(), list_result_json()));
    }
}
