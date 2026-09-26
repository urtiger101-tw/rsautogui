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
mod keys;
mod locate;
mod settings;
mod tween;
mod types;
mod window;

pub use backend::FakeBackend;
pub use display::Display;
pub use error::{Error, Result};
pub use gui::{AutoGui, KeyHold};
pub use locate::LocateOptions;
pub use settings::Settings;
pub use tween::Tween;
pub use types::Box;
pub use types::{MouseButton, Point, Rect, Rgb, Screenshot, ScrollDirection, Size, center};
pub use window::Window;

pub mod tweens {
    pub use crate::tween::*;
}

pub fn default_locate_options() -> LocateOptions {
    LocateOptions::default()
}
