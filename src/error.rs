use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("fail-safe triggered at corner {0:?}")]
    FailSafe(crate::types::Point),
    #[error("image not found")]
    ImageNotFound,
    #[error("unsupported platform")]
    UnsupportedPlatform,
    #[error("feature disabled: {0}")]
    FeatureDisabled(&'static str),
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
    #[error("permission denied: {0}")]
    PermissionDenied(&'static str),
    #[error("window not found")]
    WindowNotFound,
    #[error("input backend: {0}")]
    Input(String),
    #[error("screenshot: {0}")]
    Screenshot(String),
    /// No UI Automation element matched the selector (SPEC §18).
    #[error("ui element not found")]
    ElementNotFound,
    /// The selector matched this many UI Automation elements (SPEC §18).
    #[error("{0} ui elements match; narrow the selector")]
    AmbiguousElement(usize),
    /// The emergency stop chord Ctrl+Alt+F12 was pressed (SPEC §19).
    #[error("stopped by the emergency stop chord (Ctrl+Alt+F12)")]
    Stopped,
    /// The clipboard could not be opened, read or written (SPEC §20).
    #[error("clipboard: {0}")]
    Clipboard(String),
    /// A UI Automation (COM) call failed (SPEC §18).
    #[error("ui automation: {0}")]
    Uia(String),
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
