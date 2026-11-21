//! Host configuration snippets that register autogui-control as an MCP server.
use std::path::Path;
use std::str::FromStr;

/// An agent host that can launch autogui-control over stdio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    OpenAiAgents,
    OpenCode,
    Agy,
}

impl FromStr for Host {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude-code" => Ok(Self::ClaudeCode),
            "claude-desktop" => Ok(Self::ClaudeDesktop),
            "codex" => Ok(Self::Codex),
            "openai-agents" => Ok(Self::OpenAiAgents),
            "opencode" => Ok(Self::OpenCode),
            "agy" => Ok(Self::Agy),
            _ => Err(format!(
                "unknown host '{s}'; use claude-code, claude-desktop, codex, openai-agents, opencode or agy"
            )),
        }
    }
}

/// JSON string literal (also a valid Python string literal).
fn json_string(text: &str) -> Result<String, String> {
    serde_json::to_string(text).map_err(|e| e.to_string())
}

/// TOML basic string literal.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `{"<container>": {"autogui": <server>}}` pretty-printed with keys in a
/// readable order (serde_json would sort them).
fn json_server(container: &str, fields: &[(&str, String)]) -> String {
    let body: Vec<String> = fields
        .iter()
        .map(|(key, value)| format!("      \"{key}\": {value}"))
        .collect();
    format!(
        "{{\n  \"{container}\": {{\n    \"autogui\": {{\n{}\n    }}\n  }}\n}}\n",
        body.join(",\n")
    )
}

/// A host snippet split into machine-usable parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetParts {
    /// Where the snippet goes (a path or location), when the host has one.
    pub file: Option<String>,
    /// The snippet alone: JSON, TOML or Python, with no comment lines.
    pub snippet: String,
    /// A one-line shell command that registers the server instead, if any.
    pub command: Option<String>,
}

impl Host {
    /// The name accepted by `--host`.
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Codex => "codex",
            Self::OpenAiAgents => "openai-agents",
            Self::OpenCode => "opencode",
            Self::Agy => "agy",
        }
    }
}

/// Returns the snippet that registers `exe` with `host`, split into parts.
///
/// Print-only: never touches the file system. The server is registered as
/// `autogui` and started as `<exe> mcp` over stdio.
pub fn snippet_parts(host: Host, exe: &Path) -> Result<SnippetParts, String> {
    let exe = exe.to_str().ok_or("executable path is not valid Unicode")?;
    let command = json_string(exe)?;
    let args = r#"["mcp"]"#.to_string();
    let parts = |file: &str, snippet: String, command: Option<String>| SnippetParts {
        file: (!file.is_empty()).then(|| file.to_string()),
        snippet,
        command,
    };
    Ok(match host {
        Host::ClaudeCode => parts(
            "~/.claude.json or a project .mcp.json",
            json_server(
                "mcpServers",
                &[
                    ("type", r#""stdio""#.into()),
                    ("command", command),
                    ("args", args),
                ],
            ),
            Some(format!(
                "claude mcp add --scope user autogui -- \"{exe}\" mcp"
            )),
        ),
        Host::ClaudeDesktop => parts(
            "%APPDATA%\\Claude\\claude_desktop_config.json",
            json_server("mcpServers", &[("command", command), ("args", args)]),
            None,
        ),
        Host::Codex => parts(
            "~/.codex/config.toml (%USERPROFILE%\\.codex\\config.toml)",
            format!(
                "[mcp_servers.autogui]\ncommand = {}\nargs = [\"mcp\"]\n",
                toml_string(exe)
            ),
            None,
        ),
        Host::OpenAiAgents => parts(
            "",
            format!(
                "from agents import Agent\nfrom agents.mcp import MCPServerStdio\n\n\nasync def desktop_agent():\n    async with MCPServerStdio(\n        name=\"autogui\",\n        params={{\"command\": {command}, \"args\": [\"mcp\"]}},\n        client_session_timeout_seconds=60,\n    ) as autogui:\n        return Agent(name=\"Desktop\", mcp_servers=[autogui])\n"
            ),
            None,
        ),
        Host::OpenCode => parts(
            "~/.config/opencode/opencode.json",
            json_server(
                "mcp",
                &[
                    ("type", r#""local""#.into()),
                    ("command", format!("[{command}, \"mcp\"]")),
                    ("enabled", "true".into()),
                ],
            ),
            None,
        ),
        Host::Agy => parts(
            "~/.gemini/config/mcp_config.json",
            json_server("mcpServers", &[("command", command), ("args", args)]),
            None,
        ),
    })
}

/// Legacy single-string form: `#` comment lines naming the file/command,
/// then the snippet. Prefer [`snippet_parts`].
pub fn snippet(host: Host, exe: &Path) -> Result<String, String> {
    let parts = snippet_parts(host, exe)?;
    let mut out = String::new();
    if let Some(file) = &parts.file {
        out.push_str(&format!("# {}: {file}\n", host.name()));
    }
    if let Some(command) = &parts.command {
        out.push_str(&format!("# Or run once:\n{command}\n\n"));
    }
    out.push_str(&parts.snippet);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_names() {
        for (name, host) in [
            ("claude-code", Host::ClaudeCode),
            ("claude-desktop", Host::ClaudeDesktop),
            ("codex", Host::Codex),
            ("openai-agents", Host::OpenAiAgents),
            ("opencode", Host::OpenCode),
            ("agy", Host::Agy),
        ] {
            assert_eq!(name.parse::<Host>(), Ok(host));
        }
        assert!("claude".parse::<Host>().is_err());
    }

    const EXE: &str = r"C:\Program Files\Auto Gui\autogui-control.exe";
    const ESCAPED: &str = r#""C:\\Program Files\\Auto Gui\\autogui-control.exe""#;

    fn text(host: Host) -> String {
        snippet(host, Path::new(EXE)).unwrap()
    }

    /// The snippet without its `#` comment lines.
    fn body(text: &str) -> String {
        text.lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn claude_code_command_and_json() {
        let text = text(Host::ClaudeCode);
        assert!(text.contains(&format!(
            "claude mcp add --scope user autogui -- \"{EXE}\" mcp\n"
        )));
        let json = &text[text.find('{').unwrap()..];
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"mcpServers":{"autogui":{"type":"stdio","command":EXE,"args":["mcp"]}}})
        );
        assert!(json.contains(ESCAPED));
    }

    #[test]
    fn json_hosts_round_trip_paths_with_spaces_and_backslashes() {
        for (host, expected) in [
            (
                Host::ClaudeDesktop,
                serde_json::json!({"mcpServers":{"autogui":{"command":EXE,"args":["mcp"]}}}),
            ),
            (
                Host::Agy,
                serde_json::json!({"mcpServers":{"autogui":{"command":EXE,"args":["mcp"]}}}),
            ),
            (
                Host::OpenCode,
                serde_json::json!({"mcp":{"autogui":{"type":"local","command":[EXE,"mcp"],"enabled":true}}}),
            ),
        ] {
            let text = text(host);
            assert!(text.starts_with("# "), "{text}");
            let value: serde_json::Value = serde_json::from_str(&body(&text)).unwrap();
            assert_eq!(value, expected, "{host:?}");
        }
    }

    #[test]
    fn parts_hold_clean_snippets() {
        for host in [
            Host::ClaudeCode,
            Host::ClaudeDesktop,
            Host::OpenCode,
            Host::Agy,
        ] {
            let parts = snippet_parts(host, Path::new(EXE)).unwrap();
            assert!(serde_json::from_str::<serde_json::Value>(&parts.snippet).is_ok());
            assert!(!parts.snippet.starts_with('#'));
        }
        let code = snippet_parts(Host::ClaudeCode, Path::new(EXE)).unwrap();
        assert!(
            code.command
                .is_some_and(|c| c.starts_with("claude mcp add"))
        );
        assert_eq!(Host::Agy.name().parse::<Host>(), Ok(Host::Agy));
    }

    #[test]
    fn codex_toml_escapes() {
        let text = text(Host::Codex);
        assert!(text.contains("[mcp_servers.autogui]\n"));
        assert!(text.contains(&format!("command = {ESCAPED}\n")));
        assert!(text.contains("args = [\"mcp\"]\n"));
        assert_eq!(toml_string("a\"b\tc\u{1}"), r#""a\"b\tc\u0001""#);
    }

    #[test]
    fn openai_agents_python() {
        let text = text(Host::OpenAiAgents);
        assert!(text.contains("from agents.mcp import MCPServerStdio"));
        assert!(text.contains(&format!(
            "params={{\"command\": {ESCAPED}, \"args\": [\"mcp\"]}}"
        )));
    }
}
