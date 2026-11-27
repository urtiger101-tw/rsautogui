//! Windows UI Automation (SPEC §18): accessibility-tree snapshots, element
//! selectors and pattern actions.
//!
//! The COM work lives in `uia_win.rs`, on one dedicated MTA worker thread
//! owned by [`UiAutomation`]; only plain data crosses threads. This file holds
//! the public types, the pure selector/rendering logic and the stub used off
//! Windows (`UnsupportedPlatform`) or without the `uia` feature
//! (`FeatureDisabled("uia")`).
//!
//! Element text (names, values) comes from other applications: treat it as
//! untrusted data. Values of password fields are never returned.
// Helpers used only by the COM backend are dead in stub builds.
#![cfg_attr(not(all(windows, feature = "uia")), allow(dead_code))]

use std::time::Duration;

use crate::error::{Error, Result};
use crate::types::Rect;
use crate::window::Window;

/// Longest wait for one request by the plain (non-`_within`) methods.
pub(crate) const UI_CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// Message prefix of errors after an action was sent but not confirmed.
pub(crate) const UNCONFIRMED: &str =
    "unconfirmed: action sent but unconfirmed; re-observe with ui_snapshot before retrying";

/// Hard cap on nodes visited by one walk, whatever the caller asks for.
pub(crate) const HARD_MAX_NODES: usize = 5000;
/// Hard cap on walk depth.
pub(crate) const HARD_MAX_DEPTH: usize = 64;
/// Nodes visited when a snapshot filters by `query`/`role`, so matches deeper
/// than `max_nodes` unfiltered nodes can still be found.
pub(crate) const FILTERED_WALK_NODES: usize = 1000;
/// Nodes visited to resolve a name/role/automation_id selector. The node
/// cap bounds the cost, so resolution walks to [`HARD_MAX_DEPTH`].
pub(crate) const RESOLVE_WALK_NODES: usize = 1000;
/// Characters kept from an element name or value.
pub(crate) const MAX_NAME_CHARS: usize = 512;
pub(crate) const MAX_VALUE_CHARS: usize = 4096;

/// Pattern names reported in [`UiNode::patterns`].
pub(crate) const PATTERN_INVOKE: &str = "invoke";
pub(crate) const PATTERN_TOGGLE: &str = "toggle";
pub(crate) const PATTERN_SELECT: &str = "select";
pub(crate) const PATTERN_EXPAND: &str = "expand_collapse";
pub(crate) const PATTERN_VALUE: &str = "value";
pub(crate) const PATTERN_FOCUS: &str = "focus";

/// What [`UiAutomation::snapshot`] walks and returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiSnapshotOptions {
    /// Case-insensitive substring of the name or automation id.
    pub query: Option<String>,
    /// Control type name such as `Button` (case-insensitive).
    pub role: Option<String>,
    /// Most nodes returned (default 150).
    pub max_nodes: usize,
    /// Deepest level walked; the window itself is depth 0 (default 12).
    pub max_depth: usize,
}

impl Default for UiSnapshotOptions {
    fn default() -> Self {
        Self {
            query: None,
            role: None,
            max_nodes: 150,
            max_depth: 12,
        }
    }
}

/// One element of the control view. `index` is its breadth-first position in
/// the window's tree, stable while the tree does not change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiNode {
    pub index: usize,
    pub depth: usize,
    /// Control type name: `Button`, `Edit`, `MenuItem`, ...
    pub role: String,
    pub name: String,
    pub automation_id: String,
    /// Bounding rectangle in Windows desktop physical pixels (like window rects).
    pub rect: Rect,
    pub enabled: bool,
    pub focused: bool,
    pub offscreen: bool,
    /// Available actions: `invoke`, `toggle`, `select`, `expand_collapse`,
    /// `value`, `focus`.
    pub patterns: Vec<String>,
    /// ValuePattern value; `None` without a ValuePattern and for passwords.
    pub value: Option<String>,
    /// UIA RuntimeId: identifies the element while it exists; empty when
    /// the provider does not report one.
    pub runtime_id: Vec<i32>,
}

/// A bounded view of a window's UI Automation tree. The walk is
/// breadth-first (that is what `index` numbers); `nodes` are listed in tree
/// (depth-first) order so indentation by `depth` reads as a tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiSnapshot {
    pub nodes: Vec<UiNode>,
    /// True when the node limit cut the walk or the result short.
    pub truncated: bool,
}

/// How [`UiSelector::name`] is compared; both are case-insensitive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NameMatch {
    Exact,
    #[default]
    Contains,
}

/// Picks exactly one element. All given fields must match. With
/// `NameMatch::Contains`, several matches of which exactly one has the exact
/// name resolve to that one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiSelector {
    pub index: Option<usize>,
    pub name: Option<String>,
    pub name_match: NameMatch,
    pub role: Option<String>,
    pub automation_id: Option<String>,
    /// Exact UIA RuntimeId (from [`UiNode::runtime_id`]); pins the selector
    /// to that element even when look-alike siblings exist.
    pub runtime_id: Option<Vec<i32>>,
}

/// Pattern-based element actions; none of them moves the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    /// InvokePattern.
    Invoke,
    /// TogglePattern.
    Toggle,
    /// SelectionItemPattern.
    Select,
    /// ExpandCollapsePattern.
    Expand,
    /// ExpandCollapsePattern.
    Collapse,
    /// SetFocus.
    Focus,
}

/// UI Automation client. All COM objects live on one worker thread; a call
/// that does not finish in time abandons that worker and the next call starts
/// a fresh one.
pub struct UiAutomation {
    engine: platform::Engine,
}

impl std::fmt::Debug for UiAutomation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UiAutomation")
    }
}

impl UiAutomation {
    /// Starts the worker thread and connects to UI Automation.
    pub fn new() -> Result<Self> {
        Ok(Self {
            engine: platform::Engine::new()?,
        })
    }

    /// Walks `window` breadth-first within the option limits.
    pub fn snapshot(&mut self, window: &Window, options: &UiSnapshotOptions) -> Result<UiSnapshot> {
        self.snapshot_within(window, options, UI_CALL_TIMEOUT, &|| Ok(()))
    }

    /// Re-walks `window` and returns the single element matching `selector`.
    pub fn resolve(&mut self, window: &Window, selector: &UiSelector) -> Result<UiNode> {
        self.resolve_within(window, selector, UI_CALL_TIMEOUT, &|| Ok(()))
    }

    /// Resolves `selector` and performs `action` through its UIA pattern.
    /// Unsupported patterns and disabled elements are `InvalidArgument`.
    pub fn act(
        &mut self,
        window: &Window,
        selector: &UiSelector,
        action: UiAction,
    ) -> Result<UiNode> {
        self.act_within(window, selector, action, UI_CALL_TIMEOUT, &|| Ok(()))
    }

    /// Sets the element's ValuePattern value and returns the value read back
    /// (empty for password fields, whose value is never read).
    pub fn set_value(
        &mut self,
        window: &Window,
        selector: &UiSelector,
        text: &str,
    ) -> Result<String> {
        self.set_value_within(window, selector, text, UI_CALL_TIMEOUT, &|| Ok(()))
    }

    /// [`snapshot`](Self::snapshot) that waits at most `timeout` and calls
    /// `check` (cancellation, emergency stop) every few milliseconds while
    /// the worker runs; the first `check` error is returned at once.
    pub fn snapshot_within(
        &mut self,
        window: &Window,
        options: &UiSnapshotOptions,
        timeout: Duration,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<UiSnapshot> {
        validate_options(options)?;
        window.title()?; // validates the HWND and its original process/thread
        let (nodes, parents, truncated) = self.engine.walk(
            window.hwnd(),
            walk_limit(options),
            options.max_depth,
            timeout,
            check,
        )?;
        Ok(finish(tree_order(nodes, &parents), truncated, options))
    }

    /// [`resolve`](Self::resolve) with a `timeout` and a `check` callback.
    pub fn resolve_within(
        &mut self,
        window: &Window,
        selector: &UiSelector,
        timeout: Duration,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<UiNode> {
        validate_selector(selector)?;
        window.title()?;
        self.engine.resolve(
            window.hwnd(),
            selector.clone(),
            resolve_limit(selector),
            timeout,
            check,
        )
    }

    /// [`act`](Self::act) with a `timeout` and a `check` callback. When
    /// `check` fails or the timeout passes before the action starts, the
    /// action is never performed; once it has started, a timeout is an
    /// "unconfirmed" [`Error::Uia`].
    pub fn act_within(
        &mut self,
        window: &Window,
        selector: &UiSelector,
        action: UiAction,
        timeout: Duration,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<UiNode> {
        validate_selector(selector)?;
        window.title()?;
        before_mutation(window)?;
        let node = self.engine.act(
            window.hwnd(),
            selector.clone(),
            resolve_limit(selector),
            action,
            timeout,
            check,
        )?;
        after_mutation(window)?;
        Ok(node)
    }

    /// [`set_value`](Self::set_value) with a `timeout` and a `check`
    /// callback, with the same guarantees as [`act_within`](Self::act_within).
    pub fn set_value_within(
        &mut self,
        window: &Window,
        selector: &UiSelector,
        text: &str,
        timeout: Duration,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<String> {
        validate_selector(selector)?;
        window.title()?;
        before_mutation(window)?;
        let value = self.engine.set_value(
            window.hwnd(),
            selector.clone(),
            resolve_limit(selector),
            text.to_owned(),
            timeout,
            check,
        )?;
        after_mutation(window)?;
        Ok(value)
    }
}

fn before_mutation(window: &Window) -> Result<()> {
    #[cfg(windows)]
    {
        crate::window_win::check_failsafe(&window.settings)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Ok(())
    }
}

fn after_mutation(window: &Window) -> Result<()> {
    before_mutation(window)?;
    if !window.settings.pause.is_zero() {
        std::thread::sleep(window.settings.pause);
    }
    Ok(())
}

impl UiSnapshot {
    /// One compact, indented line per node, e.g.
    /// `[12] Button "Save" id=SaveBtn (10,20,80,24) [invoke] disabled`,
    /// plus a final note when the snapshot was truncated.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for node in &self.nodes {
            out.push_str(&"  ".repeat(node.depth.min(HARD_MAX_DEPTH)));
            out.push_str(&format!("[{}] {}", node.index, node.role));
            if !node.name.is_empty() {
                out.push_str(&format!(" \"{}\"", clip(&node.name, 80)));
            }
            if !node.automation_id.is_empty() {
                out.push_str(&format!(" id={}", clip(&node.automation_id, 60)));
            }
            if let Some(value) = &node.value {
                out.push_str(&format!(" value=\"{}\"", clip(value, 60)));
            }
            let r = node.rect;
            out.push_str(&format!(" ({},{},{},{})", r.left, r.top, r.width, r.height));
            if !node.patterns.is_empty() {
                out.push_str(&format!(" [{}]", node.patterns.join(",")));
            }
            if !node.enabled {
                out.push_str(" disabled");
            }
            if node.focused {
                out.push_str(" focused");
            }
            if node.offscreen {
                out.push_str(" offscreen");
            }
            out.push('\n');
        }
        if self.truncated {
            out.push_str(&format!(
                "... truncated after {} nodes; narrow with query/role or raise max_nodes\n",
                self.nodes.len()
            ));
        }
        out
    }
}

/// Single-line, length-limited display text with quotes escaped.
pub(crate) fn clip(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (count, ch) in text.chars().enumerate() {
        if count == max_chars {
            out.push('…');
            break;
        }
        match ch {
            '\r' | '\n' | '\t' => out.push(' '),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The first `max_chars` characters of `text`.
pub(crate) fn cap(text: String, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((end, _)) => text[..end].to_owned(),
        None => text,
    }
}

/// The value a node may expose: never a password, and at most
/// [`MAX_VALUE_CHARS`] characters.
pub(crate) fn visible_value(is_password: bool, raw: Option<String>) -> Option<String> {
    if is_password {
        return None;
    }
    raw.map(|value| cap(value, MAX_VALUE_CHARS))
}

/// Human-friendly name of a `UIA_*ControlTypeId`.
pub(crate) fn role_name(control_type: i32) -> &'static str {
    const NAMES: [&str; 41] = [
        "Button",
        "Calendar",
        "CheckBox",
        "ComboBox",
        "Edit",
        "Hyperlink",
        "Image",
        "ListItem",
        "List",
        "Menu",
        "MenuBar",
        "MenuItem",
        "ProgressBar",
        "RadioButton",
        "ScrollBar",
        "Slider",
        "Spinner",
        "StatusBar",
        "Tab",
        "TabItem",
        "Text",
        "ToolBar",
        "ToolTip",
        "Tree",
        "TreeItem",
        "Custom",
        "Group",
        "Thumb",
        "DataGrid",
        "DataItem",
        "Document",
        "SplitButton",
        "Window",
        "Pane",
        "Header",
        "HeaderItem",
        "Table",
        "TitleBar",
        "Separator",
        "SemanticZoom",
        "AppBar",
    ];
    control_type
        .checked_sub(50000)
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| NAMES.get(i).copied())
        .unwrap_or("Unknown")
}

fn fold(text: &str) -> String {
    text.to_lowercase()
}

/// Role names compare case-insensitively, ignoring spaces and underscores
/// (`menu item` == `MenuItem`).
pub(crate) fn role_eq(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| *c != ' ' && *c != '_')
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    norm(a) == norm(b)
}

fn name_matches(node: &UiNode, name: &str, how: NameMatch) -> bool {
    let (have, want) = (fold(&node.name), fold(name));
    match how {
        NameMatch::Exact => have == want,
        NameMatch::Contains => have.contains(&want),
    }
}

fn matches(node: &UiNode, selector: &UiSelector) -> bool {
    selector.index.is_none_or(|i| node.index == i)
        && selector
            .name
            .as_deref()
            .is_none_or(|name| name_matches(node, name, selector.name_match))
        && selector
            .role
            .as_deref()
            .is_none_or(|role| role_eq(&node.role, role))
        && selector
            .automation_id
            .as_deref()
            .is_none_or(|id| fold(&node.automation_id) == fold(id))
        && selector
            .runtime_id
            .as_deref()
            .is_none_or(|id| !id.is_empty() && node.runtime_id == id)
}

pub(crate) fn validate_selector(selector: &UiSelector) -> Result<()> {
    if selector.index.is_none()
        && selector.name.is_none()
        && selector.role.is_none()
        && selector.automation_id.is_none()
        && selector.runtime_id.is_none()
    {
        return Err(Error::InvalidArgument(
            "selector needs an index, name, role or automation_id",
        ));
    }
    Ok(())
}

/// Position in `nodes` of the single node matching `selector`.
pub(crate) fn select(nodes: &[UiNode], selector: &UiSelector) -> Result<usize> {
    validate_selector(selector)?;
    let found: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| matches(node, selector))
        .map(|(i, _)| i)
        .collect();
    match found.as_slice() {
        [] => Err(Error::ElementNotFound),
        [only] => Ok(*only),
        many => {
            if let (NameMatch::Contains, Some(name)) =
                (selector.name_match, selector.name.as_deref())
            {
                let exact: Vec<usize> = many
                    .iter()
                    .copied()
                    .filter(|&i| name_matches(&nodes[i], name, NameMatch::Exact))
                    .collect();
                if let [only] = exact.as_slice() {
                    return Ok(*only);
                }
            }
            Err(Error::AmbiguousElement(many.len()))
        }
    }
}

pub(crate) fn validate_options(options: &UiSnapshotOptions) -> Result<()> {
    if options.max_nodes == 0 || options.max_nodes > HARD_MAX_NODES {
        return Err(Error::InvalidArgument("max_nodes must be 1..5000"));
    }
    if options.max_depth > HARD_MAX_DEPTH {
        return Err(Error::InvalidArgument("max_depth must be 0..64"));
    }
    Ok(())
}

fn filtered(options: &UiSnapshotOptions) -> bool {
    options.query.as_deref().is_some_and(|q| !q.is_empty())
        || options.role.as_deref().is_some_and(|r| !r.is_empty())
}

/// Nodes to visit for `options`: `max_nodes`, or more when filtering.
pub(crate) fn walk_limit(options: &UiSnapshotOptions) -> usize {
    if filtered(options) {
        options.max_nodes.clamp(FILTERED_WALK_NODES, HARD_MAX_NODES)
    } else {
        options.max_nodes.min(HARD_MAX_NODES)
    }
}

/// Nodes to visit to resolve `selector`. Breadth-first indexes do not depend
/// on the limits, so an index only needs `index + 1` nodes.
pub(crate) fn resolve_limit(selector: &UiSelector) -> usize {
    selector
        .index
        .map(|i| i.saturating_add(1).min(HARD_MAX_NODES))
        .unwrap_or(RESOLVE_WALK_NODES)
}

fn query_matches(node: &UiNode, query: &str) -> bool {
    let query = fold(query);
    fold(&node.name).contains(&query) || fold(&node.automation_id).contains(&query)
}

/// Reorders breadth-first `nodes` (position == walk index) depth-first,
/// using each node's parent walk index. Children keep their sibling order.
pub(crate) fn tree_order(nodes: Vec<UiNode>, parents: &[Option<usize>]) -> Vec<UiNode> {
    let count = nodes.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut roots = Vec::new();
    for (i, parent) in parents.iter().enumerate().take(count) {
        match parent {
            Some(p) if *p < i => children[*p].push(i),
            _ => roots.push(i),
        }
    }
    // Nodes without a parent entry (should not happen) are kept as roots.
    roots.extend(parents.len()..count);
    let mut order = Vec::with_capacity(count);
    let mut stack: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(i) = stack.pop() {
        order.push(i);
        stack.extend(children[i].iter().rev());
    }
    let mut slots: Vec<Option<UiNode>> = nodes.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|i| slots.get_mut(i).and_then(Option::take))
        .collect()
}

/// Applies the snapshot filters to walked nodes (which keep their walk
/// index) and the `max_nodes` cap.
pub(crate) fn finish(
    nodes: Vec<UiNode>,
    walk_truncated: bool,
    options: &UiSnapshotOptions,
) -> UiSnapshot {
    let query = options.query.as_deref().filter(|q| !q.is_empty());
    let role = options.role.as_deref().filter(|r| !r.is_empty());
    let mut kept: Vec<UiNode> = nodes
        .into_iter()
        .filter(|node| query.is_none_or(|q| query_matches(node, q)))
        .filter(|node| role.is_none_or(|r| role_eq(&node.role, r)))
        .collect();
    let mut truncated = walk_truncated;
    if kept.len() > options.max_nodes {
        kept.truncate(options.max_nodes);
        truncated = true;
    }
    UiSnapshot {
        nodes: kept,
        truncated,
    }
}

/// Rejects an action whose pattern the element does not report.
pub(crate) fn require_pattern(node: &UiNode, action: UiAction) -> Result<()> {
    if !node.enabled {
        return Err(Error::InvalidArgument("element is disabled"));
    }
    let (pattern, message) = match action {
        UiAction::Invoke => (PATTERN_INVOKE, "element does not support InvokePattern"),
        UiAction::Toggle => (PATTERN_TOGGLE, "element does not support TogglePattern"),
        UiAction::Select => (
            PATTERN_SELECT,
            "element does not support SelectionItemPattern",
        ),
        UiAction::Expand | UiAction::Collapse => (
            PATTERN_EXPAND,
            "element does not support ExpandCollapsePattern",
        ),
        UiAction::Focus => return Ok(()),
    };
    if node.patterns.iter().any(|p| p == pattern) {
        Ok(())
    } else {
        Err(Error::InvalidArgument(message))
    }
}

/// Rejects set_value on elements without a usable ValuePattern.
pub(crate) fn require_value(node: &UiNode) -> Result<()> {
    if !node.patterns.iter().any(|p| p == PATTERN_VALUE) {
        return Err(Error::InvalidArgument("element has no ValuePattern"));
    }
    if !node.enabled {
        return Err(Error::InvalidArgument("element is disabled"));
    }
    Ok(())
}

/// The error for platforms or builds without UI Automation.
#[cfg_attr(all(windows, feature = "uia"), allow(dead_code))]
pub(crate) fn unavailable(windows: bool) -> Error {
    if windows {
        Error::FeatureDisabled("uia")
    } else {
        Error::UnsupportedPlatform
    }
}

#[cfg(all(windows, feature = "uia"))]
use crate::uia_win as platform;

#[cfg(not(all(windows, feature = "uia")))]
mod platform {
    use super::*;

    /// Uninhabited: `new` always fails on this platform/build.
    pub(crate) enum Engine {}

    impl Engine {
        pub(crate) fn new() -> Result<Self> {
            Err(unavailable(cfg!(windows)))
        }
        pub(crate) fn walk(
            &mut self,
            _: isize,
            _: usize,
            _: usize,
            _: Duration,
            _: &dyn Fn() -> Result<()>,
        ) -> Result<(Vec<UiNode>, Vec<Option<usize>>, bool)> {
            match *self {}
        }
        pub(crate) fn resolve(
            &mut self,
            _: isize,
            _: UiSelector,
            _: usize,
            _: Duration,
            _: &dyn Fn() -> Result<()>,
        ) -> Result<UiNode> {
            match *self {}
        }
        #[allow(clippy::too_many_arguments)]
        pub(crate) fn act(
            &mut self,
            _: isize,
            _: UiSelector,
            _: usize,
            _: UiAction,
            _: Duration,
            _: &dyn Fn() -> Result<()>,
        ) -> Result<UiNode> {
            match *self {}
        }
        #[allow(clippy::too_many_arguments)]
        pub(crate) fn set_value(
            &mut self,
            _: isize,
            _: UiSelector,
            _: usize,
            _: String,
            _: Duration,
            _: &dyn Fn() -> Result<()>,
        ) -> Result<String> {
            match *self {}
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn node(index: usize, depth: usize, role: &str, name: &str, id: &str) -> UiNode {
        UiNode {
            index,
            depth,
            role: role.into(),
            name: name.into(),
            automation_id: id.into(),
            rect: Rect::new(10, 20, 80, 24),
            enabled: true,
            focused: false,
            offscreen: false,
            patterns: vec![PATTERN_INVOKE.into()],
            value: None,
            runtime_id: vec![42, index as i32],
        }
    }

    fn tree() -> Vec<UiNode> {
        vec![
            node(0, 0, "Window", "Untitled - Notepad", ""),
            node(1, 1, "Button", "Save", "SaveBtn"),
            node(2, 1, "Button", "Save As", "SaveAsBtn"),
            node(3, 1, "MenuItem", "File", "FileMenu"),
            node(4, 2, "Edit", "Text editor", "Editor"),
            node(7, 2, "Button", "Open", ""),
            node(8, 2, "Text", "Open", ""),
        ]
    }

    fn by_name(name: &str, how: NameMatch) -> UiSelector {
        UiSelector {
            name: Some(name.into()),
            name_match: how,
            ..Default::default()
        }
    }

    #[test]
    fn select_exact_and_case_insensitive() {
        let nodes = tree();
        assert_eq!(
            select(&nodes, &by_name("save as", NameMatch::Exact)).unwrap(),
            2
        );
        assert_eq!(
            select(&nodes, &by_name("FILE", NameMatch::Exact)).unwrap(),
            3
        );
        assert!(matches!(
            select(&nodes, &by_name("sav", NameMatch::Exact)),
            Err(Error::ElementNotFound)
        ));
    }

    #[test]
    fn select_contains_prefers_a_single_exact_name() {
        let nodes = tree();
        // "save" is contained in "Save" and "Save As"; only "Save" is exact.
        assert_eq!(
            select(&nodes, &by_name("save", NameMatch::Contains)).unwrap(),
            1
        );
        assert_eq!(
            select(&nodes, &by_name("EDIT", NameMatch::Contains)).unwrap(),
            4
        );
        // "sav" is in both and exact in neither.
        assert!(matches!(
            select(&nodes, &by_name("sav", NameMatch::Contains)),
            Err(Error::AmbiguousElement(2))
        ));
    }

    #[test]
    fn select_by_role_automation_id_and_index() {
        let nodes = tree();
        let open = by_name("open", NameMatch::Exact);
        assert!(matches!(
            select(&nodes, &open),
            Err(Error::AmbiguousElement(2))
        ));
        let open_button = UiSelector {
            role: Some("button".into()),
            ..open.clone()
        };
        assert_eq!(select(&nodes, &open_button).unwrap(), 5);
        let menu = UiSelector {
            role: Some("menu item".into()),
            ..Default::default()
        };
        assert_eq!(select(&nodes, &menu).unwrap(), 3);
        let id = UiSelector {
            automation_id: Some("saveasbtn".into()),
            ..Default::default()
        };
        assert_eq!(select(&nodes, &id).unwrap(), 2);
        // Index is the walk index, not the slice position.
        let index = UiSelector {
            index: Some(8),
            ..Default::default()
        };
        assert_eq!(select(&nodes, &index).unwrap(), 6);
        // Index plus fields verifies the element is still the same.
        let stale = UiSelector {
            index: Some(8),
            role: Some("Button".into()),
            ..Default::default()
        };
        assert!(matches!(
            select(&nodes, &stale),
            Err(Error::ElementNotFound)
        ));
        let missing = UiSelector {
            index: Some(5),
            ..Default::default()
        };
        assert!(matches!(
            select(&nodes, &missing),
            Err(Error::ElementNotFound)
        ));
        let buttons = UiSelector {
            role: Some("Button".into()),
            ..Default::default()
        };
        assert!(matches!(
            select(&nodes, &buttons),
            Err(Error::AmbiguousElement(3))
        ));
    }

    #[test]
    fn runtime_id_pins_against_look_alike_siblings() {
        let mut nodes = tree();
        // A look-alike "Save" button now sits at the old index 1; the
        // original moved to index 9.
        let mut moved = nodes[1].clone();
        moved.index = 9;
        nodes[1].runtime_id = vec![42, 77];
        nodes.push(moved);
        let pinned = UiSelector {
            runtime_id: Some(vec![42, 1]),
            ..Default::default()
        };
        assert_eq!(select(&nodes, &pinned).unwrap(), nodes.len() - 1);
        let gone = UiSelector {
            runtime_id: Some(vec![42, 1234]),
            ..Default::default()
        };
        assert!(matches!(select(&nodes, &gone), Err(Error::ElementNotFound)));
        // An empty RuntimeId never matches (fallback selectors omit it).
        let empty = UiSelector {
            runtime_id: Some(Vec::new()),
            ..Default::default()
        };
        nodes[0].runtime_id.clear();
        assert!(matches!(
            select(&nodes, &empty),
            Err(Error::ElementNotFound)
        ));
    }

    #[test]
    fn empty_selector_is_invalid() {
        assert!(matches!(
            select(&tree(), &UiSelector::default()),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(
            select(&[], &by_name("x", NameMatch::Contains)),
            Err(Error::ElementNotFound)
        ));
    }

    #[test]
    fn to_text_renders_one_line_per_node() {
        let mut save = node(12, 2, "Button", "Save", "SaveBtn");
        save.enabled = false;
        let mut edit = node(13, 3, "Edit", "Body \"quoted\"\nline", "");
        edit.patterns = vec![PATTERN_VALUE.into(), PATTERN_FOCUS.into()];
        edit.value = Some("hello".into());
        edit.focused = true;
        edit.offscreen = true;
        let snapshot = UiSnapshot {
            nodes: vec![save, edit],
            truncated: false,
        };
        assert_eq!(
            snapshot.to_text(),
            "    [12] Button \"Save\" id=SaveBtn (10,20,80,24) [invoke] disabled\n\
             \x20     [13] Edit \"Body \\\"quoted\\\" line\" value=\"hello\" (10,20,80,24) [value,focus] focused offscreen\n"
        );
    }

    #[test]
    fn to_text_notes_truncation_and_clips_long_text() {
        let mut long = node(0, 0, "Document", &"x".repeat(200), "");
        long.patterns.clear();
        let text = UiSnapshot {
            nodes: vec![long],
            truncated: true,
        }
        .to_text();
        let first = text.lines().next().unwrap();
        assert!(first.starts_with(&format!("[0] Document \"{}…\"", "x".repeat(80))));
        assert!(first.ends_with("(10,20,80,24)"));
        assert!(
            text.ends_with(
                "... truncated after 1 nodes; narrow with query/role or raise max_nodes\n"
            )
        );
        assert_eq!(UiSnapshot::default().to_text(), "");
    }

    #[test]
    fn password_values_are_never_visible() {
        assert_eq!(visible_value(true, Some("hunter2".into())), None);
        assert_eq!(
            visible_value(false, Some("abc".into())).as_deref(),
            Some("abc")
        );
        assert_eq!(visible_value(false, None), None);
        let long = visible_value(false, Some("é".repeat(MAX_VALUE_CHARS + 5))).unwrap();
        assert_eq!(long.chars().count(), MAX_VALUE_CHARS);
    }

    #[test]
    fn filters_keep_walk_indexes_and_cap() {
        let options = UiSnapshotOptions {
            query: Some("OPEN".into()),
            ..Default::default()
        };
        let snapshot = finish(tree(), false, &options);
        let indexes: Vec<usize> = snapshot.nodes.iter().map(|n| n.index).collect();
        assert_eq!(indexes, [7, 8]);
        assert!(!snapshot.truncated);
        let by_id = finish(
            tree(),
            false,
            &UiSnapshotOptions {
                query: Some("btn".into()),
                role: Some("BUTTON".into()),
                ..Default::default()
            },
        );
        assert_eq!(by_id.nodes.len(), 2);
        let capped = finish(
            tree(),
            false,
            &UiSnapshotOptions {
                max_nodes: 3,
                ..Default::default()
            },
        );
        assert_eq!(capped.nodes.len(), 3);
        assert!(capped.truncated);
        assert!(finish(tree(), true, &UiSnapshotOptions::default()).truncated);
    }

    #[test]
    fn tree_order_is_depth_first_and_keeps_indexes() {
        // 0 -> (1 -> 3, 4), (2 -> 5)
        let nodes: Vec<UiNode> = (0..6)
            .map(|i| node(i, [0, 1, 1, 2, 2, 2][i], "Pane", "", ""))
            .collect();
        let parents = [None, Some(0), Some(0), Some(1), Some(1), Some(2)];
        let order: Vec<usize> = tree_order(nodes.clone(), &parents)
            .iter()
            .map(|n| n.index)
            .collect();
        assert_eq!(order, [0, 1, 3, 4, 2, 5]);
        // Missing or bogus parents never drop nodes.
        let order: Vec<usize> = tree_order(nodes, &[None, Some(0), Some(9)])
            .iter()
            .map(|n| n.index)
            .collect();
        assert_eq!(order, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn limits() {
        let mut options = UiSnapshotOptions::default();
        assert_eq!(walk_limit(&options), 150);
        options.query = Some(String::new());
        assert_eq!(walk_limit(&options), 150);
        options.role = Some("Button".into());
        assert_eq!(walk_limit(&options), FILTERED_WALK_NODES);
        options.max_nodes = 0;
        assert!(matches!(
            validate_options(&options),
            Err(Error::InvalidArgument(_))
        ));
        options.max_nodes = 10;
        options.max_depth = 65;
        assert!(matches!(
            validate_options(&options),
            Err(Error::InvalidArgument(_))
        ));
        assert_eq!(
            resolve_limit(&UiSelector {
                index: Some(9),
                ..Default::default()
            }),
            10
        );
        assert_eq!(
            resolve_limit(&by_name("x", NameMatch::Exact)),
            RESOLVE_WALK_NODES
        );
        assert_eq!(
            resolve_limit(&UiSelector {
                index: Some(usize::MAX),
                ..Default::default()
            }),
            HARD_MAX_NODES
        );
    }

    #[test]
    fn roles_and_pattern_requirements() {
        assert_eq!(role_name(50000), "Button");
        assert_eq!(role_name(50004), "Edit");
        assert_eq!(role_name(50011), "MenuItem");
        assert_eq!(role_name(50032), "Window");
        assert_eq!(role_name(50040), "AppBar");
        assert_eq!(role_name(50041), "Unknown");
        assert_eq!(role_name(0), "Unknown");
        assert_eq!(role_name(i32::MIN), "Unknown");
        let mut n = node(1, 1, "Button", "OK", "");
        assert!(require_pattern(&n, UiAction::Invoke).is_ok());
        assert!(require_pattern(&n, UiAction::Focus).is_ok());
        assert!(matches!(
            require_pattern(&n, UiAction::Toggle),
            Err(Error::InvalidArgument(
                "element does not support TogglePattern"
            ))
        ));
        assert!(matches!(
            require_value(&n),
            Err(Error::InvalidArgument("element has no ValuePattern"))
        ));
        n.enabled = false;
        assert!(matches!(
            require_pattern(&n, UiAction::Invoke),
            Err(Error::InvalidArgument("element is disabled"))
        ));
        n.patterns.push(PATTERN_VALUE.into());
        assert!(matches!(
            require_value(&n),
            Err(Error::InvalidArgument("element is disabled"))
        ));
    }

    #[test]
    fn unavailable_error_types() {
        assert!(matches!(unavailable(true), Error::FeatureDisabled("uia")));
        assert!(matches!(unavailable(false), Error::UnsupportedPlatform));
    }

    #[cfg(not(all(windows, feature = "uia")))]
    #[test]
    fn new_fails_without_windows_uia() {
        let error = UiAutomation::new().unwrap_err();
        if cfg!(windows) {
            assert!(matches!(error, Error::FeatureDisabled("uia")));
        } else {
            assert!(matches!(error, Error::UnsupportedPlatform));
        }
    }

    #[test]
    fn clip_and_cap() {
        assert_eq!(clip("a\tb\\c\u{7}", 10), "a b\\\\c");
        assert_eq!(clip("abcdef", 3), "abc…");
        assert_eq!(cap("abcdef".into(), 3), "abc");
        assert_eq!(cap("ab".into(), 3), "ab");
    }
}
