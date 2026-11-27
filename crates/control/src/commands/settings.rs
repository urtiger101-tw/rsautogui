//! `autogui-control settings [overlay on|off] [action-log on|off]`
//! (SPEC §22): shows or changes the per-user settings file.
use crate::cli::lang::tr;
use crate::user_config::{self, ACTION_LOG_ENV, OVERLAY_ENV, UserConfig};
use autogui::{Error, Result};
use serde_json::json;

const USAGE: &str = "settings [overlay on|off] [action-log on|off]";

/// Applies `key value` pairs to `config`.
pub(crate) fn apply(config: &mut UserConfig, args: &[String]) -> Result<()> {
    if !args.len().is_multiple_of(2) {
        return Err(Error::InvalidArgument(USAGE));
    }
    for pair in args.chunks(2) {
        let on = user_config::parse_switch(&pair[1])
            .ok_or(Error::InvalidArgument("settings values are on or off"))?;
        match pair[0].as_str() {
            "overlay" => config.overlay = on,
            "action-log" | "action_log" => config.action_log = on,
            _ => return Err(Error::InvalidArgument(USAGE)),
        }
    }
    Ok(())
}

fn on_off(on: bool) -> String {
    if on {
        tr!("on", "開啟")
    } else {
        tr!("off", "關閉")
    }
}

pub(crate) fn run(args: &[String], json_output: bool) -> Result<()> {
    let mut config = user_config::load_file()?;
    let changed = !args.is_empty();
    if changed {
        let before = config;
        apply(&mut config, args)?;
        // Logged before it applies, so turning the log off leaves a trace.
        crate::action_log::record_settings_change(&before, &config);
        user_config::save(&config)?;
    }
    let effective = user_config::load();
    let path = user_config::settings_path();
    let logs = user_config::log_dir();
    let overrides: Vec<(&str, String)> = [OVERLAY_ENV, ACTION_LOG_ENV]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok().map(|v| (name, v)))
        .collect();
    if json_output {
        println!(
            "{}",
            json!({
                "action": "settings",
                "changed": changed,
                "file": {"overlay": config.overlay, "action_log": config.action_log},
                "effective": {"overlay": effective.overlay, "action_log": effective.action_log},
                "settings_path": path,
                "log_dir": logs,
                "env_overrides": overrides.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
            })
        );
        return Ok(());
    }
    println!(
        "{}",
        tr!(
            "Overlay (\"Computer use! Don't move!\"): {}",
            "操作提示視窗（\"Computer use! Don't move!\"）：{}",
            on_off(effective.overlay)
        )
    );
    println!(
        "{}",
        tr!(
            "Action log: {}",
            "動作紀錄：{}",
            on_off(effective.action_log)
        )
    );
    if let Some(path) = &path {
        println!("{}", tr!("Settings file: {}", "設定檔：{}", path.display()));
    }
    if let Some(logs) = &logs {
        println!(
            "{}",
            tr!("Log folder: {}", "紀錄資料夾：{}", logs.display())
        );
    }
    for (name, value) in &overrides {
        println!(
            "{}",
            tr!(
                "Note: {}={} overrides the file in this shell",
                "注意：此 shell 的 {}={} 會覆寫設定檔",
                name,
                value
            )
        );
    }
    if changed {
        println!(
            "{}",
            tr!(
                "Saved. Running MCP servers apply it on their next tool call.",
                "已儲存。執行中的 MCP 伺服器會在下一次工具呼叫時套用。"
            )
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn applies_pairs() {
        let mut config = UserConfig::default();
        apply(&mut config, &strings(&["action-log", "off"])).unwrap();
        assert!(config.overlay && !config.action_log);
        apply(&mut config, &strings(&["overlay", "0", "action_log", "on"])).unwrap();
        assert!(!config.overlay && config.action_log);
    }

    #[test]
    fn rejects_bad_input() {
        let mut config = UserConfig::default();
        for bad in [
            &["overlay"][..],
            &["overlay", "maybe"],
            &["volume", "on"],
            &["overlay", "on", "action-log"],
        ] {
            assert!(apply(&mut config, &strings(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(config, UserConfig::default());
    }
}
