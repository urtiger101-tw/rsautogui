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

pub fn default_locate_options() -> LocateOptions {
    LocateOptions::default()
}
