# Agent / DLL 交付驗證

## 2026-11-15：Flow 操作改善

環境：Windows x64、互動主控台、1920×1080 主螢幕；Rust/Cargo 1.99.0、Windows PowerShell 5.1。驗證使用自有原生測試視窗，沒有操作瀏覽器帳號或送出生成請求。

- PASS：`cargo test --workspace --all-features --locked --offline`（82 項非 ignored 測試）；`cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`；`cargo fmt --all -- --check`。
- PASS：`tests/mcp-protocol.ps1 -Executable target/debug/autogui-control.exe`，14 工具、按需 DLL、JSON-RPC、無截圖憑證拒絕輸入及缺 DLL 錯誤。腳本保留 UTF-8 BOM，供 Windows PowerShell 5.1 正確讀取中文。
- PASS：設 `AUTOGUI_LIVE=1`、`AUTOGUI_MCP_EXE` 及 `AUTOGUI_CONTROL_EXE` 為 `target/debug/autogui-control.exe`，執行 `cargo test --all-features --locked --offline --test live_app_control recognizes_and_controls_own_native_app -- --ignored --nocapture --test-threads=1`。覆蓋最大化→最小化→還原、移動／縮放正規化、真實 click_at 與兩軸 wheel 事件、憑證消耗／越界／像素／失焦／幾何拒絕、找圖、中文及 emoji 完整輸入。
- 長文字單次量測：1,125 scalar（1,250 UTF-16 units）派送 1.09 秒、原生 EDIT 逐字完整接收 21.37 秒。派送與應用程式完成分開驗證；此數值不是瀏覽器效能保證。診斷以原生訊息計數確認接收持續進行，長文字與非同步關閉各使用 30 秒期限，短操作保留原 3 秒期限。
- PASS：安裝器對既有 0-byte 與含 BOM 純空白 JSON 的暫存安裝／預覽、原始 bytes 備份、uninstall 還原、無關設定保留，以及 shared／legacy／custom AGY Skill 路徑。舊版同條件曾回報 `Missing value`（exit 1）。
- NOT_RUN：非 Windows 平台、Rust 1.85、第二顯示器本次實機驗證、Flow 網頁端操作。此 repository 未設定 GitHub Actions，遠端 CI 無執行紀錄；本機結果不可視為遠端 CI。

本次僅提供分段鍵盤輸入；剪貼簿貼上與 UI Automation 尚未實作。截圖點擊比對點周圍最多 32×32 像素，無法偵測範圍外或最後檢查後的變動。

## 2026-09-26：原始交付

日期：2026-09-26。環境：維護者本機 Windows x64；命令於 repository 根目錄執行。

## 容量與功能

| 產物 | bytes |
|---|---:|
| 調整前 standalone EXE | 7,586,304 |
| 新 `autogui-control.exe` | 307,200 |
| 新 `autogui_runtime.dll` | 5,342,720 |
| 新 EXE + DLL 合計 | 5,649,920 |

合計減少約 **25.52%**，包含新增 MCP / Win32 CLI 工具。完整 image 預設格式、截圖、精確/相似度辨識、輸入、視窗與螢幕保護程式功能都保留。EXE 小型化與合計縮減分開統計。

同一台機器、合成 1920×1080 / 80×40 模板、Release 單次量測：精確 3.73 ms；confidence=0.999 為 310.38 ms。調整前本次基線為 4.29 / 309.41 ms。排除截圖、檔案 I/O 及 pause；不是所有畫面的延遲保證。

## 驗收

| 驗收 | 結果 |
|---|---|
| `cargo test --workspace --all-features` | PASS，52 項；另有 2 項 live 測試預設 ignored |
| `cargo test -p autogui --no-default-features` | PASS，34 項 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |
| Release workspace build、DLL CLI help/status | PASS |
| MCP 初始化／工具列舉不載入 DLL | PASS，實際查詢程序模組清單 |
| 第一次工具呼叫載入 DLL、分離 EXE 的缺 DLL 錯誤 | PASS |
| JSON-RPC lifecycle、錯誤、通知、schema、late cancellation 回歸 | PASS |
| 原生自建視窗：MCP move/resize/hide/show/minimize/maximize/restore/close | PASS，Win32 狀態及幾何讀回 |
| MCP PNG / locate / click / hotkey / 中文輸入 | PASS，PNG 解碼、按鈕事件計數、EDIT 文字讀回；截圖確認中文字形 |
| 100 個未知取消通知後取消實際找圖 | PASS，五秒 deadline 內回傳 cancelled |
| MCP 螢幕保護程式 pause → EOF | PASS，暫停中讀回 false，斷線後完整原狀態讀回一致 |
| 四種宿主可選安裝／程式單獨安裝／衝突保護 | PASS，隔離的 HomeDirectory |
| JSONC 註解保留、解除安裝還原、同位置重裝 | PASS |
| 解除安裝保留使用者新增設定、未選宿主不寫入 | PASS |
| Windows PowerShell 5.1 安裝與還原 | PASS |
| Skill quick_validate | PASS |
| 四種宿主真實登入會話的 Skill/MCP 載入 | NOT_RUN；沒有修改使用者全域設定或重啟 Agent |
| AGY 正式審查 verdict | BLOCKED：輸出上限導致 ERROR，續接受當時尚無 Git repository 限制；部分線索經本機獨立判斷並修正 |

此頁整理維護者本機驗收結果。原始桌面影像、Agent 對話、設定備份及作業日誌保留在本機，未納入公開 repository。安裝測試只接觸 `.agent` 內可丟棄 profile。

可重現的測試程式包含 [原生視窗與 MCP](../tests/live_app_control.rs)、[MCP 協定](../tests/mcp-protocol.ps1)、[隔離安裝](../tests/installer.ps1)；執行方式見 [畫面辨識指南](recognition-app-control.md)。

原有螢幕保護程式的 timer／Ctrl+C／manual resume／crash recovery／停止實際 saver 亦已在本機驗收；本次新增的是 MCP lease/EOF 路徑。
