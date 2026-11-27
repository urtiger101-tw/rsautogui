use std::thread;
use std::time::{Duration, Instant};

use crate::backend::{Backend, FakeBackend, default_backend};
use crate::error::Result;
use crate::failsafe;
use crate::settings::Settings;

pub struct AutoGui {
    pub(crate) backend: Box<dyn Backend>,
    pub(crate) settings: Settings,
    pub(crate) raise_image_not_found: bool,
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

    /// Checks the emergency stop chord (Ctrl+Alt+F12) first, then the corner fail-safe.
    pub(crate) fn before_action(&self) -> Result<()> {
        if self.settings.stop_key && crate::stop::chord_down() {
            return Err(crate::error::Error::Stopped);
        }
        let pos = self.backend.mouse_pos()?;
        failsafe::check(&self.settings, pos)
    }

    pub(crate) fn wait_input_interval(&self, interval: Duration) -> Result<()> {
        let start = Instant::now();
        while start.elapsed() < interval {
            self.before_action()?;
            thread::sleep(Duration::from_millis(16).min(interval.saturating_sub(start.elapsed())));
        }
        Ok(())
    }

    pub(crate) fn after_write(&mut self) -> Result<()> {
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

    pub(crate) fn before_read_heavy(&self) -> Result<()> {
        let pos = self.backend.mouse_pos()?;
        failsafe::check(&self.settings, pos)
    }

    pub(crate) fn after_read_heavy(&self) -> Result<()> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::FakeBackend;
    use crate::error::Error;
    use crate::locate::LocateOptions;
    use crate::tween;
    use crate::types::{MouseButton, Point, Rect, Rgb, Screenshot, Size};

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
