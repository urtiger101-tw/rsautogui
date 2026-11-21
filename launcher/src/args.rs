//! Global flag parsing for the launcher. Command arguments stay untouched;
//! the runtime applies the same rules to the process arguments.

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

/// Splits global flags from command arguments. After a literal "--" every
/// argument is positional. -h/--help counts only as the first or second
/// token and -V/--version only as the first, so "type <title> -h" still types.
pub fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut parsed = Parsed::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--" => {
                parsed.rest.extend(iter.by_ref().cloned());
            }
            "--json" => parsed.json = true,
            "--quiet" => parsed.quiet = true,
            "--no-color" => parsed.no_color = true,
            "--lang" => match iter.next() {
                Some(value) => parsed.lang = Some(check_lang(value)?),
                None => return Err("--lang needs a value (en or zh)".into()),
            },
            _ if arg.starts_with("--lang=") => {
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

fn check_lang(value: &str) -> Result<String, String> {
    if crate::help::parse_lang(value).is_some() {
        Ok(value.to_string())
    } else {
        Err(format!("unsupported language '{value}'; use en or zh"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Parsed, String> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn strips_globals_anywhere() {
        let parsed = p(&["--json", "list", "note", "--quiet", "--no-color"]).unwrap();
        assert!(parsed.json && parsed.quiet && parsed.no_color);
        assert_eq!(parsed.rest, ["list", "note"]);
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
        assert!(p(&["--lang"]).is_err());
        assert!(p(&["--lang", "fr"]).is_err());
    }

    #[test]
    fn doctor_alias() {
        assert_eq!(p(&["--doctor"]).unwrap().rest, ["doctor"]);
    }
}
