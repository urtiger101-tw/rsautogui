//! CLI commands and MCP tool session behind `autogui-control`.
//! The PyAutoGUI-compatible library API lives in the `autogui` crate.
pub mod cli;
pub(crate) mod commands;
pub(crate) mod desktop;
#[cfg(feature = "agent")]
pub mod mcp;
