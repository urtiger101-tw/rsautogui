# 畫面辨識與 Windows App 控制

本專案提供 Rust library 和 `app_control` 命令列範例。控制目標需要在主螢幕可見，並且 Windows 已登入、解除鎖定。標題查詢會使用不分大小寫的子字串；符合多個視窗時會列出候選並停止。

螢幕保護程式阻擋桌面時，可先使用 `app_control screensaver stop`。自動化期間可另開終端執行 `app_control screensaver pause 900` 暫停自動啟動；完整說明見 [螢幕保護程式控制](screensaver.md)。

## 建置與使用

在 clone 後的 repository 根目錄執行：

```powershell
cargo build --release --features opencv --example app_control
$control = '.\target\release\examples\app_control.exe'
& $control --help
& $control list
& $control capture '目標視窗標題' window.png
& $control locate '目標視窗標題' button.png 0.95 5
& $control click '目標視窗標題' button.png 0.95 5
& $control type '目標視窗標題' '要輸入的繁體中文'
```

`capture`、`locate`、`click`、`type` 都會啟用目標視窗。`type` 會輸入到該視窗目前取得鍵盤焦點的元件；可先用 `click` 點選輸入框。`click` 與 `type` 送出真實輸入，應在你選定的 App 上執行。

`locate`／`click` 的最後三個參數為 `confidence|exact`、重試秒數及模板縮放比例。省略時為 `0.95`、`5`、`1`。例如模板取自 100% 顯示比例，目標元件現在為 125% 大小時，可以先以明確的比例找圖：

```powershell
& $control locate '目標視窗標題' button.png 0.95 5 1.25
```

縮放比例不是自動 DPI 推論；有些 App 會重新排版、改字型或改變圖示，需要重新擷取模板。重試秒數是掃描之間的等待預算，單次掃描及檔案讀取可能讓總耗時超過該值。

## 準備辨識模板

從同一個 App、主題、顯示比例擷取 PNG，裁出含有文字或圖示的區塊。保留足以區分其他元件的特徵；只有單一底色的區塊通常會符合多個位置。不要把整張視窗截圖直接當成某一個按鈕的模板。

Rust 端可以直接擷取已知區域：

```rust
let mut gui = autogui::AutoGui::new()?;
let screenshot = gui.screenshot_region(autogui::Rect::new(300, 220, 100, 40))?;
screenshot.save("button.png")?;
```

座標是主螢幕左上角為原點的實體像素。Windows 的座標呼叫會暫時設定執行緒 DPI awareness 並還原，不會改動宿主 App 的全域 DPI 設定。

## 辨識方式

- `exact`：RGB 像素完全相同。字型反鋸齒、動畫、主題或縮放不同，都可能找不到。
- `confidence`：逐色彩通道的標準化互相關；大範圍使用 FFT 初篩，再對接受的候選逐一重算。分數是相似度，並非成功機率；`1.0` 也不等同於 RGB 完全相等，因為相關係數容許亮度平移。
- 純色模板沒有可用的相關變異數，因此改用 `1 - RMSE / 255` 比較顏色距離。
- `grayscale`：只比亮度，會失去顏色區別；亮色運算已修正為不溢位。
- `region`：只擷取並搜尋指定範圍，結果仍回傳主螢幕絕對座標。範圍超出主螢幕會裁切；模板比有效範圍大會回傳 `InvalidArgument`。
- 多個重疊框依 SPEC 以 IoU > 0.3 抑制；`app_control` 若最後仍有多個候選，會停止並要求更具辨識性的模板。

大範圍搜尋建議使用 Release 建置與視窗範圍。FFT 需要與搜尋畫面大小相關的額外記憶體；低門檻或大量重複圖樣會產生更多候選，增加複核成本。

## 操作可靠性

`app_control` 先選出唯一視窗，還原最小化狀態並確認前景，再在該視窗的可見範圍找圖。點擊前會重新擷取候選區域、複核圖樣、視窗位置與前景狀態。文字輸入會逐字確認前景視窗。Windows 的全域輸入無法與截圖形成原子操作；最後一次檢查和送出輸入之間，仍可能有使用者或 App 改變畫面，因此此工具不保證對快速移動或瞬間切換的 UI 零誤差。

**預設 fail-safe 開啟。把游標移到主螢幕任一角落可中止長時間搜尋、拖曳或重複輸入。** 放開按鍵與滑鼠的清理仍會執行，以免殘留按住狀態。

```rust
let mut held = gui.hold(&["ctrl"])?;
held.click()?;
drop(held); // 反序釋放修飾鍵
```

| 情況 | 結果／處理 |
|---|---|
| 螢幕保護程式、鎖定桌面，游標讀取被 Windows 拒絕 | `PermissionDenied`；恢復互動桌面後再執行 |
| Windows 拒絕切換前景 | `PermissionDenied("SetForegroundWindow")`；工具不繞過前景鎖定 |
| 視窗在搜尋時移動 | 重新掃描；點擊前移動則中止 |
| 視窗已關閉或換成不同 process/thread 的 handle | `WindowNotFound` |
| 調整大小受 App 最小尺寸限制 | 確認狀態未達成時回傳錯誤，可改用 App 允許的尺寸 |
| 管理員權限 App、保護桌面、部分遊戲 | Windows 可能限制輸入；目前沒有提升權限或繞過保護機制 |
| 背景、最小化或被遮擋的控制項 | 螢幕找圖無法辨識不可見像素，需先顯示目標 |

## 驗證

```powershell
cargo test --workspace --all-features
cargo test --no-default-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo run --release --features opencv --example recognition_bench -- --full-hd
```

真實互動測試會建立自己專用的 Win32 視窗，用按鈕收到的事件及輸入框讀回的文字驗收，並在結束時關閉該測試視窗：

```powershell
cargo build --features opencv --example app_control
$env:AUTOGUI_LIVE = '1'
$env:AUTOGUI_CONTROL_EXE = (Resolve-Path .\target\debug\examples\app_control.exe).Path
$env:AUTOGUI_EVIDENCE_DIR = (Join-Path (Get-Location) '.agent\live-evidence')
cargo test --all-features --test live_app_control -- --ignored --nocapture
```

首次實機驗收曾被 Windows `Screen-saver` 輸入桌面阻擋。新增停止功能後，已於 2026-09-26 通過上述自建視窗辨識、真實點擊、Unicode 輸入、修飾鍵與 CLI 操作驗收。結果摘要見 [交付驗證](validation-agent.md)，可使用上述命令在自己的 Windows 桌面重現。

## 實作參考

- [RustFFT 官方文件](https://docs.rs/rustfft/6.4.1/rustfft/)
- [SetThreadDpiAwarenessContext](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setthreaddpiawarenesscontext)
- [ShowWindowAsync](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindowasync)
- [SetWindowPos 的非同步旗標](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)
