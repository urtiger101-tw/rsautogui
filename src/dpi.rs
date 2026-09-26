//! Use physical pixels only for the duration of a native call. Do not change
//! the embedding application's process-wide DPI awareness.
use crate::error::Result;

#[cfg(windows)]
pub(crate) struct DpiGuard {
    previous: windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(windows)]
impl DpiGuard {
    pub(crate) fn enter() -> Result<Self> {
        use windows_sys::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
        };
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if previous.is_null() {
            return Err(crate::error::Error::Input(format!(
                "SetThreadDpiAwarenessContext: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self {
            previous,
            _thread: std::marker::PhantomData,
        })
    }
}

#[cfg(windows)]
impl Drop for DpiGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.previous);
        }
    }
}

#[cfg(windows)]
pub(crate) fn cursor_position() -> Result<crate::types::Point> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, POINT},
        UI::WindowsAndMessaging::GetCursorPos,
    };
    let _dpi = DpiGuard::enter()?;
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) } == 0 {
        let code = unsafe { GetLastError() };
        return if code == 5 {
            Err(crate::error::Error::PermissionDenied(
                "GetCursorPos requires an active, unlocked interactive desktop",
            ))
        } else {
            Err(crate::error::Error::Input(format!(
                "GetCursorPos: {}",
                std::io::Error::from_raw_os_error(code as i32)
            )))
        };
    }
    Ok(crate::types::Point::new(point.x, point.y))
}

#[cfg(not(windows))]
pub(crate) struct DpiGuard;

#[cfg(not(windows))]
impl DpiGuard {
    pub(crate) fn enter() -> Result<Self> {
        Ok(Self)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::HiDpi::*;

    #[test]
    fn nested_scopes_restore_the_callers_dpi_context() {
        let original = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE) };
        assert!(!original.is_null());
        let _restore = DpiGuard {
            previous: original,
            _thread: std::marker::PhantomData,
        };
        {
            let _outer = DpiGuard::enter().unwrap();
            {
                let _inner = DpiGuard::enter().unwrap();
                assert_ne!(
                    unsafe {
                        AreDpiAwarenessContextsEqual(
                            GetThreadDpiAwarenessContext(),
                            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                        )
                    },
                    0
                );
            }
            assert_ne!(
                unsafe {
                    AreDpiAwarenessContextsEqual(
                        GetThreadDpiAwarenessContext(),
                        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                    )
                },
                0
            );
        }
        assert_ne!(
            unsafe {
                AreDpiAwarenessContextsEqual(
                    GetThreadDpiAwarenessContext(),
                    DPI_AWARENESS_CONTEXT_UNAWARE,
                )
            },
            0
        );
    }
}
