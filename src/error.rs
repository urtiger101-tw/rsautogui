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
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
