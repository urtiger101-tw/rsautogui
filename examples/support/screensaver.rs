//! Windows CLI extension; the PyAutoGUI-compatible library API is unchanged.
use autogui::{Error, Result};

#[cfg(windows)]
#[path = "screensaver_windows.rs"]
mod platform;

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

pub(super) fn run(args: &[String]) -> Result<()> {
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
}
