use std::time::Duration;

use autogui::{AutoGui, FakeBackend};

// Peek events by wrapping through public API only is limited.
// This test asserts hotkey does not error and fail-safe geometry.

#[test]
fn hotkey_and_size() {
    let fake = FakeBackend::new(200, 100);
    let events = fake.shared_events();
    let mut g = AutoGui::with_fake(fake);
    g.settings_mut().pause = Duration::ZERO;
    g.settings_mut().failsafe = false;
    g.hotkey(&["ctrl", "c"]).unwrap();
    let s = g.size().unwrap();
    assert!(s.width > 0 && s.height > 0);
    assert_eq!(
        *events.lock().unwrap(),
        ["ctrl down", "c down", "c up", "ctrl up"]
    );
}
