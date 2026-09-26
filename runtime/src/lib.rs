//! Versioned C boundary: no Rust allocator or unwinding crosses the DLL boundary.
#[path = "../../examples/app_control.rs"]
mod app;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::ffi::c_void;

thread_local! { static SESSION: RefCell<app::mcp_tools::Session> = RefCell::new(Default::default()); }
type Check = unsafe extern "C" fn(*mut c_void) -> bool;
type Reply = unsafe extern "C" fn(*mut c_void, *const u8, usize);

#[unsafe(no_mangle)]
pub extern "C" fn autogui_abi_version() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn autogui_cli_main() -> i32 {
    if std::panic::catch_unwind(app::main).is_ok() {
        0
    } else {
        1
    }
}

/// # Safety
/// Input must reference len readable bytes. Callback/context must remain valid
/// during the synchronous call. Reply bytes are borrowed only inside callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn autogui_call_json(
    input: *const u8,
    len: usize,
    check: Check,
    reply: Reply,
    context: *mut c_void,
) -> i32 {
    if input.is_null() || len > 1_048_576 {
        return 1;
    }
    let result = std::panic::catch_unwind(|| {
        let bytes = unsafe { std::slice::from_raw_parts(input, len) };
        let request: Value = serde_json::from_slice(bytes).map_err(std::io::Error::other)?;
        let name = request["name"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("missing name"))?;
        let check = || {
            if unsafe { check(context) } {
                Ok(())
            } else {
                Err(autogui::Error::Input(
                    "request cancelled or client disconnected".into(),
                ))
            }
        };
        let value = SESSION.with(|s| {
            s.borrow_mut()
                .call(name, request["arguments"].clone(), &check)
        });
        let value = match value {
            Ok(v) => v,
            Err(e) => json!({"content":[{"type":"text","text":e.to_string()}],"isError":true}),
        };
        let bytes = serde_json::to_vec(&value).map_err(std::io::Error::other)?;
        unsafe { reply(context, bytes.as_ptr(), bytes.len()) };
        Ok::<(), std::io::Error>(())
    });
    if matches!(result, Ok(Ok(()))) { 0 } else { 1 }
}

#[unsafe(no_mangle)]
pub extern "C" fn autogui_session_close() {
    let _ = std::panic::catch_unwind(|| SESSION.with(|s| *s.borrow_mut() = Default::default()));
}
