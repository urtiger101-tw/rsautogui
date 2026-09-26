//! Operate one explicitly selected Windows window. `click` and `type` send real input.
use autogui::{AutoGui, Error, LocateOptions, Point, Rect, Screenshot, Window};
use std::time::{Duration, Instant};

#[cfg(feature = "agent")]
#[path = "support/mcp.rs"]
mod mcp;
#[cfg(feature = "agent")]
#[path = "support/mcp_tools.rs"]
pub(crate) mod mcp_tools;
#[path = "support/screensaver.rs"]
mod screensaver;
#[path = "support/window_command.rs"]
mod window_command;

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

fn unique_window(gui: &mut AutoGui, title: &str) -> autogui::Result<Window> {
    if title.trim().is_empty() {
        return Err(Error::InvalidArgument("window title must not be empty"));
    }
    let windows = gui.get_windows_with_title_ci(title)?;
    if windows.len() != 1 {
        for window in &windows {
            eprintln!("候選視窗 {}: {}", window.hwnd(), window.title()?);
        }
        return if windows.is_empty() {
            Err(Error::WindowNotFound)
        } else {
            Err(Error::InvalidArgument(
                "multiple windows match; supply a more specific title",
            ))
        };
    }
    windows.into_iter().next().ok_or(Error::WindowNotFound)
}

fn active(window: &Window) -> autogui::Result<()> {
    if !window.is_active()? || window.is_minimized()? {
        return Err(Error::PermissionDenied(
            "target window lost foreground focus",
        ));
    }
    Ok(())
}

fn screen_region(gui: &AutoGui, window: &Window) -> autogui::Result<Rect> {
    let size = gui.size()?;
    window
        .box_rect()?
        .intersect(&Rect::new(0, 0, size.width, size.height))
        .ok_or(Error::InvalidArgument(
            "target window is outside the primary monitor",
        ))
}

fn scaled_template(path: &str, scale: f64) -> autogui::Result<Screenshot> {
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

fn locate_target(
    gui: &mut AutoGui,
    window: &Window,
    needle: &Screenshot,
    confidence: Option<f32>,
    timeout: Duration,
    check_cancelled: &dyn Fn() -> autogui::Result<()>,
) -> autogui::Result<(Rect, Rect)> {
    let start = Instant::now();
    gui.set_raise_image_not_found(false);
    loop {
        check_cancelled()?;
        active(window)?;
        let window_rect = window.box_rect()?;
        let region = screen_region(gui, window)?;
        let frame = gui.screenshot_region(region)?;
        let options = LocateOptions {
            confidence,
            ..Default::default()
        };
        let matches = gui.locate_all(needle, &frame, &options)?;
        if window.is_active()? && window.box_rect()? == window_rect {
            if matches.len() > 1 {
                return Err(Error::InvalidArgument(
                    "multiple image matches; use a more distinctive template",
                ));
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

fn run() -> autogui::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!(
            "用法：app_control list [標題]\n      app_control capture <標題> <output.png>\n      app_control locate <標題> <template.png> [confidence|exact] [timeout秒] [scale]\n      app_control click <標題> <template.png> [confidence|exact] [timeout秒] [scale]\n      app_control type <標題> <文字>\n\n標題使用不分大小寫的子字串；必須只符合一個視窗。\n預設 confidence=0.95、timeout=5、scale=1。\ncapture／locate／click／type 會啟用目標視窗；click／type 送出真實輸入。\n把游標移到主螢幕角落可中止。相似度模式需 --features opencv。 "
        );
        println!(
            "\n螢幕保護程式：\n      app_control screensaver status\n      app_control screensaver pause [秒數，預設 900，最多 86400]\n      app_control screensaver resume\n      app_control screensaver stop\n\npause 等待到期、Ctrl+C 或 resume 後還原原設定；不改動密碼或鎖定原則。\nstop 只關閉正在執行的螢幕保護程式，不會解除 Windows 鎖定。"
        );
        println!(
            "\nWin32 控制：app_control window <inspect|activate|minimize|maximize|restore|hide|show|move|resize|close> <標題> [x y|width height]\nAgent：autogui-control mcp（stdio MCP server；需 agent feature）"
        );
        return Ok(());
    }
    #[cfg(feature = "agent")]
    if args[0] == "mcp" && args.len() == 1 {
        return mcp::run(mcp_tools::Session::default()).map_err(Error::Io);
    }
    if args[0] == "window" {
        return window_command::run(&args[1..]);
    }
    let command = args[0].as_str();
    if !["list", "capture", "locate", "click", "type", "screensaver"].contains(&command) {
        return Err(Error::InvalidArgument("unknown command; use --help"));
    }
    if command != "list" && command != "screensaver" && args.len() < 3 {
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
    if command == "list" {
        let windows = if let Some(title) = args.get(1) {
            gui.get_windows_with_title_ci(title)?
        } else {
            gui.get_all_windows()?
        };
        for window in windows {
            match (window.title(), window.box_rect()) {
                (Ok(title), Ok(rect)) => println!("{}\t{:?}\t{}", window.hwnd(), rect, title),
                (Err(Error::WindowNotFound), _) | (_, Err(Error::WindowNotFound)) => {}
                (Err(error), _) | (_, Err(error)) => return Err(error),
            }
        }
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
            gui.screenshot_region(screen_region(&gui, &window)?)?
                .save(&args[2])?;
            println!("已儲存 {}", args[2]);
        }
        "type" => {
            for ch in args[2].chars() {
                active(&window)?;
                gui.write(&ch.to_string(), Duration::ZERO)?;
            }
            println!("已輸入 {} 個字元", args[2].chars().count());
        }
        "locate" | "click" => {
            let (needle, confidence, timeout) =
                prepared.ok_or(Error::InvalidArgument("missing search options"))?;
            let (found, captured_window) =
                locate_target(&mut gui, &window, &needle, confidence, timeout, &|| Ok(()))?;
            let point: Point = autogui::center(found);
            if command == "click" {
                // Check the pixels and target immediately before dispatch. A
                // moving window, replaced control, or lost focus aborts input.
                let fresh = gui.screenshot_region(found)?;
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
                    return Err(Error::InvalidArgument(
                        "target window moved; retry recognition",
                    ));
                }
                gui.click_xy(point.x, point.y)?;
            }
            println!(
                "{} {:?}, center {:?}, window {}",
                command,
                found,
                point,
                window.hwnd()
            );
        }
        _ => return Err(Error::InvalidArgument("unsupported command")),
    }
    Ok(())
}

pub fn main() {
    if let Err(error) = run() {
        eprintln!("操作失敗：{error}");
        std::process::exit(1);
    }
}
