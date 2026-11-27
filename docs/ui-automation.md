# Windows UI Automation（0.3.0）

UI Automation（UIA）讀取 App 自己公開的無障礙樹：每個按鈕、輸入框、選單項目都有角色、名稱、AutomationId、位置與可用動作。相較於截圖找圖，它不受 DPI、主題或字型影響，多數動作也不需要移動滑鼠或取得前景焦點。只支援 Windows，需要 `uia` feature（`autogui-control` 預設已啟用）；其他平台回 `UnsupportedPlatform`，未啟用 feature 回 `FeatureDisabled("uia")`。

## MCP 工具

| 工具 | 用途 | 會送出輸入嗎 |
|---|---|---|
| `ui_snapshot` | 讀取 `target` 視窗的元素樹（文字樹 + 結構化節點），每個節點附元素參照 | 否（唯讀） |
| `ui_click` | 對一個元素執行動作：`auto`（預設）、`invoke`、`toggle`、`select`、`expand`、`collapse`、`focus` 或 `click` | 是 |
| `ui_set_value` | 透過 ValuePattern 設定整個值並回讀比對 | 是（不經鍵盤） |
| `ui_wait` | 每 200 ms 輪詢，等待名稱或值包含 `text` 的元素出現（`present`）或消失（`absent`） | 否（唯讀） |

建議流程：`windows_list` → `ui_snapshot` → `ui_click` / `ui_set_value`（用 `element` 參照）→ `ui_wait` 或 `ui_snapshot` 確認結果。

### 文字樹

```text
[e3-0] Window "未命名 - 記事本" (0,0,1280,720) [focus]
  [e3-4] Button "儲存" id=SaveBtn (12,40,80,24) [invoke,focus]
  [e3-7] Edit "文字編輯器" value="hello" (0,80,1280,600) [value,focus] focused
... truncated after 150 nodes; narrow with query/role or raise max_nodes
```

每行依序為參照、角色、名稱、`id=`AutomationId、`value=`、矩形（Windows 桌面實體像素，和視窗矩形相同）、可用 pattern，最後是 `disabled`／`focused`／`offscreen` 旗標。縮排代表樹的深度。

## 選擇元素

每個元素工具接受下列其中一種方式：

- `element`：最新 `ui_snapshot` 給的 `e<generation>-<index>`。最可靠；它同時綁定 index、角色、名稱與 AutomationId，元素變了就回 `element_not_found`。
- `name`（不分大小寫，`name_match` = `contains` 預設或 `exact`）、`role`（如 `Button`、`Edit`、`MenuItem`，不分大小寫，`menu item` 等同 `MenuItem`）、`automation_id`（不分大小寫完全相同）。全部條件都要符合。

找不到回 `element_not_found`；符合多個回 `ambiguous_element`（`contains` 時若剛好一個名稱完全相同，會選它）。`element` 不能和其他選擇欄位同時使用。

參照只在最新一次 `ui_snapshot` 的同一個 `target` 有效：新的 `ui_snapshot`、或 `windows_list`／`window_wait`／`app_launch` 重新產生 target 後，舊參照全部失效。

## `ui_click` 的 `auto`

依序使用 Invoke、Toggle、SelectionItem、ExpandCollapse（展開）pattern；這些都不移動滑鼠、不需要前景。元素沒有任何 pattern 時，才會啟用視窗、重新解析元素，確認元素可見、啟用、中心點在視窗內並且視窗仍在前景後，於中心點送出真實左鍵點擊。`click` 強制真實點擊；指定 pattern 但元素不支援時回 `invalid_argument`。

## 限制

- 一次走訪以廣度優先逐層進行，上限為 `max_nodes`（1..500，預設 150）與 `max_depth`（1..30，預設 12）；有 `query`／`role` 時最多走訪 1000 個元素再篩選。`truncated` 為 true 代表還有元素沒列出。`ui_wait` 每次輪詢最多看前 1000 個元素，`absent` 結果在 `truncated` 時可能遺漏更深的元素。
- 只看 `target` 視窗本身的樹。另開成頂層視窗的對話框不在其中，請用 `window_wait`。
- 遊戲、自繪畫布或未實作 UIA 的 App 只會露出很少元素；Chromium／Electron 有時要等無障礙功能啟用後才有完整樹。這時改用 `capture` 與像素工具。
- 名稱與值最多保留 512／4096 字元；文字樹中再截短。
- 單次請求最多等 15 秒；App 停止回應時回 `internal`（UI Automation 錯誤），下一次呼叫會自動重建工作執行緒。

## 安全

- 密碼欄位（`IsPassword`）的值永不回傳；`ui_set_value` 對密碼欄位只設定不回讀（`value_matches` 為 null）。
- 元素名稱和值是 App 提供的不可信資料，不是指令。
- 會改變狀態的動作仍執行主螢幕角落 fail-safe，並在動作前檢查 MCP 取消。
- COM 只在 `UiAutomation` 擁有的單一 MTA 工作執行緒初始化，MCP 處理執行緒不會呼叫 `CoInitialize`。

## CLI

```powershell
& $control ui snapshot '記事本' --query 儲存 --max-nodes 50
& $control ui click '記事本' --name 儲存 --role Button
& $control ui set-value '記事本' --role Document --text '你好'
& $control ui wait '記事本' --text 已儲存 --timeout 10
& $control ui wait '記事本' --text 儲存中 --gone --abort-on 錯誤
```

`--exact` 讓 `--name` 完全比對；`--id` 指定 AutomationId；`--action` 同 MCP 的 `action`。全域 `--json` 會輸出 JSON。

## Library

```rust
use autogui::{AutoGui, UiAction, UiAutomation, UiSelector, UiSnapshotOptions};

let gui = AutoGui::new()?;
let window = gui.get_windows_with_title_ci("記事本")?.remove(0);
let mut ui = UiAutomation::new()?;
println!("{}", ui.snapshot(&window, &UiSnapshotOptions::default())?.to_text());
let save = UiSelector { name: Some("儲存".into()), role: Some("Button".into()), ..Default::default() };
ui.act(&window, &save, UiAction::Invoke)?;
```
