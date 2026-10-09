# 無螢幕／無人值守使用

rsautogui 讀取的是 Windows 實際繪製的桌面像素，送出的是真實輸入。因此「沒有人看螢幕」可以運作，但必須仍有一個**已登入、未鎖定、正在繪製**的互動桌面。顯示器本身是否接上、是否亮著，並不是關鍵；Windows 是否仍有活動顯示輸出才是。

## 先檢查：`session status`

```powershell
& $exe session status          # 人讀輸出
& $exe session status --json   # 機器可讀；有阻擋項時 exit code 為 1
```

MCP 對應唯讀工具 `session_status {}`。它回報工作階段編號、是否在主控台、是否為遠端桌面、連線狀態、鎖定、輸入桌面、游標存取、活動顯示器，以及主螢幕中央 64×64 的實際截圖探測，最後給出 `ready` 與各項建議。此檢查不解鎖、不重新連線、不改設定。

排程腳本可在動作前以它把關：

```powershell
& $exe session status --json | Out-File "$env:TEMP\autogui-session.json"
if ($LASTEXITCODE -ne 0) { throw 'desktop not ready; see autogui-session.json' }
```

`ready=true` 只代表桌面條件成立；仍須以目標 App 的實際截圖確認畫面內容。

## 情境對照

| 情境 | 可否運作 | 作法 |
|---|---|---|
| 螢幕關閉、省電熄螢、使用者離開 | 可以 | 桌面仍繪製。螢幕保護程式啟動時先 `screensaver stop`，作業期間 `screensaver pause` |
| 主機完全沒接螢幕 | 需要虛擬輸出 | 安裝選配 IDD 虛擬顯示器（[virtual-display.md](virtual-display.md)）或 HDMI/DP 假負載。只有虛擬顯示器時它就是主螢幕，所有預設 API 與角落 fail-safe 都以它為準 |
| 遠端桌面連線中、視窗前景 | 可以 | `session status` 會提示 `remote-session` |
| 遠端桌面視窗最小化 | 不可 | Windows 停止繪製，游標存取被拒（`remote-not-rendering`）。還原視窗，或在**用戶端電腦**設定 `HKCU\Software\Microsoft\Terminal Server Client` 的 DWORD `RemoteDesktop_SuppressWhenMinimized=2` |
| 遠端桌面已中斷 | 不可 | `session-not-active`。離開前改用下方 `rdp-to-console.ps1` |
| Windows 鎖定／登入畫面 | 不可 | `locked`。需要使用者登入；本工具不繞過鎖定 |
| Windows 服務（Session 0） | 不可 | `service-session`。改用工作排程器「只在使用者登入時執行」，在使用者工作階段啟動 |
| 重新開機後無人登入 | 不可 | 需另行設定自動登入（例如 Sysinternals Autologon）。這會讓開機後桌面無密碼可用，請自行評估風險；本專案不代為設定 |

## 離開遠端桌面但保持桌面繪製

在**遠端桌面工作階段內**開啟系統管理員 PowerShell：

```powershell
.\scripts\rdp-to-console.ps1 -WhatIf   # 只檢查，不變更
.\scripts\rdp-to-console.ps1           # 會再次確認
```

腳本以 `tscon <目前工作階段> /dest:console` 把工作階段移到主控台，RDP 用戶端隨即中斷，桌面在本機主控台上維持登入且**未鎖定**；能實體接觸該電腦的人都能操作它。沒有實體螢幕時請先確保虛擬顯示器已啟用，否則主控台可能沒有活動輸出。非 RDP 工作階段、非系統管理員或已在主控台時，腳本不做任何變更。

之後可由排程工作或 MCP 呼叫 `session status` 確認 `on_console=true`、`ready=true` 再開始自動化。下次以 RDP 連入時，工作階段會自動接回用戶端。

## 限制

- 所有顯示器（實體、虛擬）共用同一組游標、鍵盤與前景焦點；不提供與使用者隔離的背景操作。需要隔離時使用 VM 或另一個使用者工作階段。
- 截圖探測若為單一顏色（`capture-uniform`）只是提示，桌布本身可能就是純色。
- 本機驗證（2026-10-09，Windows 10 19045，RDP 工作階段）：`session status` 正確回報遠端桌面工作階段 1、主控台工作階段 3、未鎖定，且因游標存取被拒判定 `remote-not-rendering`（exit 1）；RDP 視窗恢復繪製後，MCP `session_status` 回報 `ready=true` 並附 `remote-session` 警告與成功的截圖探測。`rdp-to-console.ps1` 在非系統管理員下拒絕執行且未做變更。實際 `tscon` 轉移與無實體螢幕的整機驗收**尚未執行**。
