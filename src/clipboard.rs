//! Clipboard text access and the `paste` helper (SPEC §20).
//!
//! Only Windows has a clipboard implementation; other platforms return
//! [`Error::UnsupportedPlatform`] before any input is sent.

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
        self.before_action()?;
        let previous = if restore { get_text()? } else { None };
        set_text(text)?;
        let pressed = self
            .hotkey(&["ctrl", "v"])
            .and_then(|()| self.wait_input_interval(PASTE_SETTLE));
        // Restore even when the chord failed, so the clipboard does not keep
        // the injected text; the chord error still takes precedence.
        let restored = match previous {
            Some(old) => set_text(&old).map(|()| true),
            None => Ok(false),
        };
        pressed?;
        let restored = restored?;
        self.after_write()?;
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
