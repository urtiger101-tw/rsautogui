use std::time::Duration;

use crate::error::{Error, Result};
use crate::gui::AutoGui;
use crate::settings::Settings;
use crate::types::{Point, Rect, Size};

#[derive(Clone, Debug)]
pub(crate) struct WindowSettings {
    pub(crate) pause: Duration,
    pub(crate) failsafe: bool,
    pub(crate) failsafe_points: Vec<Point>,
}

impl Default for WindowSettings {
    fn default() -> Self {
        let settings = Settings::default();
        Self {
            pause: settings.pause,
            failsafe: settings.failsafe,
            failsafe_points: settings.failsafe_points,
        }
    }
}

impl From<&Settings> for WindowSettings {
    fn from(settings: &Settings) -> Self {
        Self {
            pause: settings.pause,
            failsafe: settings.failsafe,
            failsafe_points: settings.failsafe_points.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Window {
    pub(crate) hwnd: isize,
    #[cfg(windows)]
    pub(crate) process_id: u32,
    #[cfg(windows)]
    pub(crate) thread_id: u32,
    pub(crate) settings: WindowSettings,
}

impl PartialEq for Window {
    fn eq(&self, other: &Self) -> bool {
        self.hwnd == other.hwnd
    }
}

impl Eq for Window {}

impl Window {
    pub(crate) fn with_settings(&mut self, settings: &Settings) {
        self.settings = WindowSettings::from(settings);
    }

    fn before_mutation(&self) -> Result<()> {
        platform::check_failsafe(&self.settings)
    }

    fn after_mutation(&self) -> Result<()> {
        self.before_mutation()?;
        if !self.settings.pause.is_zero() {
            std::thread::sleep(self.settings.pause);
        }
        Ok(())
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    pub fn title(&self) -> Result<String> {
        platform::title(self)
    }

    pub fn left(&self) -> Result<i32> {
        Ok(self.box_rect()?.left)
    }
    pub fn top(&self) -> Result<i32> {
        Ok(self.box_rect()?.top)
    }
    pub fn right(&self) -> Result<i32> {
        Ok(self.box_rect()?.right())
    }
    pub fn bottom(&self) -> Result<i32> {
        Ok(self.box_rect()?.bottom())
    }
    pub fn width(&self) -> Result<i32> {
        Ok(self.box_rect()?.width)
    }
    pub fn height(&self) -> Result<i32> {
        Ok(self.box_rect()?.height)
    }
    pub fn size(&self) -> Result<Size> {
        let rect = self.box_rect()?;
        Ok(Size::new(rect.width, rect.height))
    }
    pub fn box_rect(&self) -> Result<Rect> {
        platform::rect(self)
    }
    pub fn topleft(&self) -> Result<Point> {
        let rect = self.box_rect()?;
        Ok(Point::new(rect.left, rect.top))
    }
    pub fn center(&self) -> Result<Point> {
        Ok(crate::types::center(self.box_rect()?))
    }
    pub fn area(&self) -> Result<i64> {
        let rect = self.box_rect()?;
        Ok(rect.width as i64 * rect.height as i64)
    }

    pub fn is_minimized(&self) -> Result<bool> {
        platform::is_minimized(self)
    }
    pub fn is_maximized(&self) -> Result<bool> {
        platform::is_maximized(self)
    }
    pub fn is_active(&self) -> Result<bool> {
        platform::is_active(self)
    }

    pub fn activate(&self) -> Result<()> {
        self.before_mutation()?;
        platform::activate(self)?;
        self.after_mutation()
    }
    pub fn maximize(&self) -> Result<()> {
        self.before_mutation()?;
        platform::maximize(self)?;
        self.after_mutation()
    }
    pub fn minimize(&self) -> Result<()> {
        self.before_mutation()?;
        platform::minimize(self)?;
        self.after_mutation()
    }
    pub fn restore(&self) -> Result<()> {
        self.before_mutation()?;
        platform::restore(self)?;
        self.after_mutation()
    }
    pub fn close(&self) -> Result<()> {
        self.before_mutation()?;
        platform::close(self)?;
        self.after_mutation()
    }
    pub fn hide(&self) -> Result<()> {
        self.before_mutation()?;
        platform::hide(self)?;
        self.after_mutation()
    }
    pub fn show(&self) -> Result<()> {
        self.before_mutation()?;
        platform::show(self)?;
        self.after_mutation()
    }

    pub fn move_to(&self, left: i32, top: i32) -> Result<()> {
        self.before_mutation()?;
        platform::move_to(self, left, top)?;
        self.after_mutation()
    }
    pub fn move_rel(&self, dx: i32, dy: i32) -> Result<()> {
        let point = self.topleft()?;
        self.move_to(point.x.saturating_add(dx), point.y.saturating_add(dy))
    }
    pub fn resize_to(&self, width: i32, height: i32) -> Result<()> {
        self.before_mutation()?;
        platform::resize_to(self, width, height)?;
        self.after_mutation()
    }
    pub fn resize_rel(&self, dw: i32, dh: i32) -> Result<()> {
        let size = self.size()?;
        self.resize_to(
            size.width.saturating_add(dw),
            size.height.saturating_add(dh),
        )
    }
}

pub fn get_all_windows() -> Result<Vec<Window>> {
    platform::get_all_windows()
}
pub fn get_all_titles() -> Result<Vec<String>> {
    get_all_windows()?
        .into_iter()
        .filter_map(|window| match window.title() {
            Err(Error::WindowNotFound) => None,
            result => Some(result),
        })
        .collect()
}
pub fn get_active_window() -> Result<Option<Window>> {
    platform::get_active_window()
}
pub fn get_windows_with_title(title: &str) -> Result<Vec<Window>> {
    get_all_windows()?
        .into_iter()
        .filter_map(|window| match window.title() {
            Ok(window_title) if window_title.contains(title) => Some(Ok(window)),
            Ok(_) => None,
            Err(Error::WindowNotFound) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}
pub fn get_windows_with_title_ci(title: &str) -> Result<Vec<Window>> {
    let title = title.to_lowercase();
    get_all_windows()?
        .into_iter()
        .filter_map(|window| match window.title() {
            Ok(window_title) if window_title.to_lowercase().contains(&title) => Some(Ok(window)),
            Ok(_) => None,
            Err(Error::WindowNotFound) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}
pub fn get_windows_at(x: i32, y: i32) -> Result<Vec<Window>> {
    get_all_windows()?
        .into_iter()
        .filter_map(|window| match window.box_rect() {
            Ok(rect)
                if x >= rect.left && y >= rect.top && x <= rect.right() && y <= rect.bottom() =>
            {
                Some(Ok(window))
            }
            Ok(_) => None,
            Err(Error::WindowNotFound) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

impl AutoGui {
    pub fn get_all_windows(&self) -> Result<Vec<Window>> {
        let mut windows = get_all_windows()?;
        for window in &mut windows {
            window.with_settings(&self.settings);
        }
        Ok(windows)
    }
    pub fn get_all_titles(&self) -> Result<Vec<String>> {
        get_all_titles()
    }
    pub fn get_active_window(&self) -> Result<Option<Window>> {
        let mut window = get_active_window()?;
        if let Some(window) = &mut window {
            window.with_settings(&self.settings);
        }
        Ok(window)
    }
    pub fn get_windows_with_title(&self, title: &str) -> Result<Vec<Window>> {
        let mut windows = get_windows_with_title(title)?;
        for window in &mut windows {
            window.with_settings(&self.settings);
        }
        Ok(windows)
    }
    pub fn get_windows_with_title_ci(&self, title: &str) -> Result<Vec<Window>> {
        let mut windows = get_windows_with_title_ci(title)?;
        for window in &mut windows {
            window.with_settings(&self.settings);
        }
        Ok(windows)
    }
    pub fn get_windows_at(&self, x: i32, y: i32) -> Result<Vec<Window>> {
        let mut windows = get_windows_at(x, y)?;
        for window in &mut windows {
            window.with_settings(&self.settings);
        }
        Ok(windows)
    }
}

#[cfg(windows)]
use crate::window_win as platform;

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn title(_: &Window) -> Result<String> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn rect(_: &Window) -> Result<Rect> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn is_minimized(_: &Window) -> Result<bool> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn is_maximized(_: &Window) -> Result<bool> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn is_active(_: &Window) -> Result<bool> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn activate(_: &Window) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn maximize(_: &Window) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn minimize(_: &Window) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn restore(_: &Window) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn close(_: &Window) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn hide(_: &Window) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn show(_: &Window) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn move_to(_: &Window, _: i32, _: i32) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn resize_to(_: &Window, _: i32, _: i32) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn check_failsafe(_: &WindowSettings) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn get_all_windows() -> Result<Vec<Window>> {
        Err(Error::UnsupportedPlatform)
    }
    pub fn get_active_window() -> Result<Option<Window>> {
        Err(Error::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_windows_stub() {
        if cfg!(windows) {
            return;
        }
        assert!(matches!(
            get_all_windows().unwrap_err(),
            Error::UnsupportedPlatform
        ));
    }
}
