//! Batched step execution (SPEC §21).
//!
//! Owned by 0.3.0 work package WP-B. Dispatched from `Session::call`.
use super::session::Session;
use autogui::{Error, Result};
use serde_json::Value;

/// Runs the MCP tool `name` (run).
pub(super) fn call(
    _session: &mut Session,
    _name: &str,
    _args: Value,
    _check: &dyn Fn() -> Result<()>,
) -> Result<Value> {
    Err(Error::FeatureDisabled("run"))
}
