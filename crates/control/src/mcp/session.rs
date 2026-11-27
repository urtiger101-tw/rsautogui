use super::errors::{invalid_arguments, result as error_result};
use super::input::{CaptureProof, drag_path, sleep_sliced, type_chunks};
use crate::cli::{active, capture_region, locate_target, scaled_template, screen_region};
use crate::commands::{display as display_command, session, window as window_command};
use crate::desktop;
use autogui::{
    AutoGui, Display, Error, FakeBackend, LocateOptions, MouseButton, Point, Rect, Result,
    Screenshot, Window,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::BufRead;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

/// A successful result: the object as JSON text (for older clients and
/// people reading transcripts) and as `structuredContent` (matching the
/// tool's `outputSchema`). The server drops `structuredContent` for clients
/// that negotiated a protocol older than 2025-06-18.
fn text(value: Value) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false})
}
fn rect(rect: Rect) -> Value {
    json!({"left":rect.left,"top":rect.top,"width":rect.width,"height":rect.height})
}
fn display_info(display: &Display) -> Value {
    json!({"device_name":display.name(),"friendly_name":display.friendly_name(),"id":display.id(),"rect":rect(display.bounds()),"primary":display.is_primary(),"coordinates":"windows desktop physical pixels"})
}
/// Lossless PNG for MCP captures: zlib level 1 with the Sub filter. On a
/// synthetic 1920x1080 desktop this is ~3.7x faster and ~43% smaller than
/// `write_to` (image 0.25 defaults: fdeflate "fast" + adaptive filtering,
/// whose per-row filter search dominates); see `png_encode_bench`.
fn encode_png(
    width: u32,
    height: u32,
    pixels: &[u8],
    compression: image::codecs::png::CompressionType,
    filter: image::codecs::png::FilterType,
) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(&mut bytes, compression, filter)
        .write_image(pixels, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| Error::Screenshot(e.to_string()))?;
    Ok(bytes)
}
fn png(frame: Screenshot, metadata: Value) -> Result<Value> {
    let expected = u64::from(frame.width) * u64::from(frame.height) * 3;
    if frame.width == 0 || frame.height == 0 || frame.pixels.len() as u64 != expected {
        return Err(Error::InvalidArgument("invalid frame"));
    }
    let bytes = encode_png(
        frame.width,
        frame.height,
        &frame.pixels,
        image::codecs::png::CompressionType::Level(1),
        image::codecs::png::FilterType::Sub,
    )?;
    if bytes.len() > 12 * 1024 * 1024 {
        return Err(Error::InvalidArgument(
            "PNG exceeds 12 MiB; request a smaller region",
        ));
    }
    // Image tools keep the image block; the metadata is also structured.
    Ok(
        json!({"content":[{"type":"text","text":metadata.to_string()},
        {"type":"image","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(bytes)}],
        "structuredContent":metadata,"isError":false}),
    )
}
fn info(window: &Window) -> Result<Value> {
    Ok(
        json!({"hwnd":window.hwnd().to_string(),"title":window.title()?,"rect":rect(window.box_rect()?),
        "active":window.is_active()?,"minimized":window.is_minimized()?,"maximized":window.is_maximized()?}),
    )
}
fn decode<T: for<'a> Deserialize<'a>>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(invalid_arguments)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    title: Option<String>,
    process: Option<String>,
    foreground: Option<bool>,
    titled_only: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowWait {
    title: Option<String>,
    process: Option<String>,
    state: String,
    timeout: Option<f64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppLaunch {
    path: String,
    args: Option<Vec<String>>,
    cwd: Option<String>,
    wait_title: Option<String>,
    timeout: Option<f64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    target: String,
    display: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureRect {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayCapture {
    display: String,
    region: Option<CaptureRect>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayMove {
    target: String,
    display: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    target: String,
    action: String,
    x: Option<i32>,
    y: Option<i32>,
    width: Option<i32>,
    height: Option<i32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    target: String,
    display: Option<String>,
    template: String,
    confidence: Option<f64>,
    exact: Option<bool>,
    timeout: Option<f64>,
    scale: Option<f64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeText {
    target: String,
    text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturedPoint {
    target: Option<String>,
    display: Option<String>,
    capture_id: String,
    x: i32,
    y: i32,
    button: Option<String>,
    clicks: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerMove {
    target: Option<String>,
    display: Option<String>,
    capture_id: String,
    x: i32,
    y: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Drag {
    target: Option<String>,
    display: Option<String>,
    capture_id: String,
    x: i32,
    y: i32,
    to_x: i32,
    to_y: i32,
    button: Option<String>,
    duration_ms: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathPoint {
    x: i32,
    y: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DragPath {
    target: Option<String>,
    display: Option<String>,
    capture_id: String,
    points: Vec<PathPoint>,
    button: Option<String>,
    duration_ms: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scroll {
    target: Option<String>,
    display: Option<String>,
    capture_id: String,
    x: i32,
    y: i32,
    clicks: i32,
    horizontal: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hotkey {
    target: String,
    keys: Vec<String>,
    presses: Option<u32>,
}

/// What a capture-bound pointer call will do once its proof is verified.
enum PointerAction {
    Click(MouseButton, u32),
    Scroll(i32, bool),
    Move,
    Drag(Point, MouseButton, Duration),
    /// Image-relative points until the proof maps each one to desktop pixels.
    Path(Vec<Point>, MouseButton, Duration),
}
struct PointerCall {
    target: Option<String>,
    display: Option<String>,
    capture_id: String,
    x: i32,
    y: i32,
    action: PointerAction,
    /// Drag destination in image-relative pixels.
    to: Option<(i32, i32)>,
}

fn pointer_call(name: &str, args: Value) -> Result<PointerCall> {
    Ok(match name {
        "scroll" => {
            let a: Scroll = decode(args)?;
            if a.clicks == 0 || !(-100..=100).contains(&a.clicks) {
                return Err(Error::InvalidArgument(
                    "scroll clicks must be -100..100 and nonzero",
                ));
            }
            PointerCall {
                target: a.target,
                display: a.display,
                capture_id: a.capture_id,
                x: a.x,
                y: a.y,
                action: PointerAction::Scroll(a.clicks, a.horizontal.unwrap_or(false)),
                to: None,
            }
        }
        "pointer_move" => {
            let a: PointerMove = decode(args)?;
            PointerCall {
                target: a.target,
                display: a.display,
                capture_id: a.capture_id,
                x: a.x,
                y: a.y,
                action: PointerAction::Move,
                to: None,
            }
        }
        "drag" => {
            let a: Drag = decode(args)?;
            let duration = a.duration_ms.unwrap_or(300);
            if duration > 5000 {
                return Err(Error::InvalidArgument("duration_ms must be 0..5000"));
            }
            PointerCall {
                target: a.target,
                display: a.display,
                capture_id: a.capture_id,
                x: a.x,
                y: a.y,
                action: PointerAction::Drag(
                    Point::new(0, 0), // replaced by the mapped destination
                    super::input::button(a.button.as_deref())?,
                    Duration::from_millis(duration),
                ),
                to: Some((a.to_x, a.to_y)),
            }
        }
        "drag_path" => {
            let a: DragPath = decode(args)?;
            if !(2..=256).contains(&a.points.len()) {
                return Err(Error::InvalidArgument("points must contain 2..256 entries"));
            }
            let duration = a.duration_ms.unwrap_or(600);
            if duration > 10000 {
                return Err(Error::InvalidArgument("duration_ms must be 0..10000"));
            }
            let points: Vec<Point> = a.points.iter().map(|p| Point::new(p.x, p.y)).collect();
            let start = points[0];
            PointerCall {
                target: a.target,
                display: a.display,
                capture_id: a.capture_id,
                x: start.x,
                y: start.y,
                action: PointerAction::Path(
                    points,
                    super::input::button(a.button.as_deref())?,
                    Duration::from_millis(duration),
                ),
                to: None,
            }
        }
        _ => {
            let a: CapturedPoint = decode(args)?;
            let clicks = a.clicks.unwrap_or(1);
            if !(1..=3).contains(&clicks) {
                return Err(Error::InvalidArgument("clicks must be 1..3"));
            }
            PointerCall {
                target: a.target,
                display: a.display,
                capture_id: a.capture_id,
                x: a.x,
                y: a.y,
                action: PointerAction::Click(super::input::button(a.button.as_deref())?, clicks),
                to: None,
            }
        }
    })
}

/// Moves a held button from `from` to `to`. The primary display uses one
/// linear tween; an explicit display moves in three validated steps.
fn drag_move(
    gui: &mut AutoGui,
    display: Option<&Display>,
    from: Point,
    to: Point,
    duration: Duration,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    check()?;
    match display {
        None => gui.move_to(to.x, to.y, duration, autogui::tweens::linear)?,
        Some(display) => {
            for point in drag_path(from, to, 3) {
                // The button is held: keep cancellation and the fail-safe live.
                sleep_sliced(duration / 3, || {
                    check()?;
                    failsafe(gui)
                })?;
                gui.move_to_display(display, point)?;
            }
        }
    }
    check()
}

/// Moves a held button through `points[1..]` on an explicit display, one
/// validated display point per segment. The duration is split by segment
/// length, as in `AutoGui::drag_path`.
fn drag_path_display(
    gui: &mut AutoGui,
    display: &Display,
    points: &[Point],
    duration: Duration,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    check()?;
    let durations = super::input::path_durations(points, duration);
    for (dest, segment) in points[1..].iter().zip(durations) {
        // The button is held: keep cancellation and the fail-safe live.
        sleep_sliced(segment, || {
            check()?;
            failsafe(gui)
        })?;
        gui.move_to_display(display, *dest)?;
    }
    check()
}

/// The corner fail-safe on the current cursor position, for waits the
/// library does not perform itself.
fn failsafe(gui: &AutoGui) -> Result<()> {
    let settings = gui.settings();
    if !settings.failsafe {
        return Ok(());
    }
    let position = gui.position()?;
    if settings.failsafe_points.contains(&position) {
        Err(Error::FailSafe(position))
    } else {
        Ok(())
    }
}

/// Rejects unknown key names using a throwaway fake backend, so validation
/// never activates a window or sends input.
fn validate_keys(keys: &[&str]) -> Result<()> {
    let mut fake = AutoGui::with_fake(FakeBackend::new(1, 1));
    fake.settings_mut().failsafe = false;
    fake.settings_mut().pause = Duration::ZERO;
    fake.hotkey(keys)
}

fn window_filter(
    title: Option<String>,
    process: Option<String>,
    foreground: bool,
    titled_only: bool,
) -> Result<desktop::WindowFilter> {
    if title.as_ref().is_some_and(|t| t.chars().count() > 256)
        || process.as_ref().is_some_and(|p| p.chars().count() > 260)
    {
        return Err(Error::InvalidArgument("title/process filter is too long"));
    }
    Ok(desktop::WindowFilter {
        title,
        process,
        foreground,
        titled_only,
    })
}

fn wait_timeout(timeout: Option<f64>, default: f64) -> Result<Duration> {
    let timeout = timeout.unwrap_or(default);
    if !(0.0..=30.0).contains(&timeout) {
        return Err(Error::InvalidArgument("timeout must be 0..30 seconds"));
    }
    Ok(Duration::from_secs_f64(timeout))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Saver {
    action: String,
    seconds: Option<u64>,
}

#[derive(Default)]
pub struct Session {
    gui: Option<AutoGui>,
    targets: HashMap<String, Window>,
    generation: u64,
    displays: HashMap<String, Display>,
    display_generation: u64,
    capture_generation: u64,
    capture: Option<CaptureProof>,
    pause: Option<PauseLease>,
}

impl Session {
    pub(super) fn gui(&mut self) -> Result<&mut AutoGui> {
        if !cfg!(windows) {
            return Err(Error::UnsupportedPlatform);
        }
        if !cfg!(feature = "os") {
            return Err(Error::FeatureDisabled("os"));
        }
        if self.gui.is_none() {
            let mut gui = AutoGui::new()?;
            gui.settings_mut().pause = Duration::from_millis(25);
            self.gui = Some(gui);
        }
        self.gui
            .as_mut()
            .ok_or(Error::Input("backend unavailable".into()))
    }
    pub(super) fn target(&self, key: &str) -> Result<Window> {
        let window = self.targets.get(key).ok_or(Error::InvalidArgument(
            "unknown/stale target; call windows_list again",
        ))?;
        window.title()?; // validates captured HWND plus originating process/thread identity
        Ok(window.clone())
    }
    /// Replaces all window targets with a new generation, like windows_list.
    fn register(
        &mut self,
        windows: Vec<Window>,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Vec<Value>> {
        if windows.len() > 512 {
            return Err(Error::InvalidArgument("over 512 windows; filter by title"));
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(Error::InvalidArgument("restart session"))?;
        self.targets.clear();
        let mut results = Vec::new();
        for (index, window) in windows.into_iter().enumerate() {
            check()?;
            let mut detail = match info(&window) {
                Ok(v) => v,
                Err(Error::WindowNotFound) => continue,
                Err(e) => return Err(e),
            };
            let process = desktop::process_info(window.hwnd());
            detail["pid"] = json!(process.as_ref().map(|p| p.0));
            detail["process"] = json!(
                process
                    .as_ref()
                    .map(|p| { p.1.rsplit(['\\', '/']).next().unwrap_or(&p.1).to_owned() })
            );
            detail["class"] = json!(desktop::class_name(window.hwnd()));
            let key = format!("w{}-{index}", self.generation);
            detail["target"] = json!(key);
            self.targets.insert(key, window);
            results.push(detail);
        }
        Ok(results)
    }
    pub(super) fn display(&self, key: &str) -> Result<Display> {
        let display = self.displays.get(key).ok_or(Error::InvalidArgument(
            "unknown/stale display; call displays_list again",
        ))?;
        display.validate()?;
        Ok(display.clone())
    }
    pub fn call(
        &mut self,
        name: &str,
        args: Value,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Value> {
        // Readiness queries preserve the proof; calls that can change focus,
        // pixels, targets or display references invalidate it even on failure.
        if !matches!(
            name,
            "click_at"
                | "scroll"
                | "pointer_move"
                | "drag"
                | "drag_path"
                | "cursor_position"
                | "session_status"
                | "run"
        ) {
            self.capture = None;
        }
        match name {
            "displays_list" => {
                let _: Empty = decode(args)?;
                let displays = Display::all()?;
                self.display_generation = self
                    .display_generation
                    .checked_add(1)
                    .ok_or(Error::InvalidArgument("restart session"))?;
                self.displays.clear();
                let mut results = Vec::new();
                for (index, display) in displays.into_iter().enumerate() {
                    check()?;
                    let key = format!("d{}-{index}", self.display_generation);
                    let mut detail = display_info(&display);
                    detail["display"] = json!(key);
                    self.displays.insert(key, display);
                    results.push(detail);
                }
                Ok(text(
                    json!({"displays":results,"note":"Physical and IDD virtual monitors share the Windows input desktop. Re-enumerate after changing display topology."}),
                ))
            }
            "session_status" => {
                let _: Empty = decode(args)?;
                check()?;
                let facts = session::gather()?;
                Ok(text(session::to_json(&facts, &session::assess(&facts))))
            }
            "display_capture" => {
                let a: DisplayCapture = decode(args)?;
                let display = self.display(&a.display)?;
                let region = a
                    .region
                    .map(|r| Rect::new(r.left, r.top, r.width, r.height))
                    .unwrap_or(display.bounds());
                if region.width <= 0
                    || region.height <= 0
                    || i64::from(region.width) * i64::from(region.height) > 16_777_216
                {
                    return Err(Error::InvalidArgument(
                        "capture region must be positive and at most 16 megapixels",
                    ));
                }
                check()?;
                let frame = self.gui()?.screenshot_display_region(&display, region)?;
                check()?;
                self.capture_generation = self
                    .capture_generation
                    .checked_add(1)
                    .ok_or(Error::InvalidArgument("restart session"))?;
                let id = format!("c{}", self.capture_generation);
                let taken = Instant::now();
                let output = png(
                    frame.clone(),
                    json!({"display":a.display,"device_name":display.name(),"region":rect(region),"coordinates":"windows desktop physical pixels","capture_id":id,"pointer_coordinates":"image-relative pixels; origin at captured image top-left; use with display (not target)","expires_in_seconds":60,"single_use":true}),
                )?;
                // Screen-level proof: no window, bounds are the captured region.
                self.capture = Some(CaptureProof {
                    id,
                    target: None,
                    display: Some(a.display),
                    bounds: region,
                    region,
                    frame,
                    taken,
                });
                Ok(output)
            }
            "window_to_display" => {
                let a: DisplayMove = decode(args)?;
                let display = self.display(&a.display)?;
                let window = self.target(&a.target)?;
                let previous = info(&window)?;
                check()?;
                display_command::move_window(&window, &display, check)?;
                check()?;
                Ok(text(
                    json!({"target":a.target,"display":a.display,"previous":previous,"current":info(&window)?}),
                ))
            }
            "windows_list" => {
                let a: List = decode(args)?;
                let filter = window_filter(
                    a.title,
                    a.process,
                    a.foreground.unwrap_or(false),
                    a.titled_only.unwrap_or(false),
                )?;
                check()?;
                let windows = desktop::list(self.gui()?, &filter)?;
                let results = self.register(windows, check)?;
                Ok(text(
                    json!({"windows":results,"coordinates":"windows desktop physical pixels; origin at primary top-left"}),
                ))
            }
            "window_wait" => {
                let a: WindowWait = decode(args)?;
                let state = desktop::WaitState::parse(&a.state)?;
                let timeout = wait_timeout(a.timeout, 5.0)?;
                if a.title.as_deref().is_none_or(str::is_empty)
                    && a.process.as_deref().is_none_or(str::is_empty)
                {
                    return Err(Error::InvalidArgument("window_wait needs title or process"));
                }
                let filter = window_filter(a.title, a.process, false, false)?;
                let start = Instant::now();
                let windows = desktop::wait(self.gui()?, &filter, state, timeout, check)?;
                let results = self.register(windows, check)?;
                Ok(text(
                    json!({"state":a.state,"elapsed_ms":start.elapsed().as_millis() as u64,"windows":results,"note":"targets refreshed; previous targets are invalid"}),
                ))
            }
            "app_launch" => {
                let a: AppLaunch = decode(args)?;
                let timeout = wait_timeout(a.timeout, 10.0)?;
                if a.wait_title.as_deref().is_some_and(|t| t.trim().is_empty()) {
                    return Err(Error::InvalidArgument("wait_title must not be empty"));
                }
                let launch = desktop::Launch {
                    path: a.path,
                    args: a.args.unwrap_or_default(),
                    cwd: a.cwd,
                };
                check()?;
                let pid = desktop::launch(&launch)?;
                let Some(title) = a.wait_title else {
                    return Ok(text(
                        json!({"pid":pid,"note":"process started; use window_wait or windows_list to find its window"}),
                    ));
                };
                let filter = window_filter(Some(title), None, false, false)?;
                let windows = desktop::wait(
                    self.gui()?,
                    &filter,
                    desktop::WaitState::Present,
                    timeout,
                    check,
                )
                // Keep the variant/kind so describe() adds the matching hint once.
                .map_err(|e| match e {
                    Error::Io(e) => Error::Io(std::io::Error::new(
                        e.kind(),
                        format!("process {pid} started, but {e}"),
                    )),
                    e => e,
                })?;
                let results = self.register(windows, check)?;
                Ok(text(
                    json!({"pid":pid,"windows":results,"note":"targets refreshed; window may belong to a different process than pid"}),
                ))
            }
            "cursor_position" => {
                let _: Empty = decode(args)?;
                check()?;
                let point = self.gui()?.position()?;
                Ok(text(
                    json!({"x":point.x,"y":point.y,"coordinates":"windows desktop physical pixels; origin at primary top-left"}),
                ))
            }
            "window_control" => {
                let a: Control = decode(args)?;
                let pair = match a.action.as_str() {
                    "move" if a.width.is_none() && a.height.is_none() => Some((
                        a.x.ok_or(Error::InvalidArgument("x required"))?,
                        a.y.ok_or(Error::InvalidArgument("y required"))?,
                    )),
                    "resize" if a.x.is_none() && a.y.is_none() => Some((
                        a.width.ok_or(Error::InvalidArgument("width required"))?,
                        a.height.ok_or(Error::InvalidArgument("height required"))?,
                    )),
                    _ if a.x.is_none()
                        && a.y.is_none()
                        && a.width.is_none()
                        && a.height.is_none() =>
                    {
                        None
                    }
                    _ => return Err(Error::InvalidArgument("coordinates do not match action")),
                };
                let window = self.target(&a.target)?;
                check()?;
                window_command::apply(&window, &a.action, pair)?;
                if a.action == "close" {
                    Ok(text(
                        json!({"close_requested":true,"note":"WM_CLOSE posted; app may request confirmation or remain open"}),
                    ))
                } else {
                    Ok(text(info(&window)?))
                }
            }
            "capture" => {
                let a: Target = decode(args)?;
                let display = a
                    .display
                    .as_deref()
                    .map(|key| self.display(key))
                    .transpose()?;
                let window = self.target(&a.target)?;
                check()?;
                window.activate()?;
                active(&window)?;
                let bounds = window.box_rect()?;
                check()?;
                let region = screen_region(self.gui()?, &window, display.as_ref())?;
                if i64::from(region.width) * i64::from(region.height) > 16_777_216 {
                    return Err(Error::InvalidArgument(
                        "capture exceeds 16 megapixels; resize window",
                    ));
                }
                let frame = capture_region(self.gui()?, display.as_ref(), region)?;
                active(&window)?;
                if bounds != window.box_rect()? {
                    return Err(Error::InvalidArgument("window moved during capture; retry"));
                }
                check()?;
                self.capture_generation = self
                    .capture_generation
                    .checked_add(1)
                    .ok_or(Error::InvalidArgument("restart session"))?;
                let id = format!("c{}", self.capture_generation);
                let taken = Instant::now();
                let output = png(
                    frame.clone(),
                    json!({"target":a.target,"display":a.display,"region":rect(region),"coordinates":"windows desktop physical pixels","capture_id":id,"pointer_coordinates":"image-relative pixels; origin at captured image top-left","expires_in_seconds":60,"single_use":true}),
                )?;
                self.capture = Some(CaptureProof {
                    id,
                    target: Some(a.target),
                    display: a.display,
                    bounds,
                    region,
                    frame,
                    taken,
                });
                Ok(output)
            }
            "click_at" | "scroll" | "pointer_move" | "drag" | "drag_path" => {
                let proof = self.capture.take().ok_or(Error::InvalidArgument(
                    "no unused capture; call capture or display_capture first",
                ))?;
                let mut a = pointer_call(name, args)?;
                let (target, display_key) = (a.target.as_deref(), a.display.as_deref());
                let point = proof.point(target, display_key, &a.capture_id, a.x, a.y)?;
                if let (PointerAction::Drag(to, _, _), Some((x, y))) = (&mut a.action, a.to) {
                    *to = proof.point(target, display_key, &a.capture_id, x, y)?;
                }
                if let PointerAction::Path(points, _, _) = &mut a.action {
                    for p in points.iter_mut() {
                        *p = proof.point(target, display_key, &a.capture_id, p.x, p.y)?;
                    }
                }
                let window = proof
                    .target
                    .as_deref()
                    .map(|key| self.target(key))
                    .transpose()?;
                let display = proof
                    .display
                    .as_deref()
                    .map(|key| self.display(key))
                    .transpose()?;
                let gui = self.gui()?;
                proof.verify(gui, window.as_ref(), display.as_ref(), point, check)?;
                if let PointerAction::Drag(to, _, _) = a.action {
                    proof.verify(gui, window.as_ref(), display.as_ref(), to, check)?;
                }
                let path_end = match &a.action {
                    PointerAction::Path(points, _, _) => points.last().copied(),
                    _ => None,
                };
                if let Some(last) = path_end {
                    proof.verify(gui, window.as_ref(), display.as_ref(), last, check)?;
                }
                if let Some(display) = &display {
                    gui.move_to_display(display, point)?;
                } else {
                    gui.move_to_xy(point.x, point.y)?;
                }
                proof.verify(gui, window.as_ref(), display.as_ref(), point, check)?;
                if gui.position()? != point {
                    return Err(Error::InvalidArgument(
                        "cursor moved before input; capture again",
                    ));
                }
                let mut detail = json!({"target":a.target,"display":a.display,"capture_id":a.capture_id,"x":point.x,"y":point.y,"note":"input sent; capture again to verify and obtain a new single-use proof"});
                match a.action {
                    PointerAction::Scroll(clicks, horizontal) => {
                        if horizontal {
                            gui.hscroll(clicks)?;
                        } else {
                            gui.scroll(clicks)?;
                        }
                        detail["scroll_clicks"] = json!(clicks);
                        detail["horizontal"] = json!(horizontal);
                    }
                    PointerAction::Click(button, clicks) => {
                        gui.click_ex(None, None, clicks, Duration::from_millis(50), button)?;
                        detail["clicked"] = json!(true);
                        detail["button"] = json!(format!("{button:?}").to_lowercase());
                        detail["clicks"] = json!(clicks);
                    }
                    PointerAction::Move => {
                        detail["moved"] = json!(true);
                        detail["note"] = json!("cursor moved; no button or wheel input sent");
                    }
                    PointerAction::Drag(to, button, duration) => {
                        check()?;
                        if let Some(window) = &window {
                            active(window)?;
                        }
                        gui.mouse_down(button)?;
                        // The button is released on every path, including errors.
                        let moved = drag_move(gui, display.as_ref(), point, to, duration, check);
                        let released = gui.mouse_up(button);
                        moved?;
                        released?;
                        detail["dragged"] = json!(true);
                        detail["to"] = json!({"x":to.x,"y":to.y});
                        detail["button"] = json!(format!("{button:?}").to_lowercase());
                    }
                    PointerAction::Path(points, button, duration) => {
                        check()?;
                        if let Some(window) = &window {
                            active(window)?;
                        }
                        match &display {
                            // The library holds the button, releases it on every
                            // path and keeps the fail-safe on each segment.
                            None => gui.drag_path(&points, duration, button)?,
                            Some(display) => {
                                gui.mouse_down(button)?;
                                // The button is released on every path, including errors.
                                let moved =
                                    drag_path_display(gui, display, &points, duration, check);
                                let released = gui.mouse_up(button);
                                moved?;
                                released?;
                            }
                        }
                        let to = points.last().copied().unwrap_or(point);
                        detail["dragged"] = json!(true);
                        detail["points_sent"] = json!(points.len());
                        detail["to"] = json!({"x":to.x,"y":to.y});
                        detail["button"] = json!(format!("{button:?}").to_lowercase());
                    }
                }
                Ok(text(detail))
            }
            "locate" | "click_image" => {
                let a: Search = decode(args)?;
                let display = a
                    .display
                    .as_deref()
                    .map(|key| self.display(key))
                    .transpose()?;
                let timeout = a.timeout.unwrap_or(5.0);
                if !(0.0..=30.0).contains(&timeout) {
                    return Err(Error::InvalidArgument("timeout must be 0..30 seconds"));
                }
                let confidence = if a.exact.unwrap_or(false) {
                    if a.confidence.is_some() {
                        return Err(Error::InvalidArgument(
                            "exact and confidence cannot be combined",
                        ));
                    }
                    None
                } else {
                    let value = a.confidence.unwrap_or(0.95);
                    if !(0.0..=1.0).contains(&value) {
                        return Err(Error::InvalidArgument("confidence must be 0..1"));
                    }
                    if !cfg!(feature = "opencv") {
                        return Err(Error::FeatureDisabled("opencv"));
                    }
                    Some(value as f32)
                };
                if !std::path::Path::new(&a.template).is_absolute() {
                    return Err(Error::InvalidArgument(
                        "template must be an absolute local image path",
                    ));
                }
                if std::fs::metadata(&a.template)?.len() > 32 * 1024 * 1024 {
                    return Err(Error::InvalidArgument("template file exceeds 32 MiB"));
                }
                let needle = scaled_template(&a.template, a.scale.unwrap_or(1.0))?;
                if u64::from(needle.width) * u64::from(needle.height) > 4_194_304 {
                    return Err(Error::InvalidArgument("template exceeds 4 megapixels"));
                }
                let window = self.target(&a.target)?;
                check()?;
                window.activate()?;
                let gui = self.gui()?;
                let (found, bounds) = locate_target(
                    gui,
                    &window,
                    &needle,
                    confidence,
                    Duration::from_secs_f64(timeout),
                    display.as_ref(),
                    check,
                )?;
                let point = autogui::center(found);
                if name == "click_image" {
                    let frame = capture_region(gui, display.as_ref(), found)?;
                    gui.locate(
                        &needle,
                        &frame,
                        &LocateOptions {
                            confidence,
                            ..Default::default()
                        },
                    )?;
                    check()?;
                    active(&window)?;
                    if window.box_rect()? != bounds {
                        return Err(Error::InvalidArgument("window moved; retry recognition"));
                    }
                    if let Some(display) = &display {
                        gui.move_to_display(display, point)?;
                        check()?;
                        active(&window)?;
                        display.validate()?;
                        if window.box_rect()? != bounds || gui.position()? != point {
                            return Err(Error::InvalidArgument(
                                "target or cursor moved before click; retry recognition",
                            ));
                        }
                        gui.click()?;
                    } else {
                        gui.click_xy(point.x, point.y)?;
                    }
                }
                Ok(text(
                    json!({"rect":rect(found),"center":{"x":point.x,"y":point.y},"clicked":name=="click_image","display":a.display,"coordinates":"windows desktop physical pixels"}),
                ))
            }
            "type_text" => {
                let a: TypeText = decode(args)?;
                let count = a.text.chars().count();
                if count > 4096 {
                    return Err(Error::InvalidArgument(
                        "text exceeds 4096 characters; send smaller chunks",
                    ));
                }
                let window = self.target(&a.target)?;
                check()?;
                window.activate()?;
                type_chunks(&a.text, |chunk| {
                    check()?;
                    active(&window)?;
                    self.gui()?.write(chunk, Duration::ZERO)
                })?;
                Ok(text(
                    json!({"characters_sent":count,"note":"input sent; capture/inspect the app to confirm its result"}),
                ))
            }
            "hotkey" => {
                let a: Hotkey = decode(args)?;
                if a.keys.is_empty()
                    || a.keys.len() > 8
                    || a.keys.iter().any(|k| k.is_empty() || k.len() > 32)
                {
                    return Err(Error::InvalidArgument(
                        "use 1..8 canonical keys, e.g. ctrl,a or enter",
                    ));
                }
                let presses = a.presses.unwrap_or(1);
                if !(1..=50).contains(&presses) {
                    return Err(Error::InvalidArgument("presses must be 1..50"));
                }
                let keys: Vec<&str> = a.keys.iter().map(String::as_str).collect();
                // Unknown key names fail here, before focus changes.
                validate_keys(&keys)?;
                let window = self.target(&a.target)?;
                check()?;
                window.activate()?;
                for _ in 0..presses {
                    active(&window)?;
                    check()?;
                    self.gui()?.hotkey(&keys)?;
                }
                Ok(text(json!({"keys_sent":a.keys,"presses":presses})))
            }
            "screensaver" => {
                let a: Saver = decode(args)?;
                if a.action == "pause" {
                    let seconds = a.seconds.unwrap_or(900);
                    if !(1..=86400).contains(&seconds) {
                        return Err(Error::InvalidArgument("seconds must be 1..86400"));
                    }
                    if let Some(pause) = self.pause.as_mut()
                        && pause.child.try_wait()?.is_none()
                    {
                        return Err(Error::InvalidArgument(
                            "this session already owns a pause; resume first",
                        ));
                    }
                    self.pause = None;
                    check()?;
                    let pause = PauseLease::start(seconds)?;
                    check()?; // Cancellation during startup drops the lease and restores settings.
                    self.pause = Some(pause);
                    Ok(text(
                        json!({"paused":true,"seconds":seconds,"restore":"timer, resume, or MCP disconnection"}),
                    ))
                } else {
                    if a.seconds.is_some()
                        || !["status", "resume", "stop"].contains(&a.action.as_str())
                    {
                        return Err(Error::InvalidArgument(
                            "screensaver status|pause [seconds]|resume|stop",
                        ));
                    }
                    check()?;
                    let output = child_command()?.args(["screensaver", &a.action]).output()?;
                    if !output.status.success() {
                        return Err(Error::Input(
                            String::from_utf8_lossy(&output.stderr).trim().to_string(),
                        ));
                    }
                    if a.action == "resume" {
                        self.pause = None;
                    }
                    Ok(text(
                        json!({"action":a.action,"status_text":String::from_utf8_lossy(&output.stdout).trim()}),
                    ))
                }
            }
            "ui_snapshot" | "ui_click" | "ui_set_value" | "ui_wait" => {
                super::ui::call(self, name, args, check)
            }
            "wait_change" => super::change::call(self, name, args, check),
            "run" => super::batch::call(self, name, args, check),
            "paste_text" => super::paste::call(self, name, args, check),
            _ => Err(Error::InvalidArgument("unknown tool")),
        }
    }
}

impl autogui_protocol::server::Handler for Session {
    fn call(
        &mut self,
        name: &str,
        args: Value,
        check: &dyn Fn() -> std::io::Result<()>,
    ) -> std::io::Result<Value> {
        // Tool failures are results (isError), not transport errors.
        Ok(self
            .call(name, args, &|| check().map_err(Error::Io))
            .unwrap_or_else(|e| error_result(&e)))
    }
}

fn child_command() -> Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    Ok(command)
}

struct PauseLease {
    child: Child,
    input: Option<ChildStdin>,
}
impl PauseLease {
    fn start(seconds: u64) -> Result<Self> {
        let mut child = child_command()?
            .args(["screensaver", "pause", &seconds.to_string(), "--managed"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child.stdin.take();
        let output = child
            .stdout
            .take()
            .ok_or(Error::Input("pause stdout unavailable".into()))?;
        let mut lease = Self { child, input };
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut lines = std::io::BufReader::new(output).lines();
            let ready = lines
                .next()
                .is_some_and(|line| line.is_ok_and(|s| s == "AUTOGUI_PAUSE_READY"));
            let _ = send.send(ready);
            for line in lines {
                if let Err(e) = line {
                    eprintln!("pause helper output: {e}");
                    break;
                }
            }
        });
        if receive
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or(false)
            && lease.child.try_wait()?.is_none()
        {
            Ok(lease)
        } else {
            Err(Error::Input(
                "pause helper did not start; see stderr and screensaver status".into(),
            ))
        }
    }
}
impl Drop for PauseLease {
    fn drop(&mut self) {
        drop(self.input.take());
        let start = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        eprintln!(
                            "pause helper failed; run screensaver resume after unlocking Windows"
                        );
                    }
                    break;
                }
                Ok(None) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                _ => {
                    eprintln!("pause helper is still restoring; recovery journal is preserved");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::errors::{describe, report};

    fn session() -> Session {
        Session {
            capture: Some(CaptureProof {
                id: "c1".into(),
                target: Some("w1-0".into()),
                display: None,
                bounds: Rect::new(10, 10, 20, 20),
                region: Rect::new(10, 10, 20, 20),
                frame: Screenshot::solid(20, 20, autogui::Rgb::new(1, 2, 3)),
                taken: Instant::now(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn invalid_pointer_attempts_consume_latest_capture_without_input() {
        for (name, args) in [
            (
                "click_at",
                json!({"target":"w2-0","capture_id":"c1","x":1,"y":1}),
            ),
            (
                "click_at",
                json!({"target":"w1-0","capture_id":"c0","x":1,"y":1}),
            ),
            (
                "click_at",
                json!({"target":"w1-0","capture_id":"c1","x":20,"y":1}),
            ),
            (
                "click_at",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"extra":true}),
            ),
            (
                "scroll",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"clicks":0}),
            ),
            (
                "scroll",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"clicks":101}),
            ),
            (
                "click_at",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"button":"back"}),
            ),
            (
                "click_at",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"clicks":0}),
            ),
            (
                "click_at",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"clicks":4}),
            ),
            (
                "click_at",
                json!({"display":"d1-0","capture_id":"c1","x":1,"y":1}),
            ),
            (
                "click_at",
                json!({"target":"w1-0","display":"d1-0","capture_id":"c1","x":1,"y":1}),
            ),
            ("click_at", json!({"capture_id":"c1","x":1,"y":1})),
            (
                "pointer_move",
                json!({"target":"w1-0","capture_id":"c1","x":20,"y":1}),
            ),
            (
                "pointer_move",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"button":"left"}),
            ),
            (
                "drag",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"to_x":20,"to_y":1}),
            ),
            (
                "drag",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"to_x":1,"to_y":-1}),
            ),
            (
                "drag",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"to_x":2,"to_y":2,"duration_ms":5001}),
            ),
            (
                "drag",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"to_x":2,"to_y":2,"button":"x"}),
            ),
            // Valid arguments, but the target was never listed in this session.
            (
                "drag",
                json!({"target":"w1-0","capture_id":"c1","x":1,"y":1,"to_x":2,"to_y":2}),
            ),
        ] {
            let mut s = session();
            assert!(s.call(name, args.clone(), &|| Ok(())).is_err());
            assert!(s.capture.is_none());
            assert!(s.gui.is_none());
            assert!(matches!(
                s.call(name, args, &|| Ok(())),
                Err(Error::InvalidArgument(_))
            ));
        }
    }

    #[test]
    fn other_tool_calls_invalidate_capture_even_if_arguments_fail() {
        for name in [
            "windows_list",
            "displays_list",
            "capture",
            "display_capture",
            "window_control",
            "window_to_display",
            "locate",
            "click_image",
            "type_text",
            "paste_text",
            "hotkey",
            "window_wait",
            "app_launch",
        ] {
            let mut s = session();
            assert!(s.call(name, json!({"extra":true}), &|| Ok(())).is_err());
            assert!(s.capture.is_none());
            assert!(s.gui.is_none());
        }
    }

    #[test]
    fn drag_path_and_paste_reject_bad_arguments_without_input() {
        let many: Vec<Value> = (0..257).map(|i| json!({"x":i%10+1,"y":1})).collect();
        for (name, args) in [
            (
                "drag_path",
                json!({"target":"w1-0","capture_id":"c1","points":[{"x":1,"y":1}]}),
            ),
            (
                "drag_path",
                json!({"target":"w1-0","capture_id":"c1","points":many}),
            ),
            (
                "drag_path",
                json!({"target":"w1-0","capture_id":"c1","points":[{"x":1,"y":1},{"x":2,"y":2,"z":3}]}),
            ),
            (
                "drag_path",
                json!({"target":"w1-0","capture_id":"c1","points":[{"x":1,"y":1},{"x":2,"y":2}],"duration_ms":10001}),
            ),
            (
                "drag_path",
                json!({"target":"w1-0","capture_id":"c1","points":[{"x":1,"y":1},{"x":2,"y":2}],"button":"x"}),
            ),
            // Valid arguments, but the target was never listed in this session.
            (
                "drag_path",
                json!({"target":"w1-0","capture_id":"c1","points":[{"x":1,"y":1},{"x":2,"y":2}]}),
            ),
            (
                "paste_text",
                json!({"target":"w1-0","text":"a".repeat(20_001)}),
            ),
            (
                "paste_text",
                json!({"target":"w1-0","text":"hi","extra":true}),
            ),
            // Valid arguments, but the target was never listed in this session.
            ("paste_text", json!({"target":"w1-0","text":"hi"})),
        ] {
            let mut s = session();
            assert!(s.call(name, args, &|| Ok(())).is_err(), "{name} accepted");
            assert!(s.capture.is_none());
            assert!(s.gui.is_none());
        }
    }

    #[test]
    fn readiness_queries_preserve_the_proof() {
        for name in ["cursor_position", "session_status"] {
            let mut s = session();
            // Outcome depends on the desktop; the proof must survive either way.
            let _ = s.call(name, json!({}), &|| Ok(()));
            assert!(s.capture.is_some(), "{name} consumed the proof");
            let mut s = session();
            assert!(s.call(name, json!({"extra":true}), &|| Ok(())).is_err());
            assert!(s.capture.is_some());
        }
    }

    fn display_session() -> Session {
        let mut s = session();
        if let Some(proof) = s.capture.as_mut() {
            proof.target = None;
            proof.display = Some("d1-0".into());
        }
        s
    }

    #[test]
    fn display_proof_requires_matching_display_key() {
        for args in [
            json!({"display":"d1-1","capture_id":"c1","x":1,"y":1}),
            json!({"target":"w1-0","capture_id":"c1","x":1,"y":1}),
            json!({"display":"d1-0","target":"w1-0","capture_id":"c1","x":1,"y":1}),
            // Matching key, but no display was listed in this session.
            json!({"display":"d1-0","capture_id":"c1","x":1,"y":1}),
        ] {
            let mut s = display_session();
            assert!(matches!(
                s.call("click_at", args, &|| Ok(())),
                Err(Error::InvalidArgument(_))
            ));
            assert!(s.capture.is_none());
            assert!(s.gui.is_none());
        }
    }

    #[test]
    fn hotkey_validates_keys_and_presses_before_focus() {
        let mut s = session();
        let unknown = s.call(
            "hotkey",
            json!({"target":"w1-0","keys":["ctrl","notakey"]}),
            &|| Ok(()),
        );
        assert!(matches!(
            unknown,
            Err(Error::InvalidArgument("unknown key"))
        ));
        assert!(s.gui.is_none());
        for presses in [0, 51] {
            let mut s = session();
            let result = s.call(
                "hotkey",
                json!({"target":"w1-0","keys":["enter"],"presses":presses}),
                &|| Ok(()),
            );
            assert!(matches!(
                result,
                Err(Error::InvalidArgument("presses must be 1..50"))
            ));
            assert!(s.gui.is_none());
        }
        // Valid keys pass validation and reach the (unlisted) target lookup.
        let mut s = session();
        let stale = s.call(
            "hotkey",
            json!({"target":"w1-0","keys":["ctrl","a"],"presses":50}),
            &|| Ok(()),
        );
        assert!(matches!(stale, Err(Error::InvalidArgument(m)) if m.contains("stale target")));
        assert!(validate_keys(&["ctrl", "shift", "esc"]).is_ok());
    }

    #[test]
    fn window_wait_and_launch_reject_bad_arguments_without_backend() {
        for (name, args) in [
            ("window_wait", json!({"title":"x","state":"gone"})),
            (
                "window_wait",
                json!({"title":"x","state":"present","timeout":31}),
            ),
            ("window_wait", json!({"state":"present"})),
            (
                "window_wait",
                json!({"title":"","process":"","state":"absent"}),
            ),
            ("windows_list", json!({"process":"x".repeat(261)})),
            ("app_launch", json!({"path":"C:\\x.exe","timeout":-1})),
            ("app_launch", json!({"path":"C:\\x.exe","wait_title":" "})),
            (
                "app_launch",
                json!({"path":"C:\\x.exe","args":"not-an-array"}),
            ),
        ] {
            let mut s = session();
            assert!(
                matches!(
                    s.call(name, args.clone(), &|| Ok(())),
                    Err(Error::InvalidArgument(_)) | Err(Error::Io(_))
                ),
                "{name} {args}"
            );
            assert!(s.gui.is_none());
        }
        let mut s = Session::default();
        let result = s.call("app_launch", json!({"path":"notepad.exe"}), &|| Ok(()));
        if desktop::launch_allowed() {
            assert!(matches!(result, Err(Error::InvalidArgument(_))));
        } else {
            assert!(
                matches!(result, Err(Error::PermissionDenied(m)) if m.contains("AUTOGUI_ALLOW_LAUNCH"))
            );
        }
        assert!(s.gui.is_none());
    }

    #[test]
    fn cancellation_stops_windows_list_before_backend() {
        let mut s = session();
        let cancelled = || Err(crate::mcp::errors::cancelled());
        let error = s
            .call("windows_list", json!({"title":"x"}), &cancelled)
            .unwrap_err();
        assert_eq!(report(&error).code, autogui_protocol::ErrorCode::Cancelled);
        assert!(s.gui.is_none());
    }

    #[test]
    fn describe_adds_recovery_hints_per_variant() {
        let cases: Vec<(Error, &str)> = vec![
            (Error::WindowNotFound, "windows_list"),
            (
                Error::PermissionDenied("SetForegroundWindow"),
                "display_capture",
            ),
            (
                Error::PermissionDenied("target window lost foreground focus"),
                "capture",
            ),
            (
                Error::PermissionDenied("app_launch disabled; set AUTOGUI_ALLOW_LAUNCH=1"),
                "AUTOGUI_ALLOW_LAUNCH=1 to allow",
            ),
            (Error::PermissionDenied("other"), "integrity"),
            (Error::FailSafe(Point::new(0, 0)), "corner"),
            (Error::ImageNotFound, "scale"),
            (Error::UnsupportedPlatform, "Windows"),
            (Error::FeatureDisabled("opencv"), "feature"),
            (Error::Input("enigo".into()), "session_status"),
            (Error::Screenshot("dxgi".into()), "session_status"),
            (
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out waiting for window state",
                )),
                "raise timeout",
            ),
        ];
        for (error, hint) in cases {
            let text = describe(&error);
            assert!(text.starts_with(&error.to_string()), "{text}");
            assert!(text.contains("Hint: "), "{text}");
            assert!(text.contains(hint), "{text}");
        }
        // Typed cancellation: the clean inner message, then the hint.
        assert_eq!(
            describe(&crate::mcp::errors::cancelled()),
            "request cancelled or client disconnected. Hint: request was cancelled or the client disconnected; no further input was sent"
        );
        for error in [
            Error::InvalidArgument("capture expired; capture again"),
            Error::Io(std::io::Error::other("disk")),
        ] {
            assert_eq!(describe(&error), error.to_string());
        }
    }

    #[test]
    fn errors_carry_stable_codes_and_retryability() {
        let cases: Vec<(Error, &str, bool)> = vec![
            (Error::WindowNotFound, "window_not_found", false),
            (Error::ImageNotFound, "image_not_found", true),
            (Error::FailSafe(Point::new(0, 0)), "fail_safe", true),
            (
                Error::PermissionDenied("SetForegroundWindow"),
                "permission_denied",
                true,
            ),
            (
                Error::PermissionDenied("app_launch disabled; set AUTOGUI_ALLOW_LAUNCH=1"),
                "permission_denied",
                false,
            ),
            (
                Error::InvalidArgument("x required"),
                "invalid_argument",
                false,
            ),
            (
                Error::InvalidArgument("window moved during capture; retry"),
                "invalid_argument",
                true,
            ),
            (
                crate::mcp::errors::invalid_arguments("unknown field"),
                "invalid_argument",
                false,
            ),
            (Error::Input("enigo".into()), "input_failed", true),
            (Error::Screenshot("dxgi".into()), "capture_failed", true),
            (Error::UnsupportedPlatform, "unsupported", false),
            (Error::FeatureDisabled("opencv"), "unsupported", false),
            (
                Error::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "late")),
                "timeout",
                true,
            ),
            (crate::mcp::errors::cancelled(), "cancelled", true),
            // Wording alone never means cancellation.
            (
                Error::Input("request cancelled or client disconnected".into()),
                "input_failed",
                true,
            ),
            (
                Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone")),
                "not_found",
                false,
            ),
            (Error::Io(std::io::Error::other("disk")), "internal", false),
        ];
        for (error, code, retryable) in cases {
            let r = crate::mcp::errors::result(&error);
            assert_eq!(r["isError"], true);
            let s = &r["structuredContent"];
            assert_eq!(s["code"], code, "{error}");
            assert_eq!(s["retryable"], retryable, "{error}");
            // Typed I/O markers report their inner text without "io: ".
            let message = error.to_string();
            let expected = match &error {
                Error::Io(e)
                    if autogui_protocol::is_cancellation(e)
                        || e.kind() == std::io::ErrorKind::InvalidInput =>
                {
                    e.to_string()
                }
                _ => message,
            };
            assert_eq!(s["message"], expected);
            assert_eq!(r["content"][0]["text"], describe(&error));
            assert_eq!(s["hint"].is_string(), !report(&error).hint.is_empty());
        }
    }

    #[test]
    fn handler_returns_tool_errors_as_results() {
        let mut s = Session::default();
        let r = autogui_protocol::Handler::call(
            &mut s,
            "type_text",
            json!({"target":"never","text":"x","extra":1}),
            &|| Ok(()),
        )
        .unwrap();
        assert_eq!(r["isError"], true);
        assert_eq!(r["structuredContent"]["code"], "invalid_argument");
        assert!(s.gui.is_none());
    }

    #[test]
    fn argument_decode_errors_are_clean_invalid_arguments() {
        let mut s = Session::default();
        let error = s
            .call("windows_list", json!({"bogus":1}), &|| Ok(()))
            .unwrap_err();
        let e = report(&error);
        assert_eq!(e.code, autogui_protocol::ErrorCode::InvalidArgument);
        assert!(!e.retryable);
        assert!(
            e.message
                .starts_with("invalid tool arguments: unknown field"),
            "{}",
            e.message
        );
        assert!(!e.message.contains("input backend") && !e.message.contains("io:"));
        assert!(e.hint.contains("inputSchema"));
        assert!(s.gui.is_none());
    }

    /// A synthetic 1920x1080 desktop: flat panels, a gradient title bar,
    /// dense "text" rows and a noisy photo-like quarter, roughly like real
    /// screen content.
    fn synthetic_frame(width: u32, height: u32) -> Screenshot {
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        let mut seed = 0x2545_f491_u32;
        for y in 0..height {
            for x in 0..width {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 28) as u8;
                let px = if x >= width / 2 && y >= height / 2 {
                    [
                        ((x * 200 / width) as u8).wrapping_add(noise),
                        ((y * 200 / height) as u8).wrapping_add(noise),
                        120u8.wrapping_add(noise),
                    ]
                } else if y < 32 {
                    [(x * 255 / width) as u8, 80, 160]
                } else if (y / 18) % 3 == 0 && (x / 7 + y) % 5 < 2 && x % 600 < 520 {
                    [20, 20, 20]
                } else if x < 280 {
                    [243, 243, 243]
                } else {
                    [255, 255, 255]
                };
                pixels.extend_from_slice(&px);
            }
        }
        Screenshot {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn png_result_is_lossless_and_structured() {
        let frame = synthetic_frame(64, 48);
        let r = png(frame.clone(), json!({"capture_id":"c1"})).unwrap();
        assert_eq!(r["structuredContent"], json!({"capture_id":"c1"}));
        assert_eq!(r["content"][1]["mimeType"], "image/png");
        let data = base64::engine::general_purpose::STANDARD
            .decode(r["content"][1]["data"].as_str().unwrap())
            .unwrap();
        let decoded = image::load_from_memory(&data).unwrap().to_rgb8();
        assert_eq!(decoded.into_raw(), frame.pixels);
        let mut bad = frame;
        bad.pixels.pop();
        assert!(png(bad, json!({})).is_err());
    }

    /// cargo test --release -p autogui-control-core png_encode_bench -- --ignored --nocapture
    #[test]
    #[ignore = "benchmark; run in release"]
    fn png_encode_bench() {
        use image::codecs::png::{CompressionType, FilterType};
        let frame = synthetic_frame(1920, 1080);
        let time = |label: &str, encode: &dyn Fn() -> usize| {
            let start = Instant::now();
            let mut bytes = 0;
            for _ in 0..5 {
                bytes = encode();
            }
            println!(
                "{label}: {:.1} ms, {bytes} bytes",
                start.elapsed().as_secs_f64() * 1000.0 / 5.0
            );
        };
        time("write_to (previous)", &|| {
            let rgb =
                image::RgbImage::from_raw(frame.width, frame.height, frame.pixels.clone()).unwrap();
            let mut bytes = std::io::Cursor::new(Vec::new());
            rgb.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
            bytes.into_inner().len()
        });
        for (label, level, filter) in [
            ("fast/adaptive", CompressionType::Fast, FilterType::Adaptive),
            ("fast/sub", CompressionType::Fast, FilterType::Sub),
            (
                "level1/sub (used)",
                CompressionType::Level(1),
                FilterType::Sub,
            ),
            (
                "level1/adaptive",
                CompressionType::Level(1),
                FilterType::Adaptive,
            ),
            (
                "default/adaptive",
                CompressionType::Default,
                FilterType::Adaptive,
            ),
        ] {
            time(label, &|| {
                encode_png(frame.width, frame.height, &frame.pixels, level, filter)
                    .unwrap()
                    .len()
            });
        }
    }
}
