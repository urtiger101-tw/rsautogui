//! Operate one explicitly selected Windows window. `click` and `type` send real input.
//! The CLI lives in the `autogui-control-core` crate; this example keeps
//! `cargo run --example app_control` working.
fn main() {
    autogui_control_core::cli::main();
}
