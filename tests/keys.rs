//! SPEC section 6: key names resolve through the public API (recorded by FakeBackend).

use std::time::Duration;

use autogui::{AutoGui, Error, FakeBackend};

fn run(f: impl FnOnce(&mut AutoGui) -> autogui::Result<()>) -> (autogui::Result<()>, Vec<String>) {
    let fake = FakeBackend::new(200, 100);
    let events = fake.shared_events();
    let mut g = AutoGui::with_fake(fake);
    g.settings_mut().pause = Duration::ZERO;
    g.settings_mut().failsafe = false;
    let r = f(&mut g);
    let ev = events.lock().map(|e| e.clone()).unwrap_or_default();
    (r, ev)
}

#[test]
fn enter_and_esc_resolve_with_aliases() {
    for (name, canonical) in [
        ("enter", "enter"),
        ("return", "enter"),
        ("esc", "esc"),
        ("escape", "esc"),
    ] {
        let (r, ev) = run(|g| g.press(name));
        r.unwrap();
        assert_eq!(
            ev,
            [format!("{canonical} down"), format!("{canonical} up")],
            "{name}"
        );
    }
}

#[test]
fn cmd_win_meta_command_are_the_same_modifier() {
    let mut seen = Vec::new();
    for name in ["cmd", "win", "meta", "command"] {
        let (r, ev) = run(|g| g.key_down(name));
        r.unwrap();
        seen.push(ev);
    }
    assert!(seen.windows(2).all(|w| w[0] == w[1]), "{seen:?}");
}

#[test]
fn spec_key_table_is_accepted() {
    let mut keys: Vec<String> = ('a'..='z').map(String::from).collect();
    keys.extend(('0'..='9').map(String::from));
    keys.extend((1..=24).map(|n| format!("f{n}")));
    for k in [
        "enter",
        "return",
        "tab",
        "space",
        "backspace",
        "delete",
        "esc",
        "escape",
        "up",
        "down",
        "left",
        "right",
        "home",
        "end",
        "pageup",
        "pagedown",
        "shift",
        "shiftright",
        "ctrl",
        "ctrlright",
        "alt",
        "altright",
        "cmd",
        "win",
        "meta",
        "capslock",
        "numlock",
        "scrolllock",
        "printscreen",
        "insert",
        "pause",
        "sleep",
        "volumedown",
        "volumeup",
        "volumemute",
    ] {
        keys.push(k.to_string());
    }
    for k in keys {
        let (r, _) = run(|g| g.press(&k));
        assert!(r.is_ok(), "key {k} rejected: {r:?}");
    }
}

#[test]
fn unknown_key_is_invalid_argument() {
    let (r, ev) = run(|g| g.press("definitely-not-a-key"));
    assert!(matches!(r, Err(Error::InvalidArgument(_))));
    assert!(ev.is_empty());
    let (r, ev) = run(|g| g.hotkey(&["ctrl", "nope"]));
    assert!(matches!(r, Err(Error::InvalidArgument(_))));
    assert!(ev.is_empty());
}

#[test]
fn hotkey_down_in_order_up_in_reverse() {
    let (r, ev) = run(|g| g.hotkey(&["ctrl", "c"]));
    r.unwrap();
    assert_eq!(ev, ["ctrl down", "c down", "c up", "ctrl up"]);
}

#[test]
fn typewrite_is_alias_of_write() {
    let (r1, e1) = run(|g| g.write("hi\n", Duration::ZERO));
    let (r2, e2) = run(|g| g.typewrite("hi\n", Duration::ZERO));
    r1.unwrap();
    r2.unwrap();
    assert_eq!(e1, e2);
    assert!(!e1.is_empty());
}
