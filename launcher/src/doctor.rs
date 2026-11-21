//! "doctor": read-only health checks. Never changes settings or sends input.
//! Check builders are pure so they can be tested with fixture files.
use crate::help::{Lang, display_width, wrap};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn word(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub id: String,
    pub label: String,
    pub status: Status,
    pub detail: String,
    pub advice: String,
}

fn tr(lang: Lang, en: &str, zh: &str) -> String {
    match lang {
        Lang::En => en.into(),
        Lang::Zh => zh.into(),
    }
}

fn check(id: &str, label: String, status: Status, detail: String, advice: String) -> Check {
    Check {
        id: id.into(),
        label,
        status,
        detail,
        advice,
    }
}

/// Exit code: 0 unless any check failed (then 5).
pub fn exit_code(checks: &[Check]) -> i32 {
    if checks.iter().any(|c| c.status == Status::Fail) {
        5
    } else {
        0
    }
}

pub fn counts(checks: &[Check]) -> (usize, usize, usize) {
    let n = |s| checks.iter().filter(|c| c.status == s).count();
    (n(Status::Ok), n(Status::Warn), n(Status::Fail))
}

// ---- individual checks -------------------------------------------------

pub fn check_dll(path: &Path, lang: Lang) -> Check {
    let label = tr(lang, "Runtime DLL", "執行階段 DLL");
    if path.is_file() {
        check(
            "dll",
            label,
            Status::Ok,
            tr(
                lang,
                &format!("found beside EXE: {}", path.display()),
                &format!("與 EXE 同資料夾：{}", path.display()),
            ),
            String::new(),
        )
    } else {
        check(
            "dll",
            label,
            Status::Fail,
            tr(
                lang,
                &format!("not found: {}", path.display()),
                &format!("找不到：{}", path.display()),
            ),
            tr(
                lang,
                "Reinstall, or keep autogui_runtime.dll next to autogui-control.exe",
                "請重新安裝，或把 autogui_runtime.dll 放在 autogui-control.exe 旁",
            ),
        )
    }
}

pub fn check_abi(found: Result<u32, String>, expected: u32, lang: Lang) -> Check {
    let label = tr(lang, "Runtime ABI", "執行階段 ABI");
    match found {
        Ok(v) if v == expected => {
            check("abi", label, Status::Ok, format!("ABI {v}"), String::new())
        }
        Ok(v) => check(
            "abi",
            label,
            Status::Fail,
            format!("DLL ABI {v}, EXE expects {expected}"),
            tr(
                lang,
                "Reinstall so EXE and DLL come from the same package",
                "請重新安裝，讓 EXE 與 DLL 來自同一個套件",
            ),
        ),
        Err(e) => check(
            "abi",
            label,
            Status::Fail,
            e,
            tr(
                lang,
                "Fix the DLL check above first",
                "請先處理上一項 DLL 問題",
            ),
        ),
    }
}

pub fn check_versions(exe: &str, dll: Result<String, String>, lang: Lang) -> Check {
    let label = tr(lang, "Versions match", "版本一致");
    match dll {
        Ok(v) if v.trim() == exe => check(
            "versions",
            label,
            Status::Ok,
            format!("EXE {exe} = DLL {}", v.trim()),
            String::new(),
        ),
        Ok(v) => check(
            "versions",
            label,
            Status::Fail,
            format!("EXE {exe} but DLL {}", v.trim()),
            tr(
                lang,
                "Reinstall so both files come from the same package",
                "請重新安裝，讓兩個檔案來自同一個套件",
            ),
        ),
        Err(e) => check(
            "versions",
            label,
            Status::Fail,
            e,
            tr(
                lang,
                "Fix the DLL checks above first",
                "請先處理上面的 DLL 問題",
            ),
        ),
    }
}

/// Builds the session check from the session_status tool JSON.
pub fn check_session(status: Result<Value, String>, lang: Lang) -> Check {
    let label = tr(lang, "Session ready", "工作階段就緒");
    let value = match status {
        Ok(v) => v,
        Err(e) => {
            return check(
                "session",
                label,
                Status::Fail,
                e,
                tr(
                    lang,
                    "Run: autogui-control session status",
                    "請執行：autogui-control session status",
                ),
            );
        }
    };
    let findings = value["findings"].as_array().cloned().unwrap_or_default();
    let pick = |level: &str| {
        findings
            .iter()
            .filter(|f| f["level"] == level)
            .map(|f| {
                format!(
                    "{}: {}",
                    f["code"].as_str().unwrap_or("?"),
                    f["advice"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
    };
    let blockers = pick("blocker");
    let warnings = pick("warning");
    if let Some(first) = blockers.first() {
        check(
            "session",
            label,
            Status::Fail,
            tr(
                lang,
                "not ready for unattended automation",
                "無法進行無人值守自動化",
            ),
            first.clone(),
        )
    } else if let Some(first) = warnings.first() {
        check(
            "session",
            label,
            Status::Warn,
            tr(lang, "ready with warnings", "可用但有注意事項"),
            first.clone(),
        )
    } else if value["ready"] == true {
        check(
            "session",
            label,
            Status::Ok,
            tr(
                lang,
                "interactive session can capture and send input",
                "互動式工作階段可截圖與送出輸入",
            ),
            String::new(),
        )
    } else {
        check(
            "session",
            label,
            Status::Warn,
            tr(
                lang,
                "unexpected session_status reply",
                "session_status 回應格式不明",
            ),
            String::new(),
        )
    }
}

pub fn check_failsafe(lang: Lang) -> Check {
    check(
        "failsafe",
        tr(lang, "Fail-safe", "Fail-safe"),
        Status::Ok,
        tr(
            lang,
            "always on: primary-monitor corners abort input (no override exists)",
            "永遠啟用：游標移到主螢幕角落即中止輸入（無法關閉）",
        ),
        String::new(),
    )
}

pub fn check_launch_env(value: Option<&str>, lang: Lang) -> Check {
    let label = tr(lang, "app_launch", "app_launch");
    match value {
        Some("1") => check(
            "launch",
            label,
            Status::Warn,
            tr(
                lang,
                "AUTOGUI_ALLOW_LAUNCH=1 in this shell (agents may start programs)",
                "此 shell 設有 AUTOGUI_ALLOW_LAUNCH=1（agent 可啟動程式）",
            ),
            tr(
                lang,
                "Unset it unless you want app_launch",
                "不需要 app_launch 時請取消設定",
            ),
        ),
        _ => check(
            "launch",
            label,
            Status::Ok,
            tr(
                lang,
                "disabled (AUTOGUI_ALLOW_LAUNCH not set to 1)",
                "已停用（AUTOGUI_ALLOW_LAUNCH 未設為 1）",
            ),
            String::new(),
        ),
    }
}

pub fn check_handshake(
    result: Result<(usize, u128), String>,
    expected_tools: usize,
    lang: Lang,
) -> Check {
    let label = tr(lang, "MCP handshake", "MCP 交握");
    match result {
        Ok((n, ms)) if n == expected_tools => check(
            "mcp",
            label,
            Status::Ok,
            format!("initialize + tools/list: {n} tools, {ms} ms"),
            String::new(),
        ),
        Ok((n, ms)) => check(
            "mcp",
            label,
            Status::Fail,
            format!("{n} tools listed, expected {expected_tools} ({ms} ms)"),
            tr(
                lang,
                "Reinstall; the EXE catalogue is inconsistent",
                "請重新安裝；EXE 內的工具目錄不一致",
            ),
        ),
        Err(e) => check(
            "mcp",
            label,
            Status::Fail,
            e,
            tr(
                lang,
                "Run: autogui-control mcp (should wait for input)",
                "請執行：autogui-control mcp（應等待輸入）",
            ),
        ),
    }
}

// ---- agent host registration ---------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    OpenCode,
    Agy,
}

pub const HOSTS: &[Host] = &[
    Host::ClaudeCode,
    Host::ClaudeDesktop,
    Host::Codex,
    Host::OpenCode,
    Host::Agy,
];

impl Host {
    pub fn label(self) -> &'static str {
        match self {
            Host::ClaudeCode => "Claude Code",
            Host::ClaudeDesktop => "Claude Desktop",
            Host::Codex => "Codex",
            Host::OpenCode => "OpenCode",
            Host::Agy => "AGY",
        }
    }
    fn id(self) -> &'static str {
        match self {
            Host::ClaudeCode => "claude-code",
            Host::ClaudeDesktop => "claude-desktop",
            Host::Codex => "codex",
            Host::OpenCode => "opencode",
            Host::Agy => "agy",
        }
    }
}

/// Where host configuration lives; tests substitute temp directories.
#[derive(Debug, Clone, Default)]
pub struct HostEnv {
    pub home: PathBuf,
    pub appdata: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub xdg_config: Option<PathBuf>,
}

impl HostEnv {
    pub fn from_process() -> Self {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        Self {
            home: var("USERPROFILE")
                .or_else(|| var("HOME"))
                .unwrap_or_default(),
            appdata: var("APPDATA"),
            codex_home: var("CODEX_HOME"),
            xdg_config: var("XDG_CONFIG_HOME"),
        }
    }

    fn opencode_home(&self) -> PathBuf {
        match &self.xdg_config {
            Some(x) => x.join("opencode"),
            None => self.home.join(".config").join("opencode"),
        }
    }

    fn codex_dir(&self) -> PathBuf {
        self.codex_home
            .clone()
            .unwrap_or_else(|| self.home.join(".codex"))
    }

    /// Candidate config files in preference order (the first that exists wins).
    pub fn config_paths(&self, host: Host) -> Vec<PathBuf> {
        match host {
            Host::ClaudeCode => vec![self.home.join(".claude.json")],
            Host::ClaudeDesktop => self
                .appdata
                .iter()
                .map(|a| a.join("Claude").join("claude_desktop_config.json"))
                .collect(),
            Host::Codex => vec![self.codex_dir().join("config.toml")],
            Host::OpenCode => {
                let dir = self.opencode_home();
                vec![dir.join("opencode.jsonc"), dir.join("opencode.json")]
            }
            Host::Agy => vec![
                self.home
                    .join(".gemini")
                    .join("config")
                    .join("mcp_config.json"),
            ],
        }
    }

    /// Candidate skill runtime.json files.
    pub fn skill_runtime_paths(&self) -> Vec<PathBuf> {
        let skill = |root: PathBuf| root.join("autogui-control").join("runtime.json");
        vec![
            skill(self.home.join(".claude").join("skills")),
            skill(self.codex_dir().join("skills")),
            skill(self.opencode_home().join("skills")),
            skill(self.home.join(".gemini").join("config").join("skills")),
        ]
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Registration {
    NoFile,
    NoEntry,
    Entry(String),
    Unreadable(String),
}

/// Removes // and /* */ comments outside strings (OpenCode accepts JSONC).
pub fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            for n in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut prev = ' ';
            for n in chars.by_ref() {
                if prev == '*' && n == '/' {
                    break;
                }
                prev = n;
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_json(text: &str) -> Result<Value, String> {
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text)
        .or_else(|_| serde_json::from_str(&strip_jsonc(text)))
        .map_err(|e| e.to_string())
}

/// Extracts command from the [mcp_servers.autogui] table of a Codex config.toml.
pub fn codex_command(toml: &str) -> Option<String> {
    let mut inside = false;
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            let name: String = line
                .trim_matches(|c| c == '[' || c == ']' || c == ' ')
                .replace('"', "");
            inside = name == "mcp_servers.autogui";
            continue;
        }
        if inside && let Some(rest) = line.strip_prefix("command") {
            let value = rest.trim_start().strip_prefix('=')?.trim();
            return parse_toml_string(value);
        }
    }
    None
}

fn parse_toml_string(value: &str) -> Option<String> {
    if let Some(body) = value.strip_prefix('\'') {
        return body.split('\'').next().map(str::to_string);
    }
    let body = value.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            _ => out.push(c),
        }
    }
    None
}

pub fn read_registration(host: Host, path: &Path) -> Registration {
    if !path.is_file() {
        return Registration::NoFile;
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return Registration::Unreadable(e.to_string()),
    };
    if host == Host::Codex {
        return match codex_command(&text) {
            Some(c) => Registration::Entry(c),
            None => Registration::NoEntry,
        };
    }
    let json = match parse_json(&text) {
        Ok(v) => v,
        Err(e) => return Registration::Unreadable(e),
    };
    let command = if host == Host::OpenCode {
        json["mcp"]["autogui"]["command"][0].as_str()
    } else {
        json["mcpServers"]["autogui"]["command"].as_str()
    };
    match command {
        Some(c) => Registration::Entry(c.to_string()),
        None => Registration::NoEntry,
    }
}

pub fn check_host(
    host: Host,
    registration: &Registration,
    current_exe: &Path,
    lang: Lang,
) -> Check {
    let label = host.label().to_string();
    let id = format!("host.{}", host.id());
    let fix = tr(
        lang,
        &format!("Run: autogui-control mcp config --host {}", host.id()),
        &format!("請執行：autogui-control mcp config --host {}", host.id()),
    );
    match registration {
        Registration::NoFile => check(
            &id,
            label,
            Status::Ok,
            tr(lang, "not installed here (skipped)", "此電腦未安裝（略過）"),
            String::new(),
        ),
        Registration::NoEntry => check(
            &id,
            label,
            Status::Warn,
            tr(
                lang,
                "config found but autogui is not registered",
                "找到設定檔，但尚未註冊 autogui",
            ),
            fix,
        ),
        Registration::Unreadable(e) => check(
            &id,
            label,
            Status::Warn,
            tr(
                lang,
                &format!("config unreadable: {e}"),
                &format!("無法讀取設定：{e}"),
            ),
            fix,
        ),
        Registration::Entry(command) => {
            let path = Path::new(command);
            if !path.is_file() {
                check(
                    &id,
                    label,
                    Status::Fail,
                    tr(
                        lang,
                        &format!("registered command is missing: {command}"),
                        &format!("註冊的命令不存在：{command}"),
                    ),
                    fix,
                )
            } else if !same_file(path, current_exe) {
                check(
                    &id,
                    label,
                    Status::Warn,
                    tr(
                        lang,
                        &format!("registered, but points at another copy: {command}"),
                        &format!("已註冊，但指向另一份程式：{command}"),
                    ),
                    fix,
                )
            } else {
                check(
                    &id,
                    label,
                    Status::Ok,
                    tr(lang, "registered", "已註冊"),
                    String::new(),
                )
            }
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

pub fn check_skill_runtime(path: &Path, lang: Lang) -> Check {
    let label = tr(lang, "Skill runtime.json", "Skill runtime.json");
    let id = "skill";
    let advice = tr(
        lang,
        "Reinstall the skill or fix runtime.json",
        "請重新安裝 skill 或修正 runtime.json",
    );
    let json = match std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|t| parse_json(&t))
    {
        Ok(v) => v,
        Err(e) => {
            return check(
                id,
                label,
                Status::Fail,
                format!("{}: {e}", path.display()),
                advice,
            );
        }
    };
    let Some(exe) = json["executable"].as_str() else {
        return check(
            id,
            label,
            Status::Fail,
            tr(
                lang,
                &format!("{}: 'executable' missing", path.display()),
                &format!("{}：缺少 executable", path.display()),
            ),
            advice,
        );
    };
    if json["mcp_args"] != json!(["mcp"]) {
        return check(
            id,
            label,
            Status::Warn,
            tr(
                lang,
                &format!("{}: mcp_args is not [\"mcp\"]", path.display()),
                &format!("{}：mcp_args 不是 [\"mcp\"]", path.display()),
            ),
            advice,
        );
    }
    if !Path::new(exe).is_file() {
        return check(
            id,
            label,
            Status::Fail,
            tr(
                lang,
                &format!("{} points at missing {exe}", path.display()),
                &format!("{} 指向不存在的 {exe}", path.display()),
            ),
            advice,
        );
    }
    check(
        id,
        label,
        Status::Ok,
        path.display().to_string(),
        String::new(),
    )
}

/// All host and skill checks for the given environment.
pub fn host_checks(env: &HostEnv, current_exe: &Path, lang: Lang) -> Vec<Check> {
    let mut out = Vec::new();
    for &host in HOSTS {
        let registration = env
            .config_paths(host)
            .iter()
            .map(|p| read_registration(host, p))
            .find(|r| *r != Registration::NoFile)
            .unwrap_or(Registration::NoFile);
        out.push(check_host(host, &registration, current_exe, lang));
    }
    for path in env.skill_runtime_paths() {
        if path.is_file() {
            out.push(check_skill_runtime(&path, lang));
        }
    }
    out
}

// ---- MCP self-handshake -----------------------------------------------------

/// Spawns exe with "mcp", performs initialize + tools/list, returns (tool count, ms).
pub fn mcp_handshake(exe: &Path, timeout: Duration) -> Result<(usize, u128), String> {
    let start = Instant::now();
    let mut child = Command::new(exe)
        .arg("mcp")
        .env_remove("AUTOGUI_ALLOW_LAUNCH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let result = handshake_io(&mut child, timeout);
    drop(child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && matches!(child.try_wait(), Ok(None)) {
        std::thread::sleep(Duration::from_millis(20));
    }
    if matches!(child.try_wait(), Ok(None)) {
        let _ = child.kill();
        let _ = child.wait();
    }
    result.map(|n| (n, start.elapsed().as_millis()))
}

fn handshake_io(child: &mut std::process::Child, timeout: Duration) -> Result<usize, String> {
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line.unwrap_or_default()).is_err() {
                break;
            }
        }
    });
    let send = |stdin: &mut std::process::ChildStdin, v: Value| {
        writeln!(stdin, "{v}")
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("write failed: {e}"))
    };
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":crate::PROTOCOL,"capabilities":{},"clientInfo":{"name":"autogui-doctor","version":"1"}}}),
    )?;
    let reply = |id: u64| -> Result<Value, String> {
        let line = rx
            .recv_timeout(timeout)
            .map_err(|_| "no reply within timeout".to_string())?;
        let value: Value =
            serde_json::from_str(&line).map_err(|e| format!("bad JSON-RPC reply: {e}"))?;
        if value["id"] != id || !value["error"].is_null() {
            return Err(format!("unexpected reply: {value}"));
        }
        Ok(value)
    };
    reply(1)?;
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )?;
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )?;
    let list = reply(2)?;
    let tools = list["result"]["tools"]
        .as_array()
        .ok_or("tools/list has no tools array")?;
    Ok(tools.len())
}

// ---- rendering --------------------------------------------------------------

pub fn render_json(checks: &[Check]) -> Value {
    let (ok, warn, fail) = counts(checks);
    json!({
        "ok": fail == 0,
        "exit_code": exit_code(checks),
        "summary": {"ok": ok, "warn": warn, "fail": fail},
        "checks": checks.iter().map(|c| json!({
            "id": c.id, "name": c.label, "status": c.status.word(),
            "detail": c.detail, "advice": c.advice,
        })).collect::<Vec<_>>(),
    })
}

pub fn render_table(checks: &[Check], lang: Lang, ascii: bool, quiet: bool) -> String {
    let marker = |s: Status| match (s, ascii) {
        (Status::Ok, false) => "✔",
        (Status::Warn, false) => "!",
        (Status::Fail, false) => "✘",
        (Status::Ok, true) => "[ok]",
        (Status::Warn, true) => "[!!]",
        (Status::Fail, true) => "[XX]",
    };
    let mark_w = if ascii { 4 } else { 1 };
    let label_w = checks
        .iter()
        .map(|c| display_width(&c.label))
        .max()
        .unwrap_or(0)
        .min(24);
    let detail_col = 2 + mark_w + 2 + label_w + 2;
    let mut out = String::new();
    for c in checks.iter().filter(|c| !(quiet && c.status == Status::Ok)) {
        let pad = " ".repeat(label_w.saturating_sub(display_width(&c.label)));
        let lines = wrap(&c.detail, 78usize.saturating_sub(detail_col).max(20));
        for (i, line) in lines.iter().enumerate() {
            if i == 0 {
                let m = marker(c.status);
                let m_pad = " ".repeat(mark_w - display_width(m));
                out.push_str(&format!("  {m}{m_pad}  {}{pad}  {line}\n", c.label));
            } else {
                out.push_str(&format!("{}{line}\n", " ".repeat(detail_col)));
            }
        }
        if !c.advice.is_empty() && c.status != Status::Ok {
            for line in wrap(
                &format!("-> {}", c.advice),
                78usize.saturating_sub(detail_col).max(20),
            ) {
                out.push_str(&format!("{}{line}\n", " ".repeat(detail_col)));
            }
        }
    }
    let (ok, warn, fail) = counts(checks);
    out.push_str(&format!(
        "\n{}\n",
        tr(
            lang,
            &format!("Summary: {ok} ok, {warn} warn, {fail} fail"),
            &format!("摘要：{ok} 項正常、{warn} 項注意、{fail} 項失敗")
        )
    ));
    out
}

// ---- orchestration -----------------------------------------------------------

#[cfg(windows)]
fn probe_abi() -> Result<u32, String> {
    crate::windows::abi_version().map_err(|e| e.to_string())
}
#[cfg(not(windows))]
fn probe_abi() -> Result<u32, String> {
    Err("Windows runtime required".into())
}
#[cfg(windows)]
fn probe_session() -> Result<Value, String> {
    crate::windows::session_status().map_err(|e| e.to_string())
}
#[cfg(not(windows))]
fn probe_session() -> Result<Value, String> {
    Err("Windows runtime required".into())
}

/// Asks the adjacent DLL (through a child launcher process) for its version.
fn runtime_version(exe: &Path) -> Result<String, String> {
    let out = Command::new(exe)
        .arg("__runtime-version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        return Err(text
            .lines()
            .next()
            .unwrap_or("runtime version query failed")
            .to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn gather(lang: Lang) -> Vec<Check> {
    let exe = std::env::current_exe().unwrap_or_else(|_| "autogui-control.exe".into());
    let dll = crate::runtime_path();
    let dll_check = check_dll(&dll, lang);
    let dll_ok = dll_check.status == Status::Ok;
    let msg = tr(
        lang,
        "runtime DLL not available",
        "找不到可用的執行階段 DLL",
    );
    let missing = || msg.clone();
    let mut checks = vec![dll_check];
    checks.push(check_abi(
        if dll_ok { probe_abi() } else { Err(missing()) },
        crate::EXPECTED_ABI,
        lang,
    ));
    checks.push(check_versions(
        crate::VERSION,
        if dll_ok {
            runtime_version(&exe)
        } else {
            Err(missing())
        },
        lang,
    ));
    checks.push(check_session(
        if dll_ok {
            probe_session()
        } else {
            Err(missing())
        },
        lang,
    ));
    checks.push(check_failsafe(lang));
    checks.push(check_launch_env(
        std::env::var("AUTOGUI_ALLOW_LAUNCH").ok().as_deref(),
        lang,
    ));
    checks.extend(host_checks(&HostEnv::from_process(), &exe, lang));
    checks.push(check_handshake(
        mcp_handshake(&exe, Duration::from_secs(10)),
        autogui_protocol::NAMES.len(),
        lang,
    ));
    checks
}

/// Prints the report and returns the exit code (0, or 5 when any check failed).
pub fn run(lang: Lang, no_color: bool, json: bool, quiet: bool) -> i32 {
    let checks = gather(lang);
    if json {
        println!("{}", render_json(&checks));
    } else {
        println!("autogui-control {} doctor\n", crate::VERSION);
        print!("{}", render_table(&checks, lang, no_color, quiet));
    }
    exit_code(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("autogui-doctor-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn st(id: &str, status: Status) -> Check {
        check(id, id.into(), status, "d".into(), "a".into())
    }

    #[test]
    fn aggregation_exit_code_and_counts() {
        let ok = [st("a", Status::Ok), st("b", Status::Warn)];
        assert_eq!(exit_code(&ok), 0);
        let bad = [
            st("a", Status::Ok),
            st("b", Status::Warn),
            st("c", Status::Fail),
        ];
        assert_eq!(exit_code(&bad), 5);
        assert_eq!(counts(&bad), (1, 1, 1));
        let json = render_json(&bad);
        assert_eq!(json["ok"], false);
        assert_eq!(json["exit_code"], 5);
        assert_eq!(json["checks"][2]["status"], "fail");
    }

    #[test]
    fn table_uses_ascii_fallback_and_fits_80() {
        let long = check(
            "x",
            "Long".into(),
            Status::Fail,
            "p".repeat(200),
            "fix it".into(),
        );
        let checks = [st("ok", Status::Ok), long];
        for ascii in [true, false] {
            let text = render_table(&checks, Lang::En, ascii, false);
            assert!(text.lines().all(|l| display_width(l) <= 80), "{text}");
            assert_eq!(text.contains("[XX]"), ascii);
            assert_eq!(text.contains('✘'), !ascii);
            assert!(text.contains("Summary: 1 ok, 0 warn, 1 fail"));
        }
        assert!(!render_table(&checks, Lang::En, true, true).contains("[ok]"));
    }

    #[test]
    fn dll_abi_and_version_checks() {
        let dir = temp("dll");
        let dll = dir.join("autogui_runtime.dll");
        assert_eq!(check_dll(&dll, Lang::En).status, Status::Fail);
        fs::write(&dll, b"x").unwrap();
        assert_eq!(check_dll(&dll, Lang::En).status, Status::Ok);
        assert_eq!(check_abi(Ok(1), 1, Lang::En).status, Status::Ok);
        assert_eq!(check_abi(Ok(2), 1, Lang::En).status, Status::Fail);
        assert_eq!(check_abi(Err("x".into()), 1, Lang::En).status, Status::Fail);
        assert_eq!(
            check_versions("0.1.0", Ok("0.1.0\n".into()), Lang::En).status,
            Status::Ok
        );
        assert_eq!(
            check_versions("0.1.0", Ok("0.2.0".into()), Lang::En).status,
            Status::Fail
        );
    }

    #[test]
    fn session_levels() {
        let blocked = json!({"ready":false,"findings":[{"level":"blocker","code":"locked","advice":"unlock"}]});
        assert_eq!(check_session(Ok(blocked), Lang::En).status, Status::Fail);
        let warn = json!({"ready":true,"findings":[{"level":"warning","code":"rdp","advice":"x"}]});
        assert_eq!(check_session(Ok(warn), Lang::En).status, Status::Warn);
        assert_eq!(
            check_session(Ok(json!({"ready":true,"findings":[]})), Lang::En).status,
            Status::Ok
        );
        assert_eq!(
            check_session(Err("no dll".into()), Lang::En).status,
            Status::Fail
        );
    }

    #[test]
    fn launch_and_handshake_checks() {
        assert_eq!(check_launch_env(Some("1"), Lang::En).status, Status::Warn);
        assert_eq!(check_launch_env(None, Lang::En).status, Status::Ok);
        assert_eq!(
            check_handshake(Ok((19, 5)), 19, Lang::En).status,
            Status::Ok
        );
        assert_eq!(
            check_handshake(Ok((3, 5)), 19, Lang::En).status,
            Status::Fail
        );
        assert_eq!(
            check_handshake(Err("x".into()), 19, Lang::En).status,
            Status::Fail
        );
        assert_eq!(check_failsafe(Lang::Zh).status, Status::Ok);
    }

    #[test]
    fn codex_toml_and_jsonc_parsing() {
        let toml = "[mcp_servers.other]\ncommand = \"x\"\n\n[mcp_servers.autogui]\ncommand = \"C:\\\\Tools\\\\autogui-control.exe\"\nargs = [\"mcp\"]\n";
        assert_eq!(
            codex_command(toml).as_deref(),
            Some("C:\\Tools\\autogui-control.exe")
        );
        assert_eq!(
            codex_command("[mcp_servers.autogui]\ncommand = 'C:\\a.exe'\n").as_deref(),
            Some("C:\\a.exe")
        );
        assert_eq!(
            codex_command("[mcp_servers.other]\ncommand = \"x\"\n"),
            None
        );
        let jsonc = "{\n // note\n \"mcp\": {\"autogui\": {\"command\": [\"a\\\\b.exe\", \"mcp\"]}} /* c */\n}";
        let v = parse_json(jsonc).unwrap();
        assert_eq!(v["mcp"]["autogui"]["command"][0], "a\\b.exe");
        assert_eq!(strip_jsonc("\"//not\" // c"), "\"//not\" ");
    }

    #[test]
    fn host_registration_from_fixture_files() {
        let dir = temp("hosts");
        let exe = dir.join("autogui-control.exe");
        fs::write(&exe, b"x").unwrap();
        let other = dir.join("other.exe");
        fs::write(&other, b"x").unwrap();
        let home = dir.join("home");
        let appdata = dir.join("appdata");
        fs::create_dir_all(appdata.join("Claude")).unwrap();
        fs::create_dir_all(&home).unwrap();
        let esc = |p: &Path| p.display().to_string().replace('\\', "\\\\");
        fs::write(
            home.join(".claude.json"),
            format!(
                "{{\"mcpServers\":{{\"autogui\":{{\"command\":\"{}\",\"args\":[\"mcp\"]}}}}}}",
                esc(&exe)
            ),
        )
        .unwrap();
        fs::write(
            appdata.join("Claude").join("claude_desktop_config.json"),
            format!(
                "{{\"mcpServers\":{{\"autogui\":{{\"command\":\"{}\"}}}}}}",
                esc(&dir.join("gone.exe"))
            ),
        )
        .unwrap();
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::write(
            home.join(".codex").join("config.toml"),
            format!("[mcp_servers.autogui]\ncommand = \"{}\"\n", esc(&other)),
        )
        .unwrap();
        fs::create_dir_all(home.join(".gemini").join("config")).unwrap();
        fs::write(
            home.join(".gemini").join("config").join("mcp_config.json"),
            "{}",
        )
        .unwrap();
        let skill = home.join(".claude").join("skills").join("autogui-control");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("runtime.json"),
            format!(
                "{{\"executable\":\"{}\",\"mcp_args\":[\"mcp\"]}}",
                esc(&exe)
            ),
        )
        .unwrap();
        let env = HostEnv {
            home,
            appdata: Some(appdata),
            codex_home: None,
            xdg_config: None,
        };
        let checks = host_checks(&env, &exe, Lang::En);
        let status = |id: &str| checks.iter().find(|c| c.id == id).map(|c| c.status);
        assert_eq!(status("host.claude-code"), Some(Status::Ok));
        assert_eq!(status("host.claude-desktop"), Some(Status::Fail));
        assert_eq!(status("host.codex"), Some(Status::Warn));
        assert_eq!(status("host.opencode"), Some(Status::Ok));
        assert_eq!(status("host.agy"), Some(Status::Warn));
        assert_eq!(status("skill"), Some(Status::Ok));
        assert_eq!(exit_code(&checks), 5);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn broken_skill_runtime_json_fails() {
        let dir = temp("skill");
        let path = dir.join("runtime.json");
        fs::write(&path, "{not json").unwrap();
        assert_eq!(check_skill_runtime(&path, Lang::En).status, Status::Fail);
        fs::write(
            &path,
            "{\"executable\":\"C:\\\\nope.exe\",\"mcp_args\":[\"mcp\"]}",
        )
        .unwrap();
        assert_eq!(check_skill_runtime(&path, Lang::En).status, Status::Fail);
        let _ = fs::remove_dir_all(dir);
    }
}
