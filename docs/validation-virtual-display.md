# 虛擬顯示器驗證 — 2026-09-27

## 實機結果

Windows 10 19045 x64、NVIDIA RTX 3060 Ti，實體主螢幕 1920×1080，另外安裝 VirtualDrivers/Virtual-Display-Driver 25.7.23，建立一個 1920×1080、60 Hz 延伸顯示器。實體螢幕保留 primary；未修改憑證信任、test signing 或 Secure Boot。

| 驗收項目 | 結果與證據 |
|---|---|
| 固定下載／簽章 | PASS：ZIP SHA-256、SignPath CAT 與 Nefarius helper Authenticode 均通過 |
| 實際安裝 | PASS：NefCon exit 0，MttVDD 裝置狀態 OK，無須重啟 |
| Windows 延伸顯示器 | PASS：rsautogui 列出實體主螢幕與 VDD，VDD bounds `(1920,0,1920,1080)` |
| Library 實體座標 | PASS：DPI-unaware 呼叫端下，虛擬螢幕邊緣與中央共 25 點精確讀回；呼叫後 DPI context 還原 |
| CLI | PASS：移動自有原生 Win32 App、截取 1920×1080 PNG、找圖、點擊；App 收到按鈕事件 |
| MCP | PASS：移動同一 App、PNG、找圖、點擊事件、繁體中文輸入讀回 |
| 防止選錯畫面 | PASS：越界 region、過期 display 參照拒絕；省略 display 保留主螢幕行為；window 參照不受重新列舉 display 影響 |
| 非 Windows／無 os | 顯示器功能回報平台或 feature 限制；不提供模擬的 OS 顯示器 |
| 移除／重新安裝 | NOT_RUN：本機保留已啟用的虛擬顯示器；程式只移除安裝紀錄擁有的裝置 |
| 負座標實機／DRM／鎖定桌面 | NOT_RUN；負座標邊界有純邏輯測試，鎖定／保護畫面不保證可擷取 |

截圖確實顯示測試 App 的中文按鈕與「虛擬螢幕截圖與 MCP 點擊驗證完成」輸入內容；並非空白 PNG 或合成背景。兩次點擊以 App 的 `WM_COMMAND` 計數驗證，文字以該 App 的 EDIT 控制項讀回確認。

## 可重現指令

一般回歸不發送桌面輸入，互動驗收明確標記 ignored：

```powershell
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo build --release --workspace --all-features --locked
.\tests\mcp-protocol.ps1 -Executable .\target\release\autogui-control.exe

# 明確指定 display list 回傳的副螢幕名稱。此測試會建立自己的視窗並移動共用游標。
$env:AUTOGUI_LIVE = '1'
$env:AUTOGUI_DISPLAY = '\\.\DISPLAY5'
$env:AUTOGUI_MCP_EXE = "$PWD\target\release\autogui-control.exe"
$env:AUTOGUI_EVIDENCE_DIR = "$PWD\.agent\virtual-display-live"
cargo test --all-features --test live_app_control controls_own_app_on_selected_display -- --ignored --nocapture --test-threads=1
```

此機器的 `DISPLAY5` 不應硬編碼至其他電腦。互動測試只操作自己建立的視窗，結束時關閉它並還原游標／焦點；不安裝驅動或改變顯示拓撲。

本機證據（不放入公開 Git）：`.agent/virtual-display-live/result.json`、`virtual-display-app.png`、`virtual-display-mcp.png`、`virtual-display-cli.png`、`.agent/virtual-display-live.log`、`.agent/virtual-display-install.log`。安裝復原紀錄保留於 `%ProgramData%/rsautogui/virtual-display.json`。

## 審查與修正

AGY 獨立唯讀審查後，採納使用活動顯示器實體 bounds 聯集、先縮小再移動、已移除且未變更設定的安全重新安裝、容許空 HardwareID 的裝置列舉。保留嚴格游標讀回，另在 pause 後重查；25 點實機驗證支持目前的像素中心正規化公式。

初次互動驗收曾觀察到游標在 pause 期間移動，以及焦點／跨螢幕後舊模板不再匹配。已補上 pause 後位置檢查，測試重新觀察移動後的控制項，不放寬比對門檻或偽造成功。
