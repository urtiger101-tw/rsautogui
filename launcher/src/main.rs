#[path = "../../examples/support/mcp.rs"]
mod mcp;
#[cfg(windows)]
mod windows;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!(
            "AutoGui Windows control\n  autogui-control mcp\n  autogui-control list [title]\n  autogui-control capture <title> <output.png>\n  autogui-control locate|click <title> <template> [confidence|exact] [timeout] [scale]\n  autogui-control type <title> <text>\n  autogui-control window <inspect|activate|minimize|maximize|restore|show|hide|move|resize|close> <title> [x y|width height]\n  autogui-control screensaver status|pause [1..86400 seconds]|resume|stop\n\nRuntime: autogui_runtime.dll beside this EXE; loaded on demand.\nMCP initialization/tool listing do not load the DLL. Full image formats retained.\nPrimary monitor physical pixels; corner fail-safe remains enabled.\nTitles must match one window. close requests WM_CLOSE; it does not confirm exit."
        );
        return;
    }
    #[cfg(windows)]
    let result = if args == ["mcp"] {
        mcp::run(windows::LazyEngine::default())
    } else {
        windows::run_cli()
    };
    #[cfg(not(windows))]
    let result: std::io::Result<()> = Err(std::io::Error::other("Windows runtime required"));
    if let Err(e) = result {
        eprintln!("AutoGui: {e}");
        std::process::exit(1);
    }
}
