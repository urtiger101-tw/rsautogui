//! CLI commands and MCP tool session behind `autogui-control`.
//! The PyAutoGUI-compatible library API lives in the `autogui` crate.
pub(crate) mod action_log;
pub mod cli;
pub(crate) mod commands;
pub(crate) mod desktop;
#[cfg(feature = "agent")]
pub mod mcp;
pub(crate) mod overlay;
pub(crate) mod user_config;
