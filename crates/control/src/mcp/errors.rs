//! Agent-facing error text.
use autogui::Error;

/// Error text for agents: the original message plus a recovery hint.
pub fn describe(error: &Error) -> String {
    let message = error.to_string();
    let hint = match error {
        _ if message.contains("cancelled") => {
            "request was cancelled or the client disconnected; no further input was sent"
        }
        Error::WindowNotFound => {
            "the window closed or changed identity; re-run windows_list (or window_wait) for fresh targets"
        }
        Error::PermissionDenied(m) if m.contains("SetForegroundWindow") => {
            "Windows foreground lock or an elevated (administrator) window refused focus; retry window_control activate, or use display_capture then click_at with display to click the window directly"
        }
        Error::PermissionDenied(m) if m.contains("foreground") => {
            "another window took focus; re-run capture (which activates the target) before sending input"
        }
        Error::PermissionDenied(m) if m.contains("AUTOGUI_ALLOW_LAUNCH") => {
            "the user must start the MCP server with AUTOGUI_ALLOW_LAUNCH=1 to allow app_launch"
        }
        Error::PermissionDenied(_) => {
            "the target may be elevated or on a secure desktop; run the agent at the same integrity level"
        }
        Error::FailSafe(_) => {
            "the cursor is on a primary-monitor corner (fail-safe); move it off the corner, then retry"
        }
        Error::ImageNotFound => {
            "call capture to confirm the template is visible; check scale (DPI/zoom) and confidence, or use a more distinctive template"
        }
        Error::UnsupportedPlatform => "this tool works only on Windows",
        Error::FeatureDisabled(_) => "install an autogui-control build with this feature enabled",
        Error::Input(_) => {
            "the input backend failed; run session_status to check for a locked, disconnected or secure desktop"
        }
        Error::Screenshot(_) => {
            "screen capture failed; run session_status to check for a locked, disconnected or display-less session"
        }
        Error::Io(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            "the window did not reach the state in time; raise timeout or check title/process with windows_list"
        }
        Error::InvalidArgument(_) | Error::Io(_) => "",
    };
    if hint.is_empty() {
        message
    } else {
        format!("{message}. Hint: {hint}")
    }
}
