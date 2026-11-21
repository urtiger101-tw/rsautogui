//! Tool catalogue export in host-specific formats.
use serde_json::Value;
use std::str::FromStr;

/// Output format for `export`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// MCP `tools/list` array.
    Mcp,
    /// Anthropic Messages API `tools` array.
    Anthropic,
    /// OpenAI Responses API `tools` array.
    OpenAi,
    /// OpenAI Chat Completions `tools` array.
    OpenAiChat,
}

impl FromStr for ExportFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mcp" => Ok(Self::Mcp),
            "anthropic" => Ok(Self::Anthropic),
            "openai" => Ok(Self::OpenAi),
            "openai-chat" => Ok(Self::OpenAiChat),
            _ => Err(format!(
                "unknown export format '{s}'; use mcp, anthropic, openai or openai-chat"
            )),
        }
    }
}

/// Returns the tool catalogue converted to `format`.
pub fn export(format: ExportFormat) -> Result<Value, String> {
    match format {
        ExportFormat::Mcp => Ok(crate::tools::definitions().clone()),
        ExportFormat::Anthropic | ExportFormat::OpenAi | ExportFormat::OpenAiChat => {
            Err("not implemented yet".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_and_exports_mcp() {
        for (name, format) in [
            ("mcp", ExportFormat::Mcp),
            ("anthropic", ExportFormat::Anthropic),
            ("openai", ExportFormat::OpenAi),
            ("openai-chat", ExportFormat::OpenAiChat),
        ] {
            assert_eq!(name.parse::<ExportFormat>(), Ok(format));
        }
        assert!("OpenAI".parse::<ExportFormat>().is_err());
        assert_eq!(
            export(ExportFormat::Mcp).as_ref(),
            Ok(crate::tools::definitions())
        );
    }
}
