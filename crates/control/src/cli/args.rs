//! Global flags shared with the launcher (which also handles help/version).

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Globals {
    pub json: bool,
    pub quiet: bool,
}

/// Mirrors the launcher: global flags are recognised before the command
/// word and, except for `type`, after it. A literal "--" ends flag parsing
/// (later arguments are positional and the "--" is dropped).
pub(crate) fn split(args: &[String]) -> (Globals, Vec<String>) {
    let mut globals = Globals::default();
    let mut rest: Vec<String> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            rest.extend(iter.by_ref().cloned());
            break;
        }
        // Text arguments are typed verbatim, so `--json` inside them stays text.
        let global_ok = rest.first().is_none_or(|c| c != "type" && c != "paste");
        match arg.as_str() {
            "--json" if global_ok => globals.json = true,
            "--quiet" if global_ok => globals.quiet = true,
            "--no-color" if global_ok => {}
            "--lang" if global_ok => {
                iter.next();
            }
            _ if global_ok && arg.starts_with("--lang=") => {}
            _ => rest.push(arg.clone()),
        }
    }
    (globals, rest)
}

/// Splits text into pieces of at most 32 characters (one foreground check each).
pub(crate) fn type_chunks(text: &str) -> impl Iterator<Item = &str> {
    let mut remaining = text;
    std::iter::from_fn(move || {
        if remaining.is_empty() {
            return None;
        }
        let end = remaining
            .char_indices()
            .nth(32)
            .map_or(remaining.len(), |(i, _)| i);
        let (head, tail) = remaining.split_at(end);
        remaining = tail;
        Some(head)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn strips_globals_and_keeps_command_arguments() {
        let (g, rest) = split(&s(&[
            "list",
            "--json",
            "x",
            "--lang",
            "zh",
            "--quiet",
            "--no-color",
        ]));
        assert!(g.json && g.quiet);
        assert_eq!(rest, ["list", "x"]);
        let (g, rest) = split(&s(&["type", "w", "--", "--json", "--quiet"]));
        assert_eq!(g, Globals::default());
        assert_eq!(rest, ["type", "w", "--json", "--quiet"]);
        assert_eq!(split(&s(&["--lang=en", "wait", "a"])).1, ["wait", "a"]);
    }

    #[test]
    fn type_text_that_looks_like_a_flag_is_typed() {
        for text in ["--json", "--quiet", "--no-color", "--lang"] {
            let (g, rest) = split(&s(&["type", "Notepad", text]));
            assert_eq!(g, Globals::default());
            assert_eq!(rest, ["type", "Notepad", text]);
        }
        let (g, rest) = split(&s(&["--json", "type", "Notepad", "--quiet"]));
        assert!(g.json && !g.quiet);
        assert_eq!(rest, ["type", "Notepad", "--quiet"]);
    }

    #[test]
    fn type_is_batched_by_32_characters() {
        assert_eq!(type_chunks("").count(), 0);
        for count in [1usize, 31, 32, 33, 100] {
            let text = "字".repeat(count);
            let chunks: Vec<&str> = type_chunks(&text).collect();
            assert_eq!(chunks.concat(), text);
            assert_eq!(chunks.len(), count.div_ceil(32));
            assert!(chunks.iter().all(|c| c.chars().count() <= 32));
        }
    }
}
