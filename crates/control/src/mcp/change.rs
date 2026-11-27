//! Visual change wait (SPEC §7.3).
//!
//! Owned by 0.3.0 work package WP-B. Dispatched from `Session::call`.
use super::session::Session;
use autogui::{Error, Result};
use serde_json::Value;

/// Runs the MCP tool `name` (wait_change).
pub(super) fn call(
    _session: &mut Session,
    _name: &str,
    _args: Value,
    _check: &dyn Fn() -> Result<()>,
) -> Result<Value> {
    Err(Error::FeatureDisabled("wait_change"))
}
