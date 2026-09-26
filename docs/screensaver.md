# Windows 螢幕保護程式控制

`app_control` 提供 `screensaver` 子命令，能查詢狀態、定時暫停自動啟動、提早恢復與停止目前的螢幕保護程式。此功能是 Windows CLI 擴充；既有 Rust library API 維持相容。

## 使用

```powershell
cargo build --release --features opencv --example app_control
$control = '.\target\release\examples\app_control.exe'
& $control screensaver status
& $control screensaver stop
& $control screensaver pause 900
```

執行包中的檔名為 `autogui-control.exe`，將 `$control` 改成其路徑即可。

| 指令 | 行為 |
|---|---|
| `screensaver status` | 顯示啟用／執行狀態、閒置秒數、登入要求、輸入桌面及本工具暫停狀態 |
| `screensaver pause [秒數]` | 暫停自動啟動；預設 900 秒，可設 1–86400 秒。此程序會等待並暫時保持顯示器喚醒 |
| `screensaver resume` | 結束目前暫停並還原；也能復原被強制結束的暫停程序 |
| `screensaver stop` | 向專用 `Screen-saver` 桌面上的 `.scr` 視窗送出關閉要求，再確認已停止 |

`pause` 只處理之後的自動啟動。若保護程式已經在執行，先用 `stop`。`stop` 不會永久停用自動啟動。

## 自動化期間暫停

在第一個終端執行：

```powershell
& $control screensaver pause 1800
```

它會保持執行 30 分鐘；在另一個終端執行你的找圖、點擊或輸入命令。到期會自動還原，也能在第一個終端按 **Ctrl+C**，或在另一個終端執行：

```powershell
& $control screensaver resume
```

一次只允許一個暫停程序，重複執行會回報錯誤，避免覆寫原設定。確認設定可用 `screensaver status`；暫停中應顯示「啟用：false」與「本工具暫停：執行中」。

## 設定與復原

- 暫停前先將原啟用狀態寫入 `%LOCALAPPDATA%\autogui\screensaver-pause-<登入識別>.state`，並同步至磁碟，再變更 Windows 狀態。
- 只修改目前工作階段的螢幕保護啟用狀態，不寫入使用者的持久設定，也不改動逾時、登入要求或 Windows 鎖定原則。
- 正常到期、Ctrl+C、`resume` 會還原。原本已停用時保持停用；若其他程式在此期間啟用它，工具不會再次強制關閉。
- 強制結束程序、關閉終端或當機不保證能立即執行清理。重新執行 `screensaver resume` 會依紀錄復原；復原失敗會保留紀錄並回報錯誤。
- 紀錄以登入識別與登入時間隔離；後續登入不會誤用先前登入的紀錄。損毀紀錄會回報錯誤，避免猜測應還原的值。
- 暫停期間避免同時用其他工具修改螢幕保護設定；Windows 的這個設定是共用狀態。

## 限制與錯誤

| 情況 | 處理 |
|---|---|
| `ERROR_OPERATION_IN_PROGRESS (329)` | Windows 處於省電／鎖定轉換狀態。實際喚醒螢幕並完成登入後重試；失敗不會回報暫停成功 |
| `PermissionDenied` | Windows 或原則拒絕操作。工具不提權、不改權限、不切換保護桌面 |
| 已鎖定 Windows | 停止保護程式不等於解除鎖定，仍需使用者登入 |
| 第三方保護程式以 `.exe` 執行或不在專用桌面 | `stop` 不會把它當成可關閉目標；需手動關閉 |
| 保護程式不接受關閉要求 | 等待 2 秒後回報逾時，不強制終止程序 |
| 控制 Windows 休眠／自動鎖定 | 本功能不更改這些原則；暫停只持有顯示器喚醒要求 |

`screensaver` 路徑會在建立滑鼠／鍵盤後端前執行，讓螢幕保護程式造成游標存取拒絕時仍能查詢或要求停止。需要 Windows 與 `os` feature。

## 本次驗證

2026-09-26 已實測：停止正在執行的保護程式、定時還原、跨程序 `resume`、重複暫停拒絕、Ctrl+C 還原、強制結束自建暫停程序後復原。測試前後均逐項讀回啟用狀態、逾時與登入要求，確認原設定已還原。

停止保護程式後，自建 Win32 App 的圖像辨識、真實點擊、繁體中文輸入、修飾鍵及 CLI 操作也已通過。結果摘要見 [交付驗證](validation-agent.md)。

## 參考

- [SystemParametersInfoW：螢幕保護設定與持久化旗標](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfow)
- [SetThreadExecutionState：保持顯示器喚醒；本身不阻止螢幕保護程式](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate)
- [Microsoft：停止 Screen-saver 桌面中的螢幕保護程式](https://learn.microsoft.com/en-us/archive/msdn-magazine/2001/december/c-q-a-stopping-screen-savers-detecting-screen-resolution-adding-status-bar-buttons)
