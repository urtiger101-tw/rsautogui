//! Deterministic recognition benchmark; never connects to the desktop.
use autogui::{AutoGui, FakeBackend, LocateOptions, Rect, Screenshot};
use std::time::{Duration, Instant};

fn main() -> autogui::Result<()> {
    let full = std::env::args().any(|arg| arg == "--full-hd");
    let (width, height, tw, th) = if full {
        (1920, 1080, 80, 40)
    } else {
        (320, 180, 32, 16)
    };
    let mut state = 1234567u32;
    let pixels = (0..width * height * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    let hay = Screenshot::new(width, height, pixels)?;
    let expected = Rect::new(width as i32 / 2, height as i32 / 2, tw, th);
    let needle = hay.region(expected)?;
    let mut gui = AutoGui::with_fake(FakeBackend::new(10, 10));
    gui.settings_mut().pause = Duration::ZERO;
    for confidence in [None, Some(0.999)] {
        let options = LocateOptions {
            confidence,
            ..Default::default()
        };
        let start = Instant::now();
        let found = gui.locate(&needle, &hay, &options)?;
        assert_eq!(found, expected);
        println!(
            "{}x{} template {}x{}, confidence {:?}: {:.2} ms, {:?}",
            width,
            height,
            tw,
            th,
            confidence,
            start.elapsed().as_secs_f64() * 1000.0,
            found
        );
    }
    Ok(())
}
