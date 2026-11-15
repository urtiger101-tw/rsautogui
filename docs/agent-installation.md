# Agent 安裝、MCP 與按需載入 DLL

## 安裝

解壓縮整包，執行 `install.cmd`。安裝程式會分別詢問要安裝 **Skill** 與 **MCP** 的 Agent，可填 `codex,claude,opencode,agy` 的任意組合；兩者留空就是只安裝程式。預設位置為 `%LOCALAPPDATA%\Programs\AutoGui`，不需要管理員權限、不修改 PATH 或 Agent 的批准規則。安裝後重新啟動所選 Agent。

```powershell
# 先預覽
.\install.ps1 -SkillHosts codex,claude -McpHosts codex -Preview
# 無互動安裝
.\install.ps1 -SkillHosts codex,claude -McpHosts codex -NonInteractive
# 其他可讀取 SKILL.md 的 Agent
.\install.ps1 -CustomSkillRoots 'C:\my-agent\skills' -NonInteractive
# AGY 舊 CLI 路徑（預設使用 App／CLI 共用目錄）
.\install.ps1 -SkillHosts agy -AgySkillRoot "$HOME\.gemini\antigravity-cli\skills" -NonInteractive
# 只安裝程式
.\install.ps1 -NonInteractive
```

安裝器只寫所選目的地；部分 Agent 本來就會讀取其他 Agent 的共享 Skill 目錄，選項無法禁止宿主這種探索行為。已存在的 `autogui-control` Skill 或 `autogui` MCP 設定會被保留並中止安裝，避免覆蓋自訂內容。

| Agent | Skill 目錄 | MCP 設定 |
|---|---|---|
| Codex | `$CODEX_HOME/skills`，預設 `~/.codex/skills` | 用已安裝的 `codex mcp add` 更新 `$CODEX_HOME/config.toml` |
| Claude Code | `~/.claude/skills` | `~/.claude.json` 的 `mcpServers.autogui` |
| OpenCode | `$XDG_CONFIG_HOME/opencode/skills`，預設 `~/.config/opencode/skills` | 同目錄 `opencode.jsonc`（若存在）或 `opencode.json` 的 `mcp.autogui` |
| AGY App／CLI | `~/.gemini/config/skills`（共用全域目錄） | `~/.gemini/config/mcp_config.json` 的 `mcpServers.autogui` |

可用 `-HomeDirectory`、`-InstallDir`、`-CodexHome`、`-OpenCodeHome`、`-ClaudeConfig`、`-AgyConfig`、`-AgySkillRoot` 指定隔離/自訂位置。`-AgySkillRoot` 是 Skill 父目錄，只在選取 `-SkillHosts agy` 時使用；可指向 CLI 仍支援的 `~/.gemini/antigravity-cli/skills`、`~/.gemini/skills` 或其他共享目錄。安裝器不搬移既有 Skill，解除安裝依原 manifest 的檔案路徑處理，因此舊版安裝仍可用原解除安裝程式移除。

Codex 的 MCP 安裝需要本機 Codex CLI；其餘宿主只寫設定檔。JSONC 以局部插入保留既有註解、格式及其他項目。既有空檔或僅含空白的 JSON MCP 設定，在解析時視為 `{}`；原始 bytes 仍保留備份。若安裝後設定未被修改，解除安裝會逐 byte 還原空檔／空白，而非寫成 `null`；復原備份仍留在 `AutoGui.recovery-*`。

解除安裝：執行安裝目錄內 `uninstall.ps1`。安裝器會備份要修改的 MCP 設定，並記錄安裝後雜湊。解除安裝只刪除未變更的自有檔案、還原未再變更的設定；使用者後來修改過的檔案會保留並明確提示。備份包含原設定，僅留在本機，不會放入發行包。完整解除安裝後，原始設定備份移至程式目錄旁的 `.recovery-<id>` 目錄，讓原位置可重新安裝。升級前先解除安裝；若仍有自行修改的內容，整理後再安裝。

## DLL 載入

`autogui-control.exe` 是小型啟動程式與 MCP 協定端點。`--help`、MCP `initialize`、`ping`、`tools/list` 不載入 `autogui_runtime.dll`。第一次 `tools/call` 才載入 DLL；CLI 的實際控制命令也會載入。DLL 在該程序存續期間共用，結束連線時釋放暫停租約，模組本身保持映射至程序結束，以保護 Rust TLS/背景清理。

DLL 包含完整截圖、圖片格式、精確/相似度辨識、Win32、鍵鼠及螢幕保護程式功能；沒有移除圖片格式。不是每張圖片再載入一份 DLL，也沒有從網路下載模組。EXE 只從自身旁邊以絕對路徑載入 DLL，依賴搜尋限制在 DLL 目錄與 Windows System32。不要單獨搬移 EXE。

Release 使用 opt-level=s、Thin LTO、單一 codegen unit、移除符號；辨識核心與 rustfft 維持 opt-level=3；保留 panic unwind，讓鍵鼠與暫停 RAII 清理可執行。EXE 大小與 EXE+DLL 總大小分開量測。來源建置：

```powershell
cargo build --release --workspace --all-features --locked
.\scripts\package.ps1 -SkipBuild
```

## MCP

命令：`autogui-control.exe mcp`，stdio newline JSON-RPC，stdout 僅協定訊息、stderr 診斷，沒有 TCP 連接埠。支援 2024-11-05、2025-03-26、2025-06-18、2025-11-25 版本。先 initialize，再送 notifications/initialized。工具 schema 在 `mcp-tools.json`。

提供 14 個工具：`session_status`、`displays_list`、`display_capture`、`window_to_display`、`windows_list`、`window_control`、`capture`、`locate`、`click_image`、`click_at`、`scroll`、`type_text`、`hotkey`、`screensaver`。清單回傳連線專用 target／display；重新列舉會使同類舊參照失效。擷取回傳 PNG image content，不要求宿主另讀本機檔案。輸入送出後仍須確認 App 的實際結果。指定顯示器與選配驅動見 [虛擬顯示器指南](virtual-display.md)。

每行最大 1 MiB，超過會關閉連線；待處理佇列 16 筆，滿載時回傳 Server busy，保持連線。搜尋 timeout 最大 30 秒，文字最大 4096 個 Unicode scalar；每段最多 32 個字元，段前檢查取消、EOF 及前景，底層逐字 fail-safe 保留。單次原生呼叫/辨識不能中途撤回，段內失焦可能已有部分文字送出。序列處理桌面輸入，避免多工具互相搶焦點。暫停以專屬子程序持有租約，MCP 消失時管線 EOF 觸發還原；強制終止 helper 後，可由 durable journal 與 `screensaver resume` 恢復。

`click_at {target,capture_id,x,y}` 與 `scroll {target,capture_id,x,y,clicks,horizontal?}` 使用最近一次 `capture` 的 PNG 相對座標及一次性憑證（60 秒過期）。任何嘗試均消耗憑證；其他工具呼叫除 `session_status` 外亦使憑證失效，請操作後重新擷取。輸入前後檢查目標、前景、視窗／顯示器幾何、游標及點周圍最多 32×32 像素。hover 變色時先重新擷取；局部範圍外與最後檢查後的畫面變化仍須由操作者確認。詳見[座標操作說明](recognition-app-control.md#mcp-截圖座標操作與長文字)。

## Windows API

`window`/`window_control` 使用 `EnumWindows`、`GetWindowTextW`、`SetForegroundWindow`、`ShowWindow`、`SetWindowPos`、`PostMessageW(WM_CLOSE)`。`move`/`resize` 讀回幾何；Window 保存 HWND 的原程序/執行緒身分。一般鍵鼠透過 Enigo 使用 Win32 輸入；指定顯示器的點擊使用 `SendInput` 的 `MOUSEEVENTF_VIRTUALDESK`。預設主螢幕，可明確指定實體／虛擬顯示器，座標為桌面實體像素；不提供 OCR、UI Automation 控制樹、任意 SendMessage 或提權/解鎖。

`hide` 後視窗不在可見清單；MCP 可用保留的 target 執行 `show`。CLI 以可見標題搜尋，因此不要用 CLI hide 後期待同一標題搜尋能找回；優先用 MCP 操作 hide/show。

## 官方介面參考

- [MCP lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle) 與 [tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
- [OpenCode Skills](https://opencode.ai/docs/skills) 與 [MCP](https://opencode.ai/docs/mcp-servers/)
- [Claude Code Skills](https://code.claude.com/docs/en/skills) 與 [MCP](https://code.claude.com/docs/en/mcp)
- [AGY CLI 設定遷移及路徑](https://antigravity.google/docs/cli/gcli-migration/)
- [Antigravity Skills（App／CLI 共享與 CLI 舊路徑）](https://antigravity.google/docs/skills/) 與 [Customization discovery](https://antigravity.google/docs/customization-discovery/)
- Codex 註冊參數以本機 `codex mcp add --help` 為準。
