//! Agent-facing errors: library errors mapped onto the shared MCP tool-error
//! shape (`autogui_protocol::ToolError`), used by the runtime DLL and the
//! in-process `app_control mcp` server alike.
use autogui::Error;
use autogui_protocol::{ErrorCode, ToolError, is_cancellation};
use serde_json::Value;

/// Typed marker for tool arguments that failed to decode (unknown field,
/// wrong type). Carried as an `io::ErrorKind::InvalidInput` so the public
/// `autogui::Error` enum stays unchanged while the message stays dynamic.
#[derive(Debug)]
struct InvalidToolArguments(String);

impl std::fmt::Display for InvalidToolArguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid tool arguments: {}", self.0)
    }
}

impl std::error::Error for InvalidToolArguments {}

/// The error for arguments that do not match a tool's `inputSchema`.
pub(crate) fn invalid_arguments(detail: impl std::fmt::Display) -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        InvalidToolArguments(detail.to_string()),
    ))
}

/// The error a cancellation check returns once the client cancelled the
/// request or disconnected. Recognised by type, never by its wording.
pub fn cancelled() -> Error {
    Error::Io(autogui_protocol::cancellation(
        "request cancelled or client disconnected",
    ))
}

fn is_invalid_arguments(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<InvalidToolArguments>())
}

/// Stable code, human message, recovery hint and retryability for `error`.
pub fn report(error: &Error) -> ToolError {
    let message = error.to_string();
    let (code, hint) = match error {
        // The inner text: "io: " would only obscure these messages.
        Error::Io(e) if is_cancellation(e) => return ToolError::cancelled(e.to_string()),
        Error::Io(e) if is_invalid_arguments(e) => {
            return ToolError::new(ErrorCode::InvalidArgument, e.to_string()).with_hint(
                "check the tool's inputSchema in tools/list; unknown fields are rejected",
            );
        }
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
            "the wait did not reach its state in time; raise timeout, or re-observe (windows_list, capture, ui_snapshot) to check the current state",
        ),
        Error::ElementNotFound => (
            ErrorCode::ElementNotFound,
            "no UI element matched; run ui_snapshot again for fresh element refs, or use name_match contains, a role or automation_id",
        ),
        Error::AmbiguousElement(_) => (
            ErrorCode::AmbiguousElement,
            "several UI elements matched; add role or automation_id, or pass an element ref from ui_snapshot",
        ),
        Error::Stopped => (
            ErrorCode::Stopped,
            "the user pressed Ctrl+Alt+F12 to stop; tell the user, and do not resume input until they restart the MCP server",
        ),
        Error::Clipboard(_) => (
            ErrorCode::InputFailed,
            "another program holds the clipboard; retry, or use type_text",
        ),
        Error::Uia(m) if m.starts_with("unconfirmed:") => {
            return ToolError::new(ErrorCode::Internal, message)
                .with_hint("the action may already have taken effect; re-observe with ui_snapshot (or capture) before retrying")
                .with_retryable(false);
        }
        Error::Uia(_) => (
            ErrorCode::Internal,
            "UI Automation failed or the app is not responding; retry once, then fall back to capture and pixel tools",
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
