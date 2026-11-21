//! autogui-control launcher. Handles help, version, doctor and MCP
//! initialize/tools/list without loading autogui_runtime.dll.
use autogui_protocol::server as mcp;
use help::Lang;
use serde_json::json;

mod args;
mod doctor;
mod help;
mod mcp_cmd;
#[cfg(windows)]
mod windows;

/// MCP protocol version this build speaks.
// TODO: use autogui_protocol::PROTOCOL_VERSION once the protocol crate exports it.
pub const PROTOCOL: &str = "2025-11-25";
/// C ABI version of autogui_runtime.dll this launcher accepts.
pub const EXPECTED_ABI: u32 = 1;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const RUNTIME_DLL: &str = "autogui_runtime.dll";

pub const EXIT_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_BLOCKED: i32 = 5;

fn main() {
    std::process::exit(run());
}

pub fn runtime_path() -> std::path::PathBuf {
    std::env::current_exe()
        .map(|p| p.with_file_name(RUNTIME_DLL))
        .unwrap_or_else(|_| RUNTIME_DLL.into())
}

#[cfg(windows)]
fn ui_language() -> Option<u16> {
    Some(unsafe { ::windows_sys::Win32::Globalization::GetUserDefaultUILanguage() })
}
#[cfg(not(windows))]
fn ui_language() -> Option<u16> {
    None
}

/// One-line message, optional suggestion, then the pointer to --help (stderr).
pub fn usage_error(message: &str, suggestion: Option<&str>, json: bool, lang: Lang) -> i32 {
    if json {
        eprintln!(
            "{}",
            json!({"error":{"kind":"usage","exit_code":EXIT_USAGE,"message":message,"suggestion":suggestion}})
        );
        return EXIT_USAGE;
    }
    match lang {
        Lang::En => {
            eprintln!("autogui-control: {message}");
            if let Some(s) = suggestion {
                eprintln!("Did you mean '{s}'?");
            }
            eprintln!("Run autogui-control --help");
        }
        Lang::Zh => {
            eprintln!("autogui-control：{message}");
            if let Some(s) = suggestion {
                eprintln!("您是指「{s}」嗎？");
            }
            eprintln!("請執行 autogui-control --help");
        }
    }
    EXIT_USAGE
}

/// Picks the English or Traditional Chinese text.
pub fn pick<'a>(lang: Lang, en: &'a str, zh: &'a str) -> &'a str {
    match lang {
        Lang::En => en,
        Lang::Zh => zh,
    }
}

fn unknown_command(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("unknown command '{name}'"),
        Lang::Zh => format!("未知的命令「{name}」"),
    }
}

fn print_version(json: bool) {
    if json {
        let path = runtime_path();
        println!(
            "{}",
            json!({
                "version": VERSION,
                "abi": EXPECTED_ABI,
                "protocol": PROTOCOL,
                "runtime_found": path.is_file(),
                "runtime_path": path.display().to_string(),
            })
        );
    } else {
        println!("autogui-control {VERSION}");
    }
}

fn run_mcp() -> i32 {
    #[cfg(windows)]
    let result = mcp::run(windows::LazyEngine::default());
    #[cfg(not(windows))]
    let result: std::io::Result<()> = Err(std::io::Error::other("Windows runtime required"));
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("autogui-control: {e}");
            EXIT_FAILED
        }
    }
}

fn run_runtime(lang: Lang, no_color: bool) -> i32 {
    // Single-threaded here; the runtime reads these from the environment.
    unsafe {
        std::env::set_var("AUTOGUI_LANG", if lang == Lang::Zh { "zh" } else { "en" });
        if no_color {
            std::env::set_var("NO_COLOR", "1");
        }
    }
    #[cfg(windows)]
    match windows::run_cli() {
        Ok(code) => code,
        Err(e) => {
            match lang {
                Lang::En => {
                    eprintln!("autogui-control: {e}");
                    eprintln!("Run autogui-control doctor");
                }
                Lang::Zh => {
                    eprintln!("autogui-control：{e}");
                    eprintln!("請執行 autogui-control doctor");
                }
            }
            EXIT_BLOCKED
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!(
            "{}",
            pick(
                lang,
                "autogui-control: Windows runtime required",
                "autogui-control：需要 Windows 執行階段"
            )
        );
        EXIT_BLOCKED
    }
}

fn run() -> i32 {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw == ["mcp"] {
        return run_mcp();
    }
    let env_lang = std::env::var("AUTOGUI_LANG").ok();
    let parsed = match args::parse(&raw) {
        Ok(p) => p,
        Err(error) => {
            let lang = help::resolve_lang(None, env_lang.as_deref(), ui_language());
            return usage_error(
                &error.message(lang),
                None,
                raw.iter().any(|a| a == "--json"),
                lang,
            );
        }
    };
    let lang = help::resolve_lang(parsed.lang.as_deref(), env_lang.as_deref(), ui_language());
    let no_color = parsed.no_color || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    if parsed.version {
        print_version(parsed.json);
        return 0;
    }
    let command = parsed.rest.first().map(String::as_str);
    if parsed.help || command.is_none() || command == Some("help") {
        let target = if command == Some("help") {
            parsed.rest.get(1).map(String::as_str)
        } else if parsed.help {
            command
        } else {
            None
        };
        return match target {
            None => {
                print!("{}", help::overview(lang, VERSION));
                0
            }
            Some(name) => match help::find(name) {
                Some(spec) => {
                    print!("{}", help::command_help(spec, lang));
                    0
                }
                None => usage_error(
                    &unknown_command(name, lang),
                    help::suggest(name),
                    parsed.json,
                    lang,
                ),
            },
        };
    }
    let Some(command) = command else {
        return EXIT_USAGE;
    };
    if !help::is_known(command) {
        return usage_error(
            &unknown_command(command, lang),
            help::suggest(command),
            parsed.json,
            lang,
        );
    }
    match command {
        "doctor" if parsed.rest.len() == 1 => {
            doctor::run(lang, no_color, parsed.json, parsed.quiet)
        }
        "doctor" => usage_error(
            pick(lang, "doctor takes no arguments", "doctor 不接受參數"),
            None,
            parsed.json,
            lang,
        ),
        "mcp" if parsed.rest.len() == 1 => run_mcp(),
        "mcp" => mcp_cmd::run(&parsed.rest[1..], parsed.json, lang),
        _ => run_runtime(lang, no_color),
    }
}
