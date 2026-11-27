//! Clipboard paste text entry (SPEC §20).
//!
//! Owned by 0.3.0 work package WP-C. Dispatched from `Session::call`.
use super::errors::invalid_arguments;
use super::session::Session;
use crate::cli::active;
use autogui::{Error, Result, clipboard};
use serde::Deserialize;
use serde_json::{Value, json};

/// Longest text accepted by `paste_text`, in Unicode scalar values.
const MAX_CHARS: usize = 20_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasteText {
    target: String,
    text: String,
    restore_clipboard: Option<bool>,
}

/// Explains why the previous clipboard was not put back. Called only when
/// `restored` is false.
fn not_restored_note(restore: bool, had_text: bool) -> &'static str {
    if !restore {
        "input sent; clipboard restore was disabled, so the pasted text remains on the clipboard"
    } else if !had_text {
        "input sent; the previous clipboard held no text (empty or non-text), so nothing was restored and the pasted text remains on the clipboard"
    } else {
        "input sent; restoring the previous clipboard text failed, so the pasted text remains on the clipboard"
    }
}

/// Runs the MCP tool `name` (paste_text).
pub(super) fn call(
    session: &mut Session,
    name: &str,
    args: Value,
    check: &dyn Fn() -> Result<()>,
) -> Result<Value> {
    if name != "paste_text" {
        return Err(Error::FeatureDisabled("paste_text"));
    }
    let a: PasteText = serde_json::from_value(args).map_err(invalid_arguments)?;
    let count = a.text.chars().count();
    if count > MAX_CHARS {
        return Err(Error::InvalidArgument(
            "text exceeds 20000 characters; paste smaller chunks",
        ));
    }
    if a.text.contains('\0') {
        return Err(Error::InvalidArgument("text must not contain NUL"));
    }
    let restore = a.restore_clipboard.unwrap_or(true);
    let window = session.target(&a.target)?;
    check()?;
    window.activate()?;
    check()?;
    active(&window)?;
    // Read before pasting so the note can tell "nothing to restore" apart
    // from "restore failed"; a read failure stops before any input is sent.
    let had_text = restore && clipboard::get_text()?.is_some();
    let restored = session.gui()?.paste(&a.text, restore)?;
    let note = if restored {
        "input sent; capture/inspect the app to confirm its result".to_string()
    } else {
        format!(
            "input sent; capture/inspect the app to confirm its result. {}",
            not_restored_note(restore, had_text)
        )
    };
    let value = json!({
        "characters_sent": count,
        "clipboard_restored": restored,
        "note": note,
    });
    Ok(json!({
        "content": [{"type": "text", "text": value.to_string()}],
        "structuredContent": value,
        "isError": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_text_rejects_nul_before_touching_session_or_clipboard() {
        let mut session = Session::default();
        let args = json!({"target":"w1-0","text":"a\u{0}b"});
        assert!(matches!(
            call(&mut session, "paste_text", args, &|| Ok(())),
            Err(Error::InvalidArgument("text must not contain NUL"))
        ));
    }

    #[test]
    fn not_restored_note_distinguishes_the_reasons() {
        assert!(not_restored_note(false, true).contains("restore was disabled"));
        assert!(not_restored_note(true, false).contains("held no text"));
        assert!(
            not_restored_note(true, true).contains("restoring the previous clipboard text failed")
        );
    }
}
