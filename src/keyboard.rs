use std::time::Duration;

use crate::error::{Error, Result};
use crate::gui::AutoGui;

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
}
