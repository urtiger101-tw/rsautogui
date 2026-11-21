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
pub fn definitions() -> &'static Value {
    static DEFINITIONS: OnceLock<Value> = OnceLock::new();
    // The catalogue is compiled in and covered by tests; an unparsable file
    // would yield an empty list rather than abort the server.
    DEFINITIONS.get_or_init(|| {
        serde_json::from_str(include_str!("tools.json")).unwrap_or(Value::Array(Vec::new()))
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
        for tool in tools {
            assert!(tool["description"].is_string(), "{tool}");
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
        }
    }
}
