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

/// Returns the configuration text that registers `exe` with `host`.
pub fn snippet(host: Host, exe: &Path) -> Result<String, String> {
    let _ = (host, exe);
    Err("not implemented yet".into())
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
}
