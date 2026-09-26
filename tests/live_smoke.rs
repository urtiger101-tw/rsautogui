#![cfg(all(windows, feature = "os"))]

use std::path::PathBuf;

use autogui::{AutoGui, Screenshot};

struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
#[ignore = "requires an interactive Windows desktop; captures and removes a temporary screenshot"]
fn captures_primary_monitor_and_saves_png() {
    assert_eq!(
        std::env::var("AUTOGUI_LIVE").ok().as_deref(),
        Some("1"),
        "set AUTOGUI_LIVE=1 to run a real desktop test"
    );

    let mut gui = AutoGui::new().expect("open the desktop backend");
    let screenshot = gui.screenshot().expect("capture the primary monitor");
    assert!(screenshot.width > 0 && screenshot.height > 0);
    let _windows = gui.get_all_windows().expect("enumerate visible windows");

    let path = std::env::temp_dir().join(format!(
        "autogui-live-smoke-{}-{}.png",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos()
    ));
    let _cleanup = TempFile(path.clone());
    screenshot.save(&path).expect("save a PNG screenshot");
    let reloaded = Screenshot::load(&path).expect("reload the saved screenshot");
    assert_eq!(
        (reloaded.width, reloaded.height),
        (screenshot.width, screenshot.height)
    );
}
