//! "mcp tools" and "mcp config": catalogue export and host snippets.
//! Pure protocol-crate calls; the runtime DLL is never loaded.
use crate::help::Lang;
use autogui_protocol::config::{Host, snippet_parts};
use autogui_protocol::export::{ExportFormat, export};
use std::path::Path;

#[derive(Debug, PartialEq)]
pub enum Request {
    Tools(ExportFormat),
    Config(Host),
}

/// A usage problem in the arguments after "mcp"; rendered per language.
#[derive(Debug, PartialEq, Eq)]
pub enum UsageError {
    NeedsValue(String),
    Unexpected(String),
    UnknownSubcommand(String),
    NeedSubcommand,
    NeedHost,
    BadFormat(String),
    BadHost(String),
}

impl UsageError {
    pub fn message(&self, lang: Lang) -> String {
        const HOSTS: &str = "claude-code, claude-desktop, codex, openai-agents, opencode, agy";
        match (self, lang) {
            (Self::NeedsValue(n), Lang::En) => format!("{n} needs a value"),
            (Self::NeedsValue(n), Lang::Zh) => format!("{n} 需要指定值"),
            (Self::Unexpected(a), Lang::En) => format!("unexpected argument '{a}'"),
            (Self::Unexpected(a), Lang::Zh) => format!("非預期的參數「{a}」"),
            (Self::UnknownSubcommand(s), Lang::En) => {
                format!("unknown mcp subcommand '{s}'; use tools or config")
            }
            (Self::UnknownSubcommand(s), Lang::Zh) => {
                format!("未知的 mcp 子命令「{s}」；請使用 tools 或 config")
            }
            (Self::NeedSubcommand, Lang::En) => "use mcp tools or mcp config".into(),
            (Self::NeedSubcommand, Lang::Zh) => "請使用 mcp tools 或 mcp config".into(),
            (Self::NeedHost, Lang::En) => {
                format!("mcp config needs --host {}", HOSTS.replace(", ", "|"))
            }
            (Self::NeedHost, Lang::Zh) => {
                format!("mcp config 需要 --host {}", HOSTS.replace(", ", "|"))
            }
            (Self::BadFormat(f), Lang::En) => {
                format!("unknown export format '{f}'; use mcp, anthropic, openai or openai-chat")
            }
            (Self::BadFormat(f), Lang::Zh) => {
                format!("未知的匯出格式「{f}」；請使用 mcp、anthropic、openai 或 openai-chat")
            }
            (Self::BadHost(h), Lang::En) => format!("unknown host '{h}'; use {HOSTS}"),
            (Self::BadHost(h), Lang::Zh) => format!("未知的主機「{h}」；請使用 {HOSTS}"),
        }
    }
}

fn option_value(args: &[String], name: &str) -> Result<Option<String>, UsageError> {
    let mut found = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            found = Some(
                iter.next()
                    .cloned()
                    .ok_or_else(|| UsageError::NeedsValue(name.to_string()))?,
            );
        } else if let Some(v) = arg.strip_prefix(&format!("{name}=")) {
            found = Some(v.to_string());
        } else {
            return Err(UsageError::Unexpected(arg.clone()));
        }
    }
    Ok(found)
}

/// Parses the arguments after "mcp".
pub fn parse(args: &[String]) -> Result<Request, UsageError> {
    match args.first().map(String::as_str) {
        Some("tools") => {
            let format = option_value(&args[1..], "--format")?.unwrap_or_else(|| "mcp".into());
            format
                .parse()
                .map(Request::Tools)
                .map_err(|_| UsageError::BadFormat(format))
        }
        Some("config") => {
            let host = option_value(&args[1..], "--host")?.ok_or(UsageError::NeedHost)?;
            host.parse()
                .map(Request::Config)
                .map_err(|_| UsageError::BadHost(host))
        }
        Some(other) => Err(UsageError::UnknownSubcommand(other.to_string())),
        None => Err(UsageError::NeedSubcommand),
    }
}

/// Stdout text, plus where it goes and how to register (config only).
#[derive(Debug, PartialEq)]
pub struct Output {
    pub stdout: String,
    pub file: Option<String>,
    pub command: Option<String>,
}

pub fn execute(request: Request, exe: &Path, json: bool) -> Result<Output, String> {
    match request {
        Request::Tools(format) => {
            let value = export(format)?;
            let stdout = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
            Ok(Output {
                stdout,
                file: None,
                command: None,
            })
        }
        Request::Config(host) => {
            let parts = snippet_parts(host, exe)?;
            if json {
                let value = serde_json::json!({
                    "host": host.name(),
                    "file": parts.file,
                    "snippet": parts.snippet,
                    "command": parts.command,
                });
                let stdout = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
                return Ok(Output {
                    stdout,
                    file: None,
                    command: None,
                });
            }
            Ok(Output {
                stdout: parts.snippet,
                file: parts.file,
                command: parts.command,
            })
        }
    }
}

pub fn run(args: &[String], json: bool, lang: Lang) -> i32 {
    let request = match parse(args) {
        Ok(r) => r,
        Err(error) => return crate::usage_error(&error.message(lang), None, json, lang),
    };
    let exe = std::env::current_exe().unwrap_or_else(|_| "autogui-control.exe".into());
    match execute(request, &exe, json) {
        Ok(output) => {
            if let Some(file) = &output.file {
                eprintln!("{}{file}", crate::pick(lang, "# Add to: ", "# 加入至："));
            }
            if let Some(command) = &output.command {
                eprintln!("{}{command}", crate::pick(lang, "# Or run: ", "# 或執行："));
            }
            print!("{}", output.stdout);
            if !output.stdout.ends_with('\n') {
                println!();
            }
            0
        }
        Err(message) => {
            eprintln!(
                "{}{message}",
                crate::pick(lang, "autogui-control: ", "autogui-control：")
            );
            crate::EXIT_FAILED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_tools_and_config() {
        assert_eq!(parse(&a(&["tools"])), Ok(Request::Tools(ExportFormat::Mcp)));
        assert_eq!(
            parse(&a(&["tools", "--format", "openai-chat"])),
            Ok(Request::Tools(ExportFormat::OpenAiChat))
        );
        assert_eq!(
            parse(&a(&["config", "--host=codex"])),
            Ok(Request::Config(Host::Codex))
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(&a(&[])).is_err());
        assert!(parse(&a(&["tools", "--format", "yaml"])).is_err());
        let err = parse(&a(&["config", "--host", "vim"])).unwrap_err();
        assert!(!err.message(Lang::Zh).is_ascii());
        assert!(err.message(Lang::En).is_ascii());
        assert!(parse(&a(&["config"])).is_err());
        assert!(parse(&a(&["config", "--host", "vim"])).is_err());
        assert!(parse(&a(&["tools", "extra"])).is_err());
        assert!(parse(&a(&["serve"])).is_err());
    }

    #[test]
    fn mcp_export_is_pretty_json() {
        let text = execute(Request::Tools(ExportFormat::Mcp), Path::new("x.exe"), false)
            .unwrap()
            .stdout;
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value.as_array().map(Vec::len),
            Some(autogui_protocol::NAMES.len())
        );
    }

    #[test]
    fn json_host_snippets_parse_on_stdout() {
        for host in [
            Host::ClaudeCode,
            Host::ClaudeDesktop,
            Host::OpenCode,
            Host::Agy,
        ] {
            let out = execute(Request::Config(host), Path::new("C:\\x\\a.exe"), false).unwrap();
            assert!(
                serde_json::from_str::<serde_json::Value>(&out.stdout).is_ok(),
                "{host:?}: {}",
                out.stdout
            );
            assert!(out.file.is_some() || out.command.is_some());
            let json = execute(Request::Config(host), Path::new("a.exe"), true).unwrap();
            let value: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
            assert_eq!(value["host"], host.name());
            assert!(value["snippet"].is_string());
        }
    }
}
