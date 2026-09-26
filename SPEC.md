# autogui-rs 規格書（給 Grok Build）

版本：1.0  
日期：2026-09-26  
對齊對象：PyAutoGUI 0.9.x + PyScreeze locate API + PyGetWindow Win32 視窗 API  
交付物：可編譯的 Rust library crate `autogui`，不是 Python binding（PyO3 列為 v2，本規格不實作）

本文件是唯一行為來源。與 PyAutoGUI 文件衝突時，以「本文件明確寫下的差異」為準；未寫差異則對齊 PyAutoGUI。

---

## 1. 目標與非目標

### 1.1 目標

用 Rust 提供與 PyAutoGUI 同等的桌面自動化：

1. 滑鼠：移動、點擊、拖曳、滾輪、按住/放開，含 duration + tween。
2. 鍵盤：打字、單鍵、組合鍵、按住/放開。
3. 安全：FAILSAFE 四角中斷、每次呼叫後 PAUSE。
4. 螢幕：主螢幕尺寸、滑鼠座標、截圖、讀像素、顏色比對。
5. 找圖：在螢幕或既有圖上找模板，支援 region / grayscale / confidence。
6. **Windows 視窗控制（v1 必做）**：列舉、標題搜尋、啟用、最大/最小/還原、移動、縮放、關閉。

### 1.2 非目標（v1 禁止實作）

- `alert` / `confirm` / `prompt` 訊息框
- OCR
- 鍵盤 hook / 讀目前按鍵狀態 / keylogging
- 多螢幕虛擬桌面座標（僅主螢幕）
- Android / iOS
- macOS / Linux 視窗控制（API 要存在，執行時回 `UnsupportedPlatform`）
- 與 PyAutoGUI 函式名稱 100% snake_case 以外的別名洪水
- 遊戲手把 / HID 虛擬裝置

### 1.3 成功標準

在 Windows 10/11 上，下列腳本語意可對應 PyAutoGUI：

```text
size() → 主螢幕寬高
move_to(100, 150, duration=0.5)
click()
write("Hello", interval=0.05)
hotkey(["ctrl", "c"])
box = locate_on_screen("button.png", confidence=0.9)
click_image("button.png")
wins = get_windows_with_title("Notepad")
wins[0].activate(); wins[0].resize_to(800, 600)
```

Linux/macOS：輸入 + 截圖 + 找圖必須能編過；視窗方法編過但呼叫失敗。

---

## 2. 平台矩陣

| 能力 | Windows | macOS | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| 滑鼠鍵盤 | 必做 | 必做（需 Accessibility） | 必做 | Best-effort；失敗回明確錯誤 |
| 截圖 | 必做 | 必做 | 必做 | 必做（權限不足則錯誤） |
| 找圖 | 必做 | 必做 | 必做 | 必做 |
| 視窗控制 | **必做** | 回 UnsupportedPlatform | 同左 | 同左 |
| Fail-safe / pause | 必做 | 必做 | 必做 | 必做 |

Windows 視窗後端必須使用 Win32：

- `EnumWindows` / `IsWindowVisible` / `GetWindowTextW`
- `GetForegroundWindow` / `SetForegroundWindow` / `ShowWindow`
- `GetWindowRect` / `SetWindowPos`
- `IsIconic` / `IsZoomed`
- `PostMessageW(WM_CLOSE)`
- 必要時 `AttachThreadInput` 處理前景鎖定

不得依賴 Python、PowerShell 或外部 exe 操作視窗。

---

## 3. Crate 與 features

```toml
[package]
name = "autogui"
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "PyAutoGUI-compatible desktop automation in Rust"
```

Features：

| feature | 預設 | 作用 |
|---|---|---|
| `locate` | 開 | `image` crate 精確模板比對 |
| `opencv` | 關 | `confidence` 模糊比對；沒開卻傳 confidence → `Error::FeatureDisabled` |
| `windows-extra` | Win 自動 | 僅文件用，不改變 API |

依賴建議（可等價替換，但公開 API 不變）：

- `enigo`
- `xcap`（或 `screenshots`）
- `image`
- `thiserror`
- `windows`（target_os = windows）
- 可選：`opencv`（feature `opencv`）

---

## 4. 座標、型別、錯誤

### 4.1 座標系

- 原點：主螢幕左上 `(0, 0)`
- X 向右增加，Y 向下增加
- 單位：實體像素（不要自動除 Retina scale；截圖與點擊必須同一座標系）
- 型別：`i32`

### 4.2 公開型別

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point { pub x: i32, pub y: i32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size { pub width: i32, pub height: i32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Box {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb { pub r: u8, pub g: u8, pub b: u8 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton { Left, Middle, Right }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection { Vertical, Horizontal }

pub fn center(b: Box) -> Point; // (left + width/2, top + height/2)
```

`Box` 必須能轉成 region `(left, top, width, height)`。

### 4.3 錯誤

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("fail-safe triggered at corner {0:?}")]
    FailSafe(Point),
    #[error("image not found")]
    ImageNotFound,
    #[error("unsupported platform")]
    UnsupportedPlatform,
    #[error("feature disabled: {0}")]
    FeatureDisabled(&'static str),
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
    #[error("permission denied: {0}")]
    PermissionDenied(&'static str),
    #[error("window not found")]
    WindowNotFound,
    #[error("input backend: {0}")]
    Input(String),
    #[error("screenshot: {0}")]
    Screenshot(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

找圖找不到：**回傳 `Err(Error::ImageNotFound)`**，不要回 `Ok(None)`（對齊 PyAutoGUI ≥ 0.9.41 預設）。  
另提供：

```rust
impl AutoGui {
    pub fn set_raise_image_not_found(&mut self, yes: bool);
}
```

`false` 時 locate 類改回 `Ok(None)` / 空 Vec。

### 4.4 Settings

不要用行程級可變全域當唯一入口。主 API 是 `AutoGui` 實例。

```rust
pub struct Settings {
    pub pause: Duration,          // 預設 100ms
    pub failsafe: bool,           // 預設 true
    pub failsafe_points: Vec<Point>, // 預設主螢幕四角
    pub minimum_duration: Duration,  // 預設 0；duration 小於此視為瞬間移動
    pub darwin_catch_up: Duration,   // 預設 10ms；僅 macOS 在事件後額外睡
}

impl Default for Settings { /* 如上 */ }

pub struct AutoGui { /* 私有 */ }

impl AutoGui {
    pub fn new() -> Result<Self>;
    pub fn with_settings(settings: Settings) -> Result<Self>;
    pub fn settings(&self) -> &Settings;
    pub fn settings_mut(&mut self) -> &mut Settings;
}
```

每次 **會送出輸入、截圖、改視窗、找圖掃描** 的公開方法結尾：

1. 若 `failsafe`：讀滑鼠位置，落在任一 failsafe point（含 0px 容差，點即觸發）→ `Err(FailSafe)`。
2. `thread::sleep(pause)`。
3. 若 macOS：再睡 `darwin_catch_up`。

`position()` / `size()` / `on_screen()` **不**睡 pause（對齊「讀狀態」），但仍建議做 failsafe 可配置；v1 規定：**只有寫入類與 locate/screenshot 睡 pause**。

---

## 5. 螢幕與滑鼠

### 5.1 查詢

```rust
impl AutoGui {
    pub fn size(&self) -> Result<Size>;           // 主螢幕
    pub fn position(&self) -> Result<Point>;      // 滑鼠
    pub fn on_screen(&self, x: i32, y: i32) -> Result<bool>;
}
```

### 5.2 Tween

內建與 PyTweening 對齊的 easing，參數 `t ∈ [0, 1]` → `[0, 1]`：

- `linear`
- `ease_in_quad` / `ease_out_quad` / `ease_in_out_quad`
- `ease_in_cubic` / `ease_out_cubic` / `ease_in_out_cubic`
- `ease_in_quart` / `ease_out_quart` / `ease_in_out_quart`
- `ease_in_quint` / `ease_out_quint` / `ease_in_out_quint`
- `ease_in_sine` / `ease_out_sine` / `ease_in_out_sine`
- `ease_in_expo` / `ease_out_expo` / `ease_in_out_expo`
- `ease_in_circ` / `ease_out_circ` / `ease_in_out_circ`
- `ease_in_elastic` / `ease_out_elastic` / `ease_in_out_elastic`
- `ease_in_back` / `ease_out_back` / `ease_in_out_back`
- `ease_in_bounce` / `ease_out_bounce` / `ease_in_out_bounce`

```rust
pub type Tween = fn(f64) -> f64;
```

`move_to` 預設 `linear`。duration=0 或低於 `minimum_duration`：一次跳到終點。  
中間幀：至少每 16ms 送一個絕對座標。macOS drag 禁止 duration=0（強制最少 0.1s）。

### 5.3 移動與拖曳

```rust
impl AutoGui {
    pub fn move_to(&mut self, x: i32, y: i32, duration: Duration, tween: Tween) -> Result<()>;
    pub fn move_rel(&mut self, dx: i32, dy: i32, duration: Duration, tween: Tween) -> Result<()>;
    pub fn drag_to(&mut self, x: i32, y: i32, duration: Duration, tween: Tween, button: MouseButton) -> Result<()>;
    pub fn drag_rel(&mut self, dx: i32, dy: i32, duration: Duration, tween: Tween, button: MouseButton) -> Result<()>;
}
```

便利函式（duration=0、linear、左鍵）必須提供，名稱：

- `move_to_instant(x, y)`
- `click_at(x, y)` 等可放在 `prelude` 或 impl 預設參數風格的 overload。

Rust 沒有預設參數：用額外方法或 builder。規格要求最少這組便利方法：

```rust
pub fn move_to_xy(&mut self, x: i32, y: i32) -> Result<()>; // duration 0
pub fn click(&mut self) -> Result<()>;
pub fn click_xy(&mut self, x: i32, y: i32) -> Result<()>;
pub fn click_button(&mut self, button: MouseButton) -> Result<()>;
pub fn double_click(&mut self) -> Result<()>;
pub fn triple_click(&mut self) -> Result<()>;
pub fn right_click(&mut self) -> Result<()>;
pub fn middle_click(&mut self) -> Result<()>;
```

### 5.4 點擊細節

```rust
pub fn click_ex(
    &mut self,
    x: Option<i32>,
    y: Option<i32>,
    clicks: u32,
    interval: Duration,
    button: MouseButton,
) -> Result<()>;
```

規則：

- `x`/`y` 皆 Some：先 `move_to` 再點。
- 皆 None：在現位置點。
- 只給一個：`InvalidArgument`。
- `clicks` 次，間隔 `interval`。
- 每次 click = down + up。

```rust
pub fn mouse_down(&mut self, button: MouseButton) -> Result<()>;
pub fn mouse_up(&mut self, button: MouseButton) -> Result<()>;
pub fn scroll(&mut self, clicks: i32) -> Result<()>;          // 正=上
pub fn hscroll(&mut self, clicks: i32) -> Result<()>;
pub fn scroll_at(&mut self, clicks: i32, x: i32, y: i32) -> Result<()>;
```

---

## 6. 鍵盤

鍵名對齊 `pyautogui.KEYBOARD_KEYS`（小寫字串）。v1 必支援：

```
a–z, 0–9
enter, return, tab, space, backspace, delete, esc, escape
up, down, left, right
home, end, pageup, pagedown
shift, shiftright, ctrl, ctrlright, alt, altright, cmd / win / meta
capslock, numlock, scrolllock
f1–f24
printscreen, insert, pause, sleep
volumedown, volumeup, volumemute
```

`cmd`、`win`、`meta`、`command` 視為同一修飾鍵（依 OS 對應 Super/Win/Command）。

```rust
impl AutoGui {
    pub fn write(&mut self, text: &str, interval: Duration) -> Result<()>;
    pub fn press(&mut self, key: &str) -> Result<()>;
    pub fn press_times(&mut self, key: &str, presses: u32, interval: Duration) -> Result<()>;
    pub fn key_down(&mut self, key: &str) -> Result<()>;
    pub fn key_up(&mut self, key: &str) -> Result<()>;
    pub fn hotkey(&mut self, keys: &[&str]) -> Result<()>;
    pub fn hold(&mut self, keys: &[&str]) -> Result<KeyHold<'_>>;
}

pub struct KeyHold<'a> { /* Drop 時 key_up 全部 */ }
```

規則：

- `write`：可印字元走文字輸入（處理 Unicode，不要拆成美式鍵盤掃描碼亂打）。`\n` = Enter，`\t` = Tab。
- `hotkey(['ctrl','c'])`：依序 down，反序 up。
- 未知鍵名：`InvalidArgument`。
- `hold` 用 RAII，panic 也要盡力釋放（Drop）。

---

## 7. 截圖與找圖

### 7.1 截圖

```rust
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
    // RGB8 像素，row-major
}

impl AutoGui {
    pub fn screenshot(&mut self) -> Result<Screenshot>;
    pub fn screenshot_region(&mut self, region: Box) -> Result<Screenshot>;
    pub fn screenshot_to_file<P: AsRef<Path>>(&mut self, path: P) -> Result<Screenshot>;
    pub fn pixel(&mut self, x: i32, y: i32) -> Result<Rgb>;
    pub fn pixel_matches_color(&mut self, x: i32, y: i32, color: Rgb, tolerance: u8) -> Result<bool>;
}
```

`pixel_matches_color`：每個通道 `|a-b| <= tolerance`。

`Screenshot` 必須能：

- `save(path)`
- `get_pixel(x, y) -> Rgb`
- 轉成找圖 haystack

### 7.2 找圖參數

```rust
pub struct LocateOptions {
    pub grayscale: bool,          // 預設 false
    pub confidence: Option<f32>,  // 0.0–1.0；None = 精確匹配
    pub region: Option<Box>,
    pub min_search_time: Duration, // 預設 0；>0 時重試直到超時或找到
}

impl Default for LocateOptions { /* ... */ }
```

```rust
impl AutoGui {
    pub fn locate(&self, needle: &Screenshot, haystack: &Screenshot, opt: &LocateOptions) -> Result<Box>;
    pub fn locate_all(&self, needle: &Screenshot, haystack: &Screenshot, opt: &LocateOptions) -> Result<Vec<Box>>;
    pub fn locate_on_screen<P: AsRef<Path>>(&mut self, image: P, opt: &LocateOptions) -> Result<Box>;
    pub fn locate_all_on_screen<P: AsRef<Path>>(&mut self, image: P, opt: &LocateOptions) -> Result<Vec<Box>>;
    pub fn locate_center_on_screen<P: AsRef<Path>>(&mut self, image: P, opt: &LocateOptions) -> Result<Point>;
    pub fn click_image<P: AsRef<Path>>(&mut self, image: P, opt: &LocateOptions) -> Result<Point>;
}
```

也要接受已載入的 `Screenshot` 當 needle，避免只能吃檔案。

行為：

- 精確模式（`confidence is None`）：逐像素 RGB 全等（grayscale 時比亮度）。
- `confidence is Some`：需 `opencv` feature，使用標準化互相關或 template match；分數 < confidence → 當沒找到。
- `locate_all` 必須抑制重疊（預設：重疊 IoU > 0.3 的重複框丟掉，留分數高者；精確模式留先掃到的）。
- `click_image`：`locate_center_on_screen` + `click_xy`。
- region 超出螢幕：裁切到交集；交集為空 → `InvalidArgument`。
- needle 大於 haystack → `InvalidArgument`。

效能目標（非硬門檻，寫進 README）：

- 1920×1080 截圖 < 200ms
- 精確 locate 小模板（≤80×40）於 1920×1080 < 2s

---

## 8. Windows 視窗控制

### 8.1 查詢函式

```rust
impl AutoGui {
    pub fn get_all_windows(&self) -> Result<Vec<Window>>;
    pub fn get_all_titles(&self) -> Result<Vec<String>>;
    pub fn get_active_window(&self) -> Result<Option<Window>>;
    pub fn get_windows_with_title(&self, title: &str) -> Result<Vec<Window>>;
    pub fn get_windows_at(&self, x: i32, y: i32) -> Result<Vec<Window>>;
}
```

規則：

- 只回 **可見** 視窗（`IsWindowVisible` 且面積 > 0，排除 tool window 可選，v1：可見且有標題或有面積都列入；`get_all_titles` 可含空字串，與 PyGetWindow 相同）。
- `get_windows_with_title`：**子字串、大小寫敏感**（對齊 PyGetWindow `title in titlebar`）。另提供：

```rust
pub fn get_windows_with_title_ci(&self, title: &str) -> Result<Vec<Window>>; // 大小寫不敏感，額外 API
```

- `get_windows_at`：視窗矩形含該點（含邊）。
- 非 Windows：全部回 `Err(UnsupportedPlatform)`。
- `Window` 可 clone；內部持有 `HWND`（`isize`）。視窗已銷毀後的方法回 `WindowNotFound`。

### 8.2 Window

```rust
pub struct Window { /* HWND + 快取標題可失效 */ }

impl Window {
    pub fn title(&self) -> Result<String>;
    pub fn hwnd(&self) -> isize;

    pub fn left(&self) -> Result<i32>;
    pub fn top(&self) -> Result<i32>;
    pub fn right(&self) -> Result<i32>;
    pub fn bottom(&self) -> Result<i32>;
    pub fn width(&self) -> Result<i32>;
    pub fn height(&self) -> Result<i32>;
    pub fn size(&self) -> Result<Size>;
    pub fn box_rect(&self) -> Result<Box>;
    pub fn topleft(&self) -> Result<Point>;
    pub fn center(&self) -> Result<Point>;
    pub fn area(&self) -> Result<i64>;

    pub fn is_minimized(&self) -> Result<bool>;
    pub fn is_maximized(&self) -> Result<bool>;
    pub fn is_active(&self) -> Result<bool>;

    pub fn activate(&self) -> Result<()>;
    pub fn maximize(&self) -> Result<()>;
    pub fn minimize(&self) -> Result<()>;
    pub fn restore(&self) -> Result<()>;
    pub fn close(&self) -> Result<()>; // WM_CLOSE，可能被對話框擋住
    pub fn hide(&self) -> Result<()>;
    pub fn show(&self) -> Result<()>;

    pub fn move_to(&self, left: i32, top: i32) -> Result<()>;
    pub fn move_rel(&self, dx: i32, dy: i32) -> Result<()>;
    pub fn resize_to(&self, width: i32, height: i32) -> Result<()>;
    pub fn resize_rel(&self, dw: i32, dh: i32) -> Result<()>;
}
```

幾何一律用 `GetWindowRect` 的螢幕座標（含邊框）。  
`activate` 失敗（被前景鎖定）必須回 `PermissionDenied("SetForegroundWindow")`，禁止靜默失敗。  
`resize_to` / `move_to` 對最大化視窗：先 `restore` 再改位置，或文件註明「最大化時 SetWindowPos 可能被忽略」——規格選定：**先 restore 再套用**，行為可預測。

視窗方法也要跑 pause（改狀態算寫入）。fail-safe 在呼叫當下檢查滑鼠。

---

## 9. 目錄結構

```text
autogui-rs/
  AGENTS.md
  SPEC.md
  README.md
  CHANGELOG.md
  Cargo.toml
  src/
    lib.rs              // 重匯出公開 API
    error.rs
    types.rs
    settings.rs
    tween.rs
    failsafe.rs
    gui.rs              // AutoGui
    mouse.rs
    keyboard.rs
    keys.rs             // 鍵名表
    screen.rs           // size/position/screenshot/pixel
    locate.rs
    window.rs           // 跨平台 stub
    window_win.rs       // cfg(windows)
  examples/
    cheat_sheet.rs
    spiral_drag.rs
  tests/
    tween_failsafe.rs
    locate_synthetic.rs // 用生成圖，不碰真實螢幕
    keys.rs
```

`lib.rs` 保持薄。禁止把 Win32 呼叫塞進 `gui.rs`。

---

## 10. 測試規格

### 10.1 必須有的單元測試（不碰真實 GUI）

| 測試 | 斷言 |
|---|---|
| tween 邊界 | f(0)=0, f(1)=1（elastic/back/bounce 允許短暫越界，但 0/1 端點固定） |
| center(Box) | 偶數寬高無條件捨去與 Py 一致（整數除法） |
| failsafe 幾何 | 四角點命中、離角 1px 不命中 |
| pixel tolerance | (10,10,10) vs (12,8,10) tolerance 2 → true；1 → false |
| locate 合成圖 | 紅底 200×200 中放 10×10 藍塊，locate 回正確 Box |
| locate 找不到 | ImageNotFound |
| locate_all 兩個不相鄰塊 | 長度 2 |
| 鍵名表 | enter/esc/cmd/win 可解析 |
| 熱鍵順序 | 用 mock backend 記錄 down/up 順序為 ctrl down, c down, c up, ctrl up |

### 10.2 Mock 後端

`AutoGui` 內部 trait：

```rust
pub(crate) trait Backend {
    fn mouse_pos(&self) -> Result<Point>;
    fn move_mouse(&mut self, p: Point) -> Result<()>;
    fn button(&mut self, b: MouseButton, down: bool) -> Result<()>;
    fn scroll(&mut self, dy: i32, dx: i32) -> Result<()>;
    fn key(&mut self, key: &str, down: bool) -> Result<()>;
    fn text(&mut self, s: &str) -> Result<()>;
    fn screen_size(&self) -> Result<Size>;
    fn capture(&self, region: Option<Box>) -> Result<Screenshot>;
}
```

單元測試注入 `FakeBackend`。正式 `new()` 用 OS backend。

### 10.3 Live 測試（`#[ignore]`）

- 移動滑鼠到 (50,50) 再讀 position，容差 2px（DPI/加速可能造成偏差；若 OS 加速干擾，測試文件註明並用相對移動）。
- Windows only：開 `notepad.exe`，`get_windows_with_title("Untitled")` 或當地語系標題，`resize_to(640,480)`，讀 size 容差 20px（邊框）。測完 `close()`。

環境變數：`AUTOGUI_LIVE=1`。

---

## 11. 安全與權限

- README 必須用粗體寫：預設 fail-safe 開啟；測試時把滑鼠甩到角落可中止。
- 範例禁止 `failsafe = false`，除非範例檔名含 `unsafe`。
- macOS README：要開輔助使用權限。
- Linux README：X11 通常可動；Wayland 可能要 uinput 群組或失敗。
- Windows README：部分全螢幕遊戲/系統視窗 `SetForegroundWindow` 會失敗。
- 本庫可用於 RPA / 測試；文件不得教導規避防作弊或未授權操作他人電腦。

---

## 12. README 最低內容

1. 安裝：`autogui = "0.1"`
2. 20 行 cheat sheet（對應 SPEC 成功標準）
3. Features 表
4. 平台限制（視窗僅 Windows）
5. Fail-safe
6. Live test 指令
7. 與 PyAutoGUI 差異表（見 §13）

---

## 13. 與 PyAutoGUI 的刻意差異

| PyAutoGUI | autogui-rs |
|---|---|
| 模組級全域 `PAUSE`/`FAILSAFE` | `AutoGui.settings` |
| 預設參數很多 | 便利方法 + `*_ex` / `LocateOptions` |
| locate 可回 None（舊版） | 預設 `ImageNotFound` |
| PIL.Image | `Screenshot` |
| 視窗屬性可直接賦值 `win.width = 100` | 只用 `resize_to` / `move_to` |
| `typewrite` | `write`（保留 `typewrite` 為 `write` 的 deprecated alias 函式） |
| 訊息框 | 不做 |
| 僅主螢幕 | 相同 |

允許 `pub fn typewrite(...)` 轉呼叫 `write`。

---

## 14. 里程碑（Grok Build 必須按序）

**M0 — 骨架（0.5h）**  
workspace、空模組、`cargo test` 綠、FakeBackend trait。

**M1 — 純邏輯**  
types / tween / failsafe / Settings。測試齊。

**M2 — 輸入**  
mouse + keyboard + keys，FakeBackend 測試熱鍵順序與 click_ex。接 enigo。

**M3 — 螢幕**  
size/position/screenshot/pixel。合成測試 + 真實截圖 smoke（ignore）。

**M4 — 找圖**  
精確匹配必做。`opencv` feature 可先 stub 成 FeatureDisabled，但 API 要在。合成圖測試必過。

**M5 — Windows 視窗**  
完整 Win32。非 Windows stub。Notepad live 測試寫好但 ignore。

**M6 — 文件**  
README、examples/cheat_sheet.rs、CHANGELOG、clipp/fmt 綠。

每個里程碑結束時提交訊息格式：

```
feat(mN): <one line>
```

---

## 15. 公開 prelude

```rust
pub use crate::{
    AutoGui, Settings, Error, Result,
    Point, Size, Box, Rgb, MouseButton,
    LocateOptions, Window, Tween,
    center,
};
```

`Box` 與 `std::boxed::Box` 衝突：公開型別名 **`Rect`** 當主名稱，`Box` 作為 `pub type Box = Rect` 保留，README 主推 `Rect`。

修正：程式裡用 `Rect`，Py 對齊別名 `pub type Box = Rect`。

---

## 16. Grok Build 驗收清單（agent 自勾）

- [ ] `cargo test` 非 ignored 全過
- [ ] `cargo clippy --all-targets -- -D warnings` 過
- [ ] 無 `unwrap`/`expect` 在 `src/` library 路徑（測試可）
- [ ] 視窗 API 在 Linux 編譯成功
- [ ] `locate_on_screen` / `get_windows_with_title` 出現在 README
- [ ] examples 可 `cargo run --example cheat_sheet`（failsafe 開；範例只印 size/position，不亂移滑鼠，或移完移回）
- [ ] `cheat_sheet` 範例預設 **不要**自動拖曳；spiral_drag 單獨 example 並在檔首註解警告

---

## 17. 給 Grok Build 的實作備註

- 先寫 FakeBackend 與測試，再接 enigo，避免沒有實體顯示器時無法開發。
- HWND 用 `windows::Win32::Foundation::HWND`，對外 `isize`。
- 列舉視窗時跳過不可見與 `WS_EX_TOOLWINDOW` 可作為過濾；v1 預設：**可見即可**，與 PyGetWindow 接近即可，不追求像素級清單一致。
- DPI Awareness：Windows 宣告 `PerMonitorV2`（在 README 說明呼叫端 exe 的 DPI 意識會影響座標）。Library 不強行改 process DPI，但 example 可呼叫 `SetProcessDpiAwarenessContext`。
- 找圖精確模式可用簡單雙迴圈；影像大時可先做 2× 降採樣粗搜再精修，但結果必須與全像素搜尋一致。
- 禁止在 library 裡 `println!`。
- 禁止新增 network dependency。
