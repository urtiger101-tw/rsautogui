//! Operate one explicitly selected Windows window. `click` and `type` send real input.
use autogui::{AutoGui, Display, Error, LocateOptions, Point, Rect, Screenshot, Window};
use std::time::{Duration, Instant};

use crate::commands::{display as display_command, screensaver, session, window as window_command};
use crate::desktop;
use std::sync::atomic::{AtomicBool, Ordering};

mod args;
mod errors;
pub(crate) mod lang;

use lang::tr;

/// Exit code for panics and other internal errors.
pub use errors::INTERNAL as EXIT_INTERNAL;

static QUIET: AtomicBool = AtomicBool::new(false);
static JSON: AtomicBool = AtomicBool::new(false);

/// True when --quiet asked to suppress success messages.
pub(crate) fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

fn json_output() -> bool {
    JSON.load(Ordering::Relaxed)
}

fn number(value: Option<&String>, default: f64) -> autogui::Result<f64> {
    let value = value
        .map(|s| {
            s.parse::<f64>()
                .map_err(|_| Error::InvalidArgument("invalid numeric argument"))
        })
        .transpose()?
        .unwrap_or(default);
    if !value.is_finite() {
        return Err(Error::InvalidArgument("numeric arguments must be finite"));
    }
    Ok(value)
}

/// Splits "--process <name>" and "--foreground" from positional arguments.
fn parse_filter(args: &[String]) -> autogui::Result<(desktop::WindowFilter, Vec<String>)> {
    let mut filter = desktop::WindowFilter::default();
    let mut positional = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--process" => {
                let name = iter
                    .next()
                    .ok_or(Error::InvalidArgument("--process needs an executable name"))?;
                filter.process = Some(name.clone());
            }
            "--foreground" => filter.foreground = true,
            _ => positional.push(arg.clone()),
        }
    }
    Ok((filter, positional))
}

fn print_windows(windows: &[Window]) -> autogui::Result<()> {
    let mut rows = Vec::new();
    for window in windows {
        match (window.title(), window.box_rect()) {
            (Ok(title), Ok(rect)) => {
                let (pid, process) = desktop::process_info(window.hwnd())
                    .map(|(pid, path)| (pid.to_string(), path))
                    .unwrap_or_default();
                let class = desktop::class_name(window.hwnd()).unwrap_or_default();
                if json_output() {
                    rows.push(serde_json::json!({
                        "hwnd": window.hwnd(),
                        "rect": {"left": rect.left, "top": rect.top, "width": rect.width, "height": rect.height},
                        "title": title,
                        "pid": pid.parse::<u32>().ok(),
                        "process": process,
                        "class": class,
                    }));
                } else {
                    println!(
                        "{}\t{:?}\t{}\t{}\t{}\t{}",
                        window.hwnd(),
                        rect,
                        title,
                        pid,
                        process,
                        class
                    );
                }
            }
            (Err(Error::WindowNotFound), _) | (_, Err(Error::WindowNotFound)) => {}
            (Err(error), _) | (_, Err(error)) => return Err(error),
        }
    }
    if json_output() {
        println!("{}", serde_json::Value::Array(rows));
    }
    Ok(())
}

pub(crate) fn unique_window(gui: &mut AutoGui, title: &str) -> autogui::Result<Window> {
    if title.trim().is_empty() {
        return Err(Error::InvalidArgument("window title must not be empty"));
    }
    let windows = gui.get_windows_with_title_ci(title)?;
    if windows.len() != 1 {
        for window in &windows {
            eprintln!(
                "{}",
                tr!(
                    "Candidate window {}: {}",
                    "候選視窗 {}: {}",
                    window.hwnd(),
                    window.title()?
                )
            );
        }
        return if windows.is_empty() {
            Err(Error::WindowNotFound)
        } else {
            Err(Error::InvalidArgument(errors::AMBIGUOUS_WINDOW))
        };
    }
    windows.into_iter().next().ok_or(Error::WindowNotFound)
}

pub(crate) fn active(window: &Window) -> autogui::Result<()> {
    if !window.is_active()? || window.is_minimized()? {
        return Err(Error::PermissionDenied(
            "target window lost foreground focus",
        ));
    }
    Ok(())
}

pub(crate) fn screen_region(
    gui: &AutoGui,
    window: &Window,
    display: Option<&Display>,
) -> autogui::Result<Rect> {
    let bounds = if let Some(display) = display {
        display.validate()?;
        display.bounds()
    } else {
        let size = gui.size()?;
        Rect::new(0, 0, size.width, size.height)
    };
    window
        .box_rect()?
        .intersect(&bounds)
        .ok_or(Error::InvalidArgument(
            "target window is outside the selected display (default: primary)",
        ))
}

pub(crate) fn capture_region(
    gui: &mut AutoGui,
    display: Option<&Display>,
    region: Rect,
) -> autogui::Result<Screenshot> {
    match display {
        Some(display) => gui.screenshot_display_region(display, region),
        None => gui.screenshot_region(region),
    }
}

pub(crate) fn scaled_template(path: &str, scale: f64) -> autogui::Result<Screenshot> {
    if !(0.1..=4.0).contains(&scale) {
        return Err(Error::InvalidArgument("scale must be between 0.1 and 4.0"));
    }
    let image = Screenshot::load(path)?;
    if scale == 1.0 {
        return Ok(image);
    }
    let width = (f64::from(image.width) * scale).round().max(1.0) as u32;
    let height = (f64::from(image.height) * scale).round().max(1.0) as u32;
    let rgb = image::RgbImage::from_raw(image.width, image.height, image.pixels)
        .ok_or(Error::InvalidArgument("invalid template image"))?;
    let scaled =
        image::imageops::resize(&rgb, width, height, image::imageops::FilterType::Triangle);
    Screenshot::new(width, height, scaled.into_raw())
}

pub(crate) fn locate_target(
    gui: &mut AutoGui,
    window: &Window,
    needle: &Screenshot,
    confidence: Option<f32>,
    timeout: Duration,
    display: Option<&Display>,
    check_cancelled: &dyn Fn() -> autogui::Result<()>,
) -> autogui::Result<(Rect, Rect)> {
    let start = Instant::now();
    gui.set_raise_image_not_found(false);
    loop {
        check_cancelled()?;
        active(window)?;
        let window_rect = window.box_rect()?;
        let region = screen_region(gui, window, display)?;
        let frame = capture_region(gui, display, region)?;
        let options = LocateOptions {
            confidence,
            ..Default::default()
        };
        let matches = gui.locate_all(needle, &frame, &options)?;
        if window.is_active()? && window.box_rect()? == window_rect {
            if matches.len() > 1 {
                return Err(Error::InvalidArgument(errors::AMBIGUOUS_IMAGE));
            }
            if let Some(found) = matches.first() {
                return Ok((
                    Rect::new(
                        found.left + region.left,
                        found.top + region.top,
                        found.width,
                        found.height,
                    ),
                    window_rect,
                ));
            }
        }
        if start.elapsed() >= timeout {
            return Err(Error::ImageNotFound);
        }
        std::thread::sleep(Duration::from_millis(50).min(timeout.saturating_sub(start.elapsed())));
    }
}

fn run(mut args: Vec<String>) -> autogui::Result<()> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("Help is provided by the launcher: run 'autogui-control --help'.");
        return Ok(());
    }
    if args[0] == "__runtime-version" {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    #[cfg(feature = "agent")]
    if args[0] == "mcp" && args.len() == 1 {
        return autogui_protocol::server::run(crate::mcp::session::Session::default())
            .map_err(Error::Io);
    }
    if args[0] == "window" {
        return window_command::run(&args[1..]);
    }
    if args[0] == "display" {
        return display_command::run(&args[1..]);
    }
    if args[0] == "ui" {
        return crate::commands::ui::run(&args[1..], json_output());
    }
    // Diagnostics must work even when the input backend cannot start.
    if args[0] == "session" {
        let mut rest = args[1..].to_vec();
        if json_output() && !rest.iter().any(|a| a == "--json") {
            if rest.is_empty() {
                rest.push("status".into());
            }
            rest.push("--json".into());
        }
        return session::run(&rest);
    }
    let display = if matches!(args[0].as_str(), "capture" | "locate" | "click") {
        if let Some(index) = args
            .iter()
            .enumerate()
            .skip(3)
            .find_map(|(i, s)| (s == "--display").then_some(i))
        {
            if index + 2 != args.len() {
                return Err(Error::InvalidArgument(
                    "--display <exact device name> must be the final option",
                ));
            }
            let display = display_command::select(&args[index + 1])?;
            args.truncate(index);
            Some(display)
        } else {
            None
        }
    } else {
        None
    };
    let command = args[0].as_str();
    if ![
        "list",
        "wait",
        "capture",
        "locate",
        "click",
        "type",
        "paste",
        "screensaver",
    ]
    .contains(&command)
    {
        return Err(Error::InvalidArgument("unknown command; use --help"));
    }
    if !matches!(command, "list" | "wait" | "screensaver") && args.len() < 3 {
        return Err(Error::InvalidArgument(
            "missing title or image/text argument; use --help",
        ));
    }
    if !cfg!(windows) {
        return Err(Error::UnsupportedPlatform);
    }
    if !cfg!(feature = "os") {
        return Err(Error::FeatureDisabled("os"));
    }
    // Screensaver controls must remain usable before Enigo/cursor access succeeds.
    if command == "screensaver" {
        return screensaver::run(&args[1..]);
    }
    let mut gui = AutoGui::new()?;
    gui.settings_mut().pause = Duration::from_millis(25);
    if command == "list" || command == "wait" {
        let (filter, positional) = parse_filter(&args[1..])?;
        if command == "list" {
            if positional.len() > 1 {
                return Err(Error::InvalidArgument(
                    "list [title] [--process name] [--foreground]",
                ));
            }
            let filter = desktop::WindowFilter {
                title: positional.first().cloned(),
                ..filter
            };
            print_windows(&desktop::list(&mut gui, &filter)?)?;
            return Ok(());
        }
        if positional.len() > 3 {
            return Err(Error::InvalidArgument(
                "wait <title> [present|absent|foreground] [timeout] [--process name]",
            ));
        }
        let state = positional
            .get(1)
            .map(|s| desktop::WaitState::parse(s))
            .transpose()?
            .unwrap_or(desktop::WaitState::Present);
        let seconds = number(positional.get(2), 10.0)?;
        if !(0.0..=3600.0).contains(&seconds) {
            return Err(Error::InvalidArgument(
                "timeout must be between 0 and 3600 seconds",
            ));
        }
        let filter = desktop::WindowFilter {
            title: positional.first().cloned().filter(|t| !t.is_empty()),
            ..filter
        };
        if filter.title.is_none() && filter.process.is_none() {
            return Err(Error::InvalidArgument("wait needs a title or --process"));
        }
        let found = desktop::wait(
            &mut gui,
            &filter,
            state,
            Duration::from_secs_f64(seconds),
            &|| Ok(()),
        )?;
        print_windows(&found)?;
        return Ok(());
    }
    let window = unique_window(&mut gui, &args[1])?;
    // Validate image and numeric arguments before changing foreground focus.
    let prepared = if command == "click" || command == "locate" {
        let confidence = if args.get(3).is_some_and(|s| s == "exact") {
            None
        } else {
            let value = number(args.get(3), 0.95)?;
            if !(0.0..=1.0).contains(&value) {
                return Err(Error::InvalidArgument("confidence must be between 0 and 1"));
            }
            if !cfg!(feature = "opencv") {
                return Err(Error::FeatureDisabled("opencv"));
            }
            Some(value as f32)
        };
        let seconds = number(args.get(4), 5.0)?;
        if !(0.0..=3600.0).contains(&seconds) {
            return Err(Error::InvalidArgument(
                "timeout must be between 0 and 3600 seconds",
            ));
        }
        Some((
            scaled_template(&args[2], number(args.get(5), 1.0)?)?,
            confidence,
            Duration::from_secs_f64(seconds),
        ))
    } else {
        None
    };
    window.activate()?;
    active(&window)?;
    match command {
        "capture" => {
            let region = screen_region(&gui, &window, display.as_ref())?;
            capture_region(&mut gui, display.as_ref(), region)?.save(&args[2])?;
            if json_output() {
                println!(
                    "{}",
                    serde_json::json!({"action": "capture", "path": args[2]})
                );
            } else if !quiet() {
                println!("{}", tr!("Saved {}", "已儲存 {}", args[2]));
            }
        }
        "type" => {
            // Up to 32 characters per foreground check; fail-safe runs per chunk.
            for chunk in args::type_chunks(&args[2]) {
                active(&window)?;
                gui.write(chunk, Duration::ZERO)?;
            }
            let typed = args[2].chars().count();
            if json_output() {
                println!(
                    "{}",
                    serde_json::json!({"action": "type", "characters": typed})
                );
            } else if !quiet() {
                println!("{}", tr!("Typed {} characters", "已輸入 {} 個字元", typed));
            }
        }
        "paste" => {
            // Clipboard entry (SPEC §20): one foreground check, then Ctrl+V.
            active(&window)?;
            let restored = gui.paste(&args[2], true)?;
            let pasted = args[2].chars().count();
            if json_output() {
                println!(
                    "{}",
                    serde_json::json!({"action": "paste", "characters": pasted, "clipboard_restored": restored})
                );
            } else if !quiet() {
                println!(
                    "{}",
                    tr!("Pasted {} characters", "已貼上 {} 個字元", pasted)
                );
            }
        }
        "locate" | "click" => {
            let (needle, confidence, timeout) =
                prepared.ok_or(Error::InvalidArgument("missing search options"))?;
            let (found, captured_window) = locate_target(
                &mut gui,
                &window,
                &needle,
                confidence,
                timeout,
                display.as_ref(),
                &|| Ok(()),
            )?;
            let point: Point = autogui::center(found);
            if command == "click" {
                // Check the pixels and target immediately before dispatch. A
                // moving window, replaced control, or lost focus aborts input.
                let fresh = capture_region(&mut gui, display.as_ref(), found)?;
                gui.locate(
                    &needle,
                    &fresh,
                    &LocateOptions {
                        confidence,
                        ..Default::default()
                    },
                )?;
                active(&window)?;
                if window.box_rect()? != captured_window {
                    return Err(Error::InvalidArgument(errors::WINDOW_MOVED));
                }
                if let Some(display) = &display {
                    gui.move_to_display(display, point)?;
                    active(&window)?;
                    display.validate()?;
                    if window.box_rect()? != captured_window || gui.position()? != point {
                        return Err(Error::InvalidArgument(errors::CURSOR_MOVED));
                    }
                    gui.click()?;
                } else {
                    gui.click_xy(point.x, point.y)?;
                }
            }
            if json_output() {
                println!(
                    "{}",
                    serde_json::json!({
                        "action": command,
                        "rect": {"left": found.left, "top": found.top, "width": found.width, "height": found.height},
                        "center": {"x": point.x, "y": point.y},
                        "hwnd": window.hwnd(),
                    })
                );
            } else {
                println!(
                    "{} {:?}, center {:?}, window {}",
                    command,
                    found,
                    point,
                    window.hwnd()
                );
            }
        }
        _ => return Err(Error::InvalidArgument("unsupported command")),
    }
    Ok(())
}

/// Runs the CLI with the process arguments and returns the exit code.
pub fn run_cli() -> i32 {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let (globals, rest) = args::split(&raw);
    QUIET.store(globals.quiet, Ordering::Relaxed);
    JSON.store(globals.json, Ordering::Relaxed);
    match run(rest) {
        Ok(()) => 0,
        Err(error) => errors::report(&error, globals.json),
    }
}

/// Runs the CLI with the process arguments; exits with a non-zero code on error.
pub fn main() {
    let code = run_cli();
    if code != 0 {
        std::process::exit(code);
    }
}
