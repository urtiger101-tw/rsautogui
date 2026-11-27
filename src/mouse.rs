use std::thread;
use std::time::{Duration, Instant};

use crate::Display;
use crate::error::{Error, Result};
use crate::gui::AutoGui;
use crate::tween::{self, Tween};
use crate::types::{MouseButton, Point};

impl AutoGui {
    pub(crate) fn animate_move(
        &mut self,
        dest: Point,
        duration: Duration,
        tween: Tween,
    ) -> Result<()> {
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

    /// Moves to `points[0]`, holds `button`, drags through the remaining
    /// points and releases. `duration` is split across the segments in
    /// proportion to their Euclidean length (an equal split when every
    /// segment is zero-length). The button is released on every exit path.
    pub fn drag_path(
        &mut self,
        points: &[Point],
        duration: Duration,
        button: MouseButton,
    ) -> Result<()> {
        if !(2..=MAX_PATH_POINTS).contains(&points.len()) {
            return Err(Error::InvalidArgument("drag_path needs 2..=256 points"));
        }
        let durations = segment_durations(points, duration);
        self.before_action()?;
        self.animate_move(points[0], Duration::ZERO, tween::linear)?;
        self.before_action()?;
        self.backend.button(button, true)?;
        let moved = self.drag_segments(&points[1..], &durations);
        let release = self.backend.button(button, false);
        moved?;
        release?;
        self.after_write()
    }

    fn drag_segments(&mut self, destinations: &[Point], durations: &[Duration]) -> Result<()> {
        for (dest, segment) in destinations.iter().zip(durations) {
            self.before_action()?;
            self.animate_move(*dest, *segment, tween::linear)?;
        }
        Ok(())
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

    /// Click a physical desktop coordinate on a validated Windows display.
    /// Windows has one shared cursor/focus across physical and virtual monitors.
    pub fn click_display(&mut self, display: &Display, point: Point) -> Result<()> {
        self.move_to_display(display, point)?;
        self.click()
    }

    /// Move the shared cursor to an explicit display point without clicking.
    /// Callers can recheck their target/focus/cancellation before sending input.
    pub fn move_to_display(&mut self, display: &Display, point: Point) -> Result<()> {
        self.before_action()?;
        display.validate()?;
        if !display.contains(point) {
            return Err(Error::InvalidArgument(
                "point is outside the selected display",
            ));
        }
        self.backend.move_mouse_desktop(point)?;
        let start = Instant::now();
        while self.backend.mouse_pos()? != point {
            self.before_action()?;
            if start.elapsed() >= Duration::from_millis(100) {
                return Err(Error::Input(
                    "cursor did not reach selected display point; no click sent".into(),
                ));
            }
            thread::sleep(Duration::from_millis(2));
        }
        display.validate()?;
        self.after_write()?;
        if self.backend.mouse_pos()? != point {
            return Err(Error::Input(
                "cursor moved during the configured pause; no click sent".into(),
            ));
        }
        Ok(())
    }
}

/// Largest point list accepted by [`AutoGui::drag_path`].
const MAX_PATH_POINTS: usize = 256;

/// Splits `total` across the segments of `points` in proportion to their
/// Euclidean length. Zero total length falls back to an equal split.
fn segment_durations(points: &[Point], total: Duration) -> Vec<Duration> {
    let lengths: Vec<f64> = points
        .windows(2)
        .map(|w| {
            let dx = f64::from(w[1].x) - f64::from(w[0].x);
            let dy = f64::from(w[1].y) - f64::from(w[0].y);
            dx.hypot(dy)
        })
        .collect();
    let sum: f64 = lengths.iter().sum();
    if sum <= 0.0 {
        let count = lengths.len().max(1) as f64;
        return vec![total.div_f64(count); lengths.len()];
    }
    lengths.iter().map(|len| total.mul_f64(len / sum)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::FakeBackend;
    use crate::error::Error;

    fn fake(width: i32, height: i32) -> AutoGui {
        let mut g = AutoGui::with_fake(FakeBackend::new(width, height));
        g.settings_mut().pause = Duration::ZERO;
        g
    }

    #[test]
    fn segment_durations_split_by_length() {
        let pts = [Point::new(0, 0), Point::new(30, 40), Point::new(30, 60)];
        // Lengths 50 and 20 of a total 70.
        let parts = segment_durations(&pts, Duration::from_millis(700));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], Duration::from_millis(500));
        assert_eq!(parts[1], Duration::from_millis(200));
    }

    #[test]
    fn segment_durations_zero_length_splits_equally() {
        let pts = [Point::new(5, 5), Point::new(5, 5), Point::new(5, 5)];
        let parts = segment_durations(&pts, Duration::from_millis(600));
        assert_eq!(
            parts,
            [Duration::from_millis(300), Duration::from_millis(300)]
        );
    }

    #[test]
    fn drag_path_event_order() {
        let fake = FakeBackend::new(200, 100);
        let events = fake.shared_events();
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        g.settings_mut().failsafe = false;
        let pts = [Point::new(10, 10), Point::new(50, 10), Point::new(50, 30)];
        g.drag_path(&pts, Duration::ZERO, MouseButton::Left)
            .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            [
                "move 10,10",
                "button Left down",
                "move 50,10",
                "move 50,30",
                "button Left up",
            ]
        );
    }

    #[test]
    fn drag_path_rejects_point_counts_outside_2_to_256() {
        let mut g = fake(200, 100);
        let one = [Point::new(10, 10)];
        assert!(matches!(
            g.drag_path(&one, Duration::ZERO, MouseButton::Left),
            Err(Error::InvalidArgument(_))
        ));
        let many: Vec<Point> = (0..257).map(|i| Point::new(i % 100 + 1, 10)).collect();
        assert!(matches!(
            g.drag_path(&many, Duration::ZERO, MouseButton::Left),
            Err(Error::InvalidArgument(_))
        ));
        let max: Vec<Point> = (0..256).map(|i| Point::new(i % 100 + 1, 10)).collect();
        assert!(g.drag_path(&max, Duration::ZERO, MouseButton::Left).is_ok());
    }

    #[test]
    fn drag_path_releases_button_when_failsafe_trips_mid_path() {
        let mut fake = FakeBackend::new(200, 100);
        fake.pos = Point::new(100, 50);
        let events = fake.shared_events();
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        g.settings_mut().failsafe = true;
        // The second segment ends on the top-left corner: the check before
        // the third segment fails with the button held.
        let pts = [Point::new(100, 50), Point::new(0, 0), Point::new(150, 50)];
        let result = g.drag_path(&pts, Duration::ZERO, MouseButton::Left);
        assert!(matches!(result, Err(Error::FailSafe(_))));
        let log = events.lock().unwrap();
        assert_eq!(log.last().map(String::as_str), Some("button Left up"));
        assert_eq!(log.iter().filter(|e| e.contains("Left down")).count(), 1);
        assert_eq!(log.iter().filter(|e| e.contains("Left up")).count(), 1);
    }
}
