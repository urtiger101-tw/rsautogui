//! The one tool-error shape shared by every MCP path: the launcher server
//! (runtime load/ABI failures, cancellation) and the runtime DLL (library
//! errors mapped in `autogui-control-core`).
//!
//! A tool execution error is a normal `tools/call` result with
//! `isError: true`. Its text block is for people (message plus a recovery
//! hint); `structuredContent` carries the same facts for programs.
use serde_json::{Value, json};

/// Stable, snake_case error codes reported in `structuredContent.code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Arguments failed validation, or a reference/proof is stale.
    InvalidArgument,
    /// The target window closed or changed identity.
    WindowNotFound,
    /// The template image was not found on screen before the timeout.
    ImageNotFound,
    /// A file or other named resource does not exist.
    NotFound,
    /// The cursor sits on a primary-monitor corner.
    FailSafe,
    /// Windows refused the operation (focus lock, integrity level, policy).
    PermissionDenied,
    /// A wait did not reach its state in time.
    Timeout,
    /// The client cancelled the request or disconnected.
    Cancelled,
    /// Not available on this platform or in this build.
    Unsupported,
    /// The input backend failed to send mouse or keyboard input.
    InputFailed,
    /// Screen capture failed.
    CaptureFailed,
    /// The runtime DLL could not be loaded or has the wrong ABI.
    RuntimeUnavailable,
    /// Anything else.
    Internal,
}

impl ErrorCode {
    /// Every code, for documentation and tests.
    pub const ALL: [Self; 13] = [
        Self::InvalidArgument,
        Self::WindowNotFound,
        Self::ImageNotFound,
        Self::NotFound,
        Self::FailSafe,
        Self::PermissionDenied,
        Self::Timeout,
        Self::Cancelled,
        Self::Unsupported,
        Self::InputFailed,
        Self::CaptureFailed,
        Self::RuntimeUnavailable,
        Self::Internal,
    ];

    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "invalid_argument",
            Self::WindowNotFound => "window_not_found",
            Self::ImageNotFound => "image_not_found",
            Self::NotFound => "not_found",
            Self::FailSafe => "fail_safe",
            Self::PermissionDenied => "permission_denied",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Unsupported => "unsupported",
            Self::InputFailed => "input_failed",
            Self::CaptureFailed => "capture_failed",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::Internal => "internal",
        }
    }

    /// Whether repeating the same request later may succeed without changing
    /// its arguments (a transient condition rather than a bad request).
    fn retryable(self) -> bool {
        matches!(
            self,
            Self::ImageNotFound
                | Self::FailSafe
                | Self::Timeout
                | Self::Cancelled
                | Self::InputFailed
                | Self::CaptureFailed
        )
    }
}

/// A tool execution error: what went wrong, how to recover, and whether a
/// plain retry may help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
    /// Recovery advice; empty when there is none.
    pub hint: String,
    pub retryable: bool,
}

/// Typed marker carried inside an [`std::io::Error`] for a request the
/// client cancelled, or a state-changing request refused because the client
/// disconnected. Recognised by type ([`is_cancellation`]), never by message
/// text, so an unrelated error that mentions "cancelled" keeps its own code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancellation(String);

impl std::fmt::Display for Cancellation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Cancellation {}

/// An I/O error carrying the [`Cancellation`] marker.
pub fn cancellation(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        Cancellation(message.into()),
    )
}

/// Whether `error` was made by [`cancellation`].
pub fn is_cancellation(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<Cancellation>())
}

const CANCELLED_HINT: &str =
    "request was cancelled or the client disconnected; no further input was sent";

impl ToolError {
    /// A new error with the code's default retryability and no hint.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: String::new(),
            retryable: code.retryable(),
        }
    }

    /// Sets the recovery hint.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// Replaces the message (keeps code, hint and retryability).
    #[must_use]
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Overrides the code's default retryability.
    #[must_use]
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Human text: the message, then `. Hint: ...` when there is a hint.
    pub fn text(&self) -> String {
        if self.hint.is_empty() {
            self.message.clone()
        } else {
            format!("{}. Hint: {}", self.message, self.hint)
        }
    }

    /// The machine-readable object placed in `structuredContent`.
    /// `hint` is null when there is none.
    pub fn structured(&self) -> Value {
        json!({
            "code": self.code.as_str(),
            "message": self.message,
            "hint": if self.hint.is_empty() { Value::Null } else { json!(self.hint) },
            "retryable": self.retryable,
        })
    }

    /// The complete `tools/call` result (`isError: true`).
    pub fn to_result(&self) -> Value {
        json!({
            "content": [{"type": "text", "text": self.text()}],
            "structuredContent": self.structured(),
            "isError": true,
        })
    }

    /// The error for a cancelled or disconnected request.
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Cancelled, message).with_hint(CANCELLED_HINT)
    }

    /// Classifies an I/O error raised outside the runtime (launcher DLL
    /// loading, ABI boundary, cancellation, or a handler's own I/O).
    pub fn from_io(error: &std::io::Error) -> Self {
        use std::io::ErrorKind;
        let message = error.to_string();
        if is_cancellation(error) {
            return Self::cancelled(message);
        }
        if message.contains("autogui_runtime.dll")
            || (message.starts_with("Runtime ")
                && (message.contains("export missing") || message.contains("ABI version")))
        {
            return Self::new(ErrorCode::RuntimeUnavailable, message).with_hint(
                "reinstall autogui-control so autogui-control.exe and autogui_runtime.dll come from the same build and sit in the same folder",
            );
        }
        if message.starts_with("Runtime ") {
            return Self::new(ErrorCode::Internal, message).with_hint(
                "the runtime failed internally (see the server's stderr); retry once, then restart the MCP server",
            );
        }
        let code = match error.kind() {
            ErrorKind::TimedOut => ErrorCode::Timeout,
            ErrorKind::NotFound => ErrorCode::NotFound,
            ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            ErrorKind::InvalidInput | ErrorKind::InvalidData => ErrorCode::InvalidArgument,
            _ => ErrorCode::Internal,
        };
        Self::new(code, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    #[test]
    fn codes_are_unique_snake_case() {
        let names: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        for name in &names {
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            assert_eq!(names.iter().filter(|n| *n == name).count(), 1, "{name}");
        }
    }

    #[test]
    fn result_has_text_and_structured_error() {
        let e = ToolError::new(ErrorCode::WindowNotFound, "window not found")
            .with_hint("re-run windows_list");
        let r = e.to_result();
        assert_eq!(r["isError"], true);
        assert_eq!(
            r["content"][0]["text"],
            "window not found. Hint: re-run windows_list"
        );
        assert_eq!(
            r["structuredContent"],
            json!({"code":"window_not_found","message":"window not found","hint":"re-run windows_list","retryable":false})
        );
        let plain = ToolError::new(ErrorCode::Internal, "disk").structured();
        assert_eq!(plain["hint"], Value::Null);
        assert_eq!(ToolError::new(ErrorCode::Internal, "disk").text(), "disk");
    }

    #[test]
    fn classifies_launcher_errors() {
        let cases = [
            (
                Error::other(
                    "Cannot load adjacent autogui_runtime.dll: x. Keep EXE and DLL together.",
                ),
                ErrorCode::RuntimeUnavailable,
            ),
            (
                Error::other("Runtime ABI version mismatch"),
                ErrorCode::RuntimeUnavailable,
            ),
            (
                Error::other("Runtime call export missing"),
                ErrorCode::RuntimeUnavailable,
            ),
            (
                Error::other("Runtime call failed at ABI boundary"),
                ErrorCode::Internal,
            ),
            (
                cancellation("request cancelled or client disconnected"),
                ErrorCode::Cancelled,
            ),
            // Only the typed marker means cancellation, not the wording.
            (
                Error::other("request cancelled or client disconnected"),
                ErrorCode::Internal,
            ),
            (Error::new(ErrorKind::TimedOut, "late"), ErrorCode::Timeout),
            (Error::new(ErrorKind::NotFound, "gone"), ErrorCode::NotFound),
            (Error::other("disk"), ErrorCode::Internal),
        ];
        for (error, code) in cases {
            let e = ToolError::from_io(&error);
            assert_eq!(e.code, code, "{error}");
            assert_eq!(e.message, error.to_string());
        }
        let missing = ToolError::from_io(&Error::other("Cannot load adjacent autogui_runtime.dll"));
        assert!(missing.text().contains("Hint: "));
        assert!(!missing.retryable);
        assert!(ToolError::from_io(&cancellation("request cancelled")).retryable);
        assert!(is_cancellation(&cancellation("x")));
        assert!(!is_cancellation(&Error::new(ErrorKind::Interrupted, "x")));
    }
}
