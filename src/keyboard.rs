use std::time::Duration;

use crate::error::{Error, Result};
use crate::gui::AutoGui;

/// Most characters sent to the backend in one call, so the fail-safe is
/// honoured at least every few dozen characters of a long string.
const TEXT_BATCH: usize = 32;

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
    pub fn write(&mut self, text: &str, interval: Duration) -> Result<()> {
        self.typewrite(text, interval)
    }

    pub fn typewrite(&mut self, text: &str, interval: Duration) -> Result<()> {
        self.before_action()?;
        let mut buffer = String::new();
        let mut buffered = 0;
        for ch in text.chars() {
            self.before_action()?;
            if ch == '\n' || ch == '\t' {
                self.flush_text(&mut buffer, &mut buffered)?;
                let key = if ch == '\n' { "enter" } else { "tab" };
                self.backend.key(key, true)?;
                self.backend.key(key, false)?;
            } else {
                buffer.push(ch);
                buffered += 1;
                if !interval.is_zero() || buffered >= TEXT_BATCH {
                    self.flush_text(&mut buffer, &mut buffered)?;
                }
            }
            self.wait_input_interval(interval)?;
        }
        self.flush_text(&mut buffer, &mut buffered)?;
        self.after_write()
    }

    /// Sends buffered text, checking the fail-safe immediately before: the
    /// per-character checks in `typewrite` run before anything is sent.
    fn flush_text(&mut self, buffer: &mut String, buffered: &mut usize) -> Result<()> {
        if buffer.is_empty() {
            return Ok(());
        }
        self.before_action()?;
        self.backend.text(buffer)?;
        buffer.clear();
        *buffered = 0;
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Backend, FakeBackend};
    use crate::types::{MouseButton, Point, Rect, Screenshot, Size};
    use std::cell::Cell;
    use std::rc::Rc;

    fn gui(fake: FakeBackend) -> AutoGui {
        let mut g = AutoGui::with_fake(fake);
        g.settings_mut().pause = Duration::ZERO;
        g
    }

    fn texts(events: &[String]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| e.strip_prefix("text ").map(str::to_string))
            .collect()
    }

    #[test]
    fn long_text_is_sent_in_small_batches() {
        let mut fake = FakeBackend::new(200, 100);
        fake.pos = Point::new(10, 10);
        let events = fake.shared_events();
        let mut g = gui(fake);
        let input: String = ('a'..='z').cycle().take(100).collect();
        g.typewrite(&input, Duration::ZERO).unwrap();
        let sent = texts(&events.lock().unwrap());
        assert_eq!(sent.concat(), input);
        assert_eq!(sent.len(), 4);
        assert!(sent.iter().all(|s| s.chars().count() <= TEXT_BATCH));
        // Multi-byte characters count as characters, not bytes.
        let events = {
            let mut fake = FakeBackend::new(200, 100);
            fake.pos = Point::new(10, 10);
            let events = fake.shared_events();
            gui(fake)
                .typewrite(&"\u{4e2d}".repeat(40), Duration::ZERO)
                .unwrap();
            events
        };
        let sent = texts(&events.lock().unwrap());
        assert_eq!(
            sent.iter().map(|s| s.chars().count()).collect::<Vec<_>>(),
            [32, 8]
        );
    }

    /// Moves the cursor onto a fail-safe corner once any text was sent.
    struct CornerAfterText {
        fake: FakeBackend,
        sent: Rc<Cell<bool>>,
    }

    impl Backend for CornerAfterText {
        fn mouse_pos(&self) -> Result<Point> {
            if self.sent.get() {
                Ok(Point::new(0, 0))
            } else {
                self.fake.mouse_pos()
            }
        }
        fn move_mouse(&mut self, p: Point) -> Result<()> {
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
            self.sent.set(true);
            self.fake.text(s)
        }
        fn screen_size(&self) -> Result<Size> {
            self.fake.screen_size()
        }
        fn capture(&self, region: Option<Rect>) -> Result<Screenshot> {
            self.fake.capture(region)
        }
    }

    #[test]
    fn fail_safe_stops_typewrite_after_one_batch() {
        let mut fake = FakeBackend::new(200, 100);
        fake.pos = Point::new(10, 10);
        let events = fake.shared_events();
        let mut g = gui(FakeBackend::new(200, 100));
        g.backend = Box::new(CornerAfterText {
            fake,
            sent: Rc::default(),
        });
        let result = g.typewrite(&"x".repeat(100), Duration::ZERO);
        assert!(matches!(result, Err(Error::FailSafe(_))));
        let sent = texts(&events.lock().unwrap());
        assert_eq!(sent, ["x".repeat(TEXT_BATCH)]);
    }
}
