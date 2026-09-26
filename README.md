# rsautogui

Rust-powered Windows desktop automation for AI agents: image recognition, Win32 app control, an MCP server, and reusable Skills.

rsautogui 提供 Rust 原生桌面自動化，包含畫面辨識、Windows App 控制、MCP 與 Agent Skills。Rust library 的 crate 名稱維持 `autogui`，CLI 維持 `autogui-control`，以保留既有使用方式。

The `autogui` library is independently implemented in Rust and modeled on PyAutoGUI 0.9.x. It provides mouse and keyboard input, fail-safe handling, primary-screen capture, image matching, and Windows window control. This project is not affiliated with PyAutoGUI.

## Windows Agent 套件：按需載入 DLL、MCP、Skills

完整功能保留在 `autogui_runtime.dll`，搭配小型 `autogui-control.exe`。Help、MCP 初始化及工具列舉不載入 DLL；第一次執行控制工具時才載入。原本所有 `image` 預設格式仍保留。Release 同時使用大小最佳化、Thin LTO 與符號移除，辨識核心及 FFT 維持效能最佳化，並保留 unwind 清理。

發行包解壓縮後執行 `install.cmd`，可分別選擇安裝 **Skill** 與 **MCP** 給 Codex、Claude、OpenCode、AGY，也能只安裝程式或自訂 Skill 目錄。安裝器備份既有設定；解除安裝保護後續修改。不要將 EXE 與 DLL 分開移動。

```powershell
git clone https://github.com/urtiger101-tw/rsautogui.git
cd rsautogui
cargo build --release --workspace --all-features --locked
.\target\release\autogui-control.exe --help
.\target\release\autogui-control.exe mcp
.\scripts\package.ps1 -SkipBuild
```

MCP 提供 8 個工具，涵蓋視窗列舉、Win32 控制、PNG 畫面、找圖、點擊、文字、快捷鍵與螢幕保護程式。Agent 適用操作 Skill 隨包提供；詳見 [安裝、DLL 與 MCP 指南](docs/agent-installation.md)。

## Install

```toml
[dependencies]
autogui = { path = "../rsautogui", features = ["opencv"] }
```

The default `os` feature uses Enigo for real input and XCap for screen capture. For deterministic, headless code, construct `AutoGui::with_fake(FakeBackend::new(width, height))`. Building with `default-features = false` also selects the fake backend.

`with_fake` replaces mouse/keyboard/capture only. Window APIs still use Win32 and are not simulated; `app_control` refuses to run without the `os` feature.

Use the path to this checkout. The `opencv` compatibility feature enables normalized cross-correlation matching for `LocateOptions::confidence`; the implementation uses pure Rust FFT and integer integral images and does not require a system OpenCV installation. Every accepted FFT candidate is verified directly.

## 畫面辨識與 App 控制

完整操作方式見 [繁體中文指南](docs/recognition-app-control.md)。Windows 範例提供指定視窗的截圖、找圖、點擊與文字輸入；標題或圖像符合多個目標時會回報錯誤。

```powershell
cargo build --release --features opencv --example app_control
.\target\release\examples\app_control.exe list
.\target\release\examples\app_control.exe capture "目標視窗標題" window.png
.\target\release\examples\app_control.exe locate "目標視窗標題" button.png 0.95 5
.\target\release\examples\app_control.exe click "目標視窗標題" button.png 0.95 5
```

`capture`、`locate`、`click`、`type` 會啟用目標視窗；`click` 與 `type` 會送出真實輸入。需要 Windows、`os` feature，以及已解除鎖定的互動桌面。`Screen-saver`／鎖定桌面造成的游標存取拒絕會回傳 `PermissionDenied`。

## 螢幕保護程式

```powershell
.\target\release\examples\app_control.exe screensaver status
.\target\release\examples\app_control.exe screensaver stop
.\target\release\examples\app_control.exe screensaver pause 900
# 在另一個終端提早還原，或復原異常結束的暫停：
.\target\release\examples\app_control.exe screensaver resume
```

`pause` 可設定 1–86400 秒，預設 15 分鐘；等待期間可按 Ctrl+C 還原。原設定會先保存以便異常結束後復原。`stop` 只停止目前的保護程式，Windows 鎖定仍需登入。完整行為、復原路徑與限制見 [螢幕保護程式指南](docs/screensaver.md)。

## Cheat sheet

This example only reads the screen and pointer. Uncomment an action after choosing its target and deciding that the real input is intended.

```rust
use std::time::Duration;
use autogui::{tweens, AutoGui, LocateOptions};

fn main() -> autogui::Result<()> {
    let mut gui = AutoGui::new()?;
    let size = gui.size()?;
    let position = gui.position()?;
    println!("screen: {}x{} at {:?}", size.width, size.height, position);
    println!("on screen: {}", gui.on_screen(position.x, position.y)?);

    // Input and window mutations affect the real desktop:
    // gui.move_to(100, 150, Duration::from_millis(500), tweens::ease_in_out_quad)?;
    // gui.click()?;
    // gui.write("Hello", Duration::from_millis(50))?;
    // gui.hotkey(&["ctrl", "c"])?;

    let options = LocateOptions::default();
    // let button = gui.locate_center_on_screen("button.png", &options)?;
    // gui.click_xy(button.x, button.y)?;
    // let windows = gui.get_windows_with_title("Notepad")?;
    // windows[0].resize_to(800, 600)?;
    let _ = (tweens::linear, options, Duration::ZERO);
    Ok(())
}
```

**Fail-safe is on by default.** Mutating and screen-search calls check the configured primary-monitor corners before and after work, and during long searches, drags, repeated inputs and input intervals. Move the cursor to a corner to interrupt. New actions stop; key/button releases still run so input does not remain held. Every public mutating call also observes `Settings::pause`.

## Capabilities

| Capability | Windows | macOS | Linux |
|---|---|---|---|
| Mouse and keyboard (`os`) | Enigo | Enigo; Accessibility permission may be required | Enigo; X11 supported, Wayland depends on compositor permissions |
| Screenshot and image locate | XCap, PNG/JPEG/BMP/PPM files | XCap | XCap; Wayland capture support depends on the desktop portal |
| Window control | Win32 | `UnsupportedPlatform` | `UnsupportedPlatform` |

Windows coordinate operations temporarily use a per-monitor DPI-aware thread context and restore the caller's context afterward; they do not change process-wide DPI settings. This path requires Windows 10 1703 or newer. Window activation restores minimized windows and confirms foreground ownership. Show/move/resize requests use asynchronous dispatch and verify the resulting state; application size constraints can cause an error. `close()` posts a close request, which a target app can veto with a save dialog.

`AutoGui::new()` connects to the real desktop. Unit tests should use `with_fake`. The `spiral_drag` example moves the real cursor and is kept separate from the read-only `cheat_sheet` example.

## Tests

```bash
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

The ignored live smoke test only reads the desktop and writes a temporary screenshot:

```powershell
$env:AUTOGUI_LIVE = "1"
cargo test --test live_smoke -- --ignored
```

The native app-control test creates and closes its own fixture window, verifies a real button-click receipt and Unicode text, and restores the previous pointer/focus where possible:

```powershell
cargo build --features opencv --example app_control
$env:AUTOGUI_LIVE = "1"
$env:AUTOGUI_CONTROL_EXE = (Resolve-Path .\target\debug\examples\app_control.exe).Path
cargo test --all-features --test live_app_control -- --ignored --nocapture
```

## Recognition performance

Measured on this Windows development machine with deterministic synthetic RGB images; capture, I/O and `Settings::pause` are excluded. These are benchmarks, not latency guarantees for arbitrary screens.

| Search | Image / template | Build | Measured time |
|---|---|---|---|
| Previous confidence implementation | 320×180 / 32×16 | Debug | 1967.63 ms |
| Current confidence implementation | 320×180 / 32×16 | Debug | 264.57 ms |
| Current exact matching | 1920×1080 / 80×40 | Release | 3.97 ms |
| Current confidence matching | 1920×1080 / 80×40 | Release | 307.65 ms |

Reproduce with `cargo run --release --features opencv --example recognition_bench -- --full-hd`. Use a window-sized `LocateOptions::region` to reduce capture, search time and FFT memory. The project goals remain <200 ms for 1080p capture and <2 s for exact matching of an ≤80×40 template; capture latency has not been reprofiled for the DLL release; the owned-window screenshot and input workflows have passed on the interactive Windows desktop.

## Differences from PyAutoGUI

- `AutoGui` instances and `Settings` replace process-wide `PAUSE` / `FAILSAFE` variables.
- `Rect` is the main rectangle type; `Box` is a compatibility alias.
- Image misses return `Error::ImageNotFound`; use `try_locate_on_screen` for an optional result. `set_raise_image_not_found(false)` makes locate-all calls return an empty vector on misses.
- `Screenshot` replaces Pillow images. Common raster formats are loaded and saved through the Rust `image` crate.
- Message boxes are out of scope. Window control is available only on Windows.
- Multi-monitor virtual-desktop coordinates and OCR are out of scope.

## License

[MIT](LICENSE).
