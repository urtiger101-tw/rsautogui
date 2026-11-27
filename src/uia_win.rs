//! UI Automation over COM (Windows, feature `uia`; SPEC §18).
//!
//! One MTA worker thread owns every COM object. Callers send boxed jobs over
//! a channel and wait with a timeout; a worker that does not answer in time
//! is abandoned (never joined) and the next call starts a new one. Only plain
//! data (`UiNode`, `String`, errors) crosses back.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VT_BSTR, VariantClear};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest,
    IUIAutomationElement, IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern,
    IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern, IUIAutomationValuePattern,
    TreeScope, TreeScope_Children, TreeScope_Element, UIA_AutomationIdPropertyId,
    UIA_BoundingRectanglePropertyId, UIA_ControlTypePropertyId, UIA_E_ELEMENTNOTAVAILABLE,
    UIA_E_ELEMENTNOTENABLED, UIA_E_TIMEOUT, UIA_ExpandCollapsePatternId,
    UIA_HasKeyboardFocusPropertyId, UIA_InvokePatternId, UIA_IsEnabledPropertyId,
    UIA_IsExpandCollapsePatternAvailablePropertyId, UIA_IsInvokePatternAvailablePropertyId,
    UIA_IsKeyboardFocusablePropertyId, UIA_IsOffscreenPropertyId, UIA_IsPasswordPropertyId,
    UIA_IsSelectionItemPatternAvailablePropertyId, UIA_IsTogglePatternAvailablePropertyId,
    UIA_IsValuePatternAvailablePropertyId, UIA_NamePropertyId, UIA_PROPERTY_ID,
    UIA_SelectionItemPatternId, UIA_TogglePatternId, UIA_ValuePatternId, UIA_ValueValuePropertyId,
};
use windows::core::{BSTR, Interface};

use crate::error::{Error, Result};
use crate::types::Rect;
use crate::uia::{
    HARD_MAX_DEPTH, MAX_NAME_CHARS, PATTERN_EXPAND, PATTERN_FOCUS, PATTERN_INVOKE, PATTERN_SELECT,
    PATTERN_TOGGLE, PATTERN_VALUE, UiAction, UiNode, UiSelector, cap, require_pattern,
    require_value, role_name, select, visible_value,
};

/// Longest wait for one request (a walk, or a resolve plus an action).
const CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest wait for a new worker to initialize COM and UI Automation.
const INIT_TIMEOUT: Duration = Duration::from_secs(10);
/// A walk stops expanding (and reports truncation) after this long.
const WALK_BUDGET: Duration = Duration::from_secs(8);
/// IUIAutomation2 timeouts, in milliseconds.
const CONNECTION_TIMEOUT_MS: u32 = 2000;
const TRANSACTION_TIMEOUT_MS: u32 = 3000;

type Job = Box<dyn FnOnce(&Com) + Send + 'static>;

/// Handle to the worker thread. Holds no COM objects itself.
pub(crate) struct Engine {
    jobs: Option<mpsc::Sender<Job>>,
}

impl Engine {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            jobs: Some(spawn()?),
        })
    }

    /// Runs `job` on the worker and waits up to [`CALL_TIMEOUT`].
    fn run<T: Send + 'static>(
        &mut self,
        job: impl FnOnce(&Com) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let sender = match &self.jobs {
            Some(sender) => sender.clone(),
            None => {
                let sender = spawn()?;
                self.jobs = Some(sender.clone());
                sender
            }
        };
        let (reply, answer) = mpsc::sync_channel(1);
        let boxed: Job = Box::new(move |com: &Com| {
            let _ = reply.send(job(com));
        });
        if sender.send(boxed).is_err() {
            self.jobs = None;
            return Err(Error::Uia("worker thread exited; retry".into()));
        }
        match answer.recv_timeout(CALL_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Abandon the stuck worker: dropping the sender lets it exit
                // once its COM call returns. It is never joined.
                self.jobs = None;
                Err(Error::Uia(format!(
                    "no response within {} s; the app may be hung",
                    CALL_TIMEOUT.as_secs()
                )))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.jobs = None;
                Err(Error::Uia("worker thread exited; retry".into()))
            }
        }
    }

    pub(crate) fn walk(
        &mut self,
        hwnd: isize,
        max_nodes: usize,
        max_depth: usize,
    ) -> Result<(Vec<UiNode>, Vec<Option<usize>>, bool)> {
        self.run(move |com| {
            let walked = walk(com, hwnd, max_nodes, max_depth)?;
            Ok((walked.nodes, walked.parents, walked.truncated))
        })
    }

    pub(crate) fn resolve(
        &mut self,
        hwnd: isize,
        selector: UiSelector,
        limit: usize,
    ) -> Result<UiNode> {
        self.run(move |com| {
            let walked = walk(com, hwnd, limit, HARD_MAX_DEPTH)?;
            let (node, _, _) = walked.pick(&selector)?;
            Ok(node.clone())
        })
    }

    pub(crate) fn act(
        &mut self,
        hwnd: isize,
        selector: UiSelector,
        limit: usize,
        action: UiAction,
    ) -> Result<UiNode> {
        self.run(move |com| {
            let walked = walk(com, hwnd, limit, HARD_MAX_DEPTH)?;
            let (node, element, _) = walked.pick(&selector)?;
            require_pattern(node, action)?;
            perform(element, action)?;
            Ok(node.clone())
        })
    }

    pub(crate) fn set_value(
        &mut self,
        hwnd: isize,
        selector: UiSelector,
        limit: usize,
        text: String,
    ) -> Result<String> {
        self.run(move |com| {
            let walked = walk(com, hwnd, limit, HARD_MAX_DEPTH)?;
            let (node, element, password) = walked.pick(&selector)?;
            require_value(node)?;
            unsafe {
                let pattern: IUIAutomationValuePattern = element
                    .GetCurrentPatternAs(UIA_ValuePatternId)
                    .map_err(|e| com_error("ValuePattern", &e))?;
                pattern
                    .SetValue(&BSTR::from(text.as_str()))
                    .map_err(|e| com_error("ValuePattern.SetValue", &e))?;
                if password {
                    return Ok(String::new());
                }
                Ok(pattern
                    .CurrentValue()
                    .map_err(|e| com_error("ValuePattern.CurrentValue", &e))?
                    .to_string())
            }
        })
    }
}

/// Starts a worker and waits until UI Automation is ready on it.
fn spawn() -> Result<mpsc::Sender<Job>> {
    let (jobs, inbox) = mpsc::channel::<Job>();
    let (ready, started) = mpsc::sync_channel::<Result<()>>(1);
    std::thread::Builder::new()
        .name("autogui-uia".into())
        .spawn(move || worker(inbox, ready))?;
    match started.recv_timeout(INIT_TIMEOUT) {
        Ok(Ok(())) => Ok(jobs),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(Error::Uia(
            "UI Automation did not initialize in time".into(),
        )),
    }
}

/// Balances a successful `CoInitializeEx` on the worker thread.
struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn worker(inbox: mpsc::Receiver<Job>, ready: mpsc::SyncSender<Result<()>>) {
    // Physical pixels for BoundingRectangle, like window rects. This thread
    // is ours, so the guard simply lives as long as the worker.
    let _dpi = crate::dpi::DpiGuard::enter().ok();
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        let _ = ready.send(Err(Error::Uia(format!(
            "CoInitializeEx failed: 0x{:08X}",
            hr.0 as u32
        ))));
        return;
    }
    let _apartment = Apartment;
    // Declared after the apartment, so COM objects drop before CoUninitialize.
    let com = match Com::new() {
        Ok(com) => com,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    while let Ok(job) = inbox.recv() {
        job(&com);
    }
}

/// COM state that never leaves the worker thread.
pub(crate) struct Com {
    automation: IUIAutomation,
    /// Element properties only.
    props: IUIAutomationCacheRequest,
    /// Element plus control-view children, with the same properties.
    children: IUIAutomationCacheRequest,
}

const PROPERTIES: [UIA_PROPERTY_ID; 15] = [
    UIA_ControlTypePropertyId,
    UIA_NamePropertyId,
    UIA_AutomationIdPropertyId,
    UIA_BoundingRectanglePropertyId,
    UIA_IsEnabledPropertyId,
    UIA_HasKeyboardFocusPropertyId,
    UIA_IsKeyboardFocusablePropertyId,
    UIA_IsOffscreenPropertyId,
    UIA_IsPasswordPropertyId,
    UIA_IsInvokePatternAvailablePropertyId,
    UIA_IsTogglePatternAvailablePropertyId,
    UIA_IsSelectionItemPatternAvailablePropertyId,
    UIA_IsExpandCollapsePatternAvailablePropertyId,
    UIA_IsValuePatternAvailablePropertyId,
    UIA_ValueValuePropertyId,
];

impl Com {
    fn new() -> Result<Self> {
        unsafe {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                    .or_else(|_| CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER))
                    .map_err(|e| com_error("CoCreateInstance(CUIAutomation)", &e))?;
            if let Ok(automation2) = automation.cast::<IUIAutomation2>() {
                // Best effort: older systems keep their default timeouts.
                let _ = automation2.SetConnectionTimeout(CONNECTION_TIMEOUT_MS);
                let _ = automation2.SetTransactionTimeout(TRANSACTION_TIMEOUT_MS);
            }
            let props = cache_request(&automation, TreeScope_Element)?;
            let children = cache_request(
                &automation,
                TreeScope(TreeScope_Element.0 | TreeScope_Children.0),
            )?;
            Ok(Self {
                automation,
                props,
                children,
            })
        }
    }
}

fn cache_request(
    automation: &IUIAutomation,
    scope: TreeScope,
) -> Result<IUIAutomationCacheRequest> {
    unsafe {
        let condition = automation
            .ControlViewCondition()
            .map_err(|e| com_error("ControlViewCondition", &e))?;
        let request = automation
            .CreateCacheRequest()
            .map_err(|e| com_error("CreateCacheRequest", &e))?;
        for property in PROPERTIES {
            request
                .AddProperty(property)
                .map_err(|e| com_error("CacheRequest.AddProperty", &e))?;
        }
        request
            .SetTreeScope(scope)
            .map_err(|e| com_error("CacheRequest.SetTreeScope", &e))?;
        request
            .SetTreeFilter(&condition)
            .map_err(|e| com_error("CacheRequest.SetTreeFilter", &e))?;
        Ok(request)
    }
}

/// Maps a COM failure; context is a static API name, never app text.
fn com_error(context: &str, error: &windows::core::Error) -> Error {
    let code = error.code().0 as u32;
    match code {
        UIA_E_ELEMENTNOTAVAILABLE => Error::ElementNotFound,
        UIA_E_ELEMENTNOTENABLED => Error::InvalidArgument("element is disabled"),
        UIA_E_TIMEOUT => Error::Uia(format!("{context}: the app did not respond in time")),
        _ => Error::Uia(format!(
            "{context} failed: 0x{code:08X} {}",
            error.message()
        )),
    }
}

/// Reads and frees a VARIANT.
struct Variant(VARIANT);

impl Variant {
    fn bool(&self) -> bool {
        unsafe {
            let inner = &self.0.Anonymous.Anonymous;
            inner.vt == VT_BOOL && inner.Anonymous.boolVal.0 != 0
        }
    }
    fn string(&self) -> Option<String> {
        unsafe {
            let inner = &self.0.Anonymous.Anonymous;
            (inner.vt == VT_BSTR).then(|| inner.Anonymous.bstrVal.to_string())
        }
    }
}

impl Drop for Variant {
    fn drop(&mut self) {
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}

fn cached(element: &IUIAutomationElement, property: UIA_PROPERTY_ID) -> Option<Variant> {
    unsafe { element.GetCachedPropertyValue(property) }
        .ok()
        .map(Variant)
}

fn cached_bool(element: &IUIAutomationElement, property: UIA_PROPERTY_ID) -> bool {
    cached(element, property).is_some_and(|v| v.bool())
}

/// The node for a cached element, and whether it is a password field.
fn read_node(element: &IUIAutomationElement, index: usize, depth: usize) -> (UiNode, bool) {
    unsafe {
        let control = element.CachedControlType().map(|c| c.0).unwrap_or(0);
        let name = element
            .CachedName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let automation_id = element
            .CachedAutomationId()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let r: RECT = element.CachedBoundingRectangle().unwrap_or_default();
        let enabled = element.CachedIsEnabled().is_ok_and(|b| b.as_bool());
        let focused = element.CachedHasKeyboardFocus().is_ok_and(|b| b.as_bool());
        let focusable = element
            .CachedIsKeyboardFocusable()
            .is_ok_and(|b| b.as_bool());
        let offscreen = element.CachedIsOffscreen().is_ok_and(|b| b.as_bool());
        let password = element.CachedIsPassword().is_ok_and(|b| b.as_bool());
        let mut patterns = Vec::new();
        for (property, pattern) in [
            (UIA_IsInvokePatternAvailablePropertyId, PATTERN_INVOKE),
            (UIA_IsTogglePatternAvailablePropertyId, PATTERN_TOGGLE),
            (
                UIA_IsSelectionItemPatternAvailablePropertyId,
                PATTERN_SELECT,
            ),
            (
                UIA_IsExpandCollapsePatternAvailablePropertyId,
                PATTERN_EXPAND,
            ),
            (UIA_IsValuePatternAvailablePropertyId, PATTERN_VALUE),
        ] {
            if cached_bool(element, property) {
                patterns.push(pattern.to_owned());
            }
        }
        if focusable {
            patterns.push(PATTERN_FOCUS.to_owned());
        }
        let raw_value = if patterns.iter().any(|p| p == PATTERN_VALUE) && !password {
            Some(
                cached(element, UIA_ValueValuePropertyId)
                    .and_then(|v| v.string())
                    .unwrap_or_default(),
            )
        } else {
            None
        };
        let node = UiNode {
            index,
            depth,
            role: role_name(control).to_owned(),
            name: cap(name, MAX_NAME_CHARS),
            automation_id: cap(automation_id, MAX_NAME_CHARS),
            rect: Rect::new(
                r.left,
                r.top,
                r.right.saturating_sub(r.left).max(0),
                r.bottom.saturating_sub(r.top).max(0),
            ),
            enabled,
            focused,
            offscreen,
            patterns,
            value: visible_value(password, raw_value),
        };
        (node, password)
    }
}

/// Control-view children of `element`, each with cached properties.
fn children(com: &Com, element: &IUIAutomationElement) -> Result<Vec<IUIAutomationElement>> {
    unsafe {
        let updated = element
            .BuildUpdatedCache(&com.children)
            .map_err(|e| com_error("BuildUpdatedCache", &e))?;
        let array = match updated.GetCachedChildren() {
            Ok(array) => array,
            // S_OK with a null array: no children.
            Err(e) if e.code().is_ok() => return Ok(Vec::new()),
            Err(e) => return Err(com_error("GetCachedChildren", &e)),
        };
        let length = array.Length().map_err(|e| com_error("Length", &e))?;
        Ok((0..length)
            .filter_map(|i| array.GetElement(i).ok())
            .collect())
    }
}

struct Walked {
    nodes: Vec<UiNode>,
    /// Walk index of each node's parent (`None` for the window).
    parents: Vec<Option<usize>>,
    passwords: Vec<bool>,
    elements: Vec<IUIAutomationElement>,
    truncated: bool,
}

impl Walked {
    fn push(&mut self, element: IUIAutomationElement, depth: usize, parent: Option<usize>) {
        let (node, password) = read_node(&element, self.nodes.len(), depth);
        self.nodes.push(node);
        self.parents.push(parent);
        self.passwords.push(password);
        self.elements.push(element);
    }

    fn pick(&self, selector: &UiSelector) -> Result<(&UiNode, &IUIAutomationElement, bool)> {
        let at = select(&self.nodes, selector)?;
        match (
            self.nodes.get(at),
            self.elements.get(at),
            self.passwords.get(at),
        ) {
            (Some(node), Some(element), Some(password)) => Ok((node, element, *password)),
            _ => Err(Error::ElementNotFound),
        }
    }
}

/// Breadth-first, level by level, over the control view: node indexes are
/// stable whatever the limits. Never uses FindAll(TreeScope_Subtree).
fn walk(com: &Com, hwnd: isize, max_nodes: usize, max_depth: usize) -> Result<Walked> {
    let started = Instant::now();
    let root = unsafe {
        com.automation
            .ElementFromHandleBuildCache(HWND(hwnd as *mut core::ffi::c_void), &com.props)
    }
    .map_err(|e| match com_error("ElementFromHandle", &e) {
        Error::ElementNotFound => Error::WindowNotFound,
        other => other,
    })?;
    let mut walked = Walked {
        nodes: Vec::new(),
        parents: Vec::new(),
        passwords: Vec::new(),
        elements: Vec::new(),
        truncated: false,
    };
    walked.push(root, 0, None);
    let mut level_start = 0;
    'walk: for depth in 1..=max_depth.min(HARD_MAX_DEPTH) {
        let level_end = walked.nodes.len();
        if level_start == level_end {
            break;
        }
        for parent in level_start..level_end {
            if walked.nodes.len() >= max_nodes || started.elapsed() > WALK_BUDGET {
                walked.truncated = true;
                break 'walk;
            }
            let Some(element) = walked.elements.get(parent).cloned() else {
                break 'walk;
            };
            let kids = match children(com, &element) {
                Ok(kids) => kids,
                // The element vanished or refused: skip its subtree.
                Err(Error::ElementNotFound | Error::InvalidArgument(_)) => continue,
                Err(Error::Uia(message)) if !message.contains("did not respond") => continue,
                Err(error) => return Err(error),
            };
            for kid in kids {
                if walked.nodes.len() >= max_nodes {
                    walked.truncated = true;
                    break 'walk;
                }
                walked.push(kid, depth, Some(parent));
            }
        }
        level_start = level_end;
    }
    Ok(walked)
}

fn perform(element: &IUIAutomationElement, action: UiAction) -> Result<()> {
    unsafe {
        match action {
            UiAction::Invoke => element
                .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                .and_then(|p| p.Invoke())
                .map_err(|e| com_error("InvokePattern.Invoke", &e)),
            UiAction::Toggle => element
                .GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                .and_then(|p| p.Toggle())
                .map_err(|e| com_error("TogglePattern.Toggle", &e)),
            UiAction::Select => element
                .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                    UIA_SelectionItemPatternId,
                )
                .and_then(|p| p.Select())
                .map_err(|e| com_error("SelectionItemPattern.Select", &e)),
            UiAction::Expand => element
                .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                    UIA_ExpandCollapsePatternId,
                )
                .and_then(|p| p.Expand())
                .map_err(|e| com_error("ExpandCollapsePattern.Expand", &e)),
            UiAction::Collapse => element
                .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                    UIA_ExpandCollapsePatternId,
                )
                .and_then(|p| p.Collapse())
                .map_err(|e| com_error("ExpandCollapsePattern.Collapse", &e)),
            UiAction::Focus => element.SetFocus().map_err(|e| com_error("SetFocus", &e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn com_errors_map_to_typed_errors() {
        let gone = windows::core::Error::from_hresult(windows::core::HRESULT(
            UIA_E_ELEMENTNOTAVAILABLE as i32,
        ));
        assert!(matches!(com_error("x", &gone), Error::ElementNotFound));
        let disabled = windows::core::Error::from_hresult(windows::core::HRESULT(
            UIA_E_ELEMENTNOTENABLED as i32,
        ));
        assert!(matches!(
            com_error("x", &disabled),
            Error::InvalidArgument("element is disabled")
        ));
        let hung = windows::core::Error::from_hresult(windows::core::HRESULT(UIA_E_TIMEOUT as i32));
        assert!(matches!(com_error("Walk", &hung), Error::Uia(m) if m.contains("did not respond")));
        let other =
            windows::core::Error::from_hresult(windows::core::HRESULT(0x8000_4005_u32 as i32));
        assert!(
            matches!(com_error("Walk", &other), Error::Uia(m) if m.starts_with("Walk failed: 0x80004005"))
        );
    }

    #[test]
    fn variant_reads_bool_and_string() {
        let mut v = VARIANT::default();
        unsafe {
            let inner = &mut *v.Anonymous.Anonymous;
            inner.vt = VT_BOOL;
            inner.Anonymous.boolVal = windows::Win32::Foundation::VARIANT_BOOL(-1);
        }
        let v = Variant(v);
        assert!(v.bool());
        assert_eq!(v.string(), None);
        assert!(!Variant(VARIANT::default()).bool());
    }
}
