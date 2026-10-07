#![cfg(all(windows, feature = "os", feature = "opencv"))]

use autogui::{AutoGui, Error, LocateOptions, Point, Rect};
#[cfg(feature = "agent")]
#[path = "support/mcp_client.rs"]
mod mcp_client;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, UpdateWindow},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::EM_SETSEL,
        HiDpi::{
            AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_CONTEXT_UNAWARE,
            GetThreadDpiAwarenessContext, SetThreadDpiAwarenessContext,
        },
        WindowsAndMessaging::*,
    },
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

struct DpiScope(DPI_AWARENESS_CONTEXT);
impl DpiScope {
    fn set(context: DPI_AWARENESS_CONTEXT) -> Self {
        let old = unsafe { SetThreadDpiAwarenessContext(context) };
        assert!(!old.is_null());
        Self(old)
    }
}
impl Drop for DpiScope {
    fn drop(&mut self) {
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            }
        }
        WM_COMMAND if wparam & 0xffff == 100 && (wparam >> 16) & 0xffff == 0 => {
            let counter = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const AtomicUsize;
            if !counter.is_null() {
                unsafe { &*counter }.fetch_add(1, Ordering::SeqCst);
            }
            return 0;
        }
        WM_CLOSE => {
            unsafe {
                DestroyWindow(hwnd);
            }
            return 0;
        }
        WM_COMMAND if wparam & 0xffff == 102 => {
            // The fixture owns this application shortcut; bare classic EDIT
            // controls do not supply Ctrl+A without application support.
            unsafe {
                SendMessageW(GetDlgItem(hwnd, 101), EM_SETSEL, 0, -1);
            }
            return 0;
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[derive(Clone, Copy)]
struct Handles {
    window: isize,
    button: isize,
    edit: isize,
}

struct Fixture {
    handles: Handles,
    clicks: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
    previous_foreground: isize,
    previous_pointer: Point,
}

impl Fixture {
    fn open(title: &str, previous_pointer: Point) -> Self {
        let previous_foreground = unsafe { GetForegroundWindow() } as isize;
        let title = title.to_string();
        let clicks = Arc::new(AtomicUsize::new(0));
        let callback_clicks = Arc::clone(&clicks);
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let _dpi = DpiScope::set(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let class_name = wide(&title);
            let module = unsafe { GetModuleHandleW(std::ptr::null()) };
            let class = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: module,
                lpszClassName: class_name.as_ptr(),
                hbrBackground: (COLOR_WINDOW + 1) as _,
                hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
                ..Default::default()
            };
            assert_ne!(unsafe { RegisterClassW(&class) }, 0);
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    class_name.as_ptr(),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    220,
                    180,
                    520,
                    340,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    module,
                    Arc::as_ptr(&callback_clicks) as *const _,
                )
            };
            assert!(!hwnd.is_null());
            let button = unsafe {
                CreateWindowExW(
                    0,
                    wide("BUTTON").as_ptr(),
                    wide("辨識測試按鈕 7X").as_ptr(),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
                    40,
                    60,
                    190,
                    60,
                    hwnd,
                    100usize as _,
                    module,
                    std::ptr::null(),
                )
            };
            let edit = unsafe {
                CreateWindowExW(
                    WS_EX_CLIENTEDGE,
                    wide("EDIT").as_ptr(),
                    wide("").as_ptr(),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                    40,
                    155,
                    350,
                    40,
                    hwnd,
                    101usize as _,
                    module,
                    std::ptr::null(),
                )
            };
            assert!(!button.is_null() && !edit.is_null());
            // Win32 probes this buffer with DWORD alignment although ACCEL's
            // packed Rust ABI only guarantees two-byte alignment.
            #[repr(align(4))]
            struct AlignedAccelerator(ACCEL);
            let shortcut = AlignedAccelerator(ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: u16::from(b'A'),
                cmd: 102,
            });
            let accelerator = unsafe { CreateAcceleratorTableW(&shortcut.0, 1) };
            assert!(
                !accelerator.is_null(),
                "CreateAcceleratorTableW: {}",
                std::io::Error::last_os_error()
            );
            for control in [button, edit] {
                unsafe {
                    SendMessageW(
                        control,
                        WM_SETFONT,
                        GetStockObject(DEFAULT_GUI_FONT) as usize,
                        1,
                    );
                    UpdateWindow(control);
                }
            }
            unsafe {
                UpdateWindow(hwnd);
            }
            sender
                .send(Handles {
                    window: hwnd as isize,
                    button: button as isize,
                    edit: edit as isize,
                })
                .unwrap();
            let mut message = MSG::default();
            while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
                if unsafe { TranslateAcceleratorW(hwnd, accelerator, &message) } != 0 {
                    continue;
                }
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            unsafe {
                DestroyAcceleratorTable(accelerator);
                UnregisterClassW(class_name.as_ptr(), module);
            }
        });
        let handles = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("create native fixture");
        Self {
            handles,
            clicks,
            thread: Some(thread),
            previous_foreground,
            previous_pointer,
        }
    }

    fn restore_desktop(&self) {
        let _dpi = DpiScope::set(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        unsafe {
            // Do not steal focus back if the user has switched to another app.
            if GetForegroundWindow() == self.handles.window as HWND {
                let mut point = windows_sys::Win32::Foundation::POINT::default();
                let width = GetSystemMetrics(SM_CXSCREEN);
                let height = GetSystemMetrics(SM_CYSCREEN);
                if GetCursorPos(&mut point) != 0
                    && !((point.x == 0 || point.x == width - 1)
                        && (point.y == 0 || point.y == height - 1))
                {
                    SetCursorPos(self.previous_pointer.x, self.previous_pointer.y);
                }
                if IsWindow(self.previous_foreground as HWND) != 0 {
                    SetForegroundWindow(self.previous_foreground as HWND);
                }
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.restore_desktop();
        unsafe {
            if IsWindow(self.handles.window as HWND) != 0 {
                PostMessageW(self.handles.window as HWND, WM_CLOSE, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn native_rect(hwnd: isize) -> Rect {
    let _dpi = DpiScope::set(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let mut rect = RECT::default();
    assert_ne!(unsafe { GetWindowRect(hwnd as HWND, &mut rect) }, 0);
    Rect::new(
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    )
}
fn native_text(hwnd: isize) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd as HWND) };
    let mut buffer = vec![0u16; len as usize + 1];
    let read = unsafe { GetWindowTextW(hwnd as HWND, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..read as usize])
}
fn wait_until(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "fixture did not receive expected operation"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
struct TempFile(std::path::PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
#[ignore = "creates its own Windows fixture, sends real input only to that fixture; requires AUTOGUI_LIVE=1"]
fn recognizes_and_controls_own_native_app() {
    assert_eq!(std::env::var("AUTOGUI_LIVE").ok().as_deref(), Some("1"));
    // Deliberately emulate a DPI-unaware embedding application.
    let _caller_dpi = DpiScope::set(DPI_AWARENESS_CONTEXT_UNAWARE);
    let mut gui = AutoGui::new().expect("connect desktop backend");
    gui.settings_mut().pause = Duration::from_millis(10);
    let pointer = gui.position().unwrap();
    assert!(
        !gui.settings().failsafe_points.contains(&pointer),
        "cursor is at the fail-safe corner"
    );
    let title = format!("AutoGui recognition fixture {}", std::process::id());
    let fixture = Fixture::open(&title, pointer);
    let windows = gui.get_windows_with_title(&title).unwrap();
    assert_eq!(windows.len(), 1);
    let window = &windows[0];
    assert_eq!(window.hwnd(), fixture.handles.window);
    window.resize_to(640, 480).unwrap();
    window.move_to(200, 150).unwrap();
    assert_eq!(
        window.box_rect().unwrap(),
        native_rect(fixture.handles.window)
    );
    assert_eq!(window.box_rect().unwrap(), Rect::new(200, 150, 640, 480));
    window.minimize().unwrap();
    assert!(window.is_minimized().unwrap());
    window
        .activate()
        .expect("Windows must allow fixture activation before input");
    assert!(!window.is_minimized().unwrap() && window.is_active().unwrap());
    std::thread::sleep(Duration::from_millis(200));

    let button_rect = native_rect(fixture.handles.button);
    let mut template = gui.screenshot_region(button_rect).unwrap();
    assert_eq!(
        (template.width, template.height),
        (button_rect.width as u32, button_rect.height as u32)
    );
    assert!(
        template
            .pixels
            .as_chunks::<3>()
            .0
            .iter()
            .any(|p| p != &template.pixels[..3]),
        "fixture button must be visibly rendered"
    );
    let pixel = template.get_pixel(2, 2).unwrap();
    template
        .set_pixel(2, 2, autogui::Rgb::new(pixel.r ^ 1, pixel.g, pixel.b))
        .unwrap();
    let template_path =
        std::env::temp_dir().join(format!("autogui-fixture-{}.png", std::process::id()));
    let _temp = TempFile(template_path.clone());
    template.save(&template_path).unwrap();
    let options = LocateOptions {
        confidence: Some(0.98),
        region: Some(window.box_rect().unwrap()),
        min_search_time: Duration::from_secs(1),
        ..Default::default()
    };
    let found = gui
        .locate_screenshot_on_screen(&template, &options)
        .unwrap();
    assert_eq!(found, button_rect);
    assert!(window.is_active().unwrap());
    gui.click_image(&template_path, &options).unwrap();
    wait_until(|| fixture.clicks.load(Ordering::SeqCst) == 1);

    let edit = autogui::center(native_rect(fixture.handles.edit));
    assert!(window.is_active().unwrap());
    gui.click_xy(edit.x, edit.y).unwrap();
    gui.write("Rust 畫面辨識測試 123", Duration::from_millis(5))
        .unwrap();
    wait_until(|| native_text(fixture.handles.edit) == "Rust 畫面辨識測試 123");
    {
        let mut held = gui.hold(&["ctrl"]).unwrap();
        held.press("a").unwrap();
    }
    gui.write("驗證完成", Duration::ZERO).unwrap();
    wait_until(|| native_text(fixture.handles.edit) == "驗證完成");
    println!(
        "PASS native workflow: physical coordinates, restore/activate, confidence recognition, click receipt, Unicode text and held modifier"
    );

    if let Ok(executable) = std::env::var("AUTOGUI_CONTROL_EXE") {
        let output = std::process::Command::new(&executable)
            .args(["type", &title, "+CLI"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        wait_until(|| native_text(fixture.handles.edit) == "驗證完成+CLI");
        let output = std::process::Command::new(&executable)
            .args([
                "click",
                &title,
                template_path.to_str().unwrap(),
                "0.95",
                "2",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        wait_until(|| fixture.clicks.load(Ordering::SeqCst) == 2);
        println!(
            "PASS app_control CLI: {}",
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }

    #[cfg(feature = "agent")]
    let mut mcp_session = if let Ok(executable) = std::env::var("AUTOGUI_MCP_EXE") {
        use base64::Engine;
        use serde_json::json;
        let mut client = mcp_client::Client::start(&executable);
        let list = client.tool("windows_list", json!({"title":title}));
        assert_eq!(list["windows"].as_array().unwrap().len(), 1);
        let target = list["windows"][0]["target"].as_str().unwrap().to_owned();
        let stale = target;
        let list = client.tool("windows_list", json!({"title":title}));
        let target = list["windows"][0]["target"].as_str().unwrap().to_owned();
        assert_eq!(
            client.raw_tool("window_control", json!({"target":stale,"action":"inspect"}))["isError"],
            true
        );
        assert_eq!(
            client.raw_tool(
                "window_control",
                json!({"target":target,"action":"move","x":1})
            )["isError"],
            true
        );
        client.tool(
            "window_control",
            json!({"target":target,"action":"move","x":220,"y":170}),
        );
        assert_eq!(native_rect(fixture.handles.window).left, 220);
        client.tool(
            "window_control",
            json!({"target":target,"action":"resize","width":680,"height":500}),
        );
        assert_eq!(native_rect(fixture.handles.window).width, 680);
        client.tool("window_control", json!({"target":target,"action":"hide"}));
        assert_eq!(
            unsafe { IsWindowVisible(fixture.handles.window as HWND) },
            0
        );
        client.tool("window_control", json!({"target":target,"action":"show"}));
        assert_ne!(
            unsafe { IsWindowVisible(fixture.handles.window as HWND) },
            0
        );
        client.tool(
            "window_control",
            json!({"target":target,"action":"minimize"}),
        );
        assert!(window.is_minimized().unwrap());
        client.tool(
            "window_control",
            json!({"target":target,"action":"maximize"}),
        );
        assert!(window.is_maximized().unwrap());
        client.tool(
            "window_control",
            json!({"target":target,"action":"restore"}),
        );
        client.tool(
            "window_control",
            json!({"target":target,"action":"move","x":200,"y":150}),
        );
        client.tool(
            "window_control",
            json!({"target":target,"action":"resize","width":640,"height":480}),
        );
        let captured = client.raw_tool("capture", json!({"target":target}));
        assert_eq!(captured["isError"], false, "{captured}");
        let data = base64::engine::general_purpose::STANDARD
            .decode(captured["content"][1]["data"].as_str().unwrap())
            .unwrap();
        let image = image::load_from_memory(&data).unwrap();
        assert_eq!((image.width(), image.height()), (640, 480));
        let count = fixture.clicks.load(Ordering::SeqCst);
        let cancelled = client.cancelled_tool(
            "locate",
            json!({"target":target,"template":template_path,"exact":true,"scale":2,"timeout":30}),
        );
        assert_eq!(cancelled["isError"], true, "{cancelled}");
        assert!(
            cancelled["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("cancelled"),
            "{cancelled}"
        );
        client.tool(
            "locate",
            json!({"target":target,"template":template_path,"confidence":0.95,"timeout":2}),
        );
        client.tool(
            "click_image",
            json!({"target":target,"template":template_path,"confidence":0.95,"timeout":2}),
        );
        wait_until(|| fixture.clicks.load(Ordering::SeqCst) == count + 1);
        let edit = autogui::center(native_rect(fixture.handles.edit));
        gui.click_xy(edit.x, edit.y).unwrap();
        client.tool("hotkey", json!({"target":target,"keys":["ctrl","a"]}));
        client.tool(
            "type_text",
            json!({"target":target,"text":"MCP 原生 DLL 驗證完成"}),
        );
        wait_until(|| native_text(fixture.handles.edit) == "MCP 原生 DLL 驗證完成");
        println!(
            "PASS MCP via lazy DLL: lifecycle, scoped/stale targets, Win32 move/resize/hide/show/minimize/maximize/restore, PNG, locate/click receipt, hotkey and Unicode text"
        );
        Some((client, target))
    } else {
        None
    };

    if let Ok(directory) = std::env::var("AUTOGUI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        gui.screenshot_region(window.box_rect().unwrap())
            .unwrap()
            .save(std::path::Path::new(&directory).join("fixture-window.png"))
            .unwrap();
        template
            .save(std::path::Path::new(&directory).join("fixture-template.png"))
            .unwrap();
    }
    // A cleared title must not be replaced by a stale cached title.
    unsafe {
        SetWindowTextW(fixture.handles.window as HWND, wide("").as_ptr());
    }
    assert_eq!(window.title().unwrap(), "");
    assert_ne!(
        unsafe {
            AreDpiAwarenessContextsEqual(
                GetThreadDpiAwarenessContext(),
                DPI_AWARENESS_CONTEXT_UNAWARE,
            )
        },
        0,
        "library restored caller DPI context"
    );
    fixture.restore_desktop();
    #[cfg(feature = "agent")]
    if let Some((client, target)) = mcp_session.as_mut() {
        let result = client.tool(
            "window_control",
            serde_json::json!({"target":target,"action":"close"}),
        );
        assert_eq!(result["close_requested"], true);
    } else {
        window.close().unwrap();
    }
    #[cfg(not(feature = "agent"))]
    window.close().unwrap();
    wait_until(|| unsafe { IsWindow(fixture.handles.window as HWND) } == 0);
    assert!(matches!(window.title(), Err(Error::WindowNotFound)));
    assert!(matches!(window.activate(), Err(Error::WindowNotFound)));
    // Fixture Drop joins its GUI thread and never closes another application's window.
}

#[cfg(feature = "agent")]
#[test]
#[ignore = "requires AUTOGUI_LIVE=1, AUTOGUI_DISPLAY and AUTOGUI_MCP_EXE; real input only to its own fixture"]
fn controls_own_app_on_selected_display() {
    use autogui::Display;
    use base64::Engine;
    use serde_json::json;
    assert_eq!(std::env::var("AUTOGUI_LIVE").ok().as_deref(), Some("1"));
    let name = std::env::var("AUTOGUI_DISPLAY").expect("select an explicit non-primary display");
    let executable = std::env::var("AUTOGUI_MCP_EXE").expect("provide a built CLI/MCP executable");
    let _caller_dpi = DpiScope::set(DPI_AWARENESS_CONTEXT_UNAWARE);
    let display = Display::all()
        .unwrap()
        .into_iter()
        .find(|d| d.name() == name)
        .unwrap();
    assert!(
        !display.is_primary(),
        "this fixture verifies a secondary display"
    );
    let bounds = display.bounds();
    let mut gui = AutoGui::new().unwrap();
    gui.settings_mut().pause = Duration::from_millis(10);
    let pointer = gui.position().unwrap();
    assert!(!gui.settings().failsafe_points.contains(&pointer));
    let title = format!("rsautogui virtual display fixture {}", std::process::id());
    let fixture = Fixture::open(&title, pointer);
    let window = gui.get_windows_with_title(&title).unwrap().remove(0);
    window.resize_to(640, 480).unwrap();
    let command = |args: &[&str]| {
        let result = std::process::Command::new(&executable)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        println!("{}", String::from_utf8_lossy(&result.stdout).trim());
    };
    command(&["display", "move", &name, &title]);
    let region = window.box_rect().unwrap();
    assert_eq!(region.intersect(&bounds), Some(region));
    assert_eq!(region, native_rect(fixture.handles.window));
    window.activate().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    // Real SendInput quantization, including edge pixels, from an unaware caller.
    for x in [
        bounds.left,
        bounds.left + 1,
        bounds.left + bounds.width / 2,
        bounds.left + bounds.width - 2,
        bounds.left + bounds.width - 1,
    ] {
        for y in [
            bounds.top,
            bounds.top + 1,
            bounds.top + bounds.height / 2,
            bounds.top + bounds.height - 2,
            bounds.top + bounds.height - 1,
        ] {
            let point = Point::new(x, y);
            assert!(!gui.settings().failsafe_points.contains(&point));
            println!("Checking virtual display cursor {point:?}");
            gui.move_to_display(&display, point).unwrap();
            assert_eq!(gui.position().unwrap(), point);
        }
    }
    let button = native_rect(fixture.handles.button);
    let template = gui.screenshot_display_region(&display, button).unwrap();
    assert!(
        template
            .pixels
            .as_chunks::<3>()
            .0
            .iter()
            .any(|p| p != &template.pixels[..3])
    );
    let template_path = std::env::temp_dir().join(format!(
        "rsautogui-display-button-{}.png",
        std::process::id()
    ));
    let _temp = TempFile(template_path.clone());
    template.save(&template_path).unwrap();
    command(&[
        "locate",
        &title,
        template_path.to_str().unwrap(),
        "0.98",
        "2",
        "1",
        "--display",
        &name,
    ]);
    command(&[
        "click",
        &title,
        template_path.to_str().unwrap(),
        "0.98",
        "2",
        "1",
        "--display",
        &name,
    ]);
    {
        let _dpi = DpiScope::set(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let point = gui.position().unwrap();
        let hit = unsafe {
            WindowFromPoint(windows_sys::Win32::Foundation::POINT {
                x: point.x,
                y: point.y,
            })
        };
        println!(
            "After CLI: cursor={point:?}, button={:?}, button HWND={}, hit HWND={}, received={}",
            native_rect(fixture.handles.button),
            fixture.handles.button,
            hit as isize,
            fixture.clicks.load(Ordering::SeqCst)
        );
        if let Ok(directory) = std::env::var("AUTOGUI_EVIDENCE_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            gui.screenshot_display_region(&display, window.box_rect().unwrap())
                .unwrap()
                .save(std::path::Path::new(&directory).join("after-cli.png"))
                .unwrap();
        }
    }
    wait_until(|| fixture.clicks.load(Ordering::SeqCst) == 1);

    let mut client = mcp_client::Client::start(&executable);
    let list = client.tool("displays_list", json!({}));
    let selected = list["displays"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_name"] == name)
        .unwrap()["display"]
        .as_str()
        .unwrap()
        .to_owned();
    let windows = client.tool("windows_list", json!({"title":title}));
    assert_eq!(windows["windows"].as_array().unwrap().len(), 1);
    let target = windows["windows"][0]["target"].as_str().unwrap().to_owned();
    // Exercise movement from the primary display through MCP, independently of CLI.
    window.move_to(220, 180).unwrap();
    client.tool(
        "window_to_display",
        json!({"target":target,"display":selected}),
    );
    assert_eq!(
        window.box_rect().unwrap().intersect(&bounds),
        Some(window.box_rect().unwrap())
    );
    let captured = client.raw_tool("capture", json!({"target":target,"display":selected}));
    assert_eq!(captured["isError"], false, "{captured}");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(captured["content"][1]["data"].as_str().unwrap())
        .unwrap();
    let image = image::load_from_memory(&bytes).unwrap();
    assert_eq!((image.width(), image.height()), (640, 480));
    assert_eq!(
        client.raw_tool("capture", json!({"target":target}))["isError"],
        true,
        "default primary capture must not silently select another display"
    );
    assert_eq!(client.raw_tool("display_capture", json!({"display":selected,"region":{"left":bounds.left-1,"top":bounds.top,"width":2,"height":2}}))["isError"], true);
    // Focus and a move across different DPI monitors can change button pixels.
    // Observe the current control instead of treating an earlier frame as current.
    gui.move_to_display(
        &display,
        Point::new(
            bounds.left + bounds.width - 2,
            bounds.top + bounds.height - 2,
        ),
    )
    .unwrap();
    gui.screenshot_display_region(&display, native_rect(fixture.handles.button))
        .unwrap()
        .save(&template_path)
        .unwrap();
    client.tool("locate", json!({"target":target,"display":selected,"template":template_path,"confidence":0.98,"timeout":2}));
    client.tool("click_image", json!({"target":target,"display":selected,"template":template_path,"confidence":0.98,"timeout":2}));
    wait_until(|| fixture.clicks.load(Ordering::SeqCst) == 2);
    gui.click_display(&display, autogui::center(native_rect(fixture.handles.edit)))
        .unwrap();
    client.tool(
        "type_text",
        json!({"target":target,"text":"虛擬螢幕截圖與 MCP 點擊驗證完成"}),
    );
    wait_until(|| native_text(fixture.handles.edit) == "虛擬螢幕截圖與 MCP 點擊驗證完成");
    let full = client.raw_tool("display_capture", json!({"display":selected}));
    assert_eq!(full["isError"], false, "{full}");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(full["content"][1]["data"].as_str().unwrap())
        .unwrap();
    let image = image::load_from_memory(&bytes).unwrap();
    assert_eq!(
        (image.width(), image.height()),
        (bounds.width as u32, bounds.height as u32)
    );
    if let Ok(directory) = std::env::var("AUTOGUI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        let directory = std::path::Path::new(&directory);
        std::fs::write(directory.join("virtual-display-mcp.png"), &bytes).unwrap();
        gui.screenshot_display_region(&display, window.box_rect().unwrap())
            .unwrap()
            .save(directory.join("virtual-display-app.png"))
            .unwrap();
        template
            .save(directory.join("virtual-display-button.png"))
            .unwrap();
        command(&[
            "display",
            "capture",
            &name,
            directory.join("virtual-display-cli.png").to_str().unwrap(),
        ]);
        std::fs::write(directory.join("result.json"), serde_json::to_vec_pretty(&json!({
            "status":"PASS", "device":name, "bounds":{"left":bounds.left,"top":bounds.top,"width":bounds.width,"height":bounds.height},
            "cursor_points_verified":25, "button_clicks_received":fixture.clicks.load(Ordering::SeqCst),
            "text_received":native_text(fixture.handles.edit), "interfaces":["library","CLI","MCP"]
        })).unwrap()).unwrap();
    }
    client.tool("displays_list", json!({}));
    assert_eq!(
        client.raw_tool("display_capture", json!({"display":selected}))["isError"],
        true
    );
    // Display references and window references have separate lifetimes.
    client.tool(
        "window_control",
        json!({"target":target,"action":"inspect"}),
    );
    assert_ne!(
        unsafe {
            AreDpiAwarenessContextsEqual(
                GetThreadDpiAwarenessContext(),
                DPI_AWARENESS_CONTEXT_UNAWARE,
            )
        },
        0
    );
    println!(
        "PASS selected display: real pixels, 25 exact cursor positions, CLI/MCP clicks received, Unicode input, stale refs, bounds rejection and primary default preserved"
    );
}
