//! Clipboard text access and the `paste` helper (SPEC §20).
//!
//! Only Windows has a clipboard implementation; other platforms return
//! [`Error::UnsupportedPlatform`] before any input is sent.

use std::thread;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::gui::AutoGui;

/// Longest text accepted by [`AutoGui::paste`], in Unicode scalar values.
const MAX_PASTE_CHARS: usize = 20_000;
/// Time allowed for the target to read the clipboard after Ctrl+V.
const PASTE_SETTLE: Duration = Duration::from_millis(150);

/// Returns the clipboard's `CF_UNICODETEXT` contents, or `None` when the
/// clipboard holds no text.
#[cfg(windows)]
pub fn get_text() -> Result<Option<String>> {
    crate::clipboard_win::get_text()
}

/// Replaces the clipboard contents with `text`.
#[cfg(windows)]
pub fn set_text(text: &str) -> Result<()> {
    crate::clipboard_win::set_text(text)
}

/// Returns the clipboard's `CF_UNICODETEXT` contents, or `None` when the
/// clipboard holds no text.
#[cfg(not(windows))]
pub fn get_text() -> Result<Option<String>> {
    Err(Error::UnsupportedPlatform)
}

/// Replaces the clipboard contents with `text`.
#[cfg(not(windows))]
pub fn set_text(_text: &str) -> Result<()> {
    Err(Error::UnsupportedPlatform)
}

/// Puts `previous` back with `set`. Returns whether it was restored: false
/// when there was no text to restore or the write failed. Never errors, so
/// a failed restore cannot hide a paste that was already delivered.
fn restore_previous(previous: Option<String>, set: impl FnOnce(&str) -> Result<()>) -> bool {
    match previous {
        Some(old) => set(&old).is_ok(),
        None => false,
    }
}

/// Encodes `text` as UTF-16 code units followed by a single NUL terminator.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn encode_utf16_nul(text: &str) -> Vec<u16> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    units
}

/// Decodes UTF-16 code units up to the first NUL (or the end of the slice).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn decode_utf16_until_nul(units: &[u16]) -> String {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

impl AutoGui {
    /// Pastes `text` through the clipboard with Ctrl+V.
    ///
    /// The previous clipboard text is restored only when `restore` is true
    /// and the clipboard held text; a non-text clipboard is left as the pasted
    /// text. Returns whether the previous text was restored.
    pub fn paste(&mut self, text: &str, restore: bool) -> Result<bool> {
        if text.chars().count() > MAX_PASTE_CHARS {
            return Err(Error::InvalidArgument(
                "paste text exceeds 20000 characters",
            ));
        }
        if text.contains('\0') {
            return Err(Error::InvalidArgument("text must not contain NUL"));
        }
        self.before_action()?;
        let previous = if restore { get_text()? } else { None };
        set_text(text)?;
        // Errors up to here mean nothing was pasted. From the chord on, the
        // text may already have reached the target, so no error is returned
        // for the rest of the call: an agent retrying would paste twice.
        let chord = self.hotkey(&["ctrl", "v"]);
        thread::sleep(PASTE_SETTLE);
        let restored = restore_previous(previous, set_text);
        // A fail-safe trip here only reports the cursor position; the next
        // action runs the same check again.
        let _ = self.after_write();
        chord?;
        Ok(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trip_with_emoji_and_cjk() {
        for text in ["", "plain", "中文字 日本語 한국어", "emoji 🙂🚀 mixed 漢字"] {
            let encoded = encode_utf16_nul(text);
            assert_eq!(encoded.last(), Some(&0));
            assert_eq!(decode_utf16_until_nul(&encoded), text);
        }
    }

    #[test]
    fn utf16_encodes_supplementary_plane_as_surrogates() {
        let encoded = encode_utf16_nul("🙂");
        assert_eq!(encoded, [0xD83D, 0xDE42, 0]);
    }

    #[test]
    fn decode_stops_at_first_nul() {
        assert_eq!(decode_utf16_until_nul(&[0x41, 0x42, 0, 0x43]), "AB");
        assert_eq!(decode_utf16_until_nul(&[0x41, 0x42]), "AB");
    }

    #[test]
    fn paste_rejects_text_over_the_cap_before_any_input() {
        let fake = crate::backend::FakeBackend::new(200, 100);
        let events = fake.shared_events();
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        let long = "a".repeat(MAX_PASTE_CHARS + 1);
        assert!(matches!(
            g.paste(&long, true),
            Err(Error::InvalidArgument(_))
        ));
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn paste_rejects_nul_before_any_clipboard_or_input_access() {
        let fake = crate::backend::FakeBackend::new(200, 100);
        let events = fake.shared_events();
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        for text in ["a\0b", "\0", "tail\0"] {
            assert!(matches!(
                g.paste(text, true),
                Err(Error::InvalidArgument("text must not contain NUL"))
            ));
        }
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn restore_helper_reports_failed_or_missing_restore_as_not_restored() {
        let mut calls = 0;
        assert!(!restore_previous(None, |_| {
            calls += 1;
            Ok(())
        }));
        assert_eq!(calls, 0);
        assert!(!restore_previous(Some("old".into()), |_| Err(
            Error::Clipboard("SetClipboardData failed".into())
        )));
        assert!(restore_previous(Some("old".into()), |text| {
            assert_eq!(text, "old");
            Ok(())
        }));
    }

    #[test]
    fn paste_cap_counts_characters_not_bytes() {
        // 20000 three-byte characters are within the cap.
        let at_cap = "中".repeat(MAX_PASTE_CHARS);
        assert_eq!(at_cap.chars().count(), MAX_PASTE_CHARS);
        assert!(at_cap.len() > MAX_PASTE_CHARS);
    }

    #[cfg(not(windows))]
    #[test]
    fn clipboard_is_unsupported_off_windows() {
        assert!(matches!(get_text(), Err(Error::UnsupportedPlatform)));
        assert!(matches!(set_text("x"), Err(Error::UnsupportedPlatform)));
    }
}
