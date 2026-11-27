//! Single source of CLI help text (English and Traditional Chinese).
//! Lives in the launcher so help never needs the runtime DLL.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

/// A text in both supported languages.
#[derive(Debug, Clone, Copy)]
pub struct Text {
    pub en: &'static str,
    pub zh: &'static str,
}

impl Text {
    pub fn get(self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::Zh => self.zh,
        }
    }
}

pub struct CommandSpec {
    pub name: &'static str,
    pub usage: &'static str,
    pub summary: Text,
    pub example: &'static str,
}

const fn t(en: &'static str, zh: &'static str) -> Text {
    Text { en, zh }
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "doctor",
        usage: "doctor [--json]",
        summary: t(
            "Check install, runtime, session and agent-host registration (read-only)",
            "檢查安裝、執行階段、工作階段與 agent 主機註冊（唯讀）",
        ),
        example: "doctor",
    },
    CommandSpec {
        name: "mcp",
        usage: "mcp | mcp tools [--format mcp|anthropic|openai|openai-chat] | mcp config --host <host>",
        summary: t(
            "Run the MCP stdio server, export tool schemas, or print host config",
            "啟動 MCP stdio 伺服器、匯出工具定義，或輸出主機設定片段",
        ),
        example: "mcp config --host claude-code",
    },
    CommandSpec {
        name: "session",
        usage: "session status [--json]",
        summary: t(
            "Report whether unattended automation can work in this session",
            "回報此工作階段是否可進行無人值守自動化",
        ),
        example: "session status",
    },
    CommandSpec {
        name: "list",
        usage: "list [title] [--process name] [--foreground]",
        summary: t("List visible windows", "列出可見視窗"),
        example: "list notepad --foreground",
    },
    CommandSpec {
        name: "wait",
        usage: "wait <title> [present|absent|foreground] [timeout] [--process name]",
        summary: t(
            "Wait for a window state (default present, 10 s)",
            "等待視窗狀態（預設 present，10 秒）",
        ),
        example: "wait Untitled present 10",
    },
    CommandSpec {
        name: "capture",
        usage: "capture <title> <output.png> [--display <device>]",
        summary: t("Save a screenshot of one window", "擷取單一視窗畫面並存檔"),
        example: "capture Notepad shot.png",
    },
    CommandSpec {
        name: "locate",
        usage: "locate <title> <template.png> [confidence|exact] [timeout] [scale] [--display <device>]",
        summary: t(
            "Find a template image inside a window (no input sent)",
            "在視窗內尋找範本圖片（不送出輸入）",
        ),
        example: "locate Notepad ok.png 0.95 5",
    },
    CommandSpec {
        name: "click",
        usage: "click <title> <template.png> [confidence|exact] [timeout] [scale] [--display <device>]",
        summary: t(
            "Find a template image and click it (sends real input)",
            "尋找範本圖片並點擊（送出真實輸入）",
        ),
        example: "click Notepad ok.png 0.95 5",
    },
    CommandSpec {
        name: "type",
        usage: "type <title> <text>",
        summary: t(
            "Type text into a window (sends real input)",
            "對視窗輸入文字（送出真實輸入）",
        ),
        example: "type Notepad \"Hello\"",
    },
    CommandSpec {
        name: "paste",
        usage: "paste <title> <text>",
        summary: t(
            "Paste text through the clipboard, then restore it (sends real input)",
            "透過剪貼簿貼上文字並還原剪貼簿（送出真實輸入）",
        ),
        example: "paste Notepad \"多語言 text ✓\"",
    },
    CommandSpec {
        name: "ui",
        usage: "ui <snapshot|click|set-value|wait> <title> [--name N] [--role R] [--id A] [--text T] [--query Q] [--gone] [--timeout S]",
        summary: t(
            "Read or act on a window's UI Automation tree (click and set-value send real input)",
            "讀取或操作視窗的 UI Automation 樹（click、set-value 會送出真實輸入）",
        ),
        example: "ui click Notepad --name Save --role Button",
    },
    CommandSpec {
        name: "settings",
        usage: "settings [overlay on|off] [action-log on|off]",
        summary: t(
            "Show or change the \"Computer use! Don't move!\" notice and the local action log (both on by default)",
            "顯示或變更「Computer use! Don't move!」提示視窗與本機動作紀錄（預設皆開啟）",
        ),
        example: "settings action-log off",
    },
    CommandSpec {
        name: "window",
        usage: "window <inspect|activate|minimize|maximize|restore|show|hide|move|resize|close> <title> [x y|width height]",
        summary: t(
            "Inspect or control one window by unique title",
            "依唯一標題檢視或控制視窗",
        ),
        example: "window move Notepad 100 100",
    },
    CommandSpec {
        name: "display",
        usage: "display list | display capture <device> <output.png> | display move <device> <title>",
        summary: t(
            "List displays, capture one, or move a window onto one",
            "列出顯示器、擷取畫面，或把視窗移到指定顯示器",
        ),
        example: "display list",
    },
    CommandSpec {
        name: "screensaver",
        usage: "screensaver status | pause [seconds] | resume | stop",
        summary: t(
            "Inspect or temporarily pause the screen saver",
            "檢視或暫時停用螢幕保護程式",
        ),
        example: "screensaver pause 900",
    },
    CommandSpec {
        name: "help",
        usage: "help [command]",
        summary: t("Show help for a command", "顯示命令說明"),
        example: "help click",
    },
];

/// Accepted by the launcher but not listed in help.
pub const HIDDEN: &[&str] = &["__runtime-version"];

pub fn find(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.name == name)
}

pub fn is_known(name: &str) -> bool {
    find(name).is_some() || HIDDEN.contains(&name)
}

const WIDTH: usize = 78;

fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

/// Terminal columns used by a string (CJK characters count as two).
pub fn display_width(text: &str) -> usize {
    text.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

/// Wraps text to at most width columns; breaks at spaces, or anywhere when a
/// word does not fit (needed for Chinese and long paths).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    let mut last_space: Option<usize> = None;
    for c in text.chars() {
        let w = if is_wide(c) { 2 } else { 1 };
        if used + w > width {
            if c == ' ' {
                lines.push(std::mem::take(&mut line));
                used = 0;
                last_space = None;
                continue;
            }
            match last_space {
                Some(at) => {
                    let rest = line.split_off(at);
                    lines.push(line.trim_end().to_string());
                    line = rest.trim_start().to_string();
                    used = display_width(&line);
                }
                None => {
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                }
            }
            last_space = None;
        }
        if c == ' ' {
            if line.is_empty() {
                continue;
            }
            last_space = Some(line.len());
        }
        line.push(c);
        used += w;
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line.trim_end().to_string());
    }
    lines
}

/// Appends "  left<pad>right" with right wrapped under the first column.
fn entry(out: &mut String, left: &str, right: &str, column: usize) {
    let pad = column.saturating_sub(display_width(left));
    let lines = wrap(right, WIDTH - column - 2);
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            out.push_str(&format!("  {left}{}{line}\n", " ".repeat(pad)));
        } else {
            out.push_str(&format!("  {}{line}\n", " ".repeat(column)));
        }
    }
}

fn hang(out: &mut String, first: &str, indent: usize, text: &str) {
    for (i, line) in wrap(text, WIDTH - indent).iter().enumerate() {
        let prefix = if i == 0 {
            first.to_string()
        } else {
            " ".repeat(indent)
        };
        out.push_str(&format!("{prefix}{line}\n"));
    }
}

fn heading(lang: Lang, en: &'static str, zh: &'static str) -> &'static str {
    Text { en, zh }.get(lang)
}

const OPTIONS: &[(&str, Text)] = &[
    (
        "-h, --help",
        t(
            "Show help (also: help <command>)",
            "顯示說明（也可用 help <命令>）",
        ),
    ),
    (
        "-V, --version",
        t(
            "Print version (with --json: details)",
            "顯示版本（搭配 --json 顯示詳情）",
        ),
    ),
    (
        "--json",
        t("Machine-readable output", "輸出機器可讀的 JSON"),
    ),
    ("--quiet", t("Suppress success messages", "不顯示成功訊息")),
    (
        "--no-color",
        t(
            "ASCII symbols, no color (also NO_COLOR)",
            "只用 ASCII 符號、不著色（亦支援 NO_COLOR）",
        ),
    ),
    (
        "--",
        t(
            "End of flags; the rest is literal text. Global flags work before the command (and after it, except for type)",
            "旗標結束，其後為原文。全域旗標可放在命令前（type 以外亦可放後面）",
        ),
    ),
    (
        "--lang en|zh",
        t(
            "Language (else AUTOGUI_LANG, then Windows UI)",
            "語言（否則 AUTOGUI_LANG，再依 Windows 介面語言）",
        ),
    ),
];

const EXIT_CODES: &[(&str, Text)] = &[
    ("0", t("ok", "成功")),
    ("1", t("operation failed", "操作失敗")),
    ("2", t("usage error", "用法錯誤")),
    ("3", t("window or image not found", "找不到視窗或圖片")),
    ("4", t("timeout", "逾時")),
    (
        "5",
        t(
            "environment blocked / doctor fail",
            "環境受阻 / doctor 有失敗項",
        ),
    ),
    ("6", t("fail-safe triggered", "觸發 fail-safe")),
    ("70", t("internal error", "內部錯誤")),
];

fn options_section(out: &mut String, lang: Lang) {
    out.push_str(&format!("\n{}:\n", heading(lang, "Options", "選項")));
    for (flag, text) in OPTIONS {
        entry(out, flag, text.get(lang), 16);
    }
}

fn exit_section(out: &mut String, lang: Lang) {
    out.push_str(&format!("\n{}:\n", heading(lang, "Exit codes", "結束碼")));
    for (code, text) in EXIT_CODES {
        entry(out, code, text.get(lang), 4);
    }
}

pub fn overview(lang: Lang, version: &str) -> String {
    let mut out = String::new();
    let tagline = heading(
        lang,
        "Control Windows apps by window title and screenshots",
        "以視窗標題與截圖控制 Windows 應用程式",
    );
    hang(
        &mut out,
        "",
        0,
        &format!("autogui-control {version} - {tagline}"),
    );
    out.push_str(&format!("\n{}:\n", heading(lang, "Usage", "用法")));
    out.push_str("  autogui-control <command> [args] [options]\n");
    out.push_str("  autogui-control help <command>\n");
    out.push_str(&format!("\n{}:\n", heading(lang, "Commands", "命令")));
    for c in COMMANDS {
        entry(&mut out, c.name, c.summary.get(lang), 13);
    }
    options_section(&mut out, lang);
    out.push_str(&format!("\n{}:\n", heading(lang, "Examples", "範例")));
    for example in [
        "doctor",
        "session status",
        "list notepad",
        "capture Notepad shot.png",
        "mcp config --host claude-code",
    ] {
        out.push_str(&format!("  autogui-control {example}\n"));
    }
    exit_section(&mut out, lang);
    out.push_str(&format!(
        "\n{}\n",
        heading(
            lang,
            "Primary-monitor corners abort input (fail-safe). Titles must match one window.",
            "把游標移到主螢幕角落可中止輸入（fail-safe）。標題必須只符合一個視窗。"
        )
    ));
    out
}

pub fn command_help(spec: &CommandSpec, lang: Lang) -> String {
    let mut out = String::new();
    hang(
        &mut out,
        "",
        0,
        &format!("autogui-control {} - {}", spec.name, spec.summary.get(lang)),
    );
    out.push_str(&format!("\n{}:\n", heading(lang, "Usage", "用法")));
    hang(&mut out, "  autogui-control ", 18, spec.usage);
    options_section(&mut out, lang);
    out.push_str(&format!("\n{}:\n", heading(lang, "Examples", "範例")));
    out.push_str(&format!("  autogui-control {}\n", spec.example));
    exit_section(&mut out, lang);
    out
}

/// Optimal-string-alignment distance: an adjacent swap costs 1.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut grid = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in grid.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in grid[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (grid[i - 1][j] + 1)
                .min(grid[i][j - 1] + 1)
                .min(grid[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(grid[i - 2][j - 2] + 1);
            }
            grid[i][j] = best;
        }
    }
    grid[a.len()][b.len()]
}

/// Closest known command within a small edit distance ("did you mean").
pub fn suggest(input: &str) -> Option<&'static str> {
    let input = input.to_lowercase();
    let limit = (input.chars().count() / 3).clamp(1, 2);
    COMMANDS
        .iter()
        .map(|c| (distance(&input, c.name), c.name))
        .filter(|(d, name)| *d <= limit || name.starts_with(&input) && input.len() >= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, name)| name)
}

/// Picks the language: --lang flag, AUTOGUI_LANG, Windows UI language, else English.
pub fn resolve_lang(flag: Option<&str>, env: Option<&str>, ui_lang_id: Option<u16>) -> Lang {
    for value in [flag, env].into_iter().flatten() {
        if let Some(lang) = parse_lang(value) {
            return lang;
        }
    }
    match ui_lang_id {
        Some(id) if id & 0x3ff == 0x04 => Lang::Zh,
        _ => Lang::En,
    }
}

pub fn parse_lang(value: &str) -> Option<Lang> {
    let v = value.trim().to_ascii_lowercase();
    if v == "zh" || v.starts_with("zh-") || v.starts_with("zh_") {
        Some(Lang::Zh)
    } else if v == "en" || v.starts_with("en-") || v.starts_with("en_") {
        Some(Lang::En)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_help_fits_80_columns_in_both_languages() {
        for lang in [Lang::En, Lang::Zh] {
            let mut pages = vec![overview(lang, "0.1.0")];
            pages.extend(COMMANDS.iter().map(|c| command_help(c, lang)));
            for page in pages {
                for line in page.lines() {
                    assert!(display_width(line) <= 80, "{lang:?}: {line}");
                }
                for heading in ["Usage", "Exit codes"] {
                    if lang == Lang::En {
                        assert!(page.contains(heading));
                    }
                }
            }
        }
    }

    #[test]
    fn overview_lists_every_command() {
        let text = overview(Lang::En, "9.9.9");
        assert!(text.starts_with("autogui-control 9.9.9"));
        for c in COMMANDS {
            assert!(text.contains(c.name), "{}", c.name);
        }
        for section in [
            "Usage:",
            "Commands:",
            "Options:",
            "Examples:",
            "Exit codes:",
        ] {
            assert!(text.contains(section), "{section}");
        }
        assert!(overview(Lang::Zh, "1").contains("結束碼"));
    }

    #[test]
    fn suggests_close_commands_only() {
        assert_eq!(suggest("lst"), Some("list"));
        assert_eq!(suggest("docter"), Some("doctor"));
        assert_eq!(suggest("clik"), Some("click"));
        assert_eq!(suggest("sess"), Some("session"));
        assert_eq!(suggest("zzzzzz"), None);
        assert_eq!(suggest("lsit"), Some("list"));
        assert_eq!(suggest("wiat"), Some("wait"));
    }

    #[test]
    fn language_precedence() {
        assert_eq!(resolve_lang(Some("zh"), Some("en"), Some(0x0409)), Lang::Zh);
        assert_eq!(resolve_lang(None, Some("en-US"), Some(0x0404)), Lang::En);
        assert_eq!(resolve_lang(None, None, Some(0x0404)), Lang::Zh);
        assert_eq!(resolve_lang(None, None, Some(0x0409)), Lang::En);
        assert_eq!(resolve_lang(None, Some("fr"), None), Lang::En);
        assert_eq!(parse_lang("zh_TW"), Some(Lang::Zh));
    }

    #[test]
    fn wrap_breaks_cjk_and_long_words() {
        for line in wrap(&"漢".repeat(60), 20) {
            assert!(display_width(&line) <= 20);
        }
        assert_eq!(wrap("aaa bbb ccc", 7), vec!["aaa bbb", "ccc"]);
        assert!(wrap(&"x".repeat(50), 10).iter().all(|l| l.len() <= 10));
    }
}
