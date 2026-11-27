//! `ui` CLI subcommands (SPEC §18) and the UI Automation helpers the MCP
//! tools (`mcp/ui.rs`) share with them.
//!
//! Entry point for the lead to wire into `cli/mod.rs` `run`, next to
//! `window`/`display`:
//!
//! ```ignore
//! if args[0] == "ui" {
//!     return crate::commands::ui::run(&args[1..], json_output());
//! }
//! ```
//!
//! `pub(crate) fn run(args: &[String], json: bool) -> autogui::Result<()>`
//! where `args` excludes the leading `ui`:
//!
//! ```text
//! ui snapshot  <title> [--query Q] [--role R] [--max-nodes N] [--max-depth D]
//! ui click     <title> [--name N] [--exact] [--role R] [--id A]
//!                      [--action auto|invoke|click|toggle|select|expand|collapse|focus]
//! ui set-value <title> [--name N] [--exact] [--role R] [--id A] --text T
//! ui wait      <title> --text T [--role R] [--gone] [--abort-on T]... [--timeout S]
//! ```
use crate::cli::{active, lang::tr, unique_window};
use autogui::{
    AutoGui, Error, NameMatch, Point, Rect, Result, UiAction, UiAutomation, UiNode, UiSelector,
    UiSnapshotOptions, Window,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const USAGE: &str = "ui snapshot|click|set-value|wait <title> [options]; see --help";
/// Poll interval and walk bounds for text waits.
pub(crate) const WAIT_POLL: Duration = Duration::from_millis(200);
pub(crate) const WAIT_NODES: usize = 1000;
pub(crate) const WAIT_DEPTH: usize = 20;
/// Longest text accepted by set_value.
pub(crate) const MAX_SET_VALUE_CHARS: usize = 20_000;

/// What `ui_click` / `ui click` was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClickAction {
    /// A UIA pattern when the element has one, else a real click.
    Auto,
    /// A real mouse click at the element's centre.
    Click,
    Pattern(UiAction),
}

pub(crate) fn parse_action(action: Option<&str>) -> Result<ClickAction> {
    Ok(match action.unwrap_or("auto") {
        "auto" => ClickAction::Auto,
        "click" => ClickAction::Click,
        "invoke" => ClickAction::Pattern(UiAction::Invoke),
        "toggle" => ClickAction::Pattern(UiAction::Toggle),
        "select" => ClickAction::Pattern(UiAction::Select),
        "expand" => ClickAction::Pattern(UiAction::Expand),
        "collapse" => ClickAction::Pattern(UiAction::Collapse),
        "focus" => ClickAction::Pattern(UiAction::Focus),
        _ => {
            return Err(Error::InvalidArgument(
                "action must be auto|invoke|click|toggle|select|expand|collapse|focus",
            ));
        }
    })
}

/// How a click request is carried out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Method {
    Pattern(UiAction),
    Click,
}

impl Method {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Method::Click => "click",
            Method::Pattern(UiAction::Invoke) => "invoke",
            Method::Pattern(UiAction::Toggle) => "toggle",
            Method::Pattern(UiAction::Select) => "select",
            Method::Pattern(UiAction::Expand) => "expand",
            Method::Pattern(UiAction::Collapse) => "collapse",
            Method::Pattern(UiAction::Focus) => "focus",
        }
    }
}

/// Auto prefers Invoke, Toggle, SelectionItem, then Expand; otherwise a click.
pub(crate) fn method(node: &UiNode, action: ClickAction) -> Method {
    match action {
        ClickAction::Click => Method::Click,
        ClickAction::Pattern(action) => Method::Pattern(action),
        ClickAction::Auto => [
            ("invoke", UiAction::Invoke),
            ("toggle", UiAction::Toggle),
            ("select", UiAction::Select),
            ("expand_collapse", UiAction::Expand),
        ]
        .into_iter()
        .find(|(pattern, _)| node.patterns.iter().any(|p| p == pattern))
        .map_or(Method::Click, |(_, action)| Method::Pattern(action)),
    }
}

/// A selector that only matches `node` itself (same walk index, role, name
/// and automation id), so an action cannot land on a different element.
pub(crate) fn pin(node: &UiNode) -> UiSelector {
    UiSelector {
        index: Some(node.index),
        name: Some(node.name.clone()),
        name_match: NameMatch::Exact,
        role: Some(node.role.clone()),
        automation_id: (!node.automation_id.is_empty()).then(|| node.automation_id.clone()),
    }
}

/// Builds a selector from user fields; at least one is required.
pub(crate) fn selector(
    name: Option<String>,
    exact: bool,
    role: Option<String>,
    automation_id: Option<String>,
) -> Result<UiSelector> {
    let clean = |field: Option<String>, what: &'static str| -> Result<Option<String>> {
        match field {
            Some(text) if text.trim().is_empty() => Err(Error::InvalidArgument(what)),
            Some(text) if text.chars().count() > 512 => Err(Error::InvalidArgument(
                "selector fields must be at most 512 characters",
            )),
            other => Ok(other),
        }
    };
    let selector = UiSelector {
        index: None,
        name: clean(name, "name must not be empty")?,
        name_match: if exact {
            NameMatch::Exact
        } else {
            NameMatch::Contains
        },
        role: clean(role, "role must not be empty")?,
        automation_id: clean(automation_id, "automation_id must not be empty")?,
    };
    if selector.name.is_none() && selector.role.is_none() && selector.automation_id.is_none() {
        return Err(Error::InvalidArgument(
            "give an element ref, name, role or automation_id",
        ));
    }
    Ok(selector)
}

/// Centre of `node` for a real click, if it is enabled, on screen and
/// inside `bounds` (the window rectangle).
pub(crate) fn click_point(node: &UiNode, bounds: Rect) -> Result<Point> {
    if !node.enabled {
        return Err(Error::InvalidArgument("element is disabled"));
    }
    let rect = node.rect;
    if node.offscreen || rect.width <= 0 || rect.height <= 0 {
        return Err(Error::InvalidArgument(
            "element has no on-screen rectangle; use a pattern action or scroll it into view",
        ));
    }
    let point = autogui::center(rect);
    if point.x < bounds.left
        || point.y < bounds.top
        || point.x >= bounds.right()
        || point.y >= bounds.bottom()
    {
        return Err(Error::InvalidArgument(
            "element centre is outside the window; scroll it into view and retry",
        ));
    }
    Ok(point)
}

/// Activates `window`, re-resolves `node` (activation can move it) and
/// clicks its centre after a foreground check. The library click keeps the
/// corner fail-safe.
pub(crate) fn real_click(
    gui: &mut AutoGui,
    ui: &mut UiAutomation,
    window: &Window,
    node: &UiNode,
    check: &dyn Fn() -> Result<()>,
) -> Result<(UiNode, Point)> {
    check()?;
    window.activate()?;
    active(window)?;
    let fresh = ui.resolve(window, &pin(node))?;
    let point = click_point(&fresh, window.box_rect()?)?;
    check()?;
    active(window)?;
    gui.click_xy(point.x, point.y)?;
    Ok((fresh, point))
}

pub(crate) fn rect_json(rect: Rect) -> Value {
    json!({"left":rect.left,"top":rect.top,"width":rect.width,"height":rect.height})
}

/// A node as JSON; `element` is its session ref, when it has one.
pub(crate) fn node_json(node: &UiNode, element: Option<String>) -> Value {
    json!({
        "element": element,
        "index": node.index,
        "depth": node.depth,
        "role": node.role,
        "name": node.name,
        "automation_id": node.automation_id,
        "rect": rect_json(node.rect),
        "enabled": node.enabled,
        "focused": node.focused,
        "offscreen": node.offscreen,
        "patterns": node.patterns,
        "value": node.value,
    })
}

fn has_text(node: &UiNode, needle: &str) -> bool {
    node.name.to_lowercase().contains(needle)
        || node
            .value
            .as_deref()
            .is_some_and(|value| value.to_lowercase().contains(needle))
}

fn role_matches(node: &UiNode, role: &str) -> bool {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| *c != ' ' && *c != '_')
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    norm(&node.role) == norm(role)
}

/// First node whose name or value contains `text` (case-insensitive),
/// optionally with the given role.
pub(crate) fn text_hit<'a>(
    nodes: &'a [UiNode],
    text: &str,
    role: Option<&str>,
) -> Option<&'a UiNode> {
    let needle = text.to_lowercase();
    nodes
        .iter()
        .find(|node| role.is_none_or(|r| role_matches(node, r)) && has_text(node, &needle))
}

/// First `abort_on` string present anywhere in `nodes`, with its node.
pub(crate) fn abort_hit<'a>(
    nodes: &'a [UiNode],
    abort_on: &[String],
) -> Option<(String, &'a UiNode)> {
    abort_on
        .iter()
        .find_map(|word| text_hit(nodes, word, None).map(|node| (word.clone(), node)))
}

pub(crate) struct WaitSpec {
    pub(crate) text: String,
    pub(crate) role: Option<String>,
    /// Wait for presence (true) or absence (false).
    pub(crate) present: bool,
    pub(crate) abort_on: Vec<String>,
    pub(crate) timeout: Duration,
}

pub(crate) struct WaitOutcome {
    /// "present", "absent" or "aborted".
    pub(crate) state: &'static str,
    pub(crate) matched: Option<UiNode>,
    pub(crate) aborted_on: Option<String>,
    pub(crate) elapsed: Duration,
    /// The last walk hit its node limit, so "absent" may miss deep nodes.
    pub(crate) truncated: bool,
}

pub(crate) fn validate_wait(spec: &WaitSpec) -> Result<()> {
    if spec.text.trim().is_empty() || spec.text.chars().count() > 512 {
        return Err(Error::InvalidArgument("text must be 1..512 characters"));
    }
    if spec.role.as_deref().is_some_and(|r| r.trim().is_empty()) {
        return Err(Error::InvalidArgument("role must not be empty"));
    }
    if spec.abort_on.len() > 8
        || spec
            .abort_on
            .iter()
            .any(|a| a.trim().is_empty() || a.chars().count() > 512)
    {
        return Err(Error::InvalidArgument(
            "abort_on takes at most 8 non-empty strings",
        ));
    }
    if spec.timeout > Duration::from_secs(30) {
        return Err(Error::InvalidArgument("timeout must be 0..30 seconds"));
    }
    Ok(())
}

fn timed_out() -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "timed out waiting for ui text",
    ))
}

/// Sleeps up to `duration` in short slices, checking cancellation.
fn pause(duration: Duration, check: &dyn Fn() -> Result<()>) -> Result<()> {
    let end = Instant::now() + duration;
    loop {
        check()?;
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        std::thread::sleep(left.min(Duration::from_millis(25)));
    }
}

/// Polls snapshots every [`WAIT_POLL`] until the text is present/absent, an
/// `abort_on` string appears, or `timeout` passes (`Io(TimedOut)`).
pub(crate) fn wait(
    ui: &mut UiAutomation,
    window: &Window,
    spec: &WaitSpec,
    check: &dyn Fn() -> Result<()>,
) -> Result<WaitOutcome> {
    validate_wait(spec)?;
    let options = UiSnapshotOptions {
        query: None,
        role: None,
        max_nodes: WAIT_NODES,
        max_depth: WAIT_DEPTH,
    };
    let start = Instant::now();
    loop {
        check()?;
        let snapshot = ui.snapshot(window, &options)?;
        let outcome = |state, matched: Option<&UiNode>, aborted_on| WaitOutcome {
            state,
            matched: matched.cloned(),
            aborted_on,
            elapsed: start.elapsed(),
            truncated: snapshot.truncated,
        };
        if let Some((word, node)) = abort_hit(&snapshot.nodes, &spec.abort_on) {
            return Ok(outcome("aborted", Some(node), Some(word)));
        }
        let hit = text_hit(&snapshot.nodes, &spec.text, spec.role.as_deref());
        match (spec.present, hit) {
            (true, Some(node)) => return Ok(outcome("present", Some(node), None)),
            (false, None) => return Ok(outcome("absent", None, None)),
            _ => {}
        }
        let elapsed = start.elapsed();
        if elapsed >= spec.timeout {
            return Err(timed_out());
        }
        pause(WAIT_POLL.min(spec.timeout - elapsed), check)?;
    }
}

/// `--flag value` options and `--switch` flags after the title.
#[derive(Default)]
struct Flags {
    values: Vec<(String, String)>,
    switches: Vec<String>,
}

const SWITCHES: [&str; 2] = ["--exact", "--gone"];

impl Flags {
    fn parse(args: &[String]) -> Result<Self> {
        let mut flags = Flags::default();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if SWITCHES.contains(&arg.as_str()) {
                flags.switches.push(arg.clone());
            } else if arg.starts_with("--") {
                let value = iter
                    .next()
                    .ok_or(Error::InvalidArgument("ui option is missing its value"))?;
                flags.values.push((arg.clone(), value.clone()));
            } else {
                return Err(Error::InvalidArgument(
                    "unexpected ui argument; quote the title and use --options",
                ));
            }
        }
        Ok(flags)
    }
    fn only(&self, allowed: &[&str]) -> Result<()> {
        let known = |name: &String| allowed.contains(&name.as_str());
        if self.values.iter().all(|(name, _)| known(name)) && self.switches.iter().all(known) {
            Ok(())
        } else {
            Err(Error::InvalidArgument(
                "unknown option for this ui subcommand; see --help",
            ))
        }
    }
    fn get(&self, name: &str) -> Option<String> {
        self.values
            .iter()
            .rev()
            .find(|(flag, _)| flag == name)
            .map(|(_, value)| value.clone())
    }
    fn all(&self, name: &str) -> Vec<String> {
        self.values
            .iter()
            .filter(|(flag, _)| flag == name)
            .map(|(_, value)| value.clone())
            .collect()
    }
    fn has(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }
    fn number(&self, name: &str, default: usize, max: usize) -> Result<usize> {
        match self.get(name) {
            None => Ok(default),
            Some(text) => text
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=max).contains(n))
                .ok_or(Error::InvalidArgument(
                    "--max-nodes must be 1..500 and --max-depth 1..30",
                )),
        }
    }
    fn selector(&self) -> Result<UiSelector> {
        selector(
            self.get("--name"),
            self.has("--exact"),
            self.get("--role"),
            self.get("--id"),
        )
    }
}

enum Request {
    Snapshot(UiSnapshotOptions),
    Click(UiSelector, ClickAction),
    SetValue(UiSelector, String),
    Wait(WaitSpec),
}

fn parse(command: &str, flags: &Flags) -> Result<Request> {
    Ok(match command {
        "snapshot" => {
            flags.only(&["--query", "--role", "--max-nodes", "--max-depth"])?;
            Request::Snapshot(UiSnapshotOptions {
                query: flags.get("--query"),
                role: flags.get("--role"),
                max_nodes: flags.number("--max-nodes", 150, 500)?,
                max_depth: flags.number("--max-depth", 12, 30)?,
            })
        }
        "click" => {
            flags.only(&["--name", "--exact", "--role", "--id", "--action"])?;
            Request::Click(
                flags.selector()?,
                parse_action(flags.get("--action").as_deref())?,
            )
        }
        "set-value" => {
            flags.only(&["--name", "--exact", "--role", "--id", "--text"])?;
            let text = flags
                .get("--text")
                .ok_or(Error::InvalidArgument("set-value needs --text"))?;
            if text.chars().count() > MAX_SET_VALUE_CHARS {
                return Err(Error::InvalidArgument("text exceeds 20000 characters"));
            }
            Request::SetValue(flags.selector()?, text)
        }
        "wait" => {
            flags.only(&["--text", "--role", "--gone", "--abort-on", "--timeout"])?;
            let seconds = match flags.get("--timeout") {
                None => 5.0,
                Some(text) => text
                    .parse::<f64>()
                    .ok()
                    .filter(|s| s.is_finite() && (0.0..=30.0).contains(s))
                    .ok_or(Error::InvalidArgument("timeout must be 0..30 seconds"))?,
            };
            let spec = WaitSpec {
                text: flags
                    .get("--text")
                    .ok_or(Error::InvalidArgument("wait needs --text"))?,
                role: flags.get("--role"),
                present: !flags.has("--gone"),
                abort_on: flags.all("--abort-on"),
                timeout: Duration::from_secs_f64(seconds),
            };
            validate_wait(&spec)?;
            Request::Wait(spec)
        }
        _ => return Err(Error::InvalidArgument(USAGE)),
    })
}

/// `ui <snapshot|click|set-value|wait> <title> [options]`.
// Until the lead wires this into cli/mod.rs it is reachable only from tests.
#[allow(dead_code)]
pub(crate) fn run(args: &[String], json: bool) -> Result<()> {
    let (command, rest) = args.split_first().ok_or(Error::InvalidArgument(USAGE))?;
    let (title, rest) = rest.split_first().ok_or(Error::InvalidArgument(USAGE))?;
    // Arguments are validated before any window or COM work.
    let request = parse(command, &Flags::parse(rest)?)?;
    if !cfg!(windows) {
        return Err(Error::UnsupportedPlatform);
    }
    let mut gui = AutoGui::new()?;
    gui.settings_mut().pause = Duration::from_millis(25);
    let window = unique_window(&mut gui, title)?;
    let mut ui = UiAutomation::new()?;
    let no_cancel = || Ok(());
    match request {
        Request::Snapshot(options) => {
            let snapshot = ui.snapshot(&window, &options)?;
            if json {
                let nodes: Vec<Value> = snapshot.nodes.iter().map(|n| node_json(n, None)).collect();
                println!(
                    "{}",
                    json!({"action":"ui snapshot","hwnd":window.hwnd(),"nodes":nodes,"truncated":snapshot.truncated,"count":snapshot.nodes.len()})
                );
            } else {
                print!("{}", snapshot.to_text());
            }
        }
        Request::Click(selector, action) => {
            let node = ui.resolve(&window, &selector)?;
            let chosen = method(&node, action);
            let (node, point) = match chosen {
                Method::Pattern(action) => {
                    let node = ui.act(&window, &pin(&node), action)?;
                    let point = autogui::center(node.rect);
                    (node, point)
                }
                Method::Click => real_click(&mut gui, &mut ui, &window, &node, &no_cancel)?,
            };
            if json {
                println!(
                    "{}",
                    json!({"action":"ui click","method":chosen.name(),"element":node_json(&node, None),"center":{"x":point.x,"y":point.y}})
                );
            } else if !crate::cli::quiet() {
                println!(
                    "{} {} \"{}\" {:?}",
                    chosen.name(),
                    node.role,
                    node.name,
                    node.rect
                );
            }
        }
        Request::SetValue(selector, text) => {
            let node = ui.resolve(&window, &selector)?;
            let readback = ui.set_value(&window, &pin(&node), &text)?;
            let redacted = node.value.is_none();
            let matches = (!redacted).then_some(readback == text);
            if json {
                println!(
                    "{}",
                    json!({"action":"ui set-value","element":node_json(&node, None),"value_matches":matches})
                );
            } else if !crate::cli::quiet() {
                println!(
                    "{}",
                    match matches {
                        Some(true) => tr!("Value set and read back", "已設定值並回讀確認"),
                        Some(false) => tr!(
                            "Value set, but the app reports a different value",
                            "已設定值，但應用程式回報的值不同"
                        ),
                        None => tr!(
                            "Value set (password field; not read back)",
                            "已設定值（密碼欄位，不回讀）"
                        ),
                    }
                );
            }
        }
        Request::Wait(spec) => {
            let outcome = wait(&mut ui, &window, &spec, &no_cancel)?;
            if json {
                println!(
                    "{}",
                    json!({"action":"ui wait","state":outcome.state,"matched":outcome.matched.as_ref().map(|n| node_json(n, None)),"aborted_on":outcome.aborted_on,"elapsed_ms":outcome.elapsed.as_millis() as u64,"truncated":outcome.truncated})
                );
            } else if !crate::cli::quiet() {
                println!(
                    "{} {} ms{}",
                    outcome.state,
                    outcome.elapsed.as_millis(),
                    outcome
                        .aborted_on
                        .map(|word| format!(" ({word})"))
                        .unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(index: usize, role: &str, name: &str, patterns: &[&str]) -> UiNode {
        UiNode {
            index,
            depth: 1,
            role: role.into(),
            name: name.into(),
            automation_id: String::new(),
            rect: Rect::new(10, 10, 20, 10),
            enabled: true,
            focused: false,
            offscreen: false,
            patterns: patterns.iter().map(|p| p.to_string()).collect(),
            value: None,
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn auto_prefers_patterns_in_order() {
        let both = node(1, "TreeItem", "x", &["select", "expand_collapse"]);
        assert_eq!(
            method(&both, ClickAction::Auto),
            Method::Pattern(UiAction::Select)
        );
        let button = node(1, "Button", "x", &["toggle", "invoke"]);
        assert_eq!(
            method(&button, ClickAction::Auto),
            Method::Pattern(UiAction::Invoke)
        );
        let combo = node(1, "ComboBox", "x", &["expand_collapse", "value"]);
        assert_eq!(
            method(&combo, ClickAction::Auto),
            Method::Pattern(UiAction::Expand)
        );
        let text = node(1, "Text", "x", &[]);
        assert_eq!(method(&text, ClickAction::Auto), Method::Click);
        assert_eq!(method(&button, ClickAction::Click), Method::Click);
        assert_eq!(
            method(&text, ClickAction::Pattern(UiAction::Focus)),
            Method::Pattern(UiAction::Focus)
        );
        assert_eq!(Method::Pattern(UiAction::Collapse).name(), "collapse");
        assert!(matches!(
            parse_action(Some("press")),
            Err(Error::InvalidArgument(_))
        ));
        assert_eq!(parse_action(None).unwrap(), ClickAction::Auto);
    }

    #[test]
    fn pin_matches_only_the_same_element() {
        let mut n = node(7, "Button", "Save", &[]);
        let pinned = pin(&n);
        assert_eq!(pinned.index, Some(7));
        assert_eq!(pinned.name_match, NameMatch::Exact);
        assert_eq!(pinned.automation_id, None);
        n.automation_id = "SaveBtn".into();
        assert_eq!(pin(&n).automation_id.as_deref(), Some("SaveBtn"));
    }

    #[test]
    fn selector_validation() {
        assert!(matches!(
            selector(None, false, None, None),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(
            selector(Some(" ".into()), false, None, None),
            Err(Error::InvalidArgument("name must not be empty"))
        ));
        assert!(selector(Some("x".repeat(513)), false, None, None).is_err());
        let s = selector(Some("Save".into()), true, Some("Button".into()), None).unwrap();
        assert_eq!(s.name_match, NameMatch::Exact);
        assert_eq!(s.index, None);
    }

    #[test]
    fn click_point_requires_a_visible_enabled_element_inside_the_window() {
        let window = Rect::new(0, 0, 100, 100);
        let mut n = node(1, "Button", "OK", &[]);
        assert_eq!(click_point(&n, window).unwrap(), Point::new(20, 15));
        assert!(click_point(&n, Rect::new(50, 50, 10, 10)).is_err());
        n.offscreen = true;
        assert!(click_point(&n, window).is_err());
        n.offscreen = false;
        n.rect = Rect::new(0, 0, 0, 0);
        assert!(click_point(&n, window).is_err());
        n.rect = Rect::new(10, 10, 20, 10);
        n.enabled = false;
        assert!(matches!(
            click_point(&n, window),
            Err(Error::InvalidArgument("element is disabled"))
        ));
    }

    #[test]
    fn text_and_abort_matching() {
        let mut edit = node(3, "Edit", "Body", &["value"]);
        edit.value = Some("Hello World".into());
        let nodes = vec![
            node(1, "Button", "Save", &[]),
            edit,
            node(4, "Text", "Error: disk full", &[]),
        ];
        assert_eq!(text_hit(&nodes, "WORLD", None).map(|n| n.index), Some(3));
        assert_eq!(
            text_hit(&nodes, "save", Some("button")).map(|n| n.index),
            Some(1)
        );
        assert!(text_hit(&nodes, "save", Some("Edit")).is_none());
        let (word, hit) = abort_hit(&nodes, &["missing".into(), "error".into()]).unwrap();
        assert_eq!((word.as_str(), hit.index), ("error", 4));
        assert!(abort_hit(&nodes, &[]).is_none());
    }

    #[test]
    fn wait_validation() {
        let spec = |text: &str, abort: usize, secs: u64| WaitSpec {
            text: text.into(),
            role: None,
            present: true,
            abort_on: vec!["x".into(); abort],
            timeout: Duration::from_secs(secs),
        };
        assert!(validate_wait(&spec("ok", 8, 30)).is_ok());
        assert!(validate_wait(&spec("", 0, 5)).is_err());
        assert!(validate_wait(&spec("ok", 9, 5)).is_err());
        assert!(validate_wait(&spec("ok", 0, 31)).is_err());
    }

    #[test]
    fn cli_arguments_are_validated_before_any_window_work() {
        for bad in [
            &[][..],
            &["snapshot"],
            &["frobnicate", "Notepad"],
            &["snapshot", "Notepad", "--max-nodes", "0"],
            &["snapshot", "Notepad", "--max-depth", "31"],
            &["snapshot", "Notepad", "--name", "x"],
            &["snapshot", "Notepad", "stray"],
            &["click", "Notepad"],
            &["click", "Notepad", "--name"],
            &["click", "Notepad", "--name", "Save", "--action", "press"],
            &["set-value", "Notepad", "--name", "Body"],
            &["wait", "Notepad"],
            &["wait", "Notepad", "--text", "x", "--timeout", "31"],
            &["wait", "Notepad", "--text", "x", "--timeout", "NaN"],
        ] {
            assert!(
                matches!(run(&args(bad), false), Err(Error::InvalidArgument(_))),
                "{bad:?}"
            );
        }
        let flags = Flags::parse(&args(&["--abort-on", "a", "--gone", "--abort-on", "b"])).unwrap();
        assert_eq!(flags.all("--abort-on"), ["a", "b"]);
        assert!(flags.has("--gone"));
        assert!(matches!(
            parse(
                "wait",
                &Flags::parse(&args(&["--text", "x", "--gone"])).unwrap()
            ),
            Ok(Request::Wait(WaitSpec { present: false, .. }))
        ));
    }
}
