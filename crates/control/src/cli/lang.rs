//! Output language of the runtime (set by the launcher through AUTOGUI_LANG).

/// True when messages should be Traditional Chinese.
pub(crate) fn zh() -> bool {
    std::env::var("AUTOGUI_LANG").is_ok_and(|v| v.to_ascii_lowercase().starts_with("zh"))
}

/// `tr!(en, zh, args...)`: formats the English or Chinese template.
macro_rules! tr {
    ($en:literal, $zh:literal $(, $arg:expr)* $(,)?) => {
        if $crate::cli::lang::zh() {
            format!($zh $(, $arg)*)
        } else {
            format!($en $(, $arg)*)
        }
    };
}
pub(crate) use tr;
