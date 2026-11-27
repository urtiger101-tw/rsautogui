//! Live UI Automation test against Notepad (SPEC §18). Opens and closes a
//! real app window, so it is `#[ignore]`d and also requires AUTOGUI_LIVE=1:
//!
//! ```text
//! AUTOGUI_LIVE=1 cargo test --all-features --test live_uia -- --ignored
//! ```
//!
//! It never moves the mouse: values are set through ValuePattern only.
#![cfg(all(windows, feature = "uia", feature = "os"))]

use autogui::{AutoGui, Error, NameMatch, UiAction, UiAutomation, UiSelector, UiSnapshotOptions};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExW;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Top-level windows of class "Notepad" (classic and Windows 11 Notepad).
fn notepads() -> HashSet<isize> {
    let class = wide("Notepad");
    let mut found = HashSet::new();
    let mut after = std::ptr::null_mut();
    loop {
        let hwnd = unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                after,
                class.as_ptr(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return found;
        }
        found.insert(hwnd as isize);
        after = hwnd;
    }
}

#[test]
#[ignore]
fn notepad_snapshot_set_value_and_readback() {
    if std::env::var("AUTOGUI_LIVE").as_deref() != Ok("1") {
        eprintln!("set AUTOGUI_LIVE=1 to run");
        return;
    }
    let before = notepads();
    let mut child = std::process::Command::new("notepad.exe").spawn().unwrap();
    let start = Instant::now();
    let hwnd = loop {
        if let Some(hwnd) = notepads().difference(&before).next().copied() {
            break hwnd;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "no new Notepad window (Windows 11 may open a tab instead; close Notepad first)"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    std::thread::sleep(Duration::from_millis(500));
    let gui = AutoGui::new().unwrap();
    let window = gui
        .get_all_windows()
        .unwrap()
        .into_iter()
        .find(|w| w.hwnd() == hwnd)
        .expect("new Notepad window is listed");

    let mut ui = UiAutomation::new().unwrap();
    let snapshot = ui.snapshot(&window, &UiSnapshotOptions::default()).unwrap();
    assert_eq!(snapshot.nodes[0].index, 0);
    assert_eq!(snapshot.nodes[0].role, "Window");
    assert!(snapshot.to_text().starts_with("[0] Window"));
    let editor = snapshot
        .nodes
        .iter()
        .find(|n| {
            (n.role == "Edit" || n.role == "Document") && n.patterns.iter().any(|p| p == "value")
        })
        .expect("Notepad exposes an editor with ValuePattern")
        .clone();

    let selector = UiSelector {
        index: Some(editor.index),
        role: Some(editor.role.clone()),
        ..Default::default()
    };
    let text = "autogui uia live test";
    let readback = ui.set_value(&window, &selector, text).unwrap();
    assert_eq!(readback.trim_end_matches(['\r', '\n']), text);
    let resolved = ui.resolve(&window, &selector).unwrap();
    assert_eq!(resolved.index, editor.index);

    // Error types on a live tree.
    let missing = UiSelector {
        name: Some("no such element 7f3a".into()),
        name_match: NameMatch::Exact,
        ..Default::default()
    };
    assert!(matches!(
        ui.resolve(&window, &missing),
        Err(Error::ElementNotFound)
    ));
    assert!(matches!(
        ui.act(&window, &selector, UiAction::Toggle),
        Err(Error::InvalidArgument(_))
    ));

    // Leave no unsaved text behind, then close.
    ui.set_value(&window, &selector, "").unwrap();
    let _ = window.close();
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();
}
