use super::{active, locate_target, scaled_template, screen_region, window_command};
use autogui::{AutoGui, Error, LocateOptions, Rect, Result, Window};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::BufRead;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

fn text(value: Value) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
}
fn rect(rect: Rect) -> Value {
    json!({"left":rect.left,"top":rect.top,"width":rect.width,"height":rect.height})
}
fn info(window: &Window) -> Result<Value> {
    Ok(
        json!({"hwnd":window.hwnd().to_string(),"title":window.title()?,"rect":rect(window.box_rect()?),
        "active":window.is_active()?,"minimized":window.is_minimized()?,"maximized":window.is_maximized()?}),
    )
}
fn decode<T: for<'a> Deserialize<'a>>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| Error::Input(format!("invalid tool arguments: {e}")))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    title: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    target: String,
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
struct Hotkey {
    target: String,
    keys: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Saver {
    action: String,
    seconds: Option<u64>,
}

#[derive(Default)]
pub(crate) struct Session {
    gui: Option<AutoGui>,
    targets: HashMap<String, Window>,
    generation: u64,
    pause: Option<PauseLease>,
}

impl Session {
    fn gui(&mut self) -> Result<&mut AutoGui> {
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
    fn target(&self, key: &str) -> Result<Window> {
        let window = self.targets.get(key).ok_or(Error::InvalidArgument(
            "unknown/stale target; call windows_list again",
        ))?;
        window.title()?; // validates captured HWND plus originating process/thread identity
        Ok(window.clone())
    }
    pub(crate) fn call(
        &mut self,
        name: &str,
        args: Value,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Value> {
        match name {
            "windows_list" => {
                let args: List = decode(args)?;
                let windows = if let Some(title) = args.title {
                    self.gui()?.get_windows_with_title_ci(&title)?
                } else {
                    self.gui()?.get_all_windows()?
                };
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
                    let key = format!("w{}-{index}", self.generation);
                    detail["target"] = json!(key);
                    self.targets.insert(key, window);
                    results.push(detail);
                }
                Ok(text(
                    json!({"windows":results,"coordinates":"primary monitor physical pixels"}),
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
                let window = self.target(&a.target)?;
                check()?;
                window.activate()?;
                active(&window)?;
                let bounds = window.box_rect()?;
                check()?;
                let region = screen_region(self.gui()?, &window)?;
                if i64::from(region.width) * i64::from(region.height) > 16_777_216 {
                    return Err(Error::InvalidArgument(
                        "capture exceeds 16 megapixels; resize window",
                    ));
                }
                let frame = self.gui()?.screenshot_region(region)?;
                active(&window)?;
                if bounds != window.box_rect()? {
                    return Err(Error::InvalidArgument("window moved during capture; retry"));
                }
                check()?;
                let rgb = image::RgbImage::from_raw(frame.width, frame.height, frame.pixels)
                    .ok_or(Error::InvalidArgument("invalid frame"))?;
                let mut bytes = std::io::Cursor::new(Vec::new());
                rgb.write_to(&mut bytes, image::ImageFormat::Png)
                    .map_err(|e| Error::Screenshot(e.to_string()))?;
                if bytes.get_ref().len() > 12 * 1024 * 1024 {
                    return Err(Error::InvalidArgument("PNG exceeds 12 MiB; resize window"));
                }
                Ok(
                    json!({"content":[{"type":"text","text":json!({"target":a.target,"region":rect(region)}).to_string()},
                    {"type":"image","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())}],"isError":false}),
                )
            }
            "locate" | "click_image" => {
                let a: Search = decode(args)?;
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
                    check,
                )?;
                let point = autogui::center(found);
                if name == "click_image" {
                    let frame = gui.screenshot_region(found)?;
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
                    gui.click_xy(point.x, point.y)?;
                }
                Ok(text(
                    json!({"rect":rect(found),"center":{"x":point.x,"y":point.y},"clicked":name=="click_image"}),
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
                for ch in a.text.chars() {
                    check()?;
                    active(&window)?;
                    self.gui()?.write(&ch.to_string(), Duration::ZERO)?;
                }
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
                let window = self.target(&a.target)?;
                check()?;
                window.activate()?;
                active(&window)?;
                check()?;
                self.gui()?
                    .hotkey(&a.keys.iter().map(String::as_str).collect::<Vec<_>>())?;
                Ok(text(json!({"keys_sent":a.keys})))
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
            _ => Err(Error::InvalidArgument("unknown tool")),
        }
    }
}

impl super::mcp::Handler for Session {
    fn call(
        &mut self,
        name: &str,
        args: Value,
        check: &dyn Fn() -> std::io::Result<()>,
    ) -> std::io::Result<Value> {
        self.call(name, args, &|| check().map_err(Error::Io))
            .map_err(std::io::Error::other)
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
