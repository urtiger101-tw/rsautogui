pub fn normalize(name: &str) -> Option<String> {
    match name {
        " " => return Some("space".to_string()),
        "\t" => return Some("tab".to_string()),
        "\n" | "\r" => return Some("enter".to_string()),
        _ => {}
    }
    let n = name.trim().to_ascii_lowercase();
    let mapped = match n.as_str() {
        "return" => "enter",
        "escape" => "esc",
        "command" | "cmd" | "win" | "windows" | "meta" | "super" => "meta",
        "control" => "ctrl",
        "option" => "alt",
        "pgup" => "pageup",
        "pgdn" | "pgdown" => "pagedown",
        other => other,
    };
    if is_known(mapped) {
        Some(mapped.to_string())
    } else {
        None
    }
}

pub fn is_known(n: &str) -> bool {
    if n.len() == 1 {
        return n.chars().next().is_some_and(|c| c.is_ascii_graphic());
    }
    matches!(
        n,
        "enter"
            | "tab"
            | "space"
            | "backspace"
            | "delete"
            | "esc"
            | "up"
            | "down"
            | "left"
            | "right"
            | "home"
            | "end"
            | "pageup"
            | "pagedown"
            | "shift"
            | "shiftright"
            | "ctrl"
            | "ctrlright"
            | "alt"
            | "altright"
            | "meta"
            | "capslock"
            | "numlock"
            | "scrolllock"
            | "printscreen"
            | "insert"
            | "pause"
            | "sleep"
            | "volumedown"
            | "volumeup"
            | "volumemute"
            | "f1"
            | "f2"
            | "f3"
            | "f4"
            | "f5"
            | "f6"
            | "f7"
            | "f8"
            | "f9"
            | "f10"
            | "f11"
            | "f12"
            | "f13"
            | "f14"
            | "f15"
            | "f16"
            | "f17"
            | "f18"
            | "f19"
            | "f20"
            | "f21"
            | "f22"
            | "f23"
            | "f24"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases() {
        assert_eq!(normalize("Enter").as_deref(), Some("enter"));
        assert_eq!(normalize("esc").as_deref(), Some("esc"));
        assert_eq!(normalize("cmd").as_deref(), Some("meta"));
        assert_eq!(normalize("win").as_deref(), Some("meta"));
        assert!(normalize("not-a-key").is_none());
    }

    #[test]
    fn whitespace_and_printable_keys() {
        assert_eq!(normalize(" ").as_deref(), Some("space"));
        assert_eq!(normalize("\t").as_deref(), Some("tab"));
        assert_eq!(normalize("\n").as_deref(), Some("enter"));
        assert_eq!(normalize("!").as_deref(), Some("!"));
    }
}
