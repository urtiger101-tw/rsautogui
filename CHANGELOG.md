# Changelog

## Unreleased

## 0.3.0 — 2026-11-28

Features compared against Antigravity Computer Use: accessibility-tree control, change waits, batches, path strokes, clipboard entry and a one-chord emergency stop.

### Highlights

- **UI Automation**: agents read a window's accessibility tree and act on elements by ref, name, role or AutomationId instead of guessing pixels.
- **Emergency stop**: **Ctrl+Alt+F12** stops all input for the MCP connection until the server restarts, alongside the corner fail-safe. It is read by `GetAsyncKeyState` polling; there is still no keyboard hook and nothing is recorded.
- **"Computer use! Don't move!" notice**: while an agent works through MCP, a translucent blue, click-through notice at the top centre of the primary monitor tells people not to touch the mouse and shows the stop chord. It never takes focus and is excluded from screenshots.
- **Action log** (on by default): every MCP tool call and CLI command is appended to `%LOCALAPPDATA%\AutoGui\logs\actions-YYYY-MM-DD.jsonl`; typed and pasted text is reduced to a character count. Switch it off with `autogui-control settings action-log off`.
- **Fewer round trips**: `run` executes 1–8 steps per call; `ui_wait` and `wait_change` replace sleep-and-capture loops.

### Added

- MCP tools (19 → 27):
  - `ui_snapshot`: tree as text plus nodes with `e{gen}-{index}` refs; password values never returned.
  - `ui_click`: Invoke/Toggle/SelectionItem/ExpandCollapse/Focus, or a real click at the element centre.
  - `ui_set_value`: ValuePattern, with a readback.
  - `ui_wait`: text present/absent, with `abort_on`.
  - `wait_change`: frame-diff wait on a window or display; never sends input.
  - `drag_path`: one stroke through 2–256 capture-relative points.
  - `paste_text`: clipboard entry with restore of the previous text.
  - `run`: batched steps; `"$last"` capture ids; stops at the first failure.
- MCP error codes `element_not_found`, `ambiguous_element` and `stopped`.
- CLI `settings [overlay on|off] [action-log on|off]` with `%LOCALAPPDATA%\AutoGui\settings.json`; `AUTOGUI_OVERLAY` / `AUTOGUI_ACTION_LOG` override it per process. Logs older than 30 days are deleted automatically.
- CLI: `ui snapshot|click|set-value|wait <title>` and `paste <title> <text>`, with bilingual help.
- Library:
  - Types and client: `UiAutomation`, `UiSnapshot`, `UiNode`, `UiSelector`, `NameMatch`, `UiAction`, `UiSnapshotOptions` (feature `uia`, Windows only, COM on one MTA worker with call timeouts).
  - `Screenshot::diff` / `FrameDiff`.
  - `AutoGui::drag_path` and `AutoGui::paste`.
  - `autogui::clipboard::{get_text, set_text}`.
  - `autogui::stop::{chord_down, StopLatch}` and `Settings::stop_key` (default on).
  - New `Error` variants `ElementNotFound`, `AmbiguousElement`, `Stopped`, `Clipboard`, `Uia`.
- Docs: `docs/ui-automation.md`; SPEC §18–§21. The Skill now teaches "tree first, pixels when the tree is blind" and what to do after `stopped`.

### Changed

- `hotkey` refuses any chord containing Ctrl+Alt+F12, because synthetic keys would trip the stop latch.
- `type` and `paste` treat later `--json`/`--quiet` as text (global flags go before the command word).
- The MCP timeout hint is generic, since several tools now wait.
- New cargo feature `uia` (on in `autogui-control-core` and the runtime), using the `windows` 0.62 crate for COM only.

### Safety details

- UI Automation actions never run late: if the call is cancelled, stopped or times out before the pattern call starts, it is skipped. A timeout after it starts returns an `unconfirmed:` error (not retryable; re-observe first).
- Element refs are pinned to the UIA RuntimeId, so a ref cannot silently hit a different element after the tree changes.
- If `IsPassword` cannot be read, the element is treated as a password.
- The `ui_click` real-click fallback is clipped to the primary screen and refused when another window covers the point.
- `wait_change` reports `changed: true` with a `reason` when the window loses the foreground or is resized.
- `run` sets `isError` when a step fails, including `stopped`, `fail_safe` and `cancelled`.
- `paste_text` rejects text containing NUL and never errors after the text was delivered; a failed clipboard restore returns `clipboard_restored: false`.

### Fixed

- `install.ps1`, `doctor` and `mcp config --host claude-desktop` now find the Microsoft Store build's Claude Desktop config at `%LOCALAPPDATA%\Claude-Data\anthropicpbc.claude_*\claude_desktop_config.json`. They fall back to `%APPDATA%\Claude`.

## 0.2.3 — 2026-11-22

### Fixed

- Installer: upgrading no longer fails with `MCP server already exists: autogui`. Uninstall keeps any host config edited after installation, and Claude Code edits `~/.claude.json` constantly, so an upgrade found the previous installation's `autogui` entry still present. `install.ps1` now never overwrites an existing JSON host entry: when it already starts this installation's `autogui-control.exe mcp` it is accepted silently; otherwise it is kept and Setup shows a note. Only hosts the installation changed are recorded in the manifest.
- Installer: uninstall now removes every empty directory it recorded. It deduplicated with `Sort-Object Length -Unique`, which treats distinct paths of the same length as duplicates, so directories such as `~/.claude/skills/autogui-control/agents` could remain and block the next installation with `Skill already exists`. `install.ps1` also accepts a Skill directory that contains no files, so upgrades from 0.2.2 (whose uninstall still has the bug) succeed.

## 0.2.2 — 2026-11-21

### Fixed

- Installer: Setup no longer enables Windows RedirectionGuard (Inno Setup 6.7 turns it on by default, and `install.ps1` inherited it). With it on, junctions created by the user were not followed, so the standard Codex CLI under `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin` (a junction) was not found and Codex MCP registration was skipped. Setup runs per user and never elevates.

## 0.2.1 — 2026-11-21

### Fixed

- Installer: Codex MCP registration no longer stops the whole installation. It tries every codex CLI on PATH, then the persisted machine and user PATH, then `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin\codex.exe`, and uses the first one that can read the Codex config, so a launcher shim that cannot run is skipped. If no CLI works, `codex mcp add` fails without writing, or Codex already has an `autogui` entry, only Codex is skipped; Setup finishes and shows the note with the command to register later. `install.ps1 -CodexPath` selects a CLI explicitly.

## 0.2.0 — 2026-11-21

### Highlights

- Agents get structured results: MCP tools carry titles and `outputSchema`, return `structuredContent` on protocol 2025-06-18 and later, and report failures as `{code, message, hint, retryable}`.
- `autogui-control doctor` checks the installation, runtime DLL, session and agent-host registrations without sending input.
- `autogui-control mcp tools` exports tool schemas for Anthropic and OpenAI APIs, and `mcp config` prints registration snippets for six hosts.
- Recognition is much faster: 1080p confidence matching is 7–10× faster, and MCP screenshots are about 3× faster and 43% smaller.
- CLI failures use specific exit codes instead of always 1 (see Changed).

### Added

- CLI: `doctor [--json]` (also `--doctor`); `mcp tools [--format mcp|anthropic|openai|openai-chat]`; `mcp config --host claude-code|claude-desktop|codex|openai-agents|opencode|agy`; `-V/--version [--json]`; global `--json`, `--quiet`, `--no-color` (and `NO_COLOR`), `--lang en|zh` (and `AUTOGUI_LANG`, then the Windows UI language).
- CLI help is bilingual (English and Traditional Chinese) via `help <command>` or `<command> --help`, with "did you mean" suggestions for mistyped commands.
- MCP: tool `title` and `outputSchema`; `structuredContent` alongside the JSON text block (the text block is always kept); structured errors with the codes `invalid_argument`, `window_not_found`, `image_not_found`, `not_found`, `fail_safe`, `permission_denied`, `timeout`, `cancelled`, `unsupported`, `input_failed`, `capture_failed`, `runtime_unavailable` and `internal`; `serverInfo.title`; the `logging` capability with `logging/setLevel`; screensaver lease notifications through `notifications/message`; empty `resources/list`, `resources/templates/list` and `prompts/list`.
- Library: `autogui::prelude` for common imports; `From<Rect>` for `(i32, i32, i32, i32)`.
- Installer: `claude-desktop` MCP host (`%APPDATA%\Claude\claude_desktop_config.json`, key `mcpServers.autogui`) in `install.ps1`, and the Inno Setup task `mcp\claudedesktop`. It is MCP-only.
- Packaging: `scripts/package.ps1` fails when the five workspace manifests have different versions, and ships `docs/performance.md`.

### Changed

- **Exit codes.** Failures no longer always exit 1. `0` ok, `1` operation failed, `2` usage error, `3` window or image not found, `4` timeout, `5` environment blocked (including `PermissionDenied`, for example lost focus, and a failed `doctor`), `6` fail-safe triggered, `70` internal error. Scripts that test for `1` should test for non-zero or the specific code.
- CLI `type` sends text in batches of at most 32 characters and checks foreground and cancellation before each batch.
- MCP `structuredContent` is returned only when the negotiated protocol version is 2025-06-18 or later; older clients keep receiving the text block only.
- Library: `AutoGui` window getters take `&self`.

### Fixed

- Fail-safe and MCP cancellation are checked for the whole parallel image search, and a panicking search thread stops the others.
- `typewrite` checks the fail-safe before each batch of at most 32 characters; MCP display `drag` checks cancellation and the fail-safe every 16 ms while the button is held.
- After MCP stdin closes, already queued requests are still answered: read-only tools run, all other tools return `cancelled`.
- MCP accepts a leading UTF-8 byte-order mark on stdin (sent by Windows PowerShell 5.1 hosts).
- MCP bad arguments return a clean `invalid_argument` error; cancellation is recognised by type, not by message text.
- CLI global flags are recognised before the command word, and after it for every command except `type`, so text such as `--json` is typed as in 0.1.0. `--` ends flag parsing.
- `--lang en` no longer prints Chinese for `session`, `screensaver`, `window`, `capture` and `type`; usage errors are fully localized. "Did you mean" handles swapped letters (`lsit` → `list`).
- `mcp config` prints only the snippet on stdout (valid JSON or TOML) and the guidance on stderr; `--json` returns `{host, file, snippet, command}`.
- `capture`, `type`, `locate` and `click` honour `--json`.
- Ambiguity and race errors (several windows or images match, the window moved) exit `1` with a specific hint instead of `2` with a usage hint.
- `doctor --json` is always English, Windows paths are no longer wrapped, and a stale registration in another host is a warning, not a failure.

### Performance

Median times in milliseconds for a 1920×1080 RGB or grayscale image with an 80×40 template, before and after this release (details and method in [docs/performance.md](docs/performance.md)):

| Search | Before | 0.2.0 |
|---|---|---|
| Exact, RGB noise / UI | 3.00 / 6.37 | 0.88 / 0.86 |
| Exact, grayscale | 3.88 / 10.28 | 1.51 / 1.60 |
| Confidence 0.999, RGB | 248.5 | 25.1 |
| Confidence 0.9, grayscale | 118.8 | 16.1 |
| Confidence 0.95, RGB UI | 246.8 | 25.4 |
| Region 280×160 (exact) | 4.04 | 4.24 (within noise) |

- Peak memory for a full-HD confidence search fell from about 113 MB to about 2.5 MB per worker. Searches use up to 8 threads and poll the fail-safe between work items.
- MCP `capture` PNG at 1080p: 80–102 ms → 27–31 ms, and 1.22 MB → 0.69 MB.
- PNG codec crates build at opt-level 3, making display capture to PNG about 15% faster for about 150 KiB more runtime DLL (5370 → 5560 KiB).
- `--help` cold start is about 4.9 ms; MCP `initialize` plus `tools/list`, including process spawn, is about 15 ms.

### Internal

- CLI and MCP logic moved into `crates/protocol` (`autogui-protocol`: MCP stdio protocol, tool catalogue `tools.json`, config snippets and export) and `crates/control` (`autogui-control-core`). The runtime DLL and the `autogui-control` launcher are thin wrappers. The C exports, CLI commands and 19 MCP tools are unchanged.
- `src/gui.rs` split into `mouse`, `keyboard` and `screen` modules (SPEC §9); Win32 window code moved to `window_win.rs`.
- Integration tests `tests/tween_failsafe.rs`, `tests/locate_synthetic.rs` and `tests/keys.rs`.
- CI (`.github/workflows/ci.yml`): Windows gates (fmt, clippy, tests, release build), the MCP protocol test (`tests/mcp-protocol.ps1`), and a non-blocking Linux compile check.
- `scripts/bench.ps1` (`-Json` for machine-readable output); `recognition_bench` accepts `--iterations`, `--json` and `--capture`.
- Tool catalogue is `crates/protocol/src/tools.json`; the packaged `docs\mcp-tools.json` is unchanged.

### Earlier unreleased changes

- Added MCP `pointer_move`, `drag` and `cursor_position`, and `click_at` accepts `button` (left/right/middle) and `clicks` (1-3). Pointer actions reuse the latest single-use capture proof and its pre-input checks.
- Added MCP `window_wait` (present/absent/foreground, timeout up to 30 s) and `app_launch`. `app_launch` is disabled unless the MCP process sets `AUTOGUI_ALLOW_LAUNCH=1`, and accepts only an existing absolute `.exe`.
- `display_capture` returns a `capture_id`, so screen-level pointer actions can use a display proof; pointer tools take exactly one of `target` or `display`.
- `windows_list` accepts `process`, `foreground` and `titled_only` filters and reports `pid`, `process` and `class`. `hotkey` validates key names before activating the target window and accepts `presses` (1-50).
- Tool errors include recovery hints, for example re-running `windows_list` after `WindowNotFound`.
- Added an Inno Setup per-user installer (`installer/autogui.iss`), built with `scripts/package.ps1 -Installer` (version from `Cargo.toml`, requires Inno Setup 6). It installs only to `%LOCALAPPDATA%\Programs\AutoGui` through `install.ps1`, accepts silent `/TASKS` host selection, and removes an existing installation with its own `uninstall.ps1` before upgrading.
- Fixed installation with existing empty/whitespace MCP JSON while preserving byte-exact uninstall backups. Antigravity skills now default to the shared `.gemini/config/skills`; `-AgySkillRoot` supports legacy/custom locations.
- Fixed restore timing out when a minimized window returns to its previous maximized state; move/resize and display placement still explicitly normalize the window first.
- Added MCP `click_at` and vertical/horizontal `scroll`, bound to the latest single-use window capture for 60 seconds. Input checks target identity, foreground, geometry, display, cursor and a local pixel patch before dispatch; invalid attempts also consume the proof.
- Batched MCP `type_text` into at most 32 Unicode scalar values per foreground/cancellation check, retaining library fail-safe checks and the configured pause per batch. Clipboard paste and UI Automation remain outside this change.

- Added unattended/monitor-less readiness diagnostics: CLI `session status [--json]` (non-zero exit on blockers) and read-only MCP `session_status`. Reports session/console/RDP state, lock, input desktop, cursor access, active displays and a primary-monitor capture probe, with blocker codes such as `no-active-display`, `remote-not-rendering`, `session-not-active`, `locked` and `service-session`. Decision logic is unit-tested on all platforms.
- Added `scripts/rdp-to-console.ps1` (administrator, `-WhatIf`/confirmation) to move an RDP session to the console with `tscon` so the desktop keeps rendering after disconnect, plus `docs/headless.md`.

- Added explicit Windows display snapshots, real display/region PNG capture, display-targeted recognition/clicking with virtual-desktop SendInput coordinates, and Win32 window placement on physical or IDD virtual monitors. Existing primary-monitor defaults and corner fail-safe remain.
- Added three MCP tools (`displays_list`, `display_capture`, `window_to_display`), optional display references for capture/locate/click_image, CLI display commands, and updated Skills. Display identity/geometry is revalidated and stale references are rejected.
- Added an optional pinned VDD/NefCon preparation and administrator installation helper with ZIP hashes, signature checks, one-monitor configuration and owned-device recovery records. Driver downloads/installations are separate from the lazy Rust EXE/DLL and are never triggered by MCP.

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
