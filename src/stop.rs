//! Emergency stop chord (SPEC section 19): Ctrl+Alt+F12.
use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

/// Whether Ctrl, Alt and F12 are all down right now. Always false off
/// Windows. Injected keys are visible too, so callers must not synthesize
/// this chord.
pub fn chord_down() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_CONTROL, VK_F12, VK_MENU,
        };
        let down = |vk: u16| {
            // SAFETY: GetAsyncKeyState takes a plain integer and has no pointer arguments.
            let state = unsafe { GetAsyncKeyState(i32::from(vk)) };
            (state as u16) & 0x8000 != 0
        };
        down(VK_CONTROL) && down(VK_MENU) && down(VK_F12)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[derive(Default)]
struct Inner {
    tripped: AtomicBool,
    started: AtomicBool,
}

/// A latch that stays tripped once the stop chord was seen. `arm()` starts a
/// 30 ms polling thread that exits shortly after the latch is dropped.
/// Clones share one latch: tripping any clone trips them all.
#[derive(Clone, Default)]
pub struct StopLatch {
    inner: Arc<Inner>,
}

const POLL: Duration = Duration::from_millis(30);

fn poll(inner: Weak<Inner>) {
    loop {
        let Some(strong) = inner.upgrade() else {
            return;
        };
        if chord_down() {
            strong.tripped.store(true, Ordering::SeqCst);
        }
        drop(strong);
        std::thread::sleep(POLL);
    }
}

impl StopLatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts the polling thread; further calls do nothing.
    pub fn arm(&self) {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        let spawned = std::thread::Builder::new()
            .name("autogui-stop".into())
            .spawn(move || poll(weak));
        if spawned.is_err() {
            self.inner.started.store(false, Ordering::SeqCst);
        }
    }

    pub fn tripped(&self) -> bool {
        self.inner.tripped.load(Ordering::SeqCst)
    }

    /// Err([`Error::Stopped`]) once tripped.
    pub fn check(&self) -> Result<()> {
        if self.tripped() {
            Err(Error::Stopped)
        } else {
            Ok(())
        }
    }

    /// Trips the latch directly. For hosts that detect the stop request
    /// another way, and for tests.
    pub fn trip(&self) {
        self.inner.tripped.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tripped_latch_stays_tripped() {
        let latch = StopLatch::new();
        assert!(latch.check().is_ok());
        latch.trip();
        assert!(latch.tripped());
        assert!(matches!(latch.check(), Err(Error::Stopped)));
        assert!(matches!(latch.check(), Err(Error::Stopped)));
    }

    #[test]
    fn arm_is_idempotent_and_poller_exits_on_drop() {
        let latch = StopLatch::new();
        latch.arm();
        latch.arm();
        let weak = Arc::downgrade(&latch.inner);
        drop(latch);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while weak.strong_count() > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(weak.strong_count(), 0);
    }

    #[test]
    fn default_settings_enable_the_stop_key() {
        assert!(crate::Settings::default().stop_key);
    }
}
