//! MCP protocol pieces shared by the autogui-control launcher and runtime.
//! Depends only on `serde_json` so the launcher stays small.
pub mod config;
pub mod errors;
pub mod export;
pub mod server;
pub mod tools;

pub use errors::{Cancellation, ErrorCode, ToolError, cancellation, is_cancellation};
pub use server::{Handler, PROTOCOL_VERSION, run};
pub use tools::{NAMES, definitions, read_only};
