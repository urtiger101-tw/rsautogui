//! Live clipboard round trip (SPEC §20). Touches the real Windows clipboard.
//!
//! Ignored by default; run with `AUTOGUI_LIVE=1 cargo test -- --ignored`.
#![cfg(windows)]

#[test]
#[ignore]
fn clipboard_text_round_trip() {
    if std::env::var("AUTOGUI_LIVE").as_deref() != Ok("1") {
        return;
    }
    let previous = autogui::clipboard::get_text().expect("read clipboard");
    let text = "autogui live round trip 中文 🙂";
    autogui::clipboard::set_text(text).expect("write clipboard");
    let read_back = autogui::clipboard::get_text().expect("read clipboard back");
    // Put the previous text back before asserting so a failure does not
    // leave the clipboard holding the test string.
    if let Some(previous) = previous {
        autogui::clipboard::set_text(&previous).expect("restore clipboard");
    }
    assert_eq!(read_back.as_deref(), Some(text));
}
