//! Per-user AutoGui settings (SPEC §22): the on-screen "Computer use!
//! Don't move!" notice and the local action log. Both default to on.
//!
//! The file is `%LOCALAPPDATA%\AutoGui\settings.json`
//! (`{"overlay": true, "action_log": true}`); `autogui-control settings`
//! edits it. `AUTOGUI_OVERLAY` and `AUTOGUI_ACTION_LOG` (`0/1`, `off/on`,
//! `false/true`) override the file for one process, for example in a host's
//! MCP `env` block. The MCP server re-reads the file on every tool call, so a
//! change applies without a restart. Nothing here is reachable from an MCP
//! tool: an agent cannot turn its own log or notice off.
use autogui::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) const OVERLAY_ENV: &str = "AUTOGUI_OVERLAY";
pub(crate) const ACTION_LOG_ENV: &str = "AUTOGUI_ACTION_LOG";

/// What the user chose; unknown keys in the file are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserConfig {
    /// Show the click-through "Computer use! Don't move!" notice while an
    /// agent works through MCP.
    pub overlay: bool,
    /// Append every MCP tool call and CLI command to the daily JSONL log.
    pub action_log: bool,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            overlay: true,
            action_log: true,
        }
    }
}

/// `%LOCALAPPDATA%\AutoGui`, or `None` when the variable is missing.
pub(crate) fn data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(|v| PathBuf::from(v).join("AutoGui"))
}

pub(crate) fn settings_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("settings.json"))
}

pub(crate) fn log_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.join("logs"))
}

/// Parses the file; a UTF-8 BOM (Windows PowerShell 5 writes one) is
/// accepted.
pub(crate) fn parse(text: &str) -> serde_json::Result<UserConfig> {
    serde_json::from_str(text.trim_start_matches('\u{feff}'))
}

/// `0/1`, `off/on`, `false/true`, `no/yes` (case-insensitive).
pub(crate) fn parse_switch(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "yes" => Some(true),
        "0" | "off" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// The file's settings with environment overrides applied. A missing or
/// unreadable file means the defaults (both on), so a broken file can never
/// silently switch the log off.
pub(crate) fn resolve(file: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> UserConfig {
    let mut config = file.and_then(|text| parse(text).ok()).unwrap_or_default();
    if let Some(on) = env(OVERLAY_ENV).as_deref().and_then(parse_switch) {
        config.overlay = on;
    }
    if let Some(on) = env(ACTION_LOG_ENV).as_deref().and_then(parse_switch) {
        config.action_log = on;
    }
    config
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The effective settings for this process. Unit tests never touch the
/// user's real notice or log.
pub(crate) fn load() -> UserConfig {
    if cfg!(test) {
        return UserConfig {
            overlay: false,
            action_log: false,
        };
    }
    let text = settings_path().and_then(|p| std::fs::read_to_string(p).ok());
    if let Some(Err(e)) = text.as_deref().map(parse) {
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            eprintln!(
                "autogui: settings.json is invalid ({e}); using defaults (notice and log on)"
            );
        });
    }
    resolve(text.as_deref(), &env_var)
}

/// The file alone (no environment), for `autogui-control settings`.
pub(crate) fn load_file() -> Result<UserConfig> {
    let Some(path) = settings_path() else {
        return Ok(UserConfig::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text).map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: {e}", path.display()),
            ))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(UserConfig::default()),
        Err(e) => Err(Error::Io(e)),
    }
}

/// Writes the file through a temporary file and a rename.
pub(crate) fn save(config: &UserConfig) -> Result<PathBuf> {
    let path = settings_path().ok_or(Error::InvalidArgument("LOCALAPPDATA is unavailable"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|e| Error::Io(e.into()))?;
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temp, text + "\n")?;
    std::fs::rename(&temp, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn defaults_are_on_and_broken_files_keep_them_on() {
        assert_eq!(resolve(None, &no_env), UserConfig::default());
        assert!(UserConfig::default().overlay && UserConfig::default().action_log);
        assert_eq!(resolve(Some("{not json"), &no_env), UserConfig::default());
        assert_eq!(resolve(Some("{}"), &no_env), UserConfig::default());
        let bom = resolve(Some("\u{feff}{\"action_log\":false}"), &no_env);
        assert!(!bom.action_log, "a BOM must not hide the user's choice");
    }

    #[test]
    fn file_values_and_unknown_keys() {
        let config = resolve(
            Some(r#"{"overlay":false,"action_log":false,"future":1}"#),
            &no_env,
        );
        assert!(!config.overlay && !config.action_log);
        let config = resolve(Some(r#"{"action_log":false}"#), &no_env);
        assert!(config.overlay && !config.action_log);
    }

    #[test]
    fn environment_overrides_the_file() {
        let env = |name: &str| match name {
            OVERLAY_ENV => Some("off".to_owned()),
            ACTION_LOG_ENV => Some("1".to_owned()),
            _ => None,
        };
        let config = resolve(Some(r#"{"overlay":true,"action_log":false}"#), &env);
        assert!(!config.overlay && config.action_log);
        // An unparsable value is ignored, not treated as "off".
        let junk = |_: &str| Some("maybe".to_owned());
        assert_eq!(resolve(None, &junk), UserConfig::default());
    }

    #[test]
    fn switches() {
        for on in ["1", "on", "TRUE", " yes "] {
            assert_eq!(parse_switch(on), Some(true), "{on}");
        }
        for off in ["0", "Off", "false", "no"] {
            assert_eq!(parse_switch(off), Some(false), "{off}");
        }
        assert_eq!(parse_switch(""), None);
    }

    #[test]
    fn unit_tests_never_enable_the_real_notice_or_log() {
        let config = load();
        assert!(!config.overlay && !config.action_log);
    }
}
