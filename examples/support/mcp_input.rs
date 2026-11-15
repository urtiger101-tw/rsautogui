//! Capture-bound pointer input and bounded Unicode text batches.
use super::{active, capture_region, screen_region};
use autogui::{AutoGui, Display, Error, Point, Rect, Result, Screenshot, Window};
use std::time::{Duration, Instant};

pub(super) struct CaptureProof {
    pub id: String,
    pub target: String,
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

    pub fn point(&self, target: &str, id: &str, x: i32, y: i32) -> Result<Point> {
        if self.target != target || self.id != id {
            return Err(Error::InvalidArgument(
                "capture does not match target/id; capture again",
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
        window: &Window,
        display: Option<&Display>,
        point: Point,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        check()?;
        self.fresh()?;
        active(window)?;
        if window.box_rect()? != self.bounds || screen_region(gui, window, display)? != self.region
        {
            return Err(Error::InvalidArgument(
                "window/display geometry changed; capture again",
            ));
        }
        let (region, expected) = self.patch(point)?;
        if capture_region(gui, display, region)? != expected {
            return Err(Error::InvalidArgument(
                "pixels or hover changed; cursor may be positioned; capture again before retrying",
            ));
        }
        check()?;
        self.fresh()?;
        active(window)?;
        if window.box_rect()? != self.bounds {
            return Err(Error::InvalidArgument(
                "window moved before input; capture again",
            ));
        }
        if let Some(display) = display {
            display.validate()?;
        }
        Ok(())
    }
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
            target: "w1-0".into(),
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
        assert_eq!(p.point("w1-0", "c1", 0, 0).unwrap(), Point::new(-100, 50));
        assert_eq!(p.point("w1-0", "c1", 99, 79).unwrap(), Point::new(-1, 129));
        for (target, id, x, y) in [
            ("w2-0", "c1", 1, 1),
            ("w1-0", "c0", 1, 1),
            ("w1-0", "c1", -1, 0),
            ("w1-0", "c1", 100, 0),
            ("w1-0", "c1", 0, 80),
        ] {
            assert!(matches!(
                p.point(target, id, x, y),
                Err(Error::InvalidArgument(_))
            ));
        }
        p.taken = Instant::now() - Duration::from_secs(61);
        assert!(matches!(
            p.point("w1-0", "c1", 1, 1),
            Err(Error::InvalidArgument(_))
        ));
        p.taken = Instant::now();
        p.region.left = i32::MAX - 1;
        assert!(matches!(
            p.point("w1-0", "c1", 2, 1),
            Err(Error::InvalidArgument(_))
        ));
    }

    #[test]
    fn nonempty_edge_patches_and_changed_pixels() {
        let p = proof();
        for (x, y) in [(0, 0), (99, 0), (0, 79), (99, 79), (50, 40)] {
            let point = p.point("w1-0", "c1", x, y).unwrap();
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
