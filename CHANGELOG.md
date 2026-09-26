# Changelog

## Unreleased

- Added a small Windows launcher with an on-demand `autogui_runtime.dll`; help and MCP negotiation/listing run without loading the DLL. Kept all image codecs and library APIs. Release uses size optimization, Thin LTO and symbol stripping while recognition/FFT retain opt-level 3 and unwind cleanup remains enabled.
- Added stdio MCP with eight tools, session window references, PNG image content, checked input, cooperative cancellation and per-connection screensaver pause leases restored on disconnect.
- Exposed Win32 inspect/activate/move/resize/minimize/maximize/restore/hide/show/close in CLI and MCP, including honest WM_CLOSE request semantics.
- Added a bundled `autogui-control` Skill, selectable Codex/Claude/OpenCode/AGY Skill and MCP installation, JSONC-preserving edits, per-file manifests/backups and conservative uninstall/rollback. Added isolated installer, lazy DLL/protocol, and native MCP fixture coverage.

- Added Windows CLI `screensaver status/pause/resume/stop`, configurable pause duration, Ctrl+C/timer/manual restoration, per-logon crash recovery and concurrent-pause protection. No password, timeout or persistent screen-lock setting changes.
- Screensaver commands execute before the input backend is initialized. Stop verifies `.scr` process identity on the dedicated desktop and confirms shutdown; pause reports Windows power/lock transition errors and restores prior state on failure.
- Completed native screen-recognition, click, Unicode input and CLI acceptance after dismissing the screen saver. The disposable classic EDIT fixture now owns its Ctrl+A accelerator; its packed accelerator buffer is explicitly DWORD-aligned for Win32.

- Fixed grayscale arithmetic overflow and false positives on constant-color templates; confidence search now uses per-channel normalization, integer integral images, FFT candidate search and direct candidate verification.
- Replaced quadratic overlap suppression with spatial buckets, and capture only the requested region during screen searches while preserving absolute coordinates.
- Added cooperative fail-safe checks during recognition, drag and repeated input; key/button cleanup still runs at fail-safe corners. `KeyHold` now permits operations through its guard.
- Added scoped physical-pixel DPI handling on Windows, process/thread identity validation for window handles, current-title reads, asynchronous window state requests and state/foreground confirmation.
- Added `app_control` with window/title disambiguation, image revalidation, explicit template scaling, window capture and guarded input. Non-OS builds refuse to operate the CLI.
- Added recognition benchmarks and an ignored native fixture test covering real clicks, Unicode input and CLI operations. Interactive execution is subject to an unlocked Windows desktop.

- `AutoGui::new()` now uses real desktop input through Enigo and primary-monitor capture through XCap; `with_fake` remains for tests.
- Added common image-file loading/saving, including PNG and JPEG, for `Screenshot` and screen locate APIs.
- Implemented normalized cross-correlation for confidence matching behind the `opencv` feature flag.
- Tightened input release handling, screen-region clipping, fail-safe checks after screen reads, and Win32 window validation.
- Reworked the cheat-sheet example and documentation to avoid sending real input when copied or run.

## 0.1.0

- AutoGui instance API: mouse, keyboard, fail-safe, pause, tweens
- FakeBackend for deterministic tests
- Exact template locate on RGB buffers and common raster image files
- Optional normalized cross-correlation matching via the `opencv` feature flag
- Window API: full Win32 module behind `cfg(windows)`; other OS return UnsupportedPlatform
- Examples: cheat_sheet (read-only), spiral_drag (warns before moving)
