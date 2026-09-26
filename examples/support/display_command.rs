use autogui::{AutoGui, Display, Error, Result, Window};

pub(super) fn select(name: &str) -> Result<Display> {
    Display::all()?
        .into_iter()
        .find(|display| display.name() == name)
        .ok_or(Error::InvalidArgument(
            "display name not found; run display list again",
        ))
}

pub(super) fn move_window(
    window: &Window,
    display: &Display,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    display.validate()?;
    let bounds = display.bounds();
    if bounds.width < 64 || bounds.height < 64 {
        return Err(Error::InvalidArgument("display is too small for a window"));
    }
    check()?;
    if window.is_minimized()? || window.is_maximized()? {
        window.restore()?;
    }
    let old = window.box_rect()?;
    let width = old.width.min(bounds.width - 32);
    let height = old.height.min(bounds.height - 32);
    let left = bounds
        .left
        .checked_add(16)
        .ok_or(Error::InvalidArgument("display coordinate overflow"))?;
    let top = bounds
        .top
        .checked_add(16)
        .ok_or(Error::InvalidArgument("display coordinate overflow"))?;
    check()?;
    display.validate()?;
    if old.width != width || old.height != height {
        window.resize_to(width, height)?;
        check()?;
        display.validate()?;
    }
    window.move_to(left, top)?;
    display.validate()?;
    let actual = window.box_rect()?;
    if actual.intersect(&bounds) != Some(actual) {
        return Err(Error::Input("app could not fit on the selected display; inspect its actual position and minimum size".into()));
    }
    Ok(())
}

pub(super) fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("list") if args.len() == 1 => {
            for display in Display::all()? {
                println!(
                    "{}\t{:?}\tprimary={}\t{}",
                    display.name(),
                    display.bounds(),
                    display.is_primary(),
                    display.friendly_name()
                );
            }
            Ok(())
        }
        Some("capture") if args.len() == 3 => {
            let display = select(&args[1])?;
            AutoGui::new()?
                .screenshot_display(&display)?
                .save(&args[2])?;
            println!(
                "Saved {} from {} {:?} (desktop physical pixels)",
                args[2],
                display.name(),
                display.bounds()
            );
            Ok(())
        }
        Some("move") if args.len() == 3 => {
            let display = select(&args[1])?;
            let mut gui = AutoGui::new()?;
            let window = super::unique_window(&mut gui, &args[2])?;
            let previous = window.box_rect()?;
            move_window(&window, &display, &|| Ok(()))?;
            println!(
                "{}\tprevious={:?}\tcurrent={:?}\tdisplay={}",
                window.hwnd(),
                previous,
                window.box_rect()?,
                display.name()
            );
            Ok(())
        }
        _ => Err(Error::InvalidArgument(
            "display list | display capture <device name> <output.png> | display move <device name> <unique window title>",
        )),
    }
}
