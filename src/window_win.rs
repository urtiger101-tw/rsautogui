//! Win32 implementation of window control (Windows only).

use std::time::Duration;

use crate::dpi::DpiGuard;
use crate::error::{Error, Result};
use crate::types::Rect;
use crate::window::{Window, WindowSettings};
use std::time::Instant;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, RECT, SetLastError};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, HWND_TOP, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
    PostMessageW, SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SWP_ASYNCWINDOWPOS,
    SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindowAsync, WM_CLOSE,
};

fn from_hwnd(hwnd: HWND) -> Result<Window> {
    let mut process_id = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    if thread_id == 0 || process_id == 0 {
        return Err(Error::WindowNotFound);
    }
    Ok(Window {
        hwnd: hwnd as isize,
        process_id,
        thread_id,
        settings: WindowSettings::default(),
    })
}

fn ensure_window(window: &Window) -> Result<HWND> {
    let hwnd = window.hwnd as HWND;
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 {
        return Err(Error::WindowNotFound);
    }
    let mut process_id = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    if process_id != window.process_id || thread_id != window.thread_id {
        return Err(Error::WindowNotFound);
    }
    Ok(hwnd)
}

fn api_error(window: &Window, operation: &str) -> Error {
    let error = std::io::Error::last_os_error();
    if ensure_window(window).is_err() {
        Error::WindowNotFound
    } else {
        Error::Input(format!("{operation}: {error}"))
    }
}

pub fn title(window: &Window) -> Result<String> {
    let hwnd = ensure_window(window)?;
    unsafe {
        SetLastError(0);
    }
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length == 0 && unsafe { GetLastError() } != 0 {
        return Err(api_error(window, "GetWindowTextLengthW"));
    }
    let mut buffer = vec![0u16; length.max(0) as usize + 1];
    unsafe {
        SetLastError(0);
    }
    let read = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if read == 0 {
        if unsafe { GetLastError() } != 0 {
            return Err(api_error(window, "GetWindowTextW"));
        }
        ensure_window(window)?;
    }
    Ok(String::from_utf16_lossy(&buffer[..read.max(0) as usize]))
}

pub fn rect(window: &Window) -> Result<Rect> {
    let _dpi = DpiGuard::enter()?;
    let hwnd = ensure_window(window)?;
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return Err(api_error(window, "GetWindowRect"));
    }
    Ok(Rect::new(
        rect.left,
        rect.top,
        rect.right.saturating_sub(rect.left),
        rect.bottom.saturating_sub(rect.top),
    ))
}

pub fn is_minimized(window: &Window) -> Result<bool> {
    Ok(unsafe { IsIconic(ensure_window(window)?) } != 0)
}
pub fn is_maximized(window: &Window) -> Result<bool> {
    Ok(unsafe { IsZoomed(ensure_window(window)?) } != 0)
}
pub fn is_active(window: &Window) -> Result<bool> {
    Ok(unsafe { GetForegroundWindow() } == ensure_window(window)?)
}

fn wait_state(
    window: &Window,
    operation: &str,
    mut done: impl FnMut(HWND) -> Result<bool>,
) -> Result<()> {
    let start = Instant::now();
    loop {
        check_failsafe(&window.settings)?;
        if done(ensure_window(window)?)? {
            return Ok(());
        }
        if start.elapsed() >= Duration::from_millis(500) {
            return Err(Error::Input(format!(
                "{operation}: requested window state was not reached within 500 ms"
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ensure_restored(window: &Window) -> Result<()> {
    if is_minimized(window)? {
        restore(window)?;
    }
    if is_maximized(window)? {
        restore(window)?;
    }
    Ok(())
}

pub fn activate(window: &Window) -> Result<()> {
    let hwnd = ensure_window(window)?;
    if is_minimized(window)? {
        restore(window)?;
    }
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        show(window)?;
    }
    if unsafe { GetForegroundWindow() } == hwnd {
        return Ok(());
    }
    if unsafe { SetForegroundWindow(hwnd) } == 0 {
        return Err(Error::PermissionDenied("SetForegroundWindow"));
    }
    // Cross-thread activation can be asynchronous; the return from
    // SetForegroundWindow alone is not evidence that keyboard focus arrived.
    match wait_state(window, "SetForegroundWindow", |hwnd| {
        Ok(unsafe { GetForegroundWindow() } == hwnd)
    }) {
        Err(Error::Input(_)) => Err(Error::PermissionDenied("SetForegroundWindow")),
        result => result,
    }
}
fn request_show(window: &Window, state: i32) -> Result<()> {
    if unsafe { ShowWindowAsync(ensure_window(window)?, state) } == 0 {
        return Err(api_error(window, "ShowWindowAsync"));
    }
    Ok(())
}

pub fn maximize(window: &Window) -> Result<()> {
    request_show(window, SW_MAXIMIZE)?;
    wait_state(
        window,
        "maximize",
        |hwnd| Ok(unsafe { IsZoomed(hwnd) } != 0),
    )
}
pub fn minimize(window: &Window) -> Result<()> {
    request_show(window, SW_MINIMIZE)?;
    wait_state(
        window,
        "minimize",
        |hwnd| Ok(unsafe { IsIconic(hwnd) } != 0),
    )
}
pub fn restore(window: &Window) -> Result<()> {
    let was_minimized = is_minimized(window)?;
    request_show(window, SW_RESTORE)?;
    wait_state(window, "restore", |hwnd| {
        // Unminimizing a previously maximized window restores maximization.
        Ok(unsafe { IsIconic(hwnd) } == 0 && (was_minimized || unsafe { IsZoomed(hwnd) } == 0))
    })
}
pub fn close(window: &Window) -> Result<()> {
    if unsafe { PostMessageW(ensure_window(window)?, WM_CLOSE, 0, 0) } == 0 {
        return Err(api_error(window, "PostMessageW(WM_CLOSE)"));
    }
    Ok(()) // A close request may be vetoed by the target application's save prompt.
}
pub fn hide(window: &Window) -> Result<()> {
    request_show(window, SW_HIDE)?;
    wait_state(window, "hide", |hwnd| {
        Ok(unsafe { IsWindowVisible(hwnd) } == 0)
    })
}
pub fn show(window: &Window) -> Result<()> {
    request_show(window, SW_SHOW)?;
    wait_state(window, "show", |hwnd| {
        Ok(unsafe { IsWindowVisible(hwnd) } != 0)
    })
}
pub fn move_to(window: &Window, left: i32, top: i32) -> Result<()> {
    let _dpi = DpiGuard::enter()?;
    ensure_restored(window)?;
    let current = rect(window)?;
    if unsafe {
        SetWindowPos(
            ensure_window(window)?,
            HWND_TOP,
            left,
            top,
            current.width,
            current.height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        )
    } == 0
    {
        return Err(api_error(window, "SetWindowPos(move)"));
    }
    wait_state(window, "move", |_| {
        let current = rect(window)?;
        Ok(current.left == left && current.top == top)
    })
}
pub fn resize_to(window: &Window, width: i32, height: i32) -> Result<()> {
    if width <= 0 || height <= 0 {
        return Err(Error::InvalidArgument(
            "window width and height must be positive",
        ));
    }
    let _dpi = DpiGuard::enter()?;
    ensure_restored(window)?;
    let current = rect(window)?;
    if unsafe {
        SetWindowPos(
            ensure_window(window)?,
            HWND_TOP,
            current.left,
            current.top,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        )
    } == 0
    {
        return Err(api_error(window, "SetWindowPos(resize)"));
    }
    wait_state(
        window,
        "resize (application size constraints may apply)",
        |_| {
            let current = rect(window)?;
            Ok(current.width == width && current.height == height)
        },
    )
}

pub fn check_failsafe(settings: &WindowSettings) -> Result<()> {
    if settings.failsafe {
        let point = crate::dpi::cursor_position()?;
        if settings.failsafe_points.contains(&point) {
            return Err(Error::FailSafe(point));
        }
    }
    Ok(())
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }
    match from_hwnd(hwnd) {
        Ok(window) if rect(&window).is_ok_and(|rect| rect.width > 0 && rect.height > 0) => {
            // The caller owns this vector for the synchronous EnumWindows call.
            unsafe { &mut *(lparam as *mut Vec<Window>) }.push(window);
        }
        _ => {}
    }
    1
}
pub fn get_all_windows() -> Result<Vec<Window>> {
    let _dpi = DpiGuard::enter()?;
    let mut windows = Vec::<Window>::new();
    if unsafe { EnumWindows(Some(enum_proc), &mut windows as *mut _ as LPARAM) } == 0 {
        return Err(Error::Input(format!(
            "EnumWindows: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(windows)
}
pub fn get_active_window() -> Result<Option<Window>> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return Ok(None);
    }
    match from_hwnd(hwnd) {
        Ok(window) => Ok(Some(window)),
        Err(Error::WindowNotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SetWindowTextW, WS_OVERLAPPED,
    };

    struct OwnedWindow(HWND);
    impl Drop for OwnedWindow {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }

    #[test]
    fn owned_hidden_window_queries_and_stale_handle_errors() {
        // Hidden window on this test thread; no input or foreground changes.
        let _dpi = DpiGuard::enter().unwrap();
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "AutoGui hidden fixture\0".encode_utf16().collect();
        let native = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPED,
                40,
                50,
                320,
                200,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!native.is_null());
        let owned = OwnedWindow(native);
        let window = from_hwnd(native).unwrap();
        assert_eq!(window.title().unwrap(), "AutoGui hidden fixture");
        assert_eq!(window.box_rect().unwrap(), Rect::new(40, 50, 320, 200));
        assert_ne!(unsafe { SetWindowTextW(native, [0u16].as_ptr()) }, 0);
        assert_eq!(window.title().unwrap(), "");
        let mut wrong_identity = window.clone();
        wrong_identity.process_id = wrong_identity.process_id.wrapping_add(1);
        assert!(matches!(wrong_identity.title(), Err(Error::WindowNotFound)));
        drop(owned);
        assert!(matches!(window.title(), Err(Error::WindowNotFound)));
        assert!(matches!(window.box_rect(), Err(Error::WindowNotFound)));
    }
}
