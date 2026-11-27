//! Visual change wait (SPEC section 7.3, 21): `wait_change`.
//!
//! Read-only: it captures frames and compares them, it never sends input.
//! A timeout is a normal result (`changed:false`), not an error.
use super::errors::invalid_arguments;
use super::input::sleep_sliced;
use super::session::Session;
use crate::cli::{active, capture_region, screen_region};
use autogui::{Display, Error, Rect, Result, Screenshot, Window};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Region {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    target: Option<String>,
    display: Option<String>,
    region: Option<Region>,
    threshold: Option<f64>,
    tolerance: Option<u32>,
    timeout: Option<f64>,
    settle_ms: Option<u64>,
}

/// Polling parameters, separate from I/O so the loop is testable.
struct Poll {
    threshold: f64,
    tolerance: u8,
    timeout: Duration,
    settle: Duration,
    interval: Duration,
}

#[derive(Debug, PartialEq)]
struct Outcome {
    changed: bool,
    /// Ratio that triggered the change, or the largest ratio seen.
    ratio: f64,
    bounds: Option<Rect>,
    elapsed_ms: u128,
}

/// Takes a baseline (after `settle`), then compares a new frame every
/// `interval` until the diff ratio reaches `threshold` (and is above zero) or
/// `timeout` passes. `check` runs before every slice of every wait.
fn poll_change(
    frame: &mut dyn FnMut() -> Result<Screenshot>,
    p: &Poll,
    check: &dyn Fn() -> Result<()>,
) -> Result<Outcome> {
    check()?;
    sleep_sliced(p.settle, check)?;
    let baseline = frame()?;
    let start = Instant::now();
    let mut max_ratio = 0.0f64;
    loop {
        let remaining = p.timeout.saturating_sub(start.elapsed());
        sleep_sliced(p.interval.min(remaining), check)?;
        let diff = frame()?.diff(&baseline, p.tolerance)?;
        max_ratio = max_ratio.max(diff.ratio);
        if diff.ratio > 0.0 && diff.ratio >= p.threshold {
            return Ok(Outcome {
                changed: true,
                ratio: diff.ratio,
                bounds: diff.bounds,
                elapsed_ms: start.elapsed().as_millis(),
            });
        }
        if start.elapsed() >= p.timeout {
            return Ok(Outcome {
                changed: false,
                ratio: max_ratio,
                bounds: None,
                elapsed_ms: start.elapsed().as_millis(),
            });
        }
    }
}

fn parse(args: Value) -> Result<(Args, Poll)> {
    let a: Args = serde_json::from_value(args).map_err(invalid_arguments)?;
    if a.target.is_some() == a.display.is_some() {
        return Err(Error::InvalidArgument(
            "supply exactly one of target or display",
        ));
    }
    let threshold = a.threshold.unwrap_or(0.01);
    if !(0.0..=1.0).contains(&threshold) {
        return Err(Error::InvalidArgument("threshold must be 0..1"));
    }
    let tolerance = u8::try_from(a.tolerance.unwrap_or(8))
        .map_err(|_| Error::InvalidArgument("tolerance must be 0..255"))?;
    let timeout = a.timeout.unwrap_or(5.0);
    if !(0.0..=30.0).contains(&timeout) {
        return Err(Error::InvalidArgument("timeout must be 0..30 seconds"));
    }
    let settle = a.settle_ms.unwrap_or(0);
    if settle > 2000 {
        return Err(Error::InvalidArgument("settle_ms must be 0..2000"));
    }
    if let Some(r) = &a.region
        && (r.width <= 0 || r.height <= 0)
    {
        return Err(Error::InvalidArgument("region must have positive size"));
    }
    let poll = Poll {
        threshold,
        tolerance,
        timeout: Duration::from_secs_f64(timeout),
        settle: Duration::from_millis(settle),
        interval: Duration::from_millis(100),
    };
    Ok((a, poll))
}

enum Source {
    Window(Window),
    Screen(Display),
}

/// Runs the MCP tool `name` (wait_change).
pub(super) fn call(
    session: &mut Session,
    _name: &str,
    args: Value,
    check: &dyn Fn() -> Result<()>,
) -> Result<Value> {
    let (a, poll) = parse(args)?;
    let source = match (&a.target, &a.display) {
        (Some(key), _) => Source::Window(session.target(key)?),
        (None, Some(key)) => Source::Screen(session.display(key)?),
        (None, None) => return Err(Error::InvalidArgument("supply target or display")),
    };
    let crop = a
        .region
        .as_ref()
        .map(|r| Rect::new(r.x, r.y, r.width, r.height));
    let capture = |session: &mut Session| -> Result<Screenshot> {
        let gui = session.gui()?;
        let frame = match &source {
            Source::Window(window) => {
                active(window)?;
                let region = screen_region(gui, window, None)?;
                capture_region(gui, None, region)?
            }
            Source::Screen(display) => {
                display.validate()?;
                capture_region(gui, Some(display), display.bounds())?
            }
        };
        match crop {
            Some(rect) => frame.region(rect),
            None => Ok(frame),
        }
    };
    let outcome = {
        let mut frame = || capture(session);
        poll_change(&mut frame, &poll, check)?
    };
    let bounds = outcome
        .bounds
        .map(|b| json!({"left":b.left,"top":b.top,"width":b.width,"height":b.height}));
    let result = json!({
        "changed": outcome.changed,
        "ratio": outcome.ratio,
        "bounds": bounds,
        "elapsed_ms": outcome.elapsed_ms as u64,
    });
    Ok(
        json!({"content":[{"type":"text","text":result.to_string()}],"structuredContent":result,"isError":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use autogui::Rgb;
    use std::cell::Cell;

    fn quick(timeout_ms: u64, threshold: f64) -> Poll {
        Poll {
            threshold,
            tolerance: 0,
            timeout: Duration::from_millis(timeout_ms),
            settle: Duration::ZERO,
            interval: Duration::from_millis(5),
        }
    }

    fn grey(v: u8) -> Screenshot {
        Screenshot::solid(10, 10, Rgb::new(v, v, v))
    }

    #[test]
    fn reports_change_with_ratio_and_bounds() {
        let n = Cell::new(0);
        let mut frame = || {
            n.set(n.get() + 1);
            if n.get() < 3 {
                return Ok(grey(0));
            }
            let mut f = grey(0);
            f.pixels[0..3].copy_from_slice(&[255, 255, 255]);
            Ok(f)
        };
        let out = poll_change(&mut frame, &quick(1000, 0.01), &|| Ok(())).unwrap();
        assert!(out.changed);
        assert!((out.ratio - 0.01).abs() < 1e-12);
        assert_eq!(out.bounds, Some(Rect::new(0, 0, 1, 1)));
    }

    #[test]
    fn timeout_is_not_an_error_and_keeps_max_ratio() {
        let mut frame = || Ok(grey(0));
        let out = poll_change(&mut frame, &quick(30, 0.5), &|| Ok(())).unwrap();
        assert!(!out.changed);
        assert_eq!(out.ratio, 0.0);
        assert_eq!(out.bounds, None);
    }

    #[test]
    fn below_threshold_change_keeps_waiting() {
        let n = Cell::new(0);
        let mut frame = || {
            n.set(n.get() + 1);
            let mut f = grey(0);
            if n.get() > 1 {
                f.pixels[0..3].copy_from_slice(&[255, 255, 255]);
            }
            Ok(f)
        };
        let out = poll_change(&mut frame, &quick(30, 0.5), &|| Ok(())).unwrap();
        assert!(!out.changed);
        assert!((out.ratio - 0.01).abs() < 1e-12);
    }

    #[test]
    fn check_error_cancels_the_wait() {
        let mut frame = || Ok(grey(0));
        let calls = Cell::new(0);
        let check = || {
            calls.set(calls.get() + 1);
            if calls.get() > 3 {
                Err(Error::Stopped)
            } else {
                Ok(())
            }
        };
        let out = poll_change(&mut frame, &quick(5000, 0.5), &check);
        assert!(matches!(out, Err(Error::Stopped)));
    }

    #[test]
    fn argument_validation() {
        assert!(parse(json!({})).is_err());
        assert!(parse(json!({"target":"w1-0","display":"d1-0"})).is_err());
        assert!(parse(json!({"target":"w1-0","threshold":2})).is_err());
        assert!(parse(json!({"target":"w1-0","tolerance":300})).is_err());
        assert!(parse(json!({"target":"w1-0","timeout":31})).is_err());
        assert!(parse(json!({"target":"w1-0","settle_ms":2001})).is_err());
        assert!(
            parse(json!({"target":"w1-0","region":{"x":0,"y":0,"width":0,"height":1}})).is_err()
        );
        assert!(parse(json!({"target":"w1-0","bogus":1})).is_err());
        assert!(parse(json!({"display":"d1-0","timeout":0})).is_ok());
    }
}
