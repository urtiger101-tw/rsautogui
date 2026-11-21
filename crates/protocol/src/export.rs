//! Tool catalogue export in host-specific formats.
use serde_json::{Value, json};
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
///
/// * `Mcp`: the `tools/list` array as served (title, schemas, annotations).
/// * `Anthropic`: `[{name, description, input_schema}]` for the Messages API.
/// * `OpenAi`: Responses API function tools
///   `[{type:"function", name, description, parameters, strict:false}]`.
/// * `OpenAiChat`: Chat Completions tools
///   `[{type:"function", function:{name, description, parameters}}]`.
///
/// `strict` is false because optional arguments are left out of `required`.
/// The caller still executes each call, e.g. by forwarding it to
/// `autogui-control mcp`.
pub fn export(format: ExportFormat) -> Result<Value, String> {
    let tools = crate::tools::definitions()
        .as_array()
        .ok_or("embedded tool catalogue is unreadable")?;
    if format == ExportFormat::Mcp {
        return Ok(Value::Array(tools.clone()));
    }
    tools
        .iter()
        .map(|tool| {
            let name = &tool["name"];
            let description = &tool["description"];
            let schema = &tool["inputSchema"];
            if !name.is_string() || !description.is_string() || !schema.is_object() {
                return Err(format!(
                    "tool {name} is missing name, description or inputSchema"
                ));
            }
            Ok(match format {
                ExportFormat::Anthropic => {
                    json!({"name":name,"description":description,"input_schema":schema})
                }
                ExportFormat::OpenAi => json!({"type":"function","name":name,
                    "description":description,"parameters":schema,"strict":false}),
                _ => json!({"type":"function",
                    "function":{"name":name,"description":description,"parameters":schema}}),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
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

    fn exported(format: ExportFormat) -> Vec<Value> {
        let tools = export(format).unwrap().as_array().unwrap().clone();
        assert_eq!(tools.len(), crate::NAMES.len());
        tools
    }

    #[test]
    fn anthropic_shape() {
        for (tool, name) in exported(ExportFormat::Anthropic).iter().zip(crate::NAMES) {
            let mut keys: Vec<&String> = tool.as_object().unwrap().keys().collect();
            keys.sort();
            assert_eq!(keys, ["description", "input_schema", "name"]);
            assert_eq!(tool["name"], *name);
            assert_eq!(tool["input_schema"]["type"], "object");
        }
    }

    #[test]
    fn openai_responses_shape() {
        for (tool, name) in exported(ExportFormat::OpenAi).iter().zip(crate::NAMES) {
            assert_eq!(tool.as_object().unwrap().len(), 5);
            assert_eq!(tool["type"], "function");
            assert_eq!(tool["name"], *name);
            assert_eq!(tool["strict"], false);
            assert!(tool["description"].is_string());
            assert_eq!(tool["parameters"]["type"], "object");
        }
    }

    #[test]
    fn openai_chat_shape() {
        for (tool, name) in exported(ExportFormat::OpenAiChat).iter().zip(crate::NAMES) {
            assert_eq!(tool.as_object().unwrap().len(), 2);
            assert_eq!(tool["type"], "function");
            let function = &tool["function"];
            assert_eq!(function.as_object().unwrap().len(), 3);
            assert_eq!(function["name"], *name);
            assert!(function["description"].is_string());
            assert_eq!(function["parameters"]["type"], "object");
        }
    }
}
