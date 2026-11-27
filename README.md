# rsautogui

Rust-powered Windows desktop automation for AI agents: image recognition, Win32 app control, an MCP server, and reusable Skills.

rsautogui 提供 Rust 原生桌面自動化，包含畫面辨識、Windows App 控制、MCP 與 Agent Skills。Rust library 的 crate 名稱維持 `autogui`，CLI 維持 `autogui-control`，以保留既有使用方式。

The `autogui` library is independently implemented in Rust and modeled on PyAutoGUI 0.9.x. It provides mouse and keyboard input, fail-safe handling, primary-screen capture, image matching, and Windows window control. This project is not affiliated with PyAutoGUI.

## 快速開始

### 1. 安裝

需要 Windows 10 1703 以上（x64）。執行 `AutoGui-0.2.3-setup.exe`（每位使用者安裝，不需系統管理員權限、不修改 PATH），或解壓縮發行包後執行 `install.cmd`。預設安裝位置是 `%LOCALAPPDATA%\Programs\AutoGui`。

```powershell
$exe = "$env:LOCALAPPDATA\Programs\AutoGui\autogui-control.exe"
& $exe doctor     # 唯讀健康檢查
& $exe help       # 命令說明；加 --lang zh 顯示繁體中文
```

從原始碼建置：`cargo build --release --workspace --locked`。`autogui-control.exe` 與 `autogui_runtime.dll` 必須放在同一個資料夾。下文的 `autogui-control` 指這個 EXE；未加入 PATH 時請用完整路徑。

### 2. 健康檢查

`autogui-control doctor` 只讀取狀態，不送出任何滑鼠或鍵盤輸入。它檢查 Runtime DLL 與 ABI、EXE 與 DLL 版本、工作階段是否就緒、Claude Code／Claude Desktop／Codex／OpenCode／AGY 的 MCP 註冊、Skill 的 `runtime.json`，並實際與 MCP 進行一次握手。有失敗項時結束碼為 `5`；`doctor --json` 供腳本使用。之後任何操作失敗，都先執行 `doctor`。

### 3. 連接你的 Agent

安裝程式可直接替所選 Agent 寫入 Skill 與 MCP 設定（見 [安裝、DLL 與 MCP 指南](docs/agent-installation.md)）。若要手動設定，用 `mcp config` 產生片段，其中 `<exe>` 是 `autogui-control.exe` 的完整路徑：

| Agent | 產生片段 | 要做的事 |
|---|---|---|
| Claude Code | `mcp config --host claude-code` | 執行 `claude mcp add --scope user autogui -- "<exe>" mcp`，或合併到 `~/.claude.json` |
| Claude Desktop | `mcp config --host claude-desktop` | 合併到 `%APPDATA%\Claude\claude_desktop_config.json`（Microsoft Store 版為 `%LOCALAPPDATA%\Claude-Data\anthropicpbc.claude_*\claude_desktop_config.json`）的 `mcpServers`，然後重新啟動 Claude Desktop |
| Codex | `mcp config --host codex` | 加入 `~/.codex/config.toml` 的 `[mcp_servers.autogui]` |
| OpenAI Agents SDK | `mcp config --host openai-agents` | 使用 `MCPServerStdio`；`client_session_timeout_seconds=60`，因為 `window_wait` 最長可等 30 秒 |
| OpenCode | `mcp config --host opencode` | 合併到 `~/.config/opencode/opencode.json` 的 `mcp` |
| AGY | `mcp config --host agy` | 合併到 `~/.gemini/config/mcp_config.json` 的 `mcpServers` |

```powershell
& $exe mcp config --host claude-code
```

### 4. 把工具定義交給 Claude 或 OpenAI API

`mcp tools` 匯出工具定義，格式可為 `mcp`（預設，`tools/list` 陣列）、`anthropic`、`openai`（Responses API 的 function tools）或 `openai-chat`（Chat Completions 的 tools）。

```powershell
# Windows PowerShell 5.1 的 > 會寫成 UTF-16；用 cmd 重新導向可得到 UTF-8
cmd /c "`"$exe`" mcp tools --format anthropic > autogui-tools.json"
```

```python
import json
import anthropic

tools = json.load(open("autogui-tools.json", encoding="utf-8"))
reply = anthropic.Anthropic().messages.create(
    model="<你的模型>",
    max_tokens=1024,
    tools=tools,
    messages=[{"role": "user", "content": "列出目前開啟的視窗"}],
)
# 模型回傳的 tool_use 要由你的程式執行：把名稱與參數轉交給 autogui-control mcp（JSON-RPC tools/call）。
```

匯出只提供定義；工具的實際執行仍在 `autogui-control mcp` 內。

## 結束碼

CLI 的結束碼可用於腳本判斷，失敗時 stderr 會說明原因（搭配 `--json` 時為 JSON）。

| 代碼 | 意義 |
|---|---|
| `0` | 成功 |
| `1` | 操作失敗 |
| `2` | 用法錯誤 |
| `3` | 找不到視窗或圖片 |
| `4` | 逾時 |
| `5` | 環境受阻：doctor 有失敗項、工作階段有阻擋項、Windows 拒絕（例如焦點已丟失） |
| `6` | 觸發 fail-safe（游標到了主螢幕角落） |
| `70` | 內部錯誤 |

## 全域旗標

| 旗標 | 說明 |
|---|---|
| `--json` | 機器可讀輸出（`doctor`、`session status`、`list`、`wait`、`capture`、`type`、`locate`、`click`、`mcp config`、`-V`）；錯誤以 JSON 寫到 stderr。`mcp tools` 的輸出本來就是 JSON |
| `--quiet` | 不顯示成功訊息 |
| `--no-color` | 只用 ASCII 符號、不著色；也可設定 `NO_COLOR` |
| `--lang en\|zh` | 說明與訊息語言；依序為 `--lang`、`AUTOGUI_LANG`、Windows 介面語言，最後是英文 |
| `-V`, `--version` | 版本；加 `--json` 顯示詳細資訊 |
| `-h`, `--help`, `help <命令>` | 說明；`<命令> --help` 同樣可用 |

全域旗標放在命令之前一律有效；放在命令之後也有效，但 `type` 例外：`type` 的文字原樣輸入，因此 `autogui-control type Notepad --json` 會輸入字面上的 `--json`。`--` 之後的參數一律當作文字。`--doctor` 是 `doctor` 的別名。

`mcp config` 只把設定片段輸出到 stdout（可直接存成 JSON／TOML），要修改的檔案與 `claude mcp add` 指令等說明寫到 stderr；加 `--json` 則輸出 `{host, file, snippet, command}`。

## Windows Agent 套件：按需載入 DLL、MCP、Skills

完整功能保留在 `autogui_runtime.dll`，搭配小型 `autogui-control.exe`。Help、MCP 初始化及工具列舉不載入 DLL；第一次執行控制工具時才載入。原本所有 `image` 預設格式仍保留。Release 同時使用大小最佳化、Thin LTO 與符號移除，辨識核心及 FFT 維持效能最佳化，並保留 unwind 清理。

發行包解壓縮後執行 `install.cmd`，或執行 `AutoGui-<版本>-setup.exe`（Inno Setup 每位使用者安裝程式，見下節），可分別選擇安裝 **Skill**（Codex、Claude Code、OpenCode、AGY）與 **MCP**（再加上 Claude Desktop），也能只安裝程式或自訂 Skill 目錄。安裝器備份既有設定；解除安裝保護後續修改。不要將 EXE 與 DLL 分開移動。

```powershell
git clone https://github.com/urtiger101-tw/rsautogui.git
cd rsautogui
cargo build --release --workspace --all-features --locked
.\target\release\autogui-control.exe --help
.\target\release\autogui-control.exe mcp
.\scripts\package.ps1 -SkipBuild
.\scripts\package.ps1 -SkipBuild -Installer
```

### Inno Setup 安裝程式

`AutoGui-<版本>-setup.exe` 安裝到 `%LOCALAPPDATA%\Programs\AutoGui`，只需目前使用者權限，不修改 PATH。實際安裝仍由隨包的 `install.ps1` 執行（核對 SHA-256、寫入清單、選擇 Skill／MCP）；安裝程式不會在程式目錄寫入任何檔案。靜默安裝與升級、解除安裝的細節見 [安裝指南](docs/agent-installation.md)。

## MCP

`autogui-control mcp` 以換行分隔的 JSON-RPC 透過 stdin/stdout 溝通，沒有 TCP 連接埠。支援協定版本 2024-11-05、2025-03-26、2025-06-18 與 2025-11-25。

- **27 個工具**，涵蓋無人值守就緒檢查（`session_status`）、顯示器與視窗列舉與等待（`displays_list`、`windows_list`、`window_wait`）、指定顯示器截圖（`display_capture`、`window_to_display`）、Win32 控制（`window_control`）、啟動程式（`app_launch`）、**UI Automation 樹與元素操作**（`ui_snapshot`、`ui_click`、`ui_set_value`、`ui_wait`）、截圖與找圖（`capture`、`locate`、`click_image`）、畫面變化等待（`wait_change`）、座標操作（`click_at`、`pointer_move`、`drag`、`drag_path`、`scroll`、`cursor_position`）、文字與快捷鍵（`type_text`、`paste_text`、`hotkey`）、批次步驟（`run`，1–8 步）及螢幕保護程式（`screensaver`）。
- **緊急停止**：按 **Ctrl+Alt+F12** 後，此 MCP 連線拒絕所有輸入工具（錯誤碼 `stopped`）直到重新啟動；觀察類工具照常。以 `GetAsyncKeyState` 輪詢三個按鍵，不安裝鍵盤 hook，也不記錄按鍵。它補充、不取代主螢幕角落 fail-safe。
- **操作提示視窗**：Agent 透過 MCP 操作時，主螢幕上方中央會出現半透明藍色小視窗「**Computer use! Don't move!**」，下方提示 Ctrl+Alt+F12 可緊急停止。它不搶焦點、滑鼠可直接穿透點擊、不會出現在截圖中，也不在 `windows_list` 內；最後一次工具呼叫 15 秒後自動隱藏，緊急停止或連線結束時立即隱藏。
- **動作紀錄**（預設開啟）：每次 MCP 工具呼叫與 CLI 命令寫入 `%LOCALAPPDATA%\AutoGui\logs\actions-YYYY-MM-DD.jsonl`（一行一筆 JSON，本機時間）。輸入或貼上的文字只記字數（`"[N chars]"`），保留 30 天，從不上傳。見下方〈操作提示與動作紀錄〉。
- 每個工具都有 `title` 與 `outputSchema`；伺服器的 `serverInfo.title` 為 `AutoGui Windows Control`。
- **`structuredContent`**：協定 2025-06-18 以上會同時回傳結構化結果；文字區塊一律保留，舊版用戶端仍可讀取。
- **結構化錯誤**：工具執行失敗時 `isError: true`，`structuredContent` 內含 `{code, message, hint, retryable}`。`retryable` 表示「不改參數重試，之後可能成功」。

| `code` | 意義 | 預設 `retryable` |
|---|---|---|
| `invalid_argument` | 參數錯誤，或參照／截圖憑證已過期 | 否 |
| `window_not_found` | 目標視窗已關閉或身分已改變（重新 `windows_list`） | 否 |
| `image_not_found` | 逾時前找不到範本 | 是 |
| `not_found` | 檔案或其他指定資源不存在 | 否 |
| `fail_safe` | 游標位於主螢幕角落，已中止輸入 | 是（移開游標後） |
| `permission_denied` | Windows 拒絕（焦點鎖定、完整性層級、政策） | 否 |
| `timeout` | 等待未在時限內達到狀態 | 是 |
| `cancelled` | 用戶端取消或中斷連線；未再送出輸入 | 是 |
| `unsupported` | 此平台或建置不支援 | 否 |
| `input_failed` | 送出鍵鼠輸入失敗 | 是 |
| `capture_failed` | 截圖失敗 | 是 |
| `runtime_unavailable` | DLL 無法載入，或 EXE 與 DLL 版本不符 | 否 |
| `internal` | 其他錯誤 | 否 |

個別回應的 `retryable` 以實際欄位為準。`hint` 會給出下一步，例如「重新執行 `windows_list`」。

此外，伺服器提供 `logging` 能力與 `logging/setLevel`；螢幕保護程式的暫停租約會以 `notifications/message` 通知。`resources/list`、`resources/templates/list` 與 `prompts/list` 回傳空清單。

**重要**：`click_at`、`pointer_move`、`drag`、`scroll` 使用最近一次 `capture` 或 `display_capture` 的 `capture_id`（60 秒內有效、單次使用）與截圖內座標。任何嘗試都會消耗憑證；輸入後應重新擷取，確認實際結果。輸入送出不代表任務完成。

`app_launch` 預設停用，只有 MCP 程序設定 `AUTOGUI_ALLOW_LAUNCH=1` 時才會啟動既存的絕對路徑 `.exe`。Agent 適用的操作 Skill 隨包提供；指定顯示器與選配驅動見 [虛擬顯示器指南](docs/virtual-display.md)。

## 操作提示與動作紀錄

兩者預設開啟，以 `settings` 命令切換，設定存於 `%LOCALAPPDATA%\AutoGui\settings.json`，執行中的 MCP 伺服器在下一次工具呼叫即套用，不需重新啟動：

```powershell
autogui-control settings                     # 顯示目前狀態、設定檔與紀錄資料夾
autogui-control settings action-log off      # 關閉動作紀錄
autogui-control settings overlay off         # 關閉「Computer use! Don't move!」提示視窗
autogui-control settings overlay on action-log on
```

單一 MCP 主機可在其設定的 `env` 中以 `AUTOGUI_OVERLAY=0|1`、`AUTOGUI_ACTION_LOG=0|1` 覆寫設定檔。MCP 工具無法變更這兩項設定，Agent 不能自行關閉提示或紀錄。

紀錄範例（每行一筆）：

```json
{"ts":"2026-11-28T01:26:41.105+08:00","pid":31332,"args":{"capture_id":"c7","target":"w3-1","x":120,"y":48},"ms":64,"ok":true,"source":"mcp","tool":"click_at","window":"未命名 - 記事本"}
{"ts":"2026-11-28T01:26:42.010+08:00","pid":31332,"args":{"target":"w3-1","text":"[12 chars]"},"ms":210,"ok":true,"source":"mcp","tool":"type_text","window":"未命名 - 記事本"}
{"ts":"2026-11-28T01:26:43.500+08:00","pid":40112,"args":["type","Notepad","[9 chars]"],"code":"window_not_found","command":"type","error":"window not found","ms":1,"ok":false,"source":"cli"}
```

欄位：`ts`（本機時間）、`pid`、`source`（`mcp`／`cli`）、`tool` 或 `command`、`window`（目標視窗標題，已知時）、`args`（字串最多 300 字元）、`in_run`（`run` 批次中的步驟）、`ms`、`ok`，失敗時加 `code` 與 `error`。`type_text`、`paste_text`、`ui_set_value`、格式錯誤的呼叫與未知工具，除選擇器（`target`、`element`、`name`、`role`、`automation_id`）外所有字串都只記字數；`app_launch` 的程式參數只記個數；`settings` 的變更本身也會記錄（在生效前寫入）。不記錄截圖內容。寫入失敗不會讓操作失敗。

## 虛擬顯示器

可搭配選配的 Windows IDD 驅動，讓 Windows 建立真正的延伸顯示器，再由 rsautogui 擷取其中的 App 畫面。新增 `display list/capture/move`、MCP `displays_list`／`display_capture`／`window_to_display`，以及 `capture`／`locate`／`click` 的 `--display` 選項。既有命令預設仍使用主螢幕。

```powershell
autogui-control display list
# 裝置名稱請取自上面的實際輸出
autogui-control display capture '\\.\DISPLAY2' virtual.png
```

驅動獨立下載與安裝，不增加主程式依賴；準備工具固定版本、核對 SHA-256 與簽章，預設一個 1920×1080、60 Hz 顯示器。驅動安裝需要系統管理員；虛擬螢幕與實體螢幕共用鍵盤、滑鼠與焦點。詳見 [虛擬顯示器設定與使用](docs/virtual-display.md)。

## 無螢幕／無人值守

只要 Windows 仍有一個已登入、未鎖定、正在繪製的桌面，就能在沒人看螢幕時運作；沒接實體螢幕時，以虛擬顯示器提供輸出。遠端桌面最小化或中斷會停止繪製，可在離開前以 `scripts\rdp-to-console.ps1` 把工作階段移到主控台。

```powershell
autogui-control session status          # 有阻擋項時結束碼非 0；--json 供腳本／排程
```

MCP 對應唯讀工具 `session_status`。各情境與限制見 [無螢幕／無人值守指南](docs/headless.md)。

## 畫面辨識與 App 控制

完整操作方式見 [繁體中文指南](docs/recognition-app-control.md)。CLI 提供指定視窗的截圖、找圖、點擊與文字輸入；標題或圖像符合多個目標時會回報錯誤並停止。

```powershell
cargo build --release --workspace --locked
$control = '.\target\release\autogui-control.exe'
& $control list
& $control capture "目標視窗標題" window.png
& $control locate "目標視窗標題" button.png 0.95 5
& $control click "目標視窗標題" button.png 0.95 5
```

`capture`、`locate`、`click`、`type` 會啟用目標視窗；`click` 與 `type` 會送出真實輸入。需要 Windows，以及已解除鎖定的互動桌面。螢幕保護程式或鎖定桌面造成的游標存取拒絕，會以結束碼 `5`（`PermissionDenied`）回報。`examples/app_control` 仍保留為相容的 shim，新腳本請直接使用 `autogui-control.exe`。

## 螢幕保護程式

```powershell
& $control screensaver status
& $control screensaver stop
& $control screensaver pause 900
# 在另一個終端提早還原，或復原異常結束的暫停：
& $control screensaver resume
```

`pause` 可設定 1–86400 秒，預設 15 分鐘；等待期間可按 Ctrl+C 還原。原設定會先保存以便異常結束後復原。`stop` 只停止目前的保護程式，Windows 鎖定仍需登入。完整行為、復原路徑與限制見 [螢幕保護程式指南](docs/screensaver.md)。

## Install（Rust library）

```toml
[dependencies]
autogui = { path = "../rsautogui", features = ["opencv"] }
```

The default `os` feature uses Enigo for real input and XCap for screen capture. For deterministic, headless code, construct `AutoGui::with_fake(FakeBackend::new(width, height))`. Building with `default-features = false` also selects the fake backend.

`with_fake` replaces mouse/keyboard/capture only. Window APIs still use Win32 and are not simulated; `autogui-control` refuses window operations without the `os` feature.

Use the path to this checkout. The `opencv` compatibility feature enables normalized cross-correlation matching for `LocateOptions::confidence`; the implementation uses pure Rust FFT and integer integral images and does not require a system OpenCV installation. Every accepted FFT candidate is verified directly.

## Cheat sheet

This example only reads the screen and pointer. Uncomment an action after choosing its target and deciding that the real input is intended.

```rust
use std::time::Duration;
use autogui::prelude::*;
use autogui::tweens;

fn main() -> Result<()> {
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
    // gui.paste("多語言 text ✓", true)?;                       // clipboard entry, restored
    // let stroke = [Point::new(100, 100), Point::new(160, 140), Point::new(220, 100)];
    // gui.drag_path(&stroke, Duration::from_millis(600), MouseButton::Left)?;

    let options = LocateOptions::default();
    // let button = gui.locate_center_on_screen("button.png", &options)?;
    // gui.click_xy(button.x, button.y)?;
    // let windows = gui.get_windows_with_title("Notepad")?;
    // windows[0].resize_to(800, 600)?;
    let _ = (tweens::linear, options, Duration::ZERO);
    Ok(())
}
```

**Emergency stop: Ctrl+Alt+F12.** With `Settings::stop_key` (default on) every mutating call returns `Error::Stopped` while the chord is held, and the MCP server latches it for the connection. **Fail-safe is on by default.** Mutating and screen-search calls check the configured primary-monitor corners before and after work, and during long searches, drags, repeated inputs and input intervals. Move the cursor to a corner to interrupt. New actions stop; key/button releases still run so input does not remain held. Every public mutating call also observes `Settings::pause`.

`autogui::prelude` exports the common types (`AutoGui`, `LocateOptions`, `Rect`, `Box`, `Point`, `Size`, `Settings`, `Window`, `Error`, `Result`, and others). Tween functions live in `autogui::tweens`.

## Capabilities

| Capability | Windows | macOS | Linux |
|---|---|---|---|
| Mouse and keyboard (`os`) | Enigo | Enigo; Accessibility permission may be required | Enigo; X11 supported, Wayland depends on compositor permissions |
| Screenshot and image locate | XCap, PNG/JPEG/BMP/PPM files | XCap | XCap; Wayland capture support depends on the desktop portal |
| Window control | Win32 | `UnsupportedPlatform` | `UnsupportedPlatform` |
| UI Automation (`uia`) | COM `IUIAutomation` | `UnsupportedPlatform` | `UnsupportedPlatform` |
| Clipboard text (`paste`) | Win32 clipboard | `UnsupportedPlatform` | `UnsupportedPlatform` |

Windows coordinate operations temporarily use a per-monitor DPI-aware thread context and restore the caller's context afterward; they do not change process-wide DPI settings. This path requires Windows 10 1703 or newer. Window activation restores minimized windows and confirms foreground ownership. Show/move/resize requests use asynchronous dispatch and verify the resulting state; application size constraints can cause an error. `close()` posts a close request, which a target app can veto with a save dialog.

`AutoGui::new()` connects to the real desktop. Unit tests should use `with_fake`. The `spiral_drag` example moves the real cursor and is kept separate from the read-only `cheat_sheet` example.

## Tests

```bash
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Live tests that move the real mouse or keyboard are `#[ignore]` and need `AUTOGUI_LIVE=1`. The ignored smoke test only reads the desktop and writes a temporary screenshot:

```powershell
$env:AUTOGUI_LIVE = "1"
cargo test --test live_smoke -- --ignored
```

The native app-control test creates and closes its own fixture window, verifies a real button-click receipt and Unicode text, and restores the previous pointer/focus where possible:

```powershell
cargo build --workspace
$env:AUTOGUI_LIVE = "1"
$env:AUTOGUI_CONTROL_EXE = (Resolve-Path .\target\debug\autogui-control.exe).Path
cargo test --all-features --test live_app_control -- --ignored --nocapture
```

The installer tests use isolated fixture directories and never touch the real profile or `%APPDATA%`. They need a payload from `scripts\package.ps1 -SkipBuild`:

```powershell
.\tests\installer.ps1 -PackageRoot <payload directory>
```

## 辨識效能

0.2.0 的完整數字、量測方式與重現指令見 [效能量測](docs/performance.md)。下表為 1920×1080 合成影像、80×40 模板的中位數（毫秒），不含截圖 I/O 與 `Settings::pause`。這些是基準，不是任意畫面的延遲保證。

| 搜尋 | 優化前 | 0.2.0 |
|---|---:|---:|
| 精確比對，RGB 雜訊／UI 圖樣 | 3.00 / 6.37 | 0.88 / 0.86 |
| 精確比對，灰階 | 3.88 / 10.28 | 1.51 / 1.60 |
| 信心 0.999，RGB | 248.5 | 25.1 |
| 信心 0.9，灰階 | 118.8 | 16.1 |
| 信心 0.95，RGB UI | 246.8 | 25.4 |
| 截圖（主螢幕／400×300 區域／顯示器） | 未變 | 32.7／16.5／32.9 ms（xcap／GDI 限制） |

重現：`pwsh scripts/bench.ps1`（`-Json` 輸出機器可讀格式）。信心比對請用 `LocateOptions::region` 縮小範圍，以降低截圖、搜尋時間與 FFT 記憶體。專案目標為 1080p 截圖 < 200 ms、≤ 80×40 模板精確比對 < 2 s。

## Differences from PyAutoGUI

- `AutoGui` instances and `Settings` replace process-wide `PAUSE` / `FAILSAFE` variables.
- `Rect` is the main rectangle type; `Box` is a compatibility alias.
- Image misses return `Error::ImageNotFound`; use `try_locate_on_screen` for an optional result. `set_raise_image_not_found(false)` makes locate-all calls return an empty vector on misses.
- `Screenshot` replaces Pillow images. Common raster formats are loaded and saved through the Rust `image` crate.
- Message boxes are out of scope. Window control is available only on Windows.
- Multi-monitor virtual-desktop coordinates and OCR are out of scope.

## License

[MIT](LICENSE).
