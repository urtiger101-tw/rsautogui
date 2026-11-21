//! Agent-facing errors: library errors mapped onto the shared MCP tool-error
//! shape (`autogui_protocol::ToolError`), used by the runtime DLL and the
//! in-process `app_control mcp` server alike.
use autogui::Error;
use autogui_protocol::{ErrorCode, ToolError};
use serde_json::Value;

/// Stable code, human message, recovery hint and retryability for `error`.
pub fn report(error: &Error) -> ToolError {
    let message = error.to_string();
    if message.contains("cancelled") {
        return ToolError::cancelled(message);
    }
    let (code, hint) = match error {
        Error::WindowNotFound => (
            ErrorCode::WindowNotFound,
            "the window closed or changed identity; re-run windows_list (or window_wait) for fresh targets",
        ),
        Error::PermissionDenied(m) if m.contains("SetForegroundWindow") => {
            return ToolError::new(ErrorCode::PermissionDenied, message)
                .with_hint("Windows foreground lock or an elevated (administrator) window refused focus; retry window_control activate, or use display_capture then click_at with display to click the window directly")
                .with_retryable(true);
        }
        Error::PermissionDenied(m) if m.contains("foreground") => {
            return ToolError::new(ErrorCode::PermissionDenied, message)
                .with_hint("another window took focus; re-run capture (which activates the target) before sending input")
                .with_retryable(true);
        }
        Error::PermissionDenied(m) if m.contains("AUTOGUI_ALLOW_LAUNCH") => (
            ErrorCode::PermissionDenied,
            "the user must start the MCP server with AUTOGUI_ALLOW_LAUNCH=1 to allow app_launch",
        ),
        Error::PermissionDenied(_) => (
            ErrorCode::PermissionDenied,
            "the target may be elevated or on a secure desktop; run the agent at the same integrity level",
        ),
        Error::FailSafe(_) => (
            ErrorCode::FailSafe,
            "the cursor is on a primary-monitor corner (fail-safe); move it off the corner, then retry",
        ),
        Error::ImageNotFound => (
            ErrorCode::ImageNotFound,
            "call capture to confirm the template is visible; check scale (DPI/zoom) and confidence, or use a more distinctive template",
        ),
        Error::UnsupportedPlatform => (ErrorCode::Unsupported, "this tool works only on Windows"),
        Error::FeatureDisabled(_) => (
            ErrorCode::Unsupported,
            "install an autogui-control build with this feature enabled",
        ),
        Error::Input(m) if m.starts_with("invalid tool arguments") => (
            ErrorCode::InvalidArgument,
            "check the tool's inputSchema in tools/list; unknown fields are rejected",
        ),
        Error::Input(_) => (
            ErrorCode::InputFailed,
            "the input backend failed; run session_status to check for a locked, disconnected or secure desktop",
        ),
        Error::Screenshot(_) => (
            ErrorCode::CaptureFailed,
            "screen capture failed; run session_status to check for a locked, disconnected or display-less session",
        ),
        Error::Io(e) if e.kind() == std::io::ErrorKind::TimedOut => (
            ErrorCode::Timeout,
            "the window did not reach the state in time; raise timeout or check title/process with windows_list",
        ),
        Error::InvalidArgument(m) => {
            // "...; retry" marks a transient race (window moved, focus changed).
            return ToolError::new(ErrorCode::InvalidArgument, message)
                .with_retryable(m.ends_with("retry") || m.contains("; retry "));
        }
        Error::Io(e) => return ToolError::from_io(e).with_message(message),
    };
    ToolError::new(code, message).with_hint(hint)
}

/// Error text for people and older agents: the message plus a recovery hint.
pub fn describe(error: &Error) -> String {
    report(error).text()
}

/// The complete `isError` tool result for `error`.
pub fn result(error: &Error) -> Value {
    report(error).to_result()
}
