//! Exit codes and error rendering for the CLI.
use autogui::Error;
use serde_json::json;
use std::io::ErrorKind;

pub(crate) const FAILED: i32 = 1;
pub(crate) const USAGE: i32 = 2;
pub(crate) const NOT_FOUND: i32 = 3;
pub(crate) const TIMEOUT: i32 = 4;
pub(crate) const BLOCKED: i32 = 5;
pub(crate) const FAIL_SAFE: i32 = 6;
pub const INTERNAL: i32 = 70;

/// Maps a library error to the documented process exit code.
pub(crate) fn exit_code(error: &Error) -> i32 {
    match error {
        Error::FailSafe(_) => FAIL_SAFE,
        Error::ImageNotFound | Error::WindowNotFound | Error::ElementNotFound => NOT_FOUND,
        Error::AmbiguousElement(_) => FAILED,
        Error::Stopped => BLOCKED,
        Error::InvalidArgument(m) if is_race(m) => FAILED,
        Error::InvalidArgument(_) => USAGE,
        Error::UnsupportedPlatform | Error::FeatureDisabled(_) | Error::PermissionDenied(_) => {
            BLOCKED
        }
        Error::Io(io) if io.kind() == ErrorKind::TimedOut => TIMEOUT,
        Error::Io(_)
        | Error::Input(_)
        | Error::Screenshot(_)
        | Error::Clipboard(_)
        | Error::Uia(_) => FAILED,
    }
}

fn kind(error: &Error) -> &'static str {
    match error {
        Error::FailSafe(_) => "fail_safe",
        Error::ImageNotFound => "image_not_found",
        Error::WindowNotFound => "window_not_found",
        Error::ElementNotFound => "element_not_found",
        Error::AmbiguousElement(_) => "failed",
        Error::Stopped => "stopped",
        Error::InvalidArgument(m) if is_race(m) => "failed",
        Error::InvalidArgument(_) => "usage",
        Error::UnsupportedPlatform | Error::FeatureDisabled(_) | Error::PermissionDenied(_) => {
            "blocked"
        }
        Error::Io(io) if io.kind() == ErrorKind::TimedOut => "timeout",
        Error::Io(_)
        | Error::Input(_)
        | Error::Screenshot(_)
        | Error::Clipboard(_)
        | Error::Uia(_) => "failed",
    }
}

use super::lang::zh;

/// Runtime races reported as InvalidArgument by the library layer: the
/// arguments were fine, the desktop was ambiguous or changed. Exit 1.
pub(crate) const AMBIGUOUS_WINDOW: &str = "multiple windows match; supply a more specific title";
pub(crate) const AMBIGUOUS_IMAGE: &str = "multiple image matches; use a more distinctive template";
pub(crate) const WINDOW_MOVED: &str = "target window moved; retry recognition";
pub(crate) const CURSOR_MOVED: &str = "target or cursor moved before click; retry recognition";

fn is_race(message: &str) -> bool {
    [
        AMBIGUOUS_WINDOW,
        AMBIGUOUS_IMAGE,
        WINDOW_MOVED,
        CURSOR_MOVED,
    ]
    .contains(&message)
}

/// A next-step suggestion for common failures.
pub(crate) fn hint(error: &Error, zh: bool) -> Option<&'static str> {
    let pick = |en, zhs| Some(if zh { zhs } else { en });
    match error {
        Error::WindowNotFound => pick(
            "Run 'autogui-control list' to see window titles.",
            "請執行 autogui-control list 查看視窗標題。",
        ),
        Error::ImageNotFound => pick(
            "Check the template, confidence and that the window is visible; use 'capture' to compare.",
            "請確認範本圖片、相似度，以及視窗是否可見；可用 capture 比對。",
        ),
        Error::FailSafe(_) => pick(
            "The cursor reached a primary-monitor corner; input was aborted on purpose.",
            "游標到了主螢幕角落，已刻意中止輸入。",
        ),
        Error::PermissionDenied(_) | Error::UnsupportedPlatform | Error::FeatureDisabled(_) => {
            pick(
                "Run 'autogui-control doctor' to check the environment.",
                "請執行 autogui-control doctor 檢查環境。",
            )
        }
        Error::InvalidArgument(m) if *m == AMBIGUOUS_WINDOW => pick(
            "Use a more specific title or --process; 'autogui-control list' shows the candidates.",
            "請使用更明確的標題或 --process；autogui-control list 可列出候選視窗。",
        ),
        Error::InvalidArgument(m) if *m == AMBIGUOUS_IMAGE => pick(
            "Crop a more distinctive template or raise the confidence.",
            "請裁切更有辨識度的範本，或提高相似度。",
        ),
        Error::InvalidArgument(m) if is_race(m) => pick(
            "The window changed while running; keep it still and retry.",
            "視窗在執行時發生變化；請保持視窗不動後重試。",
        ),
        Error::InvalidArgument(_) => pick(
            "Run 'autogui-control --help' (or '<command> --help').",
            "請執行 autogui-control --help（或 <命令> --help）。",
        ),
        _ => None,
    }
}

/// Writes the error to stderr (JSON when requested) and returns its exit code.
pub(crate) fn report(error: &Error, json_output: bool) -> i32 {
    let code = exit_code(error);
    let zh = zh();
    let hint = hint(error, zh);
    if json_output {
        eprintln!(
            "{}",
            json!({"error":{"kind":kind(error),"exit_code":code,"message":error.to_string(),"hint":hint}})
        );
    } else {
        eprintln!("{}{error}", if zh { "操作失敗：" } else { "Error: " });
        if let Some(hint) = hint {
            eprintln!("{hint}");
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use autogui::Point;
    use std::io;

    #[test]
    fn exit_code_table() {
        let cases: Vec<(Error, i32)> = vec![
            (Error::FailSafe(Point { x: 0, y: 0 }), 6),
            (Error::ImageNotFound, 3),
            (Error::WindowNotFound, 3),
            (Error::InvalidArgument("x"), 2),
            (Error::InvalidArgument(AMBIGUOUS_WINDOW), 1),
            (Error::InvalidArgument(AMBIGUOUS_IMAGE), 1),
            (Error::InvalidArgument(WINDOW_MOVED), 1),
            (Error::InvalidArgument(CURSOR_MOVED), 1),
            (Error::UnsupportedPlatform, 5),
            (Error::FeatureDisabled("os"), 5),
            (Error::PermissionDenied("x"), 5),
            (Error::Io(io::Error::new(ErrorKind::TimedOut, "t")), 4),
            (Error::Io(io::Error::other("x")), 1),
            (Error::Input("x".into()), 1),
            (Error::Screenshot("x".into()), 1),
        ];
        for (error, code) in cases {
            assert_eq!(exit_code(&error), code, "{error}");
        }
        assert_eq!(INTERNAL, 70);
    }

    #[test]
    fn hints_are_bilingual() {
        assert!(hint(&Error::WindowNotFound, false).is_some_and(|h| h.contains("list")));
        assert!(hint(&Error::WindowNotFound, true).is_some_and(|h| h.contains("視窗")));
        assert!(hint(&Error::Input("x".into()), false).is_none());
        assert_eq!(kind(&Error::ImageNotFound), "image_not_found");
        let race = Error::InvalidArgument(AMBIGUOUS_WINDOW);
        assert!(hint(&race, false).is_some_and(|h| h.contains("--process")));
        assert!(hint(&race, false).is_some_and(|h| !h.contains("--help")));
    }
}
