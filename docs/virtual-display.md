# 虛擬顯示器與指定螢幕截圖

Windows IDD（Indirect Display Driver）把軟體顯示器加入 Windows 顯示拓撲。Windows/DWM 實際繪製 App 到該顯示器，rsautogui 再讀取其像素。記憶體中的假截圖或 Windows「新增桌面」並不能建立這種顯示輸出。

這是原本 v1 主螢幕 API 之外的明確選用擴充；`AutoGui::new()`、`screenshot()`、原有 CLI/MCP 未指定顯示器時仍使用主螢幕。原主螢幕角落 fail-safe 仍生效。

## 1. 選配驅動

採用 [VirtualDrivers/Virtual-Display-Driver 25.7.23](https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/tag/25.7.23) 的 x64 driver-only 套件。安裝輔助使用 [NefCon 1.14.0](https://github.com/nefarius/nefcon/releases/tag/v1.14.0)。兩者按需下載，rsautogui 的 EXE／DLL 不內嵌第三方驅動，也不在啟動時自動下載。

先在一般 PowerShell 準備檔案，此步不變更系統：

```powershell
.\scripts\virtual-display.ps1 -Action Status
.\scripts\virtual-display.ps1 -Action Prepare
```

工具固定下載版本並核對 ZIP 的 SHA-256、驅動 CAT 的有效 SignPath 簽章與 NefCon 的有效 Nefarius 簽章。預設一個 1920×1080、60 Hz 顯示器；可指定 `-Width`、`-Height`、`-RefreshRate`。僅支援 Windows x64。

確認要在電腦新增顯示驅動後，在**系統管理員 PowerShell** 執行：

```powershell
.\scripts\virtual-display.ps1 -Action Install
```

此動作使用原始 INF 建立 `Root\MttVDD` 裝置，把單螢幕設定寫入 `C:\VirtualDisplayDriver\vdd_settings.xml`，並在 `%ProgramData%\rsautogui\virtual-display.json` 留下裝置 ID／安裝狀態。既有同名驅動、設定目錄或復原紀錄會讓安裝中止，不覆寫它們。工具不匯入憑證、不啟用 test signing、不變更 Secure Boot，也不自動重啟。

若 Windows 拒絕驅動信任，依錯誤停止並保留紀錄；不要為此降低系統簽章原則。需要重開機時會回報 `reboot-required`。裝置成功安裝不代表已有可擷取的活動顯示器。

到 Windows「設定 → 系統 → 顯示器」確認虛擬顯示器使用「延伸」，保留實體螢幕為主螢幕。若主機沒有接任何實體螢幕，虛擬顯示器即為唯一且主要的顯示器，所有預設 API 與角落 fail-safe 都以它為準；無人值守條件見 [無螢幕指南](headless.md)。可移動虛擬顯示器在拓撲中的位置；左方／上方的螢幕可能具有負座標。未啟用的顯示器不會出現在 rsautogui 清單。

移除這支工具建立的裝置：

```powershell
.\scripts\virtual-display.ps1 -Action Remove
```

只按安裝紀錄移除仍符合 MttVDD 硬體 ID 的裝置；保留 driver-store 套件、設定與復原紀錄。一般 rsautogui 解除安裝不移除系統驅動。移除後可重新 `Install`：僅接受本工具已標記 `removed` 且設定 SHA-256 未變的紀錄，並先備份舊紀錄與設定。不完整的安裝或使用者改過的設定會停止，保留供檢查。

## 2. 把 App 放到虛擬顯示器並截圖

在發行包根目錄執行；`\\.\DISPLAY2` 是範例，請換成 `display list` 回傳的完整裝置名稱：

```powershell
$exe = '.\autogui-control.exe'
& $exe display list
& $exe display move '\\.\DISPLAY2' '唯一的 App 標題'
& $exe display capture '\\.\DISPLAY2' virtual-desktop.png
& $exe capture '唯一的 App 標題' app.png --display '\\.\DISPLAY2'
& $exe locate '唯一的 App 標題' 'C:\templates\button.png' 0.95 5 1 --display '\\.\DISPLAY2'
& $exe click '唯一的 App 標題' 'C:\templates\button.png' 0.95 5 1 --display '\\.\DISPLAY2'
```

`display move` 會還原最大／最小化視窗，把它移到顯示器內 16 像素處，必要時縮小以容納。App 最小尺寸限制可能導致失敗；錯誤前的移動可能已生效，請重新檢查位置。直接 `display capture` 不切換前景；視窗 `capture/locate/click` 仍會啟用目標 App。

## 3. MCP／Skill

先安裝更新後的 EXE 與 DLL，再重啟 MCP 連線。`--help`、`initialize`、`tools/list` 仍不載入 DLL。

1. `displays_list {}` 取得當次連線的 `display` 參照、裝置名稱、`rect` 與 `primary`。裝置名稱由 Windows 提供，清單不以名稱猜測實體／虛擬身分。
2. `windows_list {"title":"唯一 App 標題"}` 取得 `target`。
3. `window_to_display {"target":"...","display":"..."}` 移動 App，回傳操作前後幾何。
4. `display_capture {"display":"..."}` 回傳整個顯示器的 PNG image content；也可指定 `region`，最多 16 megapixels、PNG 最多 12 MiB。
5. `capture`／`locate`／`click_image` 加上同一個 `display` 參數即可在該顯示器操作。省略仍使用主螢幕。

所有 `rect`、`region`、搜尋結果與點擊位置都是 **Windows 桌面實體像素**。圖片內座標 `(u,v)` 對應桌面 `(region.left+u, region.top+v)`，不再乘 DPI 比例。區域必須完全位於指定顯示器內。原點可能為負數。

重新列舉會讓舊 `display` 參照失效；原有 `target` 參照依然由 `windows_list` 獨立管理。拔插、變更主螢幕、解析度或位置後重新列舉。使用前與截圖後會重新核對螢幕快照，變更時回報錯誤，不切換到其他螢幕。指定螢幕點擊使用 `SendInput(MOUSEEVENTF_VIRTUALDESK)`，讀回游標位置後才送出點擊。

## 4. Rust library

```rust
use autogui::{AutoGui, Display, Error};
let display = Display::all()?.into_iter()
    .find(|d| d.name() == r"\\.\DISPLAY2")
    .ok_or(Error::InvalidArgument("select an existing display"))?;
let mut gui = AutoGui::new()?;
gui.screenshot_display(&display)?.save("virtual-desktop.png")?;
// screenshot_display_region(&display, region) 使用桌面實體像素。
```

這些顯示器 API 需要 Windows 與 `os` feature；`FakeBackend` 不建立 OS 顯示器，明確的顯示器截圖仍讀取原生畫面。未增加 OCR 或 UI Automation。

## 限制與驗證邊界

- 虛擬顯示器屬於同一個 Windows 登入工作階段；所有顯示器共用滑鼠、鍵盤與前景焦點。需要與使用者輸入隔離時應使用 VM／另一個互動工作階段。
- 桌面必須登入且解除鎖定；虛擬顯示器不繞過鎖定桌面、UAC、DRM 或 App 權限。
- 截圖來自 XCap 的 Windows GDI monitor capture。特殊 GPU／受保護 App 可能輸出黑畫面，需以實際目標 App 確認；安裝裝置、列出螢幕與取得有效 PNG 是不同驗收階段。
- 2026-09-27 已在 Windows 10 19045 + NVIDIA RTX 3060 Ti，透過 VDD 25.7.23 的 1920×1080 延伸顯示器實測：原生 App 移入、真實 PNG、25 個精確游標位置、CLI/MCP 圖像辨識、App 收到兩次按鈕點擊與繁體中文輸入。這是指定環境的驗收，其他 App／GPU 仍需確認。詳見 [驗證紀錄](validation-virtual-display.md)。

App 在不同 DPI 顯示器之間移動，或控制項取得焦點後，像素可能改變。移動完成後重新擷取目標畫面，使用當前外觀的模板；不要把 `image not found` 當成成功，也不要自動放寬辨識門檻。

## 參考

- [Microsoft IDD 模型](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/indirect-display-driver-model-overview)
- [VirtualDrivers 原始碼與授權](https://github.com/VirtualDrivers/Virtual-Display-Driver)
- [NefCon 原始碼與授權](https://github.com/nefarius/nefcon)
- [另一個 Rust IDD 專案](https://github.com/MolotovCherry/virtual-display-rs)：供架構參考，本版不安裝其自訂憑證或驅動。
