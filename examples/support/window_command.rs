use super::{AutoGui, Error, Window, unique_window};

pub(super) fn apply(
    window: &Window,
    action: &str,
    pair: Option<(i32, i32)>,
) -> autogui::Result<()> {
    if !matches!(action, "move" | "resize") && pair.is_some() {
        return Err(Error::InvalidArgument(
            "only move/resize accept coordinates",
        ));
    }
    match action {
        "inspect" => {
            window.title()?;
            Ok(())
        }
        "activate" => window.activate(),
        "minimize" => window.minimize(),
        "maximize" => window.maximize(),
        "restore" => window.restore(),
        "hide" => window.hide(),
        "show" => window.show(),
        "close" => window.close(),
        "move" | "resize" => {
            let (a, b) = pair.ok_or(Error::InvalidArgument("move/resize require two integers"))?;
            if action == "move" {
                window.move_to(a, b)
            } else {
                window.resize_to(a, b)
            }
        }
        _ => Err(Error::InvalidArgument("unknown window action")),
    }
}

pub(super) fn run(args: &[String]) -> autogui::Result<()> {
    if args.len() != 2 && args.len() != 4 {
        return Err(Error::InvalidArgument(
            "window <action> <unique title> [x y|width height]",
        ));
    }
    let pair = if args.len() == 4 {
        Some((
            args[2]
                .parse()
                .map_err(|_| Error::InvalidArgument("invalid first integer"))?,
            args[3]
                .parse()
                .map_err(|_| Error::InvalidArgument("invalid second integer"))?,
        ))
    } else {
        None
    };
    let mut gui = AutoGui::new()?;
    let window = unique_window(&mut gui, &args[1])?;
    apply(&window, &args[0], pair)?;
    if args[0] == "close" {
        println!("已送出 WM_CLOSE；應用程式可能顯示儲存確認，並不代表已退出。");
    } else {
        println!(
            "{}\t{:?}\t{}\tactive={} minimized={} maximized={}",
            window.hwnd(),
            window.box_rect()?,
            window.title()?,
            window.is_active()?,
            window.is_minimized()?,
            window.is_maximized()?
        );
    }
    Ok(())
}
