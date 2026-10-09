//! Unattended ("headless") readiness check: can this process see and drive a
//! rendered Windows desktop right now? CLI/MCP extension; the library API is
//! unchanged. The check only reads state; it never unlocks, reconnects or
//! changes display topology.
use autogui::{Error, Rect, Result};
#[cfg(feature = "agent")]
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DisplayFact {
    pub name: String,
    pub bounds: Rect,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probe {
    /// Real pixels were read; `uniform` means every pixel had the same value.
    Captured {
        uniform: bool,
    },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    pub session_id: Option<u32>,
    pub console_session_id: Option<u32>,
    /// Remote Desktop session (SM_REMOTESESSION).
    pub remote: bool,
    /// WTS connect state, e.g. "active" or "disconnected".
    pub connect_state: Option<&'static str>,
    pub locked: Option<bool>,
    pub input_desktop: std::result::Result<String, String>,
    pub cursor: bool,
    pub displays: std::result::Result<Vec<DisplayFact>, String>,
    pub capture: Option<Probe>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Blocker,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Finding {
    pub level: Level,
    pub code: &'static str,
    pub advice: &'static str,
}

const fn blocker(code: &'static str, advice: &'static str) -> Finding {
    Finding {
        level: Level::Blocker,
        code,
        advice,
    }
}
const fn warning(code: &'static str, advice: &'static str) -> Finding {
    Finding {
        level: Level::Warning,
        code,
        advice,
    }
}

/// Pure decision logic, kept separate from Win32 queries so it is unit-tested
/// on every platform.
pub(crate) fn assess(facts: &Facts) -> Vec<Finding> {
    let mut findings = Vec::new();
    if facts.session_id == Some(0) {
        findings.push(blocker(
            "service-session",
            "Session 0 (Windows service) has no interactive desktop. Run in the logged-on user's session, e.g. a scheduled task set to run only when the user is logged on.",
        ));
    }
    if matches!(facts.connect_state, Some(state) if state != "active") {
        findings.push(blocker(
            "session-not-active",
            "This session is disconnected, so Windows stops rendering it. Reconnect, or before leaving Remote Desktop run scripts/rdp-to-console.ps1 to keep the desktop on the console.",
        ));
    }
    let desktop = facts.input_desktop.as_deref().ok();
    let before = findings.len();
    if facts.locked == Some(true) || desktop.is_some_and(|d| d.eq_ignore_ascii_case("Winlogon")) {
        findings.push(blocker(
            "locked",
            "Windows is locked or on the sign-in screen. A user must sign in; automation cannot bypass the lock.",
        ));
    } else if desktop.is_some_and(|d| d.eq_ignore_ascii_case("Screen-saver")) {
        findings.push(blocker(
            "screensaver-running",
            "A screen saver owns the input desktop. Run `screensaver stop`, then `screensaver pause` during automation.",
        ));
    } else if facts.input_desktop.is_err() {
        findings.push(blocker(
            "input-desktop-unavailable",
            "The input desktop cannot be opened from this process (locked, secure desktop, or different session).",
        ));
    }
    let rdp_active = facts.remote && facts.connect_state == Some("active");
    if !facts.cursor {
        // An unlocked, connected RDP session that denies cursor access is the
        // classic "client minimized" state: Windows suppresses its rendering.
        findings.push(if rdp_active && findings.len() == before {
            blocker(
                "remote-not-rendering",
                r"Remote Desktop session is connected but not rendering (client minimized or display suppressed). Restore the client window; on the client PC set HKCU\Software\Microsoft\Terminal Server Client\RemoteDesktop_SuppressWhenMinimized=2 (DWORD), or move the session to the console with scripts/rdp-to-console.ps1.",
            )
        } else {
            blocker(
                "cursor-unavailable",
                "The desktop cursor cannot be read, so input cannot be verified.",
            )
        });
    }
    match &facts.displays {
        Err(_) => findings.push(blocker(
            "displays-unavailable",
            "Active displays cannot be enumerated.",
        )),
        Ok(displays) if displays.is_empty() => findings.push(blocker(
            "no-active-display",
            "No active display output. Without a monitor, install the optional virtual display (scripts/virtual-display.ps1) or attach an HDMI/DP dummy plug.",
        )),
        Ok(displays) if !displays.iter().any(|d| d.primary) => findings.push(warning(
            "no-primary-display",
            "No display reports itself as primary; default APIs and fail-safe corners use the primary monitor.",
        )),
        Ok(_) => {}
    }
    match &facts.capture {
        // The probe runs through AutoGui, whose fail-safe needs the cursor; a
        // cursor blocker above already explains that failure.
        Some(Probe::Failed(_)) if facts.cursor => findings.push(blocker(
            "capture-failed",
            "Reading primary-monitor pixels failed.",
        )),
        Some(Probe::Captured { uniform: true }) => findings.push(warning(
            "capture-uniform",
            "The probe area is a single colour. It may be an empty wallpaper or an unrendered desktop; confirm by capturing a real app.",
        )),
        _ => {}
    }
    if rdp_active && facts.cursor {
        findings.push(warning(
            "remote-session",
            "Remote Desktop session: minimizing or disconnecting the client stops rendering. Keep the client window restored, or move the session to the console with scripts/rdp-to-console.ps1 before leaving.",
        ));
    }
    findings
}

pub(crate) fn ready(findings: &[Finding]) -> bool {
    !findings.iter().any(|f| f.level == Level::Blocker)
}

fn uniform(pixels: &[u8]) -> bool {
    pixels.chunks_exact(3).all(|p| p == &pixels[..3])
}

/// Gather facts from the current process's Windows session.
pub(crate) fn gather() -> Result<Facts> {
    #[cfg(all(windows, feature = "os"))]
    {
        native::gather()
    }
    #[cfg(not(all(windows, feature = "os")))]
    {
        Err(if cfg!(windows) {
            Error::FeatureDisabled("os")
        } else {
            Error::UnsupportedPlatform
        })
    }
}

#[cfg(all(windows, feature = "os"))]
mod native {
    use super::{DisplayFact, Facts, Probe, uniform};
    use autogui::{AutoGui, Display, Rect, Result};
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::System::RemoteDesktop::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SM_REMOTESESSION,
    };

    #[allow(non_upper_case_globals)] // windows-sys keeps Win32 constant names.
    fn state_name(state: WTS_CONNECTSTATE_CLASS) -> &'static str {
        match state {
            WTSActive => "active",
            WTSConnected => "connected",
            WTSConnectQuery => "connect-query",
            WTSShadow => "shadow",
            WTSDisconnected => "disconnected",
            WTSIdle => "idle",
            WTSListen => "listen",
            WTSReset => "reset",
            WTSDown => "down",
            WTSInit => "init",
            _ => "unknown",
        }
    }

    /// Connect state and lock flag from WTSSessionInfoEx (Windows 8+ semantics).
    fn session_info(session: u32) -> (Option<&'static str>, Option<bool>) {
        let mut buffer: windows_sys::core::PWSTR = std::ptr::null_mut();
        let mut bytes = 0;
        let ok = unsafe {
            WTSQuerySessionInformationW(
                WTS_CURRENT_SERVER_HANDLE,
                session,
                WTSSessionInfoEx,
                &mut buffer,
                &mut bytes,
            )
        };
        if ok == 0 || buffer.is_null() {
            return (None, None);
        }
        let result = if bytes as usize >= size_of::<WTSINFOEXW>() {
            let info = unsafe { &*(buffer as *const WTSINFOEXW) };
            if info.Level == 1 {
                let level = unsafe { info.Data.WTSInfoExLevel1 };
                let locked = match level.SessionFlags as u32 {
                    WTS_SESSIONSTATE_LOCK => Some(true),
                    WTS_SESSIONSTATE_UNLOCK => Some(false),
                    _ => None,
                };
                (Some(state_name(level.SessionState)), locked)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        unsafe { WTSFreeMemory(buffer.cast()) };
        result
    }

    fn probe(displays: &[Display]) -> Probe {
        let Some(display) = displays.iter().find(|d| d.is_primary()) else {
            return Probe::Failed("no primary display".into());
        };
        let b = display.bounds();
        let side = 64.min(b.width).min(b.height);
        let region = Rect::new(
            b.left + (b.width - side) / 2,
            b.top + (b.height - side) / 2,
            side,
            side,
        );
        let frame = AutoGui::new().and_then(|mut gui| {
            gui.settings_mut().pause = std::time::Duration::ZERO;
            gui.screenshot_display_region(display, region)
        });
        match frame {
            Ok(frame) => Probe::Captured {
                uniform: uniform(&frame.pixels),
            },
            Err(error) => Probe::Failed(error.to_string()),
        }
    }

    pub(super) fn gather() -> Result<Facts> {
        let mut session = 0;
        let session_id = (unsafe { ProcessIdToSessionId(std::process::id(), &mut session) } != 0)
            .then_some(session);
        let console = unsafe { WTSGetActiveConsoleSessionId() };
        let (connect_state, locked) = session_id.map(session_info).unwrap_or((None, None));
        let mut point = POINT::default();
        let displays = Display::all();
        let capture = displays.as_ref().ok().map(|d| probe(d));
        Ok(Facts {
            session_id,
            console_session_id: (console != u32::MAX).then_some(console),
            remote: unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0,
            connect_state,
            locked,
            input_desktop: super::super::screensaver::input_desktop().map_err(|e| e.to_string()),
            cursor: unsafe { GetCursorPos(&mut point) } != 0,
            displays: displays
                .map(|all| {
                    all.iter()
                        .map(|d| DisplayFact {
                            name: d.name().to_string(),
                            bounds: d.bounds(),
                            primary: d.is_primary(),
                        })
                        .collect()
                })
                .map_err(|e| e.to_string()),
            capture,
        })
    }
}

#[cfg(feature = "agent")]
pub(crate) fn to_json(facts: &Facts, findings: &[Finding]) -> Value {
    let displays = match &facts.displays {
        Ok(all) => json!(
            all.iter()
                .map(|d| json!({"device_name":d.name,"primary":d.primary,"rect":{"left":d.bounds.left,"top":d.bounds.top,"width":d.bounds.width,"height":d.bounds.height}}))
                .collect::<Vec<_>>()
        ),
        Err(e) => json!({"error":e}),
    };
    let capture = match &facts.capture {
        Some(Probe::Captured { uniform }) => json!({"ok":true,"uniform":uniform}),
        Some(Probe::Failed(e)) => json!({"ok":false,"error":e}),
        None => Value::Null,
    };
    json!({
        "ready": ready(findings),
        "session_id": facts.session_id,
        "console_session_id": facts.console_session_id,
        "on_console": facts.session_id.is_some() && facts.session_id == facts.console_session_id,
        "remote": facts.remote,
        "connect_state": facts.connect_state,
        "locked": facts.locked,
        "input_desktop": match &facts.input_desktop { Ok(d) => json!(d), Err(e) => json!({"error":e}) },
        "cursor_access": facts.cursor,
        "displays": displays,
        "primary_capture_probe": capture,
        "findings": findings.iter().map(|f| json!({
            "level": if f.level == Level::Blocker { "blocker" } else { "warning" },
            "code": f.code,
            "advice": f.advice,
        })).collect::<Vec<_>>(),
    })
}

fn print(facts: &Facts, findings: &[Finding]) {
    let opt = |v: Option<u32>| v.map_or("unknown".to_string(), |v| v.to_string());
    println!(
        "工作階段：{}（主控台工作階段 {}；遠端桌面：{}；連線狀態：{}）",
        opt(facts.session_id),
        opt(facts.console_session_id),
        facts.remote,
        facts.connect_state.unwrap_or("unknown")
    );
    println!(
        "鎖定：{}",
        facts.locked.map_or("unknown".into(), |v| v.to_string())
    );
    match &facts.input_desktop {
        Ok(d) => println!("輸入桌面：{d}"),
        Err(e) => println!("輸入桌面：無法讀取（{e}）"),
    }
    println!("游標存取：{}", facts.cursor);
    match &facts.displays {
        Ok(all) if all.is_empty() => println!("顯示器：無活動輸出"),
        Ok(all) => {
            for d in all {
                println!("顯示器：{}\t{:?}\tprimary={}", d.name, d.bounds, d.primary);
            }
        }
        Err(e) => println!("顯示器：無法列舉（{e}）"),
    }
    match &facts.capture {
        Some(Probe::Captured { uniform }) => {
            println!("主螢幕截圖探測：成功（單一顏色：{uniform}）")
        }
        Some(Probe::Failed(e)) => println!("主螢幕截圖探測：失敗（{e}）"),
        None => println!("主螢幕截圖探測：未執行"),
    }
    for f in findings {
        let tag = if f.level == Level::Blocker {
            "阻擋"
        } else {
            "注意"
        };
        println!("[{tag}] {}: {}", f.code, f.advice);
    }
    println!(
        "{}",
        if ready(findings) {
            "結論：可無人值守操作（仍須以實際 App 截圖確認）"
        } else {
            "結論：目前不可無人值守操作"
        }
    );
}

/// `session status [--json]`. Exits non-zero when a blocker exists so scripts
/// can gate scheduled automation on it.
pub(super) fn run(args: &[String]) -> Result<()> {
    let json_output = match args {
        [] => false,
        [status] if status == "status" => false,
        [status, flag] if status == "status" && flag == "--json" => true,
        _ => return Err(Error::InvalidArgument("use session status [--json]")),
    };
    let facts = gather()?;
    let findings = assess(&facts);
    if json_output {
        #[cfg(feature = "agent")]
        println!("{}", to_json(&facts, &findings));
        #[cfg(not(feature = "agent"))]
        return Err(Error::FeatureDisabled("agent"));
    } else {
        print(&facts, &findings);
    }
    if ready(&findings) {
        Ok(())
    } else {
        Err(Error::PermissionDenied(
            "session is not ready for unattended automation",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> Facts {
        Facts {
            session_id: Some(1),
            console_session_id: Some(1),
            remote: false,
            connect_state: Some("active"),
            locked: Some(false),
            input_desktop: Ok("Default".into()),
            cursor: true,
            displays: Ok(vec![DisplayFact {
                name: r"\\.\DISPLAY1".into(),
                bounds: Rect::new(0, 0, 1920, 1080),
                primary: true,
            }]),
            capture: Some(Probe::Captured { uniform: false }),
        }
    }

    fn codes(facts: &Facts) -> Vec<&'static str> {
        assess(facts).iter().map(|f| f.code).collect()
    }

    #[test]
    fn healthy_console_is_ready() {
        let findings = assess(&healthy());
        assert!(findings.is_empty());
        assert!(ready(&findings));
    }

    #[test]
    fn headless_without_display_is_blocked_with_driver_advice() {
        let facts = Facts {
            displays: Ok(vec![]),
            capture: None,
            ..healthy()
        };
        let findings = assess(&facts);
        assert_eq!(codes(&facts), ["no-active-display"]);
        assert!(findings[0].advice.contains("virtual-display.ps1"));
        assert!(!ready(&findings));
    }

    #[test]
    fn disconnected_rdp_and_service_session_are_blockers() {
        let facts = Facts {
            session_id: Some(0),
            remote: true,
            connect_state: Some("disconnected"),
            ..healthy()
        };
        assert_eq!(codes(&facts), ["service-session", "session-not-active"]);
    }

    #[test]
    fn active_rdp_is_only_a_warning() {
        let facts = Facts {
            remote: true,
            ..healthy()
        };
        let findings = assess(&facts);
        assert_eq!(codes(&facts), ["remote-session"]);
        assert!(ready(&findings));
    }

    #[test]
    fn minimized_rdp_client_is_identified() {
        let facts = Facts {
            session_id: Some(1),
            console_session_id: Some(3),
            remote: true,
            cursor: false,
            capture: Some(Probe::Failed("GetCursorPos".into())),
            ..healthy()
        };
        let findings = assess(&facts);
        assert_eq!(codes(&facts), ["remote-not-rendering"]);
        assert!(findings[0].advice.contains("SuppressWhenMinimized"));
        assert!(!ready(&findings));
    }

    #[test]
    fn lock_and_screensaver_are_distinguished() {
        let locked = Facts {
            locked: Some(true),
            input_desktop: Err("permission denied".into()),
            cursor: false,
            ..healthy()
        };
        assert_eq!(codes(&locked), ["locked", "cursor-unavailable"]);
        let saver = Facts {
            input_desktop: Ok("Screen-saver".into()),
            ..healthy()
        };
        assert_eq!(codes(&saver), ["screensaver-running"]);
        let winlogon = Facts {
            locked: None,
            input_desktop: Ok("Winlogon".into()),
            ..healthy()
        };
        assert_eq!(codes(&winlogon), ["locked"]);
    }

    #[test]
    fn capture_problems_are_reported() {
        // Capture failure is its own blocker when the cursor is readable.
        let failed = Facts {
            capture: Some(Probe::Failed("x".into())),
            ..healthy()
        };
        assert_eq!(codes(&failed), ["capture-failed"]);
        let blank = Facts {
            capture: Some(Probe::Captured { uniform: true }),
            ..healthy()
        };
        assert_eq!(codes(&blank), ["capture-uniform"]);
        assert!(ready(&assess(&blank)));
    }

    #[test]
    fn uniform_detects_single_colour_rgb() {
        assert!(uniform(&[7, 8, 9, 7, 8, 9]));
        assert!(!uniform(&[7, 8, 9, 7, 8, 10]));
        assert!(uniform(&[]));
    }

    #[test]
    fn cli_arguments_are_strict() {
        for bad in [vec!["start"], vec!["status", "--yaml"]] {
            let args: Vec<String> = bad.into_iter().map(String::from).collect();
            assert!(matches!(run(&args), Err(Error::InvalidArgument(_))));
        }
    }
}
