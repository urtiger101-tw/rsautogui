# Agent / DLL 交付驗證

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
