//! Global flag parsing for the launcher. Command arguments stay untouched;
//! the runtime applies the same rules to the process arguments.
use crate::help::Lang;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub json: bool,
    pub quiet: bool,
    pub no_color: bool,
    pub version: bool,
    pub help: bool,
    pub lang: Option<String>,
    /// Command and its arguments with global flags removed.
    pub rest: Vec<String>,
}

/// A bad global flag; rendered in the user's language.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    LangNeedsValue,
    UnsupportedLang(String),
}

impl ParseError {
    pub fn message(&self, lang: Lang) -> String {
        match (self, lang) {
            (Self::LangNeedsValue, Lang::En) => "--lang needs a value (en or zh)".into(),
            (Self::LangNeedsValue, Lang::Zh) => "--lang 需要指定值（en 或 zh）".into(),
            (Self::UnsupportedLang(v), Lang::En) => {
                format!("unsupported language '{v}'; use en or zh")
            }
            (Self::UnsupportedLang(v), Lang::Zh) => {
                format!("不支援的語言「{v}」；請使用 en 或 zh")
            }
        }
    }
}

/// Commands whose arguments never carry free text may take global flags
/// after the command word. Everything else (notably `type`) accepts global
/// flags only before the command word, or never after "--".
pub fn trailing_globals_allowed(command: &str) -> bool {
    command != "type"
}

/// Splits global flags from command arguments. Flags are recognised before
/// the command word and, except for `type`, after it. After a literal "--"
/// every argument is positional. -h/--help counts only as the first or second
/// token and -V/--version only as the first, so "type <title> -h" still types.
pub fn parse(args: &[String]) -> Result<Parsed, ParseError> {
    let mut parsed = Parsed::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            parsed.rest.extend(iter.by_ref().cloned());
            break;
        }
        let global_ok = parsed
            .rest
            .first()
            .is_none_or(|c| trailing_globals_allowed(c));
        match arg.as_str() {
            "--json" if global_ok => parsed.json = true,
            "--quiet" if global_ok => parsed.quiet = true,
            "--no-color" if global_ok => parsed.no_color = true,
            "--lang" if global_ok => match iter.next() {
                Some(value) => parsed.lang = Some(check_lang(value)?),
                None => return Err(ParseError::LangNeedsValue),
            },
            _ if global_ok && arg.starts_with("--lang=") => {
                parsed.lang = Some(check_lang(&arg["--lang=".len()..])?);
            }
            "-h" | "--help" if parsed.rest.len() <= 1 => parsed.help = true,
            "-V" | "--version" if parsed.rest.is_empty() => parsed.version = true,
            _ => parsed.rest.push(arg.clone()),
        }
    }
    if parsed.rest.first().is_some_and(|c| c == "--doctor") {
        parsed.rest[0] = "doctor".into();
    }
    Ok(parsed)
}

fn check_lang(value: &str) -> Result<String, ParseError> {
    if crate::help::parse_lang(value).is_some() {
        Ok(value.to_string())
    } else {
        Err(ParseError::UnsupportedLang(value.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Parsed, ParseError> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn strips_globals_anywhere() {
        let parsed = p(&["--json", "list", "note", "--quiet", "--no-color"]).unwrap();
        assert!(parsed.json && parsed.quiet && parsed.no_color);
        assert_eq!(parsed.rest, ["list", "note"]);
    }

    #[test]
    fn type_text_keeps_flag_lookalikes() {
        let parsed = p(&["type", "win", "--json"]).unwrap();
        assert!(!parsed.json);
        assert_eq!(parsed.rest, ["type", "win", "--json"]);
        let parsed = p(&["--json", "type", "win", "--quiet", "--lang", "zh"]).unwrap();
        assert!(parsed.json && !parsed.quiet && parsed.lang.is_none());
        assert_eq!(parsed.rest, ["type", "win", "--quiet", "--lang", "zh"]);
    }

    #[test]
    fn help_and_version_positions() {
        assert!(p(&["-h"]).unwrap().help);
        assert!(p(&["click", "--help"]).unwrap().help);
        assert!(!p(&["type", "win", "-h"]).unwrap().help);
        assert!(p(&["-V"]).unwrap().version);
        assert!(p(&["--version", "--json"]).unwrap().version);
        assert!(!p(&["type", "win", "-V"]).unwrap().version);
    }

    #[test]
    fn double_dash_protects_text() {
        let parsed = p(&["type", "win", "--", "--json"]).unwrap();
        assert!(!parsed.json);
        assert_eq!(parsed.rest, ["type", "win", "--json"]);
    }

    #[test]
    fn lang_forms_and_errors() {
        assert_eq!(
            p(&["--lang", "zh", "list"]).unwrap().lang.as_deref(),
            Some("zh")
        );
        assert_eq!(
            p(&["--lang=en", "list"]).unwrap().lang.as_deref(),
            Some("en")
        );
        assert_eq!(p(&["--lang"]), Err(ParseError::LangNeedsValue));
        let err = p(&["--lang", "fr"]).unwrap_err();
        assert!(err.message(Lang::En).starts_with("unsupported language"));
        assert!(err.message(Lang::Zh).contains("不支援"));
    }

    #[test]
    fn doctor_alias() {
        assert_eq!(p(&["--doctor"]).unwrap().rest, ["doctor"]);
    }
}
