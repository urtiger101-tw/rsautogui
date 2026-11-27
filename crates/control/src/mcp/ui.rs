//! UI Automation tools (SPEC §18).
//!
//! Owned by 0.3.0 work package WP-A. Dispatched from `Session::call`.
use super::session::Session;
use autogui::{Error, Result};
use serde_json::Value;

/// Runs the MCP tool `name` (ui_snapshot, ui_click, ui_set_value, ui_wait).
pub(super) fn call(
    _session: &mut Session,
    _name: &str,
    _args: Value,
    _check: &dyn Fn() -> Result<()>,
) -> Result<Value> {
    Err(Error::FeatureDisabled(
        "ui_snapshot, ui_click, ui_set_value, ui_wait",
    ))
}
