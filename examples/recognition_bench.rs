//! Deterministic recognition benchmark. It never touches the desktop unless
//! --capture is given, which only reads the primary monitor (no input is sent).
//!
//!   cargo run --release --features opencv --example recognition_bench -- [options]
//!
//! Options:
//!   --full-hd         1920x1080 haystack (default 320x180)
//!   --iterations N    timed runs per case; median and best are reported (default 5)
//!   --json            one JSON object per line instead of the human table
//!   --capture         also time primary-monitor screenshots (read-only)
use autogui::{AutoGui, FakeBackend, LocateOptions, Rect, Rgb, Screenshot};
use std::time::{Duration, Instant};

struct Case {
    name: &'static str,
    hay: Screenshot,
    needle: Screenshot,
    expected: Rect,
    options: LocateOptions,
}

fn xorshift(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

fn noise(width: u32, height: u32) -> autogui::Result<Screenshot> {
    let mut state = 1234567u32;
    let pixels = (0..width * height * 3)
        .map(|_| xorshift(&mut state) as u8)
        .collect();
    Screenshot::new(width, height, pixels)
}

/// Flat background with a grid of "buttons" carrying glyph-like dark pixels:
/// many anchor-colour hits, as on a real desktop.
fn ui_like(width: u32, height: u32, unique: Rect) -> autogui::Result<Screenshot> {
    let mut image = Screenshot::solid(width, height, Rgb::new(240, 240, 240));
    let mut state = 42u32;
    let (bw, bh) = (unique.width, unique.height);
    for top in (4..height as i32 - bh).step_by(bh as usize + 8) {
        for left in (4..width as i32 - bw).step_by(bw as usize + 8) {
            let target = Rect::new(left, top, bw, bh).intersect(&unique).is_some();
            if target {
                continue;
            }
            paint_button(&mut image, Rect::new(left, top, bw, bh), &mut state)?;
        }
    }
    let mut unique_state = 7u32;
    paint_button(&mut image, unique, &mut unique_state)?;
    Ok(image)
}

fn paint_button(image: &mut Screenshot, rect: Rect, state: &mut u32) -> autogui::Result<()> {
    for y in rect.top..rect.bottom() {
        for x in rect.left..rect.right() {
            let border =
                y == rect.top || x == rect.left || y + 1 == rect.bottom() || x + 1 == rect.right();
            let color = if border {
                Rgb::new(120, 120, 120)
            } else if xorshift(state).is_multiple_of(5) {
                Rgb::new(20, 20, 20)
            } else {
                Rgb::new(225, 225, 225)
            };
            image.set_pixel(x, y, color)?;
        }
    }
    Ok(())
}

fn cases(full: bool) -> autogui::Result<Vec<Case>> {
    let (width, height, tw, th) = if full {
        (1920, 1080, 80, 40)
    } else {
        (320, 180, 32, 16)
    };
    let expected = Rect::new(width as i32 / 2, height as i32 / 2, tw, th);
    let hay = noise(width, height)?;
    let needle = hay.region(expected)?;
    let ui_box = Rect::new(width as i32 / 2 + 5, height as i32 / 2 + 3, tw, th);
    let ui = ui_like(width, height, ui_box)?;
    let ui_needle = ui.region(ui_box)?;
    let opts = |grayscale, confidence| LocateOptions {
        grayscale,
        confidence,
        ..Default::default()
    };
    let mut list = vec![
        Case {
            name: "noise exact rgb",
            hay: hay.clone(),
            needle: needle.clone(),
            expected,
            options: opts(false, None),
        },
        Case {
            name: "noise exact gray",
            hay: hay.clone(),
            needle: needle.clone(),
            expected,
            options: opts(true, None),
        },
        Case {
            name: "ui exact rgb",
            hay: ui.clone(),
            needle: ui_needle.clone(),
            expected: ui_box,
            options: opts(false, None),
        },
        Case {
            name: "ui exact gray",
            hay: ui.clone(),
            needle: ui_needle.clone(),
            expected: ui_box,
            options: opts(true, None),
        },
    ];
    if cfg!(feature = "opencv") {
        list.extend([
            Case {
                name: "noise conf 0.999 rgb",
                hay: hay.clone(),
                needle: needle.clone(),
                expected,
                options: opts(false, Some(0.999)),
            },
            Case {
                name: "noise conf 0.9 gray",
                hay,
                needle,
                expected,
                options: opts(true, Some(0.9)),
            },
            Case {
                name: "ui conf 0.95 rgb",
                hay: ui.clone(),
                needle: ui_needle.clone(),
                expected: ui_box,
                options: opts(false, Some(0.95)),
            },
            Case {
                name: "ui conf 0.95 rgb region",
                hay: ui,
                needle: ui_needle,
                expected: ui_box,
                options: LocateOptions {
                    region: Some(Rect::new(
                        ui_box.left - 100,
                        ui_box.top - 60,
                        tw + 200,
                        th + 120,
                    )),
                    ..opts(false, Some(0.95))
                },
            },
        ]);
    }
    Ok(list)
}

fn stats(mut samples: Vec<f64>) -> (f64, f64) {
    samples.sort_by(f64::total_cmp);
    let best = samples.first().copied().unwrap_or(0.0);
    let median = samples.get(samples.len() / 2).copied().unwrap_or(0.0);
    (median, best)
}

fn report(json: bool, name: &str, size: &str, iterations: usize, samples: Vec<f64>) {
    let (median, best) = stats(samples);
    if json {
        println!(
            "{{\"case\":\"{name}\",\"size\":\"{size}\",\"iterations\":{iterations},\"median_ms\":{median:.3},\"best_ms\":{best:.3}}}"
        );
    } else {
        println!("{name:<28} {size:<22} {median:>10.2} {best:>10.2}");
    }
}

fn main() -> autogui::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let full = args.iter().any(|arg| arg == "--full-hd");
    let json = args.iter().any(|arg| arg == "--json");
    let capture = args.iter().any(|arg| arg == "--capture");
    let iterations = args
        .iter()
        .position(|arg| arg == "--iterations")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(5)
        .max(1);
    let mut gui = AutoGui::with_fake(FakeBackend::new(10, 10));
    gui.settings_mut().pause = Duration::ZERO;
    if !json {
        println!(
            "{:<28} {:<22} {:>10} {:>10}",
            "case", "haystack / template", "median ms", "best ms"
        );
    }
    for case in cases(full)? {
        let size = format!(
            "{}x{} / {}x{}",
            case.hay.width, case.hay.height, case.needle.width, case.needle.height
        );
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            let found = gui.locate(&case.needle, &case.hay, &case.options)?;
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
            if found != case.expected {
                eprintln!(
                    "{}: expected {:?}, found {:?}",
                    case.name, case.expected, found
                );
                return Err(autogui::Error::ImageNotFound);
            }
        }
        report(json, case.name, &size, iterations, samples);
    }
    if capture {
        let mut real = AutoGui::new()?;
        real.settings_mut().pause = Duration::ZERO;
        let mut full_samples = Vec::with_capacity(iterations);
        let mut size = String::new();
        for _ in 0..iterations {
            let start = Instant::now();
            let shot = real.screenshot()?;
            full_samples.push(start.elapsed().as_secs_f64() * 1000.0);
            size = format!("{}x{}", shot.width, shot.height);
        }
        report(json, "screenshot primary", &size, iterations, full_samples);
        let region = Rect::new(100, 100, 400, 300);
        let mut region_samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            real.screenshot_region(region)?;
            region_samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        report(
            json,
            "screenshot region",
            "400x300",
            iterations,
            region_samples,
        );
        // Explicit-display capture (validated topology snapshot), when available.
        if let Some(display) = autogui::Display::all()
            .ok()
            .and_then(|all| all.into_iter().find(|d| d.is_primary()))
        {
            let mut display_samples = Vec::with_capacity(iterations);
            for _ in 0..iterations {
                let start = Instant::now();
                real.screenshot_display(&display)?;
                display_samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            let bounds = display.bounds();
            let size = format!("{}x{}", bounds.width, bounds.height);
            report(
                json,
                "screenshot display",
                &size,
                iterations,
                display_samples,
            );
        }
    }
    Ok(())
}
