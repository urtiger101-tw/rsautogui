//! "mcp tools" and "mcp config": catalogue export and host snippets.
//! Pure protocol-crate calls; the runtime DLL is never loaded.
use crate::help::Lang;
use autogui_protocol::config::{Host, snippet};
use autogui_protocol::export::{ExportFormat, export};
use std::path::Path;

#[derive(Debug, PartialEq)]
pub enum Request {
    Tools(ExportFormat),
    Config(Host),
}

fn option_value(args: &[String], name: &str) -> Result<Option<String>, String> {
    let mut found = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            found = Some(
                iter.next()
                    .cloned()
                    .ok_or(format!("{name} needs a value"))?,
            );
        } else if let Some(v) = arg.strip_prefix(&format!("{name}=")) {
            found = Some(v.to_string());
        } else {
            return Err(format!("unexpected argument '{arg}'"));
        }
    }
    Ok(found)
}

/// Parses the arguments after "mcp"; the Err string is a usage message.
pub fn parse(args: &[String]) -> Result<Request, String> {
    match args.first().map(String::as_str) {
        Some("tools") => {
            let format = option_value(&args[1..], "--format")?.unwrap_or_else(|| "mcp".into());
            Ok(Request::Tools(format.parse()?))
        }
        Some("config") => {
            let host = option_value(&args[1..], "--host")?
                .ok_or("mcp config needs --host claude-code|claude-desktop|codex|openai-agents|opencode|agy")?;
            Ok(Request::Config(host.parse()?))
        }
        Some(other) => Err(format!(
            "unknown mcp subcommand '{other}'; use tools or config"
        )),
        None => Err("use mcp tools or mcp config".into()),
    }
}

pub fn execute(request: Request, exe: &Path) -> Result<String, String> {
    match request {
        Request::Tools(format) => {
            let value = export(format)?;
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
        }
        Request::Config(host) => snippet(host, exe),
    }
}

pub fn run(args: &[String], json: bool, lang: Lang) -> i32 {
    let request = match parse(args) {
        Ok(r) => r,
        Err(message) => return crate::usage_error(&message, None, json, lang),
    };
    let exe = std::env::current_exe().unwrap_or_else(|_| "autogui-control.exe".into());
    match execute(request, &exe) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(message) => {
            eprintln!("autogui-control: {message}");
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
        assert!(parse(&a(&["config"])).is_err());
        assert!(parse(&a(&["config", "--host", "vim"])).is_err());
        assert!(parse(&a(&["tools", "extra"])).is_err());
        assert!(parse(&a(&["serve"])).is_err());
    }

    #[test]
    fn mcp_export_is_pretty_json() {
        let text = execute(Request::Tools(ExportFormat::Mcp), Path::new("x.exe")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value.as_array().map(Vec::len),
            Some(autogui_protocol::NAMES.len())
        );
    }
}
