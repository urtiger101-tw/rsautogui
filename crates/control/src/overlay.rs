//! On-screen notice while an agent works (SPEC §22): a small,
//! semi-transparent, click-through window at the top centre of the primary
//! monitor that reads "Computer use! Don't move!" and names the emergency
//! stop chord.
//!
//! - It never takes focus or mouse input (`WS_EX_NOACTIVATE`,
//!   `WS_EX_TRANSPARENT`, `HTTRANSPARENT`), so foreground checks, clicks and
//!   `WindowFromPoint` hit-tests see the windows underneath.
//! - It is excluded from screen capture (`WDA_EXCLUDEFROMCAPTURE`), so
//!   screenshots, template matching and `wait_change` never see it.
//! - `windows_list` skips it ([`is_overlay`]).
//! - [`touch`] shows it for [`IDLE`] after the latest tool call; [`hide`]
//!   removes it at once (emergency stop, session end).
//!
//! One UI thread per process owns the window; callers only update atomics
//! and post messages. Off Windows every function is a no-op.
use std::time::Duration;

/// How long the notice stays after the latest tool call.
pub(crate) const IDLE: Duration = Duration::from_secs(15);

/// First line of the notice.
pub(crate) const HEADLINE: &str = "Computer use! Don't move!";

/// Second line of the notice: `AUTOGUI_LANG` when set (the CLI path),
/// otherwise the Windows display language (MCP servers have no flag).
pub(crate) fn subline() -> &'static str {
    let zh = if std::env::var_os("AUTOGUI_LANG").is_some() {
        crate::cli::lang::zh()
    } else {
        windows_ui_is_chinese()
    };
    if zh {
        "按 Ctrl+Alt+F12 緊急停止"
    } else {
        "Ctrl+Alt+F12 = emergency stop"
    }
}

#[cfg(windows)]
fn windows_ui_is_chinese() -> bool {
    // SAFETY: no arguments; returns the user's UI language id.
    let lang = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    lang & 0x3ff == 0x04 // LANG_CHINESE
}

#[cfg(not(windows))]
fn windows_ui_is_chinese() -> bool {
    false
}

#[cfg(windows)]
pub(crate) use imp::{hide, is_overlay, touch};

#[cfg(not(windows))]
pub(crate) fn touch() {}
#[cfg(not(windows))]
pub(crate) fn hide() {}
#[cfg(not(windows))]
pub(crate) fn is_overlay(_hwnd: isize) -> bool {
    false
}

#[cfg(windows)]
mod imp {
    use super::{HEADLINE, IDLE, subline};
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicI32, AtomicIsize, AtomicU64, Ordering};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DT_CENTER, DT_NOPREFIX,
        DT_SINGLELINE, DT_TOP, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRgn, GetDC,
        GetTextExtentPoint32W, PAINTSTRUCT, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
        SetWindowRgn, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetThreadDpiAwarenessContext,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
        GetMessageW, GetSystemMetrics, HTTRANSPARENT, HWND_TOPMOST, IsWindowVisible, LWA_ALPHA,
        MA_NOACTIVATE, MSG, PostMessageW, RegisterClassExW, SM_CXSCREEN, SW_HIDE,
        SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetLayeredWindowAttributes,
        SetTimer, SetWindowDisplayAffinity, SetWindowPos, ShowWindow, TranslateMessage,
        WDA_EXCLUDEFROMCAPTURE, WM_APP, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT, WM_TIMER,
        WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_EX_TRANSPARENT, WS_POPUP,
    };

    const SHOW: u32 = WM_APP;
    const HIDE: u32 = WM_APP + 1;
    const TICK_MS: u32 = 250;
    // About 44% opaque: half of the first 88%, so the desktop stays visible.
    const ALPHA: u8 = 112;
    // Calm "assistant at work" blue: an alarm palette (black/orange/yellow)
    // made people think the computer had failed.
    const BACKGROUND: u32 = rgb(37, 99, 235);
    const BORDER: u32 = rgb(147, 197, 253);
    const HEADLINE_COLOR: u32 = rgb(255, 255, 255);
    const SUBLINE_COLOR: u32 = rgb(219, 234, 254);

    const fn rgb(r: u8, g: u8, b: u8) -> u32 {
        r as u32 | (g as u32) << 8 | (b as u32) << 16
    }

    struct Shared {
        epoch: Instant,
        /// Milliseconds after `epoch` of the latest touch, plus one; 0 = none.
        last: AtomicU64,
        hwnd: AtomicIsize,
    }

    static SHARED: OnceLock<Shared> = OnceLock::new();

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub(crate) fn touch() {
        let mut fresh = false;
        let shared = SHARED.get_or_init(|| {
            fresh = true;
            Shared {
                epoch: Instant::now(),
                last: AtomicU64::new(0),
                hwnd: AtomicIsize::new(0),
            }
        });
        if fresh {
            // Started once, after SHARED is set. If the window cannot be
            // created the notice is simply absent and input is unaffected.
            let _ = std::thread::Builder::new()
                .name("autogui-overlay".into())
                .spawn(run);
        }
        let now = u64::try_from(shared.epoch.elapsed().as_millis()).unwrap_or(u64::MAX - 1);
        shared.last.store(now + 1, Ordering::Release);
        post(shared, SHOW);
    }

    pub(crate) fn hide() {
        if let Some(shared) = SHARED.get() {
            shared.last.store(0, Ordering::Release);
            post(shared, HIDE);
        }
    }

    pub(crate) fn is_overlay(hwnd: isize) -> bool {
        hwnd != 0
            && SHARED
                .get()
                .is_some_and(|s| s.hwnd.load(Ordering::Acquire) == hwnd)
    }

    fn post(shared: &Shared, message: u32) {
        let hwnd = shared.hwnd.load(Ordering::Acquire);
        if hwnd != 0 {
            // SAFETY: posting to a window owned by another thread is allowed;
            // a stale handle only makes the call fail.
            unsafe { PostMessageW(hwnd as HWND, message, 0, 0) };
        }
    }

    fn recent(shared: &Shared) -> bool {
        let last = shared.last.load(Ordering::Acquire);
        let now = u64::try_from(shared.epoch.elapsed().as_millis()).unwrap_or(u64::MAX - 1) + 1;
        last != 0 && now.saturating_sub(last) < IDLE.as_millis() as u64
    }

    // GDI fonts and the headline height, set once by the overlay thread.
    static HEAD_FONT: AtomicIsize = AtomicIsize::new(0);
    static SUB_FONT: AtomicIsize = AtomicIsize::new(0);
    static HEAD_HEIGHT: AtomicI32 = AtomicI32::new(0);

    fn font(height: i32, weight: i32) -> *mut c_void {
        let face = wide("Segoe UI");
        // SAFETY: plain GDI object creation; the face name outlives the call.
        unsafe {
            CreateFontW(
                -height,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            )
        }
    }

    fn extent(hwnd: HWND, font: *mut c_void, text: &[u16]) -> (i32, i32) {
        // SAFETY: the DC is released before returning; `text` is valid.
        unsafe {
            let dc = GetDC(hwnd);
            let old = SelectObject(dc, font as _);
            let mut size = windows_sys::Win32::Foundation::SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(dc, text.as_ptr(), (text.len() - 1) as i32, &mut size);
            SelectObject(dc, old);
            ReleaseDC(hwnd, dc);
            (size.cx, size.cy)
        }
    }

    fn show(hwnd: HWND) {
        // SAFETY: the window belongs to this thread.
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    fn paint(hwnd: HWND) {
        // SAFETY: standard WM_PAINT handling on this thread's window; every
        // GDI object created here is deleted before returning.
        unsafe {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut client = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetClientRect(hwnd, &mut client);
            let background = CreateSolidBrush(BACKGROUND);
            FillRect(dc, &client, background);
            DeleteObject(background as _);
            let scale = GetDpiForWindow(hwnd).max(96) as i32;
            let radius = 14 * scale / 96;
            let edge =
                CreateRoundRectRgn(0, 0, client.right + 1, client.bottom + 1, radius, radius);
            let border = CreateSolidBrush(BORDER);
            let thickness = (2 * scale / 96).max(1);
            FrameRgn(dc, edge, border, thickness, thickness);
            DeleteObject(border as _);
            DeleteObject(edge as _);
            SetBkMode(dc, TRANSPARENT as _);
            let head_font = HEAD_FONT.load(Ordering::Relaxed) as *mut c_void;
            let sub_font = SUB_FONT.load(Ordering::Relaxed) as *mut c_void;
            let head_h = HEAD_HEIGHT.load(Ordering::Relaxed);
            let pad = 8 * scale / 96;
            let old = SelectObject(dc, head_font as _);
            SetTextColor(dc, HEADLINE_COLOR);
            let mut head = wide(HEADLINE);
            let mut rect = RECT {
                left: 0,
                top: pad,
                right: client.right,
                bottom: client.bottom,
            };
            DrawTextW(
                dc,
                head.as_mut_ptr(),
                -1,
                &mut rect,
                DT_CENTER | DT_TOP | DT_SINGLELINE | DT_NOPREFIX,
            );
            SelectObject(dc, sub_font as _);
            SetTextColor(dc, SUBLINE_COLOR);
            let mut sub = wide(subline());
            rect.top = pad + head_h + 2 * scale / 96;
            DrawTextW(
                dc,
                sub.as_mut_ptr(),
                -1,
                &mut rect,
                DT_CENTER | DT_TOP | DT_SINGLELINE | DT_NOPREFIX,
            );
            SelectObject(dc, old);
            EndPaint(hwnd, &ps);
        }
    }

    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_NCHITTEST => HTTRANSPARENT as LRESULT,
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            WM_PAINT => {
                paint(hwnd);
                0
            }
            SHOW => {
                if SHARED.get().is_some_and(recent) {
                    show(hwnd);
                }
                0
            }
            HIDE => {
                // SAFETY: the window belongs to this thread.
                unsafe { ShowWindow(hwnd, SW_HIDE) };
                0
            }
            WM_TIMER => {
                let visible = unsafe { IsWindowVisible(hwnd) } != 0;
                // The stop latch trips on its own thread; hide at once even
                // when no tool call is running.
                if autogui::stop::chord_down()
                    && let Some(shared) = SHARED.get()
                {
                    shared.last.store(0, Ordering::Release);
                }
                if SHARED.get().is_some_and(recent) {
                    // Re-assert topmost so later topmost windows do not cover it.
                    show(hwnd);
                } else if visible {
                    // SAFETY: the window belongs to this thread.
                    unsafe { ShowWindow(hwnd, SW_HIDE) };
                }
                0
            }
            // SAFETY: default handling for every other message.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    /// The overlay thread: creates the window, then pumps messages forever.
    fn run() {
        // SAFETY: Win32 window creation and a message loop on this thread;
        // every pointer passed lives for the duration of its call.
        unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide("AutoGuiComputerUseNotice");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            if RegisterClassExW(&wc) == 0 {
                return;
            }
            let title = wide("");
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return;
            }
            SetLayeredWindowAttributes(hwnd, 0, ALPHA, LWA_ALPHA);
            // Keep the notice out of every screenshot (Windows 10 2004+).
            // Without that guarantee it would corrupt captures, template
            // matching and wait_change, so there is no notice at all.
            if SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) == 0 {
                DestroyWindow(hwnd);
                return;
            }
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            let head_font = font(20 * dpi / 96, 700);
            let sub_font = font(13 * dpi / 96, 400);
            HEAD_FONT.store(head_font as isize, Ordering::Relaxed);
            SUB_FONT.store(sub_font as isize, Ordering::Relaxed);
            let head = wide(HEADLINE);
            let sub = wide(subline());
            let (head_w, head_h) = extent(hwnd, head_font, &head);
            let (sub_w, sub_h) = extent(hwnd, sub_font, &sub);
            HEAD_HEIGHT.store(head_h, Ordering::Relaxed);
            let pad = 8 * dpi / 96;
            let width = head_w.max(sub_w) + 4 * pad;
            let height = head_h + sub_h + 2 * pad + 2 * dpi / 96;
            let x = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
            let y = 12 * dpi / 96;
            SetWindowPos(hwnd, HWND_TOPMOST, x, y, width, height, SWP_NOACTIVATE);
            let radius = 14 * dpi / 96;
            SetWindowRgn(
                hwnd,
                CreateRoundRectRgn(0, 0, width + 1, height + 1, radius, radius),
                1,
            );
            SetTimer(hwnd, 1, TICK_MS, None);
            if let Some(shared) = SHARED.get() {
                shared.hwnd.store(hwnd as isize, Ordering::Release);
                if recent(shared) {
                    show(hwnd);
                }
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_text_and_timing() {
        assert_eq!(HEADLINE, "Computer use! Don't move!");
        assert!(subline().contains("Ctrl+Alt+F12"));
        assert!(IDLE >= Duration::from_secs(5));
    }

    #[test]
    fn unknown_windows_are_not_the_overlay() {
        assert!(!is_overlay(0));
        assert!(!is_overlay(0x1234));
    }
}
