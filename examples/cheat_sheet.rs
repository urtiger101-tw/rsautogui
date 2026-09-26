//! Read-only cheat sheet. Does not move the real mouse.

use autogui::{AutoGui, LocateOptions};

fn main() -> autogui::Result<()> {
    let mut gui = AutoGui::new()?;
    gui.settings_mut().failsafe = true;

    let size = gui.size()?;
    let pos = gui.position()?;
    println!("screen: {}x{}", size.width, size.height);
    println!("mouse: {},{}", pos.x, pos.y);
    println!("on_screen 0,0: {}", gui.on_screen(0, 0)?);

    // Image locate API shape (will fail if file missing — that's expected).
    let opt = LocateOptions::default();
    match gui.locate_on_screen("button.png", &opt) {
        Ok(r) => println!("found button at {:?}", r),
        Err(e) => println!("locate_on_screen: {e}"),
    }

    match gui.get_windows_with_title("Notepad") {
        Ok(wins) => println!("notepad windows: {}", wins.len()),
        Err(e) => println!("window api: {e}"),
    }
    Ok(())
}
