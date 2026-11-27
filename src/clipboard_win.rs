//! Windows clipboard access through `user32` / `kernel32` (SPEC §20).

use std::ptr;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{GetLastError, GlobalFree};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};

use crate::clipboard::{decode_utf16_until_nul, encode_utf16_nul};
use crate::error::{Error, Result};

const CF_UNICODETEXT: u32 = 13;
const OPEN_ATTEMPTS: u32 = 5;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(20);

fn step_error(step: &str) -> Error {
    // SAFETY: GetLastError has no preconditions.
    let code = unsafe { GetLastError() };
    Error::Clipboard(format!("{step} failed (Win32 error {code})"))
}

/// Holds the clipboard open; closes it on every path when dropped.
struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<Self> {
        for attempt in 0..OPEN_ATTEMPTS {
            // SAFETY: a null owner window is allowed; the guard closes the clipboard.
            if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
                return Ok(Self);
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                thread::sleep(OPEN_RETRY_DELAY);
            }
        }
        Err(step_error("OpenClipboard"))
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: this guard opened the clipboard on this thread.
        unsafe {
            CloseClipboard();
        }
    }
}

pub(crate) fn get_text() -> Result<Option<String>> {
    // SAFETY: the format id is a documented constant; no handles are involved.
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
        return Ok(None);
    }
    let _guard = ClipboardGuard::open()?;
    // SAFETY: the clipboard is open on this thread.
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if handle.is_null() {
        return Err(step_error("GetClipboardData"));
    }
    // SAFETY: handle is a global memory handle owned by the clipboard.
    let locked = unsafe { GlobalLock(handle) } as *const u16;
    if locked.is_null() {
        return Err(step_error("GlobalLock"));
    }
    // SAFETY: the block is locked and holds GlobalSize bytes; the slice is
    // bounded by that size and decoding stops at the first NUL.
    let text = unsafe {
        let capacity = GlobalSize(handle) / std::mem::size_of::<u16>();
        let slice = std::slice::from_raw_parts(locked, capacity);
        decode_utf16_until_nul(slice)
    };
    // SAFETY: unlocks the block locked above; the return value is not an error signal.
    unsafe {
        GlobalUnlock(handle);
    }
    Ok(Some(text))
}

pub(crate) fn set_text(text: &str) -> Result<()> {
    let units = encode_utf16_nul(text);
    let bytes = units.len() * std::mem::size_of::<u16>();
    let _guard = ClipboardGuard::open()?;
    // SAFETY: the clipboard is open on this thread.
    if unsafe { EmptyClipboard() } == 0 {
        return Err(step_error("EmptyClipboard"));
    }
    // SAFETY: allocates a movable global block; freed below on failure.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    if handle.is_null() {
        return Err(step_error("GlobalAlloc"));
    }
    // SAFETY: handle is a fresh block of `bytes` bytes.
    let dest = unsafe { GlobalLock(handle) } as *mut u16;
    if dest.is_null() {
        // SAFETY: the block was allocated above and has not been shared.
        unsafe {
            GlobalFree(handle);
        }
        return Err(step_error("GlobalLock"));
    }
    // SAFETY: dest has room for `units.len()` u16 slots in the locked block.
    unsafe {
        ptr::copy_nonoverlapping(units.as_ptr(), dest, units.len());
        GlobalUnlock(handle);
    }
    // SAFETY: on success the system owns the block; on failure it is still ours.
    let stored = unsafe { SetClipboardData(CF_UNICODETEXT, handle) };
    if stored.is_null() {
        let error = step_error("SetClipboardData");
        // SAFETY: the clipboard refused the block, so this code still owns it.
        unsafe {
            GlobalFree(handle);
        }
        return Err(error);
    }
    Ok(())
}
