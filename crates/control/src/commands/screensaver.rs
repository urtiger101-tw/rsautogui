//! Windows CLI extension; the PyAutoGUI-compatible library API is unchanged.
use autogui::{Error, Result};

#[cfg(windows)]
#[path = "screensaver_windows.rs"]
mod platform;
#[cfg(windows)]
#[cfg_attr(not(feature = "agent"), allow(unused_imports))]
pub(super) use platform::input_desktop;

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Status,
    Pause(u64),
    ManagedPause(u64),
    Resume,
    Stop,
}

fn parse(args: &[String]) -> Result<Command> {
    let invalid =
        || Error::InvalidArgument("use screensaver status|pause [1..86400 seconds]|resume|stop");
    match args.first().map(String::as_str) {
        None => Ok(Command::Status),
        Some("status") if args.len() == 1 => Ok(Command::Status),
        Some("resume") if args.len() == 1 => Ok(Command::Resume),
        Some("stop") if args.len() == 1 => Ok(Command::Stop),
        Some("pause") if args.len() <= 2 || (args.len() == 3 && args[2] == "--managed") => {
            let seconds = args
                .get(1)
                .map(|s| s.parse::<u64>())
                .transpose()
                .map_err(|_| invalid())?
                .unwrap_or(900);
            if !(1..=86_400).contains(&seconds) {
                return Err(invalid());
            }
            if args.len() == 3 {
                Ok(Command::ManagedPause(seconds))
            } else {
                Ok(Command::Pause(seconds))
            }
        }
        _ => Err(invalid()),
    }
}

pub(crate) fn run(args: &[String]) -> Result<()> {
    let command = parse(args)?;
    #[cfg(windows)]
    {
        platform::run(command)
    }
    #[cfg(not(windows))]
    {
        let _ = command;
        Err(Error::UnsupportedPlatform)
    }
}

/// User-facing messages, English or Traditional Chinese (pure, unit-tested).
#[cfg(any(windows, test))]
pub(super) mod text {
    fn t(zh: bool, en: String, zhs: String) -> String {
        if zh { zhs } else { en }
    }
    pub fn restore_failed(zh: bool, error: &str, journal: &str) -> String {
        t(
            zh,
            format!(
                "Restore failed: {error}; unlock, then run screensaver resume. Record: {journal}"
            ),
            format!("還原失敗：{error}；請解除鎖定後執行 screensaver resume。紀錄：{journal}"),
        )
    }
    pub fn pause_started(zh: bool, seconds: u64, enabled: bool, journal: &str) -> [String; 2] {
        [
            t(
                zh,
                format!(
                    "Screen saver auto-start paused; restores in {seconds} s. Press Ctrl+C, or run screensaver resume elsewhere, to restore early."
                ),
                format!(
                    "已暫停螢幕保護程式自動啟動，{seconds} 秒後還原；Ctrl+C 或另開終端執行 screensaver resume 可提早還原。"
                ),
            ),
            t(
                zh,
                format!("Originally enabled: {enabled}; recovery record: {journal}"),
                format!("原本啟用：{enabled}；還原紀錄：{journal}"),
            ),
        ]
    }
    pub fn pause_finished(zh: bool) -> String {
        t(
            zh,
            "Pause finished; original settings restored.".into(),
            "暫停已結束，已完成原設定還原。".into(),
        )
    }
    pub fn restored_from_record(zh: bool) -> String {
        t(
            zh,
            "Original settings restored from the pause record.".into(),
            "已依暫停紀錄還原原設定。".into(),
        )
    }
    pub fn nothing_to_restore(zh: bool) -> String {
        t(
            zh,
            "No pending pause record; any running pause has ended.".into(),
            "目前沒有待還原的暫停紀錄；執行中的暫停已結束。".into(),
        )
    }
    pub fn enabled(zh: bool, value: bool) -> String {
        t(
            zh,
            format!("Screen saver enabled: {value}"),
            format!("螢幕保護程式啟用：{value}"),
        )
    }
    pub fn running(zh: bool, value: bool) -> String {
        t(
            zh,
            format!("Running now: {value}"),
            format!("目前執行中：{value}"),
        )
    }
    pub fn timeout(zh: bool, seconds: u32) -> String {
        t(
            zh,
            format!("Idle timeout (s): {seconds}"),
            format!("閒置啟動秒數：{seconds}"),
        )
    }
    pub fn secure(zh: bool, value: bool) -> String {
        t(
            zh,
            format!("Sign-in required on resume: {value}"),
            format!("恢復時需要登入：{value}"),
        )
    }
    pub fn desktop(zh: bool, name: &Result<String, String>) -> String {
        match name {
            Ok(name) => t(
                zh,
                format!("Input desktop: {name}"),
                format!("目前輸入桌面：{name}"),
            ),
            Err(e) => t(
                zh,
                format!("Input desktop: unreadable ({e})"),
                format!("目前輸入桌面：無法讀取（{e}）"),
            ),
        }
    }
    /// `error` is the Windows error code when the cursor is unavailable.
    pub fn cursor(zh: bool, error: Option<u32>) -> String {
        match error {
            Some(code) => t(
                zh,
                format!(
                    "Desktop cursor access: unavailable (Windows error {code}); sign-in is still required if locked."
                ),
                format!("桌面游標存取：不可用（Windows 錯誤 {code}）；若已鎖定仍需登入。"),
            ),
            None => t(
                zh,
                "Desktop cursor access: available".into(),
                "桌面游標存取：可用".into(),
            ),
        }
    }
    #[derive(Clone, Copy)]
    pub enum Pause {
        Running,
        Pending,
        Inactive,
    }
    pub fn pause_state(zh: bool, state: Pause) -> String {
        let (en, zhs) = match state {
            Pause::Running => ("Tool pause: running", "本工具暫停：執行中"),
            Pause::Pending => (
                "Tool pause: unfinished recovery record; run screensaver resume",
                "本工具暫停：有未完成的還原紀錄，請執行 screensaver resume",
            ),
            Pause::Inactive => ("Tool pause: not active", "本工具暫停：未啟用"),
        };
        t(zh, en.into(), zhs.into())
    }
    pub fn none_running(zh: bool) -> String {
        t(
            zh,
            "No screen saver is running.".into(),
            "目前沒有正在執行的螢幕保護程式。".into(),
        )
    }
    pub fn ended_by_itself(zh: bool) -> String {
        t(
            zh,
            "The screen saver ended by itself.".into(),
            "螢幕保護程式已自行結束。".into(),
        )
    }
    pub fn stopped(zh: bool) -> String {
        t(
            zh,
            "Screen saver stopped; the auto-start setting is unchanged.".into(),
            "已停止目前的螢幕保護程式；自動啟動設定維持原值。".into(),
        )
    }
}

#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
struct Record {
    original_enabled: bool,
}

#[cfg(any(windows, test))]
impl Record {
    fn encode(&self, logon: &str) -> String {
        format!(
            "autogui-screensaver-v1\n{logon}\n{}\n",
            u8::from(self.original_enabled)
        )
    }

    fn decode(text: &str, logon: &str) -> Result<Self> {
        let fields: Vec<_> = text.lines().collect();
        if fields.len() != 3 || fields[0] != "autogui-screensaver-v1" || fields[1] != logon {
            return Err(Error::InvalidArgument(
                "invalid screensaver recovery record; preserve it for manual recovery",
            ));
        }
        let original_enabled = match fields[2] {
            "0" => false,
            "1" => true,
            _ => {
                return Err(Error::InvalidArgument(
                    "invalid screensaver enabled state in recovery record",
                ));
            }
        };
        Ok(Self { original_enabled })
    }
}

// A pause which found the setting already off never owns a setting change.
// Do not turn it off again if another application subsequently enables it.
#[cfg(any(windows, test))]
fn needs_restore(record: &Record, currently_enabled: bool) -> bool {
    record.original_enabled && !currently_enabled
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn accepts_commands_and_default_duration() {
        for (args, command) in [
            (vec![], Command::Status),
            (vec!["status"], Command::Status),
            (vec!["pause"], Command::Pause(900)),
            (vec!["pause", "1"], Command::Pause(1)),
            (vec!["pause", "86400"], Command::Pause(86_400)),
            (vec!["resume"], Command::Resume),
            (vec!["stop"], Command::Stop),
        ] {
            assert_eq!(parse(&arguments(&args)).unwrap(), command);
        }
    }

    #[test]
    fn rejects_invalid_duration_and_extra_arguments() {
        for args in [
            vec!["pause", "0"],
            vec!["pause", "86401"],
            vec!["pause", "-1"],
            vec!["pause", "NaN"],
            vec!["pause", "1.5"],
            vec!["pause", "1", "2"],
            vec!["status", "extra"],
            vec!["resume", "extra"],
            vec!["stop", "extra"],
            vec!["disable"],
        ] {
            assert!(parse(&arguments(&args)).is_err(), "{args:?}");
        }
    }

    #[test]
    fn recovery_record_roundtrips_and_rejects_other_logons() {
        for original_enabled in [false, true] {
            let record = Record { original_enabled };
            let encoded = record.encode("0000000000001234");
            assert_eq!(
                Record::decode(&encoded, "0000000000001234").unwrap(),
                record
            );
            assert!(Record::decode(&encoded, "0000000000009999").is_err());
        }
    }

    #[test]
    fn corrupt_record_is_not_silently_accepted() {
        for text in [
            "",
            "true",
            "autogui-screensaver-v1\nid\n2\n",
            "autogui-screensaver-v1\nid\n1\nextra",
            "autogui-screensaver-v2\nid\n1\n",
        ] {
            assert!(Record::decode(text, "id").is_err());
        }
    }

    #[test]
    fn restores_only_the_change_owned_by_this_pause() {
        assert!(needs_restore(
            &Record {
                original_enabled: true
            },
            false
        ));
        assert!(!needs_restore(
            &Record {
                original_enabled: true
            },
            true
        ));
        assert!(!needs_restore(
            &Record {
                original_enabled: false
            },
            false
        ));
        assert!(!needs_restore(
            &Record {
                original_enabled: false
            },
            true
        ));
    }

    #[test]
    fn english_messages_have_no_cjk() {
        use text::*;
        let cjk = |s: &str| s.chars().any(|c| c as u32 > 0x2e80);
        let mut all = vec![
            restore_failed(false, "e", "j"),
            pause_finished(false),
            restored_from_record(false),
            nothing_to_restore(false),
            enabled(false, true),
            running(false, false),
            timeout(false, 600),
            secure(false, true),
            desktop(false, &Ok("Default".into())),
            desktop(false, &Err("x".into())),
            cursor(false, None),
            cursor(false, Some(5)),
            none_running(false),
            ended_by_itself(false),
            stopped(false),
        ];
        all.extend(pause_started(false, 5, true, "j"));
        for p in [Pause::Running, Pause::Pending, Pause::Inactive] {
            all.push(pause_state(false, p));
        }
        for line in &all {
            assert!(!cjk(line), "{line}");
        }
        assert!(cjk(&stopped(true)));
    }
}
