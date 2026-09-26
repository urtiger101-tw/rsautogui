//! Moves the real mouse only when `AUTOGUI_LIVE=1` is set.
//! Slam the cursor into a screen corner to abort (fail-safe).

use std::time::Duration;

use autogui::{AutoGui, MouseButton, tweens};

fn main() -> autogui::Result<()> {
    if std::env::var("AUTOGUI_LIVE").ok().as_deref() != Some("1") {
        eprintln!("This example moves the real mouse. Set AUTOGUI_LIVE=1 to run it.");
        return Ok(());
    }
    let mut gui = AutoGui::new()?;
    gui.settings_mut().failsafe = true;
    gui.settings_mut().pause = Duration::from_millis(20);

    println!("spiral_drag: fail-safe is ON. Move mouse to a corner to abort.");
    let mut distance = 200;
    while distance > 0 {
        gui.drag_rel(
            distance,
            0,
            Duration::from_millis(200),
            tweens::linear,
            MouseButton::Left,
        )?;
        distance -= 5;
        gui.drag_rel(
            0,
            distance,
            Duration::from_millis(200),
            tweens::linear,
            MouseButton::Left,
        )?;
        gui.drag_rel(
            -distance,
            0,
            Duration::from_millis(200),
            tweens::linear,
            MouseButton::Left,
        )?;
        distance -= 5;
        gui.drag_rel(
            0,
            -distance,
            Duration::from_millis(200),
            tweens::linear,
            MouseButton::Left,
        )?;
    }
    Ok(())
}
