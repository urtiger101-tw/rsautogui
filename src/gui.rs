use std::thread;
use std::time::{Duration, Instant};

use crate::backend::{Backend, FakeBackend, default_backend};
use crate::error::{Error, Result};
use crate::failsafe;
use crate::locate::{self, LocateOptions};
use crate::settings::Settings;
use crate::tween::{self, Tween};
use crate::types::{MouseButton, Point, Rect, Rgb, Screenshot, Size};
use crate::window::{self, Window};

pub struct AutoGui {
    backend: Box<dyn Backend>,
    settings: Settings,
    raise_image_not_found: bool,
}

pub struct KeyHold<'a> {
    gui: &'a mut AutoGui,
    keys: Vec<String>,
}

impl std::ops::Deref for KeyHold<'_> {
    type Target = AutoGui;
    fn deref(&self) -> &Self::Target {
        self.gui
    }
}

impl std::ops::DerefMut for KeyHold<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.gui
    }
}

impl Drop for KeyHold<'_> {
    fn drop(&mut self) {
        for k in self.keys.iter().rev() {
            let _ = self.gui.backend.key(k, false);
        }
        let _ = self.gui.after_write();
    }
}

impl AutoGui {
    pub fn new() -> Result<Self> {
        let backend = default_backend()?;
        let size = backend.screen_size()?;
        Ok(Self {
            backend,
            settings: Settings::for_screen(size.width, size.height),
            raise_image_not_found: true,
        })
    }

    pub fn with_settings(settings: Settings) -> Result<Self> {
        let mut g = Self::new()?;
        let default_points = Settings::default().failsafe_points;
        let mut settings = settings;
        if settings.failsafe_points == default_points {
            let size = g.size()?;
            settings.failsafe_points =
                Settings::for_screen(size.width, size.height).failsafe_points;
        }
        g.settings = settings;
        Ok(g)
    }

    pub fn with_fake(fake: FakeBackend) -> Self {
        let size = fake.size;
        Self {
            backend: Box::new(fake),
            settings: Settings::for_screen(size.width, size.height),
            raise_image_not_found: true,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Controls whether `locate_all` methods return `ImageNotFound` for no hits.
    /// Singular `locate` methods return `ImageNotFound` on misses; use the `try_`
    /// variants when a missing image should return `Ok(None)`.
    pub fn set_raise_image_not_found(&mut self, yes: bool) {
        self.raise_image_not_found = yes;
    }

    fn before_action(&self) -> Result<()> {
        let pos = self.backend.mouse_pos()?;
        failsafe::check(&self.settings, pos)
    }

    fn wait_input_interval(&self, interval: Duration) -> Result<()> {
        let start = Instant::now();
        while start.elapsed() < interval {
            self.before_action()?;
            thread::sleep(Duration::from_millis(16).min(interval.saturating_sub(start.elapsed())));
        }
        Ok(())
    }

    fn after_write(&mut self) -> Result<()> {
        let pos = self.backend.mouse_pos()?;
        failsafe::check(&self.settings, pos)?;
        if !self.settings.pause.is_zero() {
            thread::sleep(self.settings.pause);
        }
        if cfg!(target_os = "macos") && !self.settings.darwin_catch_up.is_zero() {
            thread::sleep(self.settings.darwin_catch_up);
        }
        Ok(())
    }

    fn before_read_heavy(&self) -> Result<()> {
        let pos = self.backend.mouse_pos()?;
        failsafe::check(&self.settings, pos)
    }

    fn after_read_heavy(&self) -> Result<()> {
        let pos = self.backend.mouse_pos()?;
        failsafe::check(&self.settings, pos)?;
        if !self.settings.pause.is_zero() {
            thread::sleep(self.settings.pause);
        }
        if cfg!(target_os = "macos") && !self.settings.darwin_catch_up.is_zero() {
            thread::sleep(self.settings.darwin_catch_up);
        }
        Ok(())
    }

    pub fn size(&self) -> Result<Size> {
        self.backend.screen_size()
    }

    pub fn position(&self) -> Result<Point> {
        self.backend.mouse_pos()
    }

    pub fn on_screen(&self, x: i32, y: i32) -> Result<bool> {
        let s = self.size()?;
        Ok(x >= 0 && y >= 0 && x < s.width && y < s.height)
    }

    fn animate_move(&mut self, dest: Point, duration: Duration, tween: Tween) -> Result<()> {
        let start = self.backend.mouse_pos()?;
        if duration <= self.settings.minimum_duration {
            self.backend.move_mouse(dest)?;
            return Ok(());
        }
        let start_t = Instant::now();
        loop {
            self.before_action()?;
            let elapsed = start_t.elapsed();
            let t = (elapsed.as_secs_f64() / duration.as_secs_f64()).min(1.0);
            let e = tween(t);
            let x = start.x as f64 + (dest.x as i64 - start.x as i64) as f64 * e;
            let y = start.y as f64 + (dest.y as i64 - start.y as i64) as f64 * e;
            self.backend
                .move_mouse(Point::new(x.round() as i32, y.round() as i32))?;
            if t >= 1.0 {
                break;
            }
            thread::sleep(Duration::from_millis(16));
        }
        self.backend.move_mouse(dest)?;
        Ok(())
    }

    pub fn move_to(&mut self, x: i32, y: i32, duration: Duration, tween: Tween) -> Result<()> {
        self.before_action()?;
        self.animate_move(Point::new(x, y), duration, tween)?;
        self.after_write()
    }

    pub fn move_to_xy(&mut self, x: i32, y: i32) -> Result<()> {
        self.move_to(x, y, Duration::ZERO, tween::linear)
    }

    pub fn move_rel(&mut self, dx: i32, dy: i32, duration: Duration, tween: Tween) -> Result<()> {
        let p = self.backend.mouse_pos()?;
        self.move_to(
            p.x.saturating_add(dx),
            p.y.saturating_add(dy),
            duration,
            tween,
        )
    }

    pub fn drag_to(
        &mut self,
        x: i32,
        y: i32,
        duration: Duration,
        tween: Tween,
        button: MouseButton,
    ) -> Result<()> {
        let mut dur = duration;
        if cfg!(target_os = "macos") && dur.is_zero() {
            dur = Duration::from_millis(100);
        }
        self.before_action()?;
        self.backend.button(button, true)?;
        let movement = self.animate_move(Point::new(x, y), dur, tween);
        let release = self.backend.button(button, false);
        movement?;
        release?;
        self.after_write()
    }

    pub fn drag_rel(
        &mut self,
        dx: i32,
        dy: i32,
        duration: Duration,
        tween: Tween,
        button: MouseButton,
    ) -> Result<()> {
        let p = self.backend.mouse_pos()?;
        self.drag_to(
            p.x.saturating_add(dx),
            p.y.saturating_add(dy),
            duration,
            tween,
            button,
        )
    }

    pub fn click_ex(
        &mut self,
        x: Option<i32>,
        y: Option<i32>,
        clicks: u32,
        interval: Duration,
        button: MouseButton,
    ) -> Result<()> {
        if x.is_some() != y.is_some() {
            return Err(Error::InvalidArgument(
                "x and y must both be set or both omitted",
            ));
        }
        self.before_action()?;
        match (x, y) {
            (Some(x), Some(y)) => {
                self.animate_move(Point::new(x, y), Duration::ZERO, tween::linear)?;
            }
            (None, None) => {}
            _ => {
                return Err(Error::InvalidArgument(
                    "x and y must both be set or both omitted",
                ));
            }
        }
        for i in 0..clicks {
            self.before_action()?;
            self.backend.button(button, true)?;
            self.backend.button(button, false)?;
            if i + 1 < clicks && !interval.is_zero() {
                self.wait_input_interval(interval)?;
            }
        }
        self.after_write()
    }

    pub fn click(&mut self) -> Result<()> {
        self.click_ex(None, None, 1, Duration::ZERO, MouseButton::Left)
    }
    pub fn click_xy(&mut self, x: i32, y: i32) -> Result<()> {
        self.click_ex(Some(x), Some(y), 1, Duration::ZERO, MouseButton::Left)
    }
    pub fn click_button(&mut self, button: MouseButton) -> Result<()> {
        self.click_ex(None, None, 1, Duration::ZERO, button)
    }
    pub fn double_click(&mut self) -> Result<()> {
        self.click_ex(None, None, 2, Duration::from_millis(50), MouseButton::Left)
    }
    pub fn triple_click(&mut self) -> Result<()> {
        self.click_ex(None, None, 3, Duration::from_millis(50), MouseButton::Left)
    }
    pub fn right_click(&mut self) -> Result<()> {
        self.click_ex(None, None, 1, Duration::ZERO, MouseButton::Right)
    }
    pub fn middle_click(&mut self) -> Result<()> {
        self.click_ex(None, None, 1, Duration::ZERO, MouseButton::Middle)
    }

    pub fn mouse_down(&mut self, button: MouseButton) -> Result<()> {
        self.before_action()?;
        self.backend.button(button, true)?;
        let result = self.after_write();
        if result.is_err() {
            let _ = self.backend.button(button, false);
        }
        result
    }
    pub fn mouse_up(&mut self, button: MouseButton) -> Result<()> {
        self.backend.button(button, false)?;
        self.after_write()
    }
    pub fn scroll(&mut self, clicks: i32) -> Result<()> {
        self.before_action()?;
        self.backend.scroll(clicks, 0)?;
        self.after_write()
    }
    pub fn hscroll(&mut self, clicks: i32) -> Result<()> {
        self.before_action()?;
        self.backend.scroll(0, clicks)?;
        self.after_write()
    }
    pub fn scroll_at(&mut self, clicks: i32, x: i32, y: i32) -> Result<()> {
        self.before_action()?;
        self.animate_move(Point::new(x, y), Duration::ZERO, tween::linear)?;
        self.scroll(clicks)
    }

    pub fn write(&mut self, text: &str, interval: Duration) -> Result<()> {
        self.typewrite(text, interval)
    }

    pub fn typewrite(&mut self, text: &str, interval: Duration) -> Result<()> {
        self.before_action()?;
        let mut buffer = String::new();
        for ch in text.chars() {
            self.before_action()?;
            if ch == '\n' || ch == '\t' {
                if !buffer.is_empty() {
                    self.backend.text(&buffer)?;
                    buffer.clear();
                }
                let key = if ch == '\n' { "enter" } else { "tab" };
                self.backend.key(key, true)?;
                self.backend.key(key, false)?;
            } else {
                buffer.push(ch);
                if !interval.is_zero() || buffer.len() >= 256 {
                    self.backend.text(&buffer)?;
                    buffer.clear();
                }
            }
            self.wait_input_interval(interval)?;
        }
        if !buffer.is_empty() {
            self.backend.text(&buffer)?;
        }
        self.after_write()
    }

    pub fn press(&mut self, key: &str) -> Result<()> {
        self.press_times(key, 1, Duration::ZERO)
    }

    pub fn press_times(&mut self, key: &str, presses: u32, interval: Duration) -> Result<()> {
        let k = crate::keys::normalize(key).ok_or(Error::InvalidArgument("unknown key"))?;
        self.before_action()?;
        for i in 0..presses {
            self.before_action()?;
            self.backend.key(&k, true)?;
            self.backend.key(&k, false)?;
            if i + 1 < presses && !interval.is_zero() {
                self.wait_input_interval(interval)?;
            }
        }
        self.after_write()
    }

    pub fn key_down(&mut self, key: &str) -> Result<()> {
        let k = crate::keys::normalize(key).ok_or(Error::InvalidArgument("unknown key"))?;
        self.before_action()?;
        self.backend.key(&k, true)?;
        let result = self.after_write();
        if result.is_err() {
            let _ = self.backend.key(&k, false);
        }
        result
    }

    pub fn key_up(&mut self, key: &str) -> Result<()> {
        let k = crate::keys::normalize(key).ok_or(Error::InvalidArgument("unknown key"))?;
        self.backend.key(&k, false)?;
        self.after_write()
    }

    pub fn hotkey(&mut self, keys: &[&str]) -> Result<()> {
        let norm: Result<Vec<String>> = keys
            .iter()
            .map(|k| crate::keys::normalize(k).ok_or(Error::InvalidArgument("unknown key")))
            .collect();
        let norm = norm?;
        self.before_action()?;
        let mut held: Vec<&str> = Vec::new();
        for k in &norm {
            if let Err(error) = self
                .before_action()
                .and_then(|()| self.backend.key(k, true))
            {
                for pressed in held.iter().rev() {
                    let _ = self.backend.key(pressed, false);
                }
                return Err(error);
            }
            held.push(k.as_str());
        }
        let mut release_error = None;
        for k in norm.iter().rev() {
            if let Err(error) = self.backend.key(k, false) {
                release_error.get_or_insert(error);
            }
        }
        if let Some(error) = release_error {
            return Err(error);
        }
        self.after_write()
    }

    pub fn hold(&mut self, keys: &[&str]) -> Result<KeyHold<'_>> {
        let norm: Result<Vec<String>> = keys
            .iter()
            .map(|k| crate::keys::normalize(k).ok_or(Error::InvalidArgument("unknown key")))
            .collect();
        let norm = norm?;
        self.before_action()?;
        let mut held: Vec<&str> = Vec::new();
        for k in &norm {
            if let Err(error) = self
                .before_action()
                .and_then(|()| self.backend.key(k, true))
            {
                for pressed in held.iter().rev() {
                    let _ = self.backend.key(pressed, false);
                }
                return Err(error);
            }
            held.push(k.as_str());
        }
        if let Err(error) = self.after_write() {
            for pressed in held.iter().rev() {
                let _ = self.backend.key(pressed, false);
            }
            return Err(error);
        }
        Ok(KeyHold {
            gui: self,
            keys: norm,
        })
    }

    pub fn screenshot(&mut self) -> Result<Screenshot> {
        self.before_read_heavy()?;
        let img = self.backend.capture(None)?;
        self.after_read_heavy()?;
        Ok(img)
    }

    pub fn screenshot_region(&mut self, region: Rect) -> Result<Screenshot> {
        self.before_read_heavy()?;
        let img = self.backend.capture(Some(region))?;
        self.after_read_heavy()?;
        Ok(img)
    }

    pub fn screenshot_to_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<Screenshot> {
        let img = self.screenshot()?;
        img.save(path)?;
        Ok(img)
    }

    pub fn pixel(&mut self, x: i32, y: i32) -> Result<Rgb> {
        self.before_read_heavy()?;
        let img = self.backend.capture(Some(Rect::new(x, y, 1, 1)))?;
        self.after_read_heavy()?;
        img.get_pixel(0, 0)
    }

    pub fn pixel_matches_color(
        &mut self,
        x: i32,
        y: i32,
        color: Rgb,
        tolerance: u8,
    ) -> Result<bool> {
        Ok(self.pixel(x, y)?.matches(color, tolerance))
    }

    pub fn locate(
        &self,
        needle: &Screenshot,
        haystack: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Rect> {
        self.before_read_heavy()?;
        let result = locate::locate_all_checked(needle, haystack, opt, true, &mut || {
            self.before_read_heavy()
        });
        self.after_read_heavy()?;
        result?.into_iter().next().ok_or(Error::ImageNotFound)
    }

    pub fn try_locate(
        &self,
        needle: &Screenshot,
        haystack: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Option<Rect>> {
        self.before_read_heavy()?;
        let result = locate::locate_all_checked(needle, haystack, opt, false, &mut || {
            self.before_read_heavy()
        });
        self.after_read_heavy()?;
        Ok(result?.into_iter().next())
    }

    pub fn locate_all(
        &self,
        needle: &Screenshot,
        haystack: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Vec<Rect>> {
        self.before_read_heavy()?;
        let result = locate::locate_all_checked(
            needle,
            haystack,
            opt,
            self.raise_image_not_found,
            &mut || self.before_read_heavy(),
        );
        self.after_read_heavy()?;
        result
    }

    pub fn locate_on_screen<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Rect> {
        let needle = Screenshot::load(image)?;
        self.locate_screenshot_on_screen(&needle, opt)
    }

    fn search_screen(
        &self,
        needle: &Screenshot,
        opt: &LocateOptions,
        raise: bool,
    ) -> Result<Vec<Rect>> {
        self.before_read_heavy()?;
        let result = locate::locate_until(
            || {
                self.before_read_heavy()?;
                let (region, offset) = if let Some(region) = opt.region {
                    let size = self.backend.screen_size()?;
                    let clipped = region
                        .intersect(&Rect::new(0, 0, size.width, size.height))
                        .ok_or(Error::InvalidArgument("region does not intersect screen"))?;
                    (Some(clipped), Point::new(clipped.left, clipped.top))
                } else {
                    (None, Point::new(0, 0))
                };
                let image = self.backend.capture(region)?;
                let local = LocateOptions {
                    region: None,
                    ..opt.clone()
                };
                let mut hits =
                    locate::locate_all_checked(needle, &image, &local, false, &mut || {
                        self.before_read_heavy()
                    })?;
                for hit in &mut hits {
                    hit.left += offset.x;
                    hit.top += offset.y;
                }
                Ok(hits)
            },
            opt.min_search_time,
            raise,
        );
        self.after_read_heavy()?;
        result
    }

    pub fn locate_screenshot_on_screen(
        &mut self,
        needle: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Rect> {
        self.search_screen(needle, opt, true)?
            .into_iter()
            .next()
            .ok_or(Error::ImageNotFound)
    }

    pub fn try_locate_screenshot_on_screen(
        &mut self,
        needle: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Option<Rect>> {
        Ok(self.search_screen(needle, opt, false)?.into_iter().next())
    }

    pub fn try_locate_on_screen<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Option<Rect>> {
        self.try_locate_screenshot_on_screen(&Screenshot::load(image)?, opt)
    }

    pub fn locate_all_on_screen<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Vec<Rect>> {
        self.search_screen(&Screenshot::load(image)?, opt, self.raise_image_not_found)
    }

    pub fn locate_center_on_screen<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Point> {
        Ok(crate::types::center(self.locate_on_screen(image, opt)?))
    }

    pub fn click_image<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Point> {
        let p = self.locate_center_on_screen(image, opt)?;
        self.click_xy(p.x, p.y)?;
        Ok(p)
    }

    pub fn get_all_windows(&mut self) -> Result<Vec<Window>> {
        let mut windows = window::get_all_windows()?;
        for window in &mut windows {
            window.with_settings(&self.settings);
        }
        Ok(windows)
    }
    pub fn get_all_titles(&mut self) -> Result<Vec<String>> {
        window::get_all_titles()
    }
    pub fn get_active_window(&mut self) -> Result<Option<Window>> {
        let mut window = window::get_active_window()?;
        if let Some(window) = &mut window {
            window.with_settings(&self.settings);
        }
        Ok(window)
    }
    pub fn get_windows_with_title(&mut self, title: &str) -> Result<Vec<Window>> {
        let mut windows = window::get_windows_with_title(title)?;
        for window in &mut windows {
            window.with_settings(&self.settings);
        }
        Ok(windows)
    }
    pub fn get_windows_with_title_ci(&mut self, title: &str) -> Result<Vec<Window>> {
        let mut windows = window::get_windows_with_title_ci(title)?;
        for window in &mut windows {
            window.with_settings(&self.settings);
        }
        Ok(windows)
    }
    pub fn get_windows_at(&mut self, x: i32, y: i32) -> Result<Vec<Window>> {
        let mut windows = window::get_windows_at(x, y)?;
        for window in &mut windows {
            window.with_settings(&self.settings);
        }
        Ok(windows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::FakeBackend;
    use crate::types::Rgb;

    fn gui() -> AutoGui {
        let mut fake = FakeBackend::new(200, 100);
        fake.pos = Point::new(10, 10);
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        g.settings_mut().failsafe = false;
        g
    }

    #[test]
    fn click_records() {
        let fake = FakeBackend::new(200, 100);
        let events = fake.shared_events();
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        g.settings_mut().failsafe = false;
        g.click().unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            ["button Left down", "button Left up"]
        );
    }

    #[test]
    fn regression_failsafe_still_releases_inputs() {
        let mut fake = FakeBackend::new(100, 50);
        fake.pos = Point::new(0, 0);
        let events = fake.shared_events();
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        assert!(matches!(g.key_up("ctrl"), Err(Error::FailSafe(_))));
        assert!(matches!(
            g.mouse_up(MouseButton::Left),
            Err(Error::FailSafe(_))
        ));
        assert_eq!(*events.lock().unwrap(), ["ctrl up", "button Left up"]);
    }

    #[test]
    fn hold_allows_operations_and_releases_modifier() {
        let fake = FakeBackend::new(100, 50);
        let events = fake.shared_events();
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        {
            let mut held = g.hold(&["ctrl"]).unwrap();
            held.click().unwrap();
        }
        assert_eq!(
            *events.lock().unwrap(),
            ["ctrl down", "button Left down", "button Left up", "ctrl up"]
        );
    }

    #[test]
    fn screen_region_returns_absolute_coordinates() {
        let mut fake = FakeBackend::new(100, 80);
        let needle = Screenshot::solid(3, 3, Rgb::new(10, 200, 20));
        for y in 31..34 {
            for x in 42..45 {
                fake.screen.set_pixel(x, y, Rgb::new(10, 200, 20)).unwrap();
            }
        }
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        let options = LocateOptions {
            region: Some(Rect::new(25, 20, 40, 30)),
            ..Default::default()
        };
        assert_eq!(
            g.locate_screenshot_on_screen(&needle, &options).unwrap(),
            Rect::new(42, 31, 3, 3)
        );
    }

    struct CornerBackend {
        fake: FakeBackend,
        captures: std::rc::Rc<std::cell::Cell<u32>>,
        moved: bool,
        abort_on_capture: bool,
    }

    impl Backend for CornerBackend {
        fn mouse_pos(&self) -> Result<Point> {
            if self.moved || (self.abort_on_capture && self.captures.get() > 0) {
                Ok(Point::new(0, 0))
            } else {
                self.fake.mouse_pos()
            }
        }
        fn move_mouse(&mut self, p: Point) -> Result<()> {
            self.moved = true;
            self.fake.move_mouse(p)
        }
        fn button(&mut self, b: MouseButton, down: bool) -> Result<()> {
            self.fake.button(b, down)
        }
        fn scroll(&mut self, dy: i32, dx: i32) -> Result<()> {
            self.fake.scroll(dy, dx)
        }
        fn key(&mut self, key: &str, down: bool) -> Result<()> {
            self.fake.key(key, down)
        }
        fn text(&mut self, s: &str) -> Result<()> {
            self.fake.text(s)
        }
        fn screen_size(&self) -> Result<Size> {
            self.fake.screen_size()
        }
        fn capture(&self, region: Option<Rect>) -> Result<Screenshot> {
            self.captures.set(self.captures.get() + 1);
            self.fake.capture(region)
        }
    }

    #[test]
    fn screen_wait_interrupts_after_first_capture() {
        let captures = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut g = gui();
        g.settings_mut().failsafe = true;
        g.backend = Box::new(CornerBackend {
            fake: FakeBackend::new(200, 100),
            captures: captures.clone(),
            moved: false,
            abort_on_capture: true,
        });
        let options = LocateOptions {
            min_search_time: Duration::from_secs(1),
            ..Default::default()
        };
        assert!(matches!(
            g.locate_screenshot_on_screen(&Screenshot::solid(2, 2, Rgb::new(255, 0, 0)), &options),
            Err(Error::FailSafe(_))
        ));
        assert_eq!(captures.get(), 1);
    }

    #[test]
    fn interrupted_drag_releases_button() {
        let fake = FakeBackend::new(200, 100);
        let events = fake.shared_events();
        let mut g = gui();
        g.settings_mut().failsafe = true;
        g.backend = Box::new(CornerBackend {
            fake,
            captures: Default::default(),
            moved: false,
            abort_on_capture: false,
        });
        assert!(matches!(
            g.drag_to(
                150,
                60,
                Duration::from_secs(1),
                tween::linear,
                MouseButton::Left
            ),
            Err(Error::FailSafe(_))
        ));
        let events = events.lock().unwrap();
        assert_eq!(events.last().map(String::as_str), Some("button Left up"));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.starts_with("move "))
                .count(),
            1
        );
    }

    #[test]
    fn hotkey_order() {
        let fake = FakeBackend::new(200, 100);
        let events = fake.shared_events();
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        g.settings_mut().failsafe = false;
        g.hotkey(&["ctrl", "c"]).unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            ["ctrl down", "c down", "c up", "ctrl up"]
        );
    }

    #[test]
    fn click_partial_coords_err() {
        let mut g = gui();
        let e = g
            .click_ex(Some(1), None, 1, Duration::ZERO, MouseButton::Left)
            .unwrap_err();
        assert!(matches!(e, Error::InvalidArgument(_)));
    }

    #[test]
    fn failsafe_corner() {
        let fake = FakeBackend::new(100, 50);
        let events = fake.shared_events();
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        g.settings_mut().failsafe = true;
        g.settings_mut().failsafe_points = vec![Point::new(0, 0)];
        // move to corner then click should trip
        g.settings_mut().failsafe = false;
        g.move_to_xy(0, 0).unwrap();
        g.settings_mut().failsafe = true;
        let e = g.click().unwrap_err();
        assert!(matches!(e, Error::FailSafe(_)));
        let e = g.hotkey(&["ctrl", "c"]).unwrap_err();
        assert!(matches!(e, Error::FailSafe(_)));
        assert_eq!(*events.lock().unwrap(), ["move 0,0"]);
    }

    #[test]
    fn failsafe_prevents_screenshot_at_corner() {
        let fake = FakeBackend::new(100, 50);
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        g.settings_mut().failsafe = true;
        g.settings_mut().failsafe_points = vec![Point::new(50, 25)];

        assert!(matches!(g.screenshot(), Err(Error::FailSafe(_))));
    }

    #[test]
    fn pixel_tolerance_via_types() {
        assert!(Rgb::new(10, 10, 10).matches(Rgb::new(12, 8, 10), 2));
        assert!(!Rgb::new(10, 10, 10).matches(Rgb::new(12, 8, 10), 1));
    }
}
