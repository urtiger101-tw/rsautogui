# 效能量測（0.2.0）

本文記錄 0.2.0 的辨識、截圖、MCP 啟動與 DLL 大小量測結果，並說明如何在自己的機器重現。數字只代表這些合成基準與開發機環境，不是任意畫面的延遲保證；比較前請在同一台機器重跑。

## 量測方式

- **辨識**：1920×1080 合成 RGB 或灰階影像，80×40 模板，數字為中位數（median，毫秒）。不含截圖 I/O 與 `Settings::pause`。
- **截圖**：唯讀擷取主螢幕或指定區域，不送出任何滑鼠或鍵盤輸入。
- **啟動**：`autogui-control.exe --help` 的程序啟動到結束；MCP 為 spawn、`initialize`、`tools/list` 與 EOF 的完整往返。
- 執行緒：全畫面信心比對最多使用 8 個工作執行緒，並在工作項目之間檢查 fail-safe。

## 辨識（1920×1080，80×40 模板，中位數 ms）

| 搜尋 | 優化前 | 0.2.0 |
|---|---:|---:|
| 精確比對，RGB 雜訊／UI 圖樣 | 3.00 / 6.37 | 0.88 / 0.86 |
| 精確比對，灰階 | 3.88 / 10.28 | 1.51 / 1.60 |
| 信心 0.999，RGB | 248.5 | 25.1 |
| 信心 0.9，灰階 | 118.8 | 16.1 |
| 信心 0.95，RGB UI | 246.8 | 25.4 |
| 區域 280×160，精確比對 | 4.04 | 4.24（在雜訊範圍內） |

全畫面信心比對的尖峰記憶體由約 113 MB 降到約 2.5 MB（每個工作執行緒）。辨識核心使用空間分桶的重疊抑制，並只擷取要求的區域。

## 截圖、MCP 與啟動

| 項目 | 結果 |
|---|---:|
| 截圖（主螢幕／400×300 區域／顯示器） | 32.7 ms／16.5 ms／32.9 ms，與優化前相近（受 xcap／GDI 限制） |
| MCP `capture` PNG，1080p | 80–102 ms → 27–31 ms；1.22 MB → 0.69 MB |
| `autogui-control --help` 冷啟動 | 約 4.9 ms |
| MCP `initialize` + `tools/list`（含程序啟動） | 約 15 ms |

截圖速度低於專案目標（1080p 截圖 < 200 ms）。

## 二進位大小

| 檔案 | 大小 |
|---|---:|
| `autogui_runtime.dll` | 5370 KiB → 5560 KiB |

PNG 編解碼 crate 改為 opt-level 3，約增加 150 KiB，換得截圖轉 PNG 約快 15%。其餘 crate 維持大小最佳化設定。

## 重現

在儲存庫根目錄執行（需要 Windows、已解除鎖定的桌面；截圖項目會讀取目前主螢幕）：

```powershell
# 建置 release 並輸出人類可讀的表格
pwsh scripts/bench.ps1

# 機器可讀 JSON（腳本或 CI 使用）
pwsh scripts/bench.ps1 -Json

# 已建置時略過建置，並增加啟動與 MCP 次數
pwsh scripts/bench.ps1 -NoBuild -Runs 50

# 只跑記憶體內的辨識案例（不截圖），每案例 5 次
cargo run --release --features opencv --example recognition_bench -- --full-hd --iterations 5
```

`scripts/bench.ps1` 不送出任何輸入。`recognition_bench` 支援 `--iterations`、`--json` 與 `--capture`（加入唯讀截圖）。

## 目標與限制

- 專案目標：1080p 截圖 < 200 ms；≤ 80×40 模板的精確比對 < 2 s。以上數字都在目標之內。
- 合成影像不代表真實 App 介面；UI 的抗鋸齒、動畫與壓縮會改變比對成本。
- 信心比對的成本隨模板與搜尋區域大小增加；大區域請用 `LocateOptions::region` 縮小範圍。
- 截圖時間受 xcap 與 Windows GDI 限制，本專案不以此處的數字保證特定顯示卡或遠端桌面的表現。
