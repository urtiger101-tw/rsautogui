//! Clipboard paste text entry (SPEC §20).
//!
//! Owned by 0.3.0 work package WP-C. Dispatched from `Session::call`.
use super::errors::invalid_arguments;
use super::session::Session;
use crate::cli::active;
use autogui::{Error, Result};
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
    let restore = a.restore_clipboard.unwrap_or(true);
    let window = session.target(&a.target)?;
    check()?;
    window.activate()?;
    check()?;
    active(&window)?;
    let restored = session.gui()?.paste(&a.text, restore)?;
    let note = if restored {
        "input sent; the previous clipboard text was restored".to_string()
    } else if !restore {
        "input sent; clipboard restore was disabled, so the pasted text remains on the clipboard"
            .to_string()
    } else {
        "input sent; the previous clipboard did not contain text, so it was not restored and the pasted text remains on the clipboard".to_string()
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
