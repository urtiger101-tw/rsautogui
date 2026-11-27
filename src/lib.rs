//! PyAutoGUI-compatible desktop automation.
//!
//! Window control is implemented on Windows only. Other platforms return
//! [`Error::UnsupportedPlatform`] for window APIs.

mod backend;
mod display;
#[cfg(any(windows, feature = "os"))]
mod dpi;
mod error;
mod failsafe;
mod gui;
mod keyboard;
mod keys;
mod locate;
mod mouse;
mod screen;
mod settings;
mod tween;
mod types;
mod window;
#[cfg(windows)]
mod window_win;

pub use backend::FakeBackend;
pub use display::Display;
pub use error::{Error, Result};
pub use gui::AutoGui;
pub use keyboard::KeyHold;
pub use locate::LocateOptions;
pub use settings::Settings;
pub use tween::Tween;
pub use types::Box;
pub use types::{MouseButton, Point, Rect, Rgb, Screenshot, ScrollDirection, Size, center};
pub use window::Window;

/// Common imports (SPEC section 15). `Rect` is the primary name; `Box` is the PyAutoGUI alias.
pub mod prelude {
    pub use crate::{
        AutoGui, Box, Error, LocateOptions, MouseButton, Point, Rect, Result, Rgb, Settings, Size,
        Tween, Window, center,
    };
}

pub mod tweens {
    pub use crate::tween::*;
}

// --- 0.3.0 WP-A: UI Automation (src/uia.rs, src/uia_win.rs; SPEC §18) ---
mod uia;
#[cfg(all(windows, feature = "uia"))]
mod uia_win;
pub use uia::{
    NameMatch, UiAction, UiAutomation, UiNode, UiSelector, UiSnapshot, UiSnapshotOptions,
};

// --- 0.3.0 WP-B: frame diff and emergency stop (src/diff.rs, src/stop.rs; SPEC §7.3, §19) ---

mod diff;
pub mod stop;
pub use diff::FrameDiff;

// --- 0.3.0 WP-C: clipboard (src/clipboard.rs, src/clipboard_win.rs; SPEC §20) ---
pub mod clipboard;
#[cfg(windows)]
mod clipboard_win;

// --- end 0.3.0 modules ---

pub fn default_locate_options() -> LocateOptions {
    LocateOptions::default()
}
