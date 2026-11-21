//! Agent-layer desktop helpers: window discovery filters, window waiting and
//! guarded application launch. Helpers never print; callers format results.
#![allow(dead_code)]
use autogui::{AutoGui, Error, Result, Window};
use std::path::Path;
use std::time::{Duration, Instant};

/// Env var that must be exactly "1" before an application may be launched.
pub(crate) const LAUNCH_ENV: &str = "AUTOGUI_ALLOW_LAUNCH";
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_ARGS: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WindowFilter {
    /// Case-insensitive title substring; None or empty means any title.
    pub title: Option<String>,
    /// Executable base name, case-insensitive; ".exe" may be omitted.
    pub process: Option<String>,
    /// Only the foreground window.
    pub foreground: bool,
    /// Only windows with a non-empty title.
    pub titled_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaitState {
    Present,
    Absent,
    Foreground,
}

impl WaitState {
    pub(crate) fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "present" => Ok(Self::Present),
            "absent" => Ok(Self::Absent),
            "foreground" => Ok(Self::Foreground),
            _ => Err(Error::InvalidArgument(
                "state must be present, absent or foreground",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Launch {
    pub path: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

/// Case-insensitive executable name match; "notepad" matches "Notepad.EXE".
pub(crate) fn process_matches(actual: &str, wanted: &str) -> bool {
    fn base(s: &str) -> String {
        let name = s.rsplit(['\\', '/']).next().unwrap_or(s).to_lowercase();
        name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
    }
    let wanted = base(wanted.trim());
    !wanted.is_empty() && base(actual) == wanted
}

/// Pure filter predicate. title is the window title, process the executable
/// path/name when it could be resolved.
pub(crate) fn filter_matches(
    f: &WindowFilter,
    title: &str,
    process: Option<&str>,
    is_foreground: bool,
) -> bool {
    if f.titled_only && title.trim().is_empty() {
        return false;
    }
    if f.foreground && !is_foreground {
        return false;
    }
    if let Some(t) = f.title.as_deref().filter(|t| !t.is_empty())
        && !title.to_lowercase().contains(&t.to_lowercase())
    {
        return false;
    }
    if let Some(p) = f.process.as_deref() {
        return process.is_some_and(|actual| process_matches(actual, p));
    }
    true
}

/// Returns (pid, full image path). None when the lookup fails.
pub(crate) fn process_info(hwnd: isize) -> Option<(u32, String)> {
    imp::process_info(hwnd)
}

pub(crate) fn class_name(hwnd: isize) -> Option<String> {
    imp::class_name(hwnd)
}

/// Visible windows matching the filter. Windows that vanish mid-scan are skipped.
pub(crate) fn list(gui: &mut AutoGui, f: &WindowFilter) -> Result<Vec<Window>> {
    let base = match f.title.as_deref().filter(|t| !t.is_empty()) {
        Some(title) => gui.get_windows_with_title_ci(title)?,
        None => gui.get_all_windows()?,
    };
    let active = if f.foreground {
        gui.get_active_window()?.map(|w| w.hwnd())
    } else {
        None
    };
    let mut out = Vec::new();
    for window in base {
        let title = match window.title() {
            Ok(t) => t,
            Err(Error::WindowNotFound) => continue,
            Err(e) => return Err(e),
        };
        let process = if f.process.is_some() {
            process_info(window.hwnd()).map(|(_, p)| p)
        } else {
            None
        };
        if filter_matches(f, &title, process.as_deref(), active == Some(window.hwnd())) {
            out.push(window);
        }
    }
    Ok(out)
}

fn timed_out() -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "timed out waiting for window state",
    ))
}

/// Polling core. probe(foreground_required) returns the current matches.
pub(crate) fn poll<T>(
    state: WaitState,
    timeout: Duration,
    interval: Duration,
    check: &dyn Fn() -> Result<()>,
    mut probe: impl FnMut(bool) -> Result<Vec<T>>,
) -> Result<Vec<T>> {
    let start = Instant::now();
    loop {
        check()?;
        let found = probe(state == WaitState::Foreground)?;
        let done = match state {
            WaitState::Absent => found.is_empty(),
            _ => !found.is_empty(),
        };
        if done {
            return Ok(found);
        }
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            return Err(timed_out());
        }
        std::thread::sleep(interval.min(timeout - elapsed));
    }
}

/// Waits until windows matching f reach state. Absent returns an empty list.
pub(crate) fn wait(
    gui: &mut AutoGui,
    f: &WindowFilter,
    state: WaitState,
    timeout: Duration,
    check: &dyn Fn() -> Result<()>,
) -> Result<Vec<Window>> {
    poll(state, timeout, POLL_INTERVAL, check, |fg| {
        let mut filter = f.clone();
        filter.foreground = fg;
        list(gui, &filter)
    })
}

/// Validates a launch request without touching the OS beyond file metadata.
pub(crate) fn validate(l: &Launch) -> Result<()> {
    if l.path.is_empty() || l.path.contains('\0') {
        return Err(Error::InvalidArgument("path must be a non-empty string"));
    }
    let path = Path::new(&l.path);
    if !path.is_absolute() {
        return Err(Error::InvalidArgument("path must be absolute"));
    }
    if cfg!(windows) && !local_drive_path(&l.path) {
        return Err(Error::InvalidArgument(
            "path must be on a local drive letter (no UNC, device or stream paths)",
        ));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if ext.as_deref() != Some("exe") {
        return Err(Error::InvalidArgument("only .exe files may be launched"));
    }
    if l.args.len() > MAX_ARGS {
        return Err(Error::InvalidArgument("at most 32 arguments"));
    }
    if l.args.iter().any(|a| a.contains('\0')) {
        return Err(Error::InvalidArgument("arguments must not contain NUL"));
    }
    if !path.is_file() {
        return Err(Error::InvalidArgument("executable does not exist"));
    }
    if let Some(cwd) = &l.cwd {
        let cwd = Path::new(cwd);
        if !cwd.is_absolute() || !cwd.is_dir() {
            return Err(Error::InvalidArgument(
                "cwd must be an existing absolute directory",
            ));
        }
    }
    Ok(())
}

/// True for plain `X:\...` paths. Rejects UNC shares, `\\?\` / `\\.\` device
/// namespaces and NTFS alternate data streams (`file.txt:x.exe`).
fn local_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() > 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
        && !path[2..].contains(':')
}

pub(crate) fn launch_allowed() -> bool {
    std::env::var(LAUNCH_ENV).is_ok_and(|v| v == "1")
}

/// Starts the program (no shell, null stdio, not waited). Returns the pid.
pub(crate) fn launch(l: &Launch) -> Result<u32> {
    launch_with(l, launch_allowed())
}

pub(crate) fn launch_with(l: &Launch, allowed: bool) -> Result<u32> {
    if !allowed {
        return Err(Error::PermissionDenied(
            "app_launch disabled; set AUTOGUI_ALLOW_LAUNCH=1",
        ));
    }
    if !cfg!(windows) {
        return Err(Error::UnsupportedPlatform);
    }
    validate(l)?;
    let mut cmd = std::process::Command::new(&l.path);
    cmd.args(&l.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(cwd) = &l.cwd {
        cmd.current_dir(cwd);
    }
    let child = cmd.spawn()?;
    // Dropping the handle neither waits nor kills the child.
    Ok(child.id())
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{CloseHandle, HWND};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};

    pub(super) fn process_info(hwnd: isize) -> Option<(u32, String)> {
        let mut pid = 0u32;
        // SAFETY: pid is a valid out pointer; an invalid HWND yields 0.
        unsafe { GetWindowThreadProcessId(hwnd as HWND, &mut pid) };
        if pid == 0 {
            return None;
        }
        // SAFETY: plain handle query; the handle is closed below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        // SAFETY: buf holds len u16 elements and len is updated on success.
        let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) };
        // SAFETY: handle came from OpenProcess and is closed exactly once.
        unsafe { CloseHandle(handle) };
        if ok == 0 || len == 0 {
            return None;
        }
        Some((pid, String::from_utf16_lossy(&buf[..len as usize])))
    }

    pub(super) fn class_name(hwnd: isize) -> Option<String> {
        let mut buf = [0u16; 256];
        // SAFETY: buf holds 256 u16 elements.
        let n = unsafe { GetClassNameW(hwnd as HWND, buf.as_mut_ptr(), buf.len() as i32) };
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n as usize]))
    }
}

#[cfg(not(windows))]
mod imp {
    pub(super) fn process_info(_: isize) -> Option<(u32, String)> {
        None
    }
    pub(super) fn class_name(_: isize) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn f() -> WindowFilter {
        WindowFilter::default()
    }

    #[test]
    fn process_name_match() {
        assert!(process_matches(
            r"C:\Windows\System32\Notepad.EXE",
            "notepad"
        ));
        assert!(process_matches("notepad.exe", "NOTEPAD.exe"));
        assert!(!process_matches("notepad2.exe", "notepad"));
        assert!(!process_matches("notepad.exe", ""));
    }

    #[test]
    fn filter_predicates() {
        assert!(filter_matches(&f(), "Doc", None, false));
        let titled = WindowFilter {
            titled_only: true,
            ..f()
        };
        assert!(!filter_matches(&titled, "  ", None, false));
        let fg = WindowFilter {
            foreground: true,
            ..f()
        };
        assert!(!filter_matches(&fg, "A", None, false));
        assert!(filter_matches(&fg, "A", None, true));
        let t = WindowFilter {
            title: Some("note".into()),
            ..f()
        };
        assert!(filter_matches(&t, "My NOTES", None, false));
        assert!(!filter_matches(&t, "Other", None, false));
        let p = WindowFilter {
            process: Some("notepad".into()),
            ..f()
        };
        assert!(filter_matches(&p, "x", Some(r"C:\a\notepad.exe"), false));
        assert!(!filter_matches(&p, "x", None, false));
        assert!(!filter_matches(&p, "x", Some("calc.exe"), false));
    }

    #[test]
    fn wait_state_parse() {
        assert_eq!(WaitState::parse("Present").unwrap(), WaitState::Present);
        assert!(WaitState::parse("gone").is_err());
    }

    const TICK: Duration = Duration::from_millis(1);

    #[test]
    fn poll_found_after_retries() {
        let n = Cell::new(0);
        let out = poll(
            WaitState::Present,
            Duration::from_secs(5),
            TICK,
            &|| Ok(()),
            |_| {
                n.set(n.get() + 1);
                Ok(if n.get() >= 3 { vec![1] } else { vec![] })
            },
        )
        .unwrap();
        assert_eq!(out, vec![1]);
        assert_eq!(n.get(), 3);
    }

    #[test]
    fn poll_times_out() {
        let err = poll(
            WaitState::Present,
            Duration::from_millis(20),
            TICK,
            &|| Ok(()),
            |_| Ok(Vec::<u8>::new()),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Io(ref e) if e.kind() == std::io::ErrorKind::TimedOut));
    }

    #[test]
    fn poll_zero_timeout_probes_once() {
        let n = Cell::new(0);
        let _ = poll(WaitState::Present, Duration::ZERO, TICK, &|| Ok(()), |_| {
            n.set(n.get() + 1);
            Ok(Vec::<u8>::new())
        });
        assert_eq!(n.get(), 1);
    }

    #[test]
    fn poll_cancelled() {
        let err = poll(
            WaitState::Present,
            Duration::from_secs(5),
            TICK,
            &|| Err(Error::PermissionDenied("cancelled")),
            |_| Ok(vec![1]),
        )
        .unwrap_err();
        assert!(matches!(err, Error::PermissionDenied("cancelled")));
    }

    #[test]
    fn poll_absent_and_foreground_flag() {
        let n = Cell::new(0);
        let out = poll(
            WaitState::Absent,
            Duration::from_secs(5),
            TICK,
            &|| Ok(()),
            |fg| {
                assert!(!fg);
                n.set(n.get() + 1);
                Ok(if n.get() < 2 { vec![1] } else { vec![] })
            },
        )
        .unwrap();
        assert!(out.is_empty());
        let saw = Cell::new(false);
        poll(
            WaitState::Foreground,
            Duration::from_secs(1),
            TICK,
            &|| Ok(()),
            |fg| {
                saw.set(fg);
                Ok(vec![1])
            },
        )
        .unwrap();
        assert!(saw.get());
    }

    fn l(path: &str) -> Launch {
        Launch {
            path: path.into(),
            args: vec![],
            cwd: None,
        }
    }

    #[test]
    fn launch_validation_errors() {
        assert!(matches!(validate(&l("")), Err(Error::InvalidArgument(_))));
        assert!(matches!(
            validate(&l("notepad.exe")),
            Err(Error::InvalidArgument("path must be absolute"))
        ));
        if cfg!(windows) {
            for p in [
                r"\\server\share\x.exe",
                r"\\?\C:\Windows\notepad.exe",
                r"\\.\C:\Windows\notepad.exe",
                r"C:\Windows\win.ini:x.exe",
                r"\Windows\notepad.exe",
            ] {
                assert!(
                    matches!(validate(&l(p)), Err(Error::InvalidArgument(m)) if m.contains("local drive") || m.contains("absolute")),
                    "{p}"
                );
            }
            assert!(local_drive_path(r"C:\Windows\notepad.exe"));
            assert!(local_drive_path("d:/tools/app.EXE"));
        }
        let dir = std::env::temp_dir();
        for ext in ["cmd", "bat", "ps1", "lnk"] {
            let p = dir.join(format!("x.{ext}"));
            assert!(matches!(
                validate(&l(&p.to_string_lossy())),
                Err(Error::InvalidArgument("only .exe files may be launched"))
            ));
        }
        let missing = dir.join("autogui-definitely-missing.exe");
        assert!(matches!(
            validate(&l(&missing.to_string_lossy())),
            Err(Error::InvalidArgument("executable does not exist"))
        ));
        let mut many = l(&missing.to_string_lossy());
        many.args = vec!["a".into(); 33];
        assert!(matches!(
            validate(&many),
            Err(Error::InvalidArgument("at most 32 arguments"))
        ));
    }

    #[test]
    fn launch_disabled_without_gate() {
        let err = launch_with(&l(r"C:\Windows\notepad.exe"), false).unwrap_err();
        assert!(matches!(err, Error::PermissionDenied(m) if m.contains("AUTOGUI_ALLOW_LAUNCH")));
    }

    #[cfg(windows)]
    #[test]
    fn class_and_process_lookup_fail_softly() {
        assert!(process_info(0).is_none());
        assert!(class_name(0).is_none());
    }

    #[test]
    #[ignore = "starts a real notepad; set AUTOGUI_LIVE=1 and AUTOGUI_ALLOW_LAUNCH=1"]
    fn live_launch_notepad() {
        if std::env::var("AUTOGUI_LIVE").as_deref() != Ok("1") {
            return;
        }
        let pid = launch(&l(r"C:\Windows\System32\notepad.exe")).unwrap();
        assert!(pid > 0);
    }
}
