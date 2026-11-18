//! Capture-bound pointer input and bounded Unicode text batches.
use super::{active, capture_region, screen_region};
use autogui::{AutoGui, Display, Error, MouseButton, Point, Rect, Result, Screenshot, Window};
use std::time::{Duration, Instant};

/// Single-use proof of what the agent observed. A window capture binds a
/// target (and optional display); a display capture has no target and binds
/// only its display, with bounds equal to the captured region.
pub(super) struct CaptureProof {
    pub id: String,
    pub target: Option<String>,
    pub display: Option<String>,
    pub bounds: Rect,
    pub region: Rect,
    pub frame: Screenshot,
    pub taken: Instant,
}

impl CaptureProof {
    fn fresh(&self) -> Result<()> {
        if self.taken.elapsed() >= Duration::from_secs(60) {
            return Err(Error::InvalidArgument("capture expired; capture again"));
        }
        Ok(())
    }

    /// Maps an image-relative point to desktop pixels. Exactly one of target
    /// (window proof) or display (display_capture proof) must be supplied.
    pub fn point(
        &self,
        target: Option<&str>,
        display: Option<&str>,
        id: &str,
        x: i32,
        y: i32,
    ) -> Result<Point> {
        let bound = match (self.target.as_deref(), target, display) {
            (_, Some(_), Some(_)) | (_, None, None) => {
                return Err(Error::InvalidArgument(
                    "supply exactly one of target (window capture) or display (display_capture)",
                ));
            }
            (Some(own), Some(target), None) => own == target,
            (None, None, Some(display)) => self.display.as_deref() == Some(display),
            _ => false,
        };
        if !bound || self.id != id {
            return Err(Error::InvalidArgument(
                "capture does not match target/display/id; capture again",
            ));
        }
        self.fresh()?;
        if !Rect::new(0, 0, self.region.width, self.region.height).contains(x, y) {
            return Err(Error::InvalidArgument(
                "point must be inside the captured image",
            ));
        }
        let x = self.region.left.checked_add(x);
        let y = self.region.top.checked_add(y);
        match (x, y) {
            (Some(x), Some(y)) if self.bounds.contains(x, y) => Ok(Point::new(x, y)),
            _ => Err(Error::InvalidArgument(
                "point outside captured window or coordinate overflow",
            )),
        }
    }

    pub fn patch(&self, point: Point) -> Result<(Rect, Screenshot)> {
        let x = point.x - self.region.left;
        let y = point.y - self.region.top;
        let left = (x - 16).max(0);
        let top = (y - 16).max(0);
        let right = (i64::from(x) + 16).min(i64::from(self.region.width)) as i32;
        let bottom = (i64::from(y) + 16).min(i64::from(self.region.height)) as i32;
        let local = Rect::new(left, top, right - left, bottom - top);
        let frame = self.frame.region(local)?;
        Ok((
            Rect::new(
                self.region.left + left,
                self.region.top + top,
                local.width,
                local.height,
            ),
            frame,
        ))
    }

    pub fn verify(
        &self,
        gui: &mut AutoGui,
        window: Option<&Window>,
        display: Option<&Display>,
        point: Point,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        check()?;
        self.fresh()?;
        if let Some(display) = display {
            display.validate()?;
        }
        // Display-level proofs have no window; foreground/geometry checks apply
        // only to window proofs. Freshness, display and pixels apply to both.
        if let Some(window) = window {
            active(window)?;
            if window.box_rect()? != self.bounds
                || screen_region(gui, window, display)? != self.region
            {
                return Err(Error::InvalidArgument(
                    "window/display geometry changed; capture again",
                ));
            }
        }
        let (region, expected) = self.patch(point)?;
        if capture_region(gui, display, region)? != expected {
            return Err(Error::InvalidArgument(
                "pixels or hover changed; cursor may be positioned; capture again before retrying",
            ));
        }
        check()?;
        self.fresh()?;
        if let Some(window) = window {
            active(window)?;
            if window.box_rect()? != self.bounds {
                return Err(Error::InvalidArgument(
                    "window moved before input; capture again",
                ));
            }
        }
        if let Some(display) = display {
            display.validate()?;
        }
        Ok(())
    }
}

/// Parses the optional pointer button name; default left.
pub(super) fn button(name: Option<&str>) -> Result<MouseButton> {
    match name.unwrap_or("left") {
        "left" => Ok(MouseButton::Left),
        "right" => Ok(MouseButton::Right),
        "middle" => Ok(MouseButton::Middle),
        _ => Err(Error::InvalidArgument(
            "button must be left, right or middle",
        )),
    }
}

/// Intermediate cursor positions for a stepped drag on an explicit display,
/// ending exactly at `to`. Points lie on the segment, so they stay inside any
/// rectangle containing both endpoints.
pub(super) fn drag_path(from: Point, to: Point, steps: i32) -> Vec<Point> {
    let steps = steps.max(1);
    (1..=steps)
        .map(|i| {
            let lerp = |a: i32, b: i32| {
                (i64::from(a) + (i64::from(b) - i64::from(a)) * i64::from(i) / i64::from(steps))
                    as i32
            };
            Point::new(lerp(from.x, to.x), lerp(from.y, to.y))
        })
        .collect()
}

pub(super) fn type_chunks(text: &str, mut send: impl FnMut(&str) -> Result<()>) -> Result<()> {
    let mut remaining = text;
    while !remaining.is_empty() {
        let end = remaining
            .char_indices()
            .nth(32)
            .map_or(remaining.len(), |(i, _)| i);
        send(&remaining[..end])?;
        remaining = &remaining[end..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use autogui::Rgb;

    fn proof() -> CaptureProof {
        CaptureProof {
            id: "c1".into(),
            target: Some("w1-0".into()),
            display: None,
            bounds: Rect::new(-100, 50, 100, 80),
            region: Rect::new(-100, 50, 100, 80),
            frame: Screenshot::solid(100, 80, Rgb::new(1, 2, 3)),
            taken: Instant::now(),
        }
    }

    #[test]
    fn capture_binding_expiry_and_half_open_coordinates() {
        let mut p = proof();
        let w = Some("w1-0");
        assert_eq!(p.point(w, None, "c1", 0, 0).unwrap(), Point::new(-100, 50));
        assert_eq!(p.point(w, None, "c1", 99, 79).unwrap(), Point::new(-1, 129));
        for (target, id, x, y) in [
            ("w2-0", "c1", 1, 1),
            ("w1-0", "c0", 1, 1),
            ("w1-0", "c1", -1, 0),
            ("w1-0", "c1", 100, 0),
            ("w1-0", "c1", 0, 80),
        ] {
            assert!(matches!(
                p.point(Some(target), None, id, x, y),
                Err(Error::InvalidArgument(_))
            ));
        }
        p.taken = Instant::now() - Duration::from_secs(61);
        assert!(matches!(
            p.point(w, None, "c1", 1, 1),
            Err(Error::InvalidArgument(_))
        ));
        p.taken = Instant::now();
        p.region.left = i32::MAX - 1;
        assert!(matches!(
            p.point(w, None, "c1", 2, 1),
            Err(Error::InvalidArgument(_))
        ));
    }

    fn display_proof() -> CaptureProof {
        CaptureProof {
            id: "c2".into(),
            target: None,
            display: Some("d1-0".into()),
            bounds: Rect::new(1920, -200, 300, 200),
            region: Rect::new(1920, -200, 300, 200),
            frame: Screenshot::solid(300, 200, Rgb::new(1, 2, 3)),
            taken: Instant::now(),
        }
    }

    #[test]
    fn display_proof_binds_display_and_rejects_target_mixups() {
        let d = display_proof();
        assert_eq!(
            d.point(None, Some("d1-0"), "c2", 0, 0).unwrap(),
            Point::new(1920, -200)
        );
        assert_eq!(
            d.point(None, Some("d1-0"), "c2", 299, 199).unwrap(),
            Point::new(2219, -1)
        );
        for (target, display) in [
            (None, Some("d1-1")),         // mismatched display key
            (None, Some("d2-0")),         // stale generation
            (Some("w1-0"), None),         // target-less proof used with a target
            (Some("w1-0"), Some("d1-0")), // both supplied
            (None, None),                 // neither supplied
        ] {
            assert!(matches!(
                d.point(target, display, "c2", 1, 1),
                Err(Error::InvalidArgument(_))
            ));
        }
        assert!(d.point(None, Some("d1-0"), "c2", 300, 0).is_err());
        let w = proof();
        for (target, display) in [
            (None, Some("d1-0")),         // targeted proof used target-less
            (Some("w1-0"), Some("d1-0")), // both
            (None, None),
        ] {
            assert!(matches!(
                w.point(target, display, "c1", 1, 1),
                Err(Error::InvalidArgument(_))
            ));
        }
        let mut expired = display_proof();
        expired.taken = Instant::now() - Duration::from_secs(61);
        assert!(expired.point(None, Some("d1-0"), "c2", 1, 1).is_err());
        let point = d.point(None, Some("d1-0"), "c2", 150, 100).unwrap();
        let (region, patch) = d.patch(point).unwrap();
        assert_eq!((region.width, region.height), (32, 32));
        assert_eq!(patch.width, 32);
    }

    #[test]
    fn drag_endpoints_map_through_the_same_proof() {
        let p = proof();
        let w = Some("w1-0");
        let start = p.point(w, None, "c1", 10, 20).unwrap();
        let end = p.point(w, None, "c1", 90, 70).unwrap();
        assert_eq!((start, end), (Point::new(-90, 70), Point::new(-10, 120)));
        assert!(p.point(w, None, "c1", 100, 70).is_err());
        assert!(p.point(w, None, "c1", 90, -1).is_err());
        let path = drag_path(start, end, 3);
        assert_eq!(path.len(), 3);
        assert_eq!(path.last(), Some(&end));
        assert!(path.iter().all(|q| p.bounds.contains(q.x, q.y)));
        assert_eq!(drag_path(start, end, 0), vec![end]);
    }

    #[test]
    fn button_names() {
        assert_eq!(button(None).unwrap(), MouseButton::Left);
        assert_eq!(button(Some("right")).unwrap(), MouseButton::Right);
        assert_eq!(button(Some("middle")).unwrap(), MouseButton::Middle);
        assert!(matches!(
            button(Some("Left")),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(button(Some("x1")), Err(Error::InvalidArgument(_))));
    }

    #[test]
    fn nonempty_edge_patches_and_changed_pixels() {
        let p = proof();
        for (x, y) in [(0, 0), (99, 0), (0, 79), (99, 79), (50, 40)] {
            let point = p.point(Some("w1-0"), None, "c1", x, y).unwrap();
            let (region, mut frame) = p.patch(point).unwrap();
            assert!(region.width > 0 && region.height > 0);
            assert!(region.width <= 32 && region.height <= 32);
            assert_eq!(region.intersect(&p.region), Some(region));
            frame.set_pixel(0, 0, Rgb::new(4, 5, 6)).unwrap();
            assert_ne!(p.patch(point).unwrap().1, frame);
        }
    }

    #[test]
    fn unicode_batches_and_stop_on_cancel_or_focus_error() {
        for count in [0, 31, 32, 33, 1000] {
            let input: String = "中🙂\n\té".chars().cycle().take(count).collect();
            let mut chunks = Vec::new();
            type_chunks(&input, |part| {
                chunks.push(part.to_string());
                Ok(())
            })
            .unwrap();
            assert_eq!(chunks.concat(), input);
            assert_eq!(chunks.len(), count.div_ceil(32));
            assert!(chunks.iter().all(|part| part.chars().count() <= 32));
        }
        let mut calls = 0;
        let error = type_chunks(&"x".repeat(1000), |_| {
            calls += 1;
            if calls == 2 {
                Err(Error::PermissionDenied("focus lost"))
            } else {
                Ok(())
            }
        });
        assert!(matches!(error, Err(Error::PermissionDenied(_))));
        assert_eq!(calls, 2);
    }
}
