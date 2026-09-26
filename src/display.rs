//! Explicit Windows display capture, including monitors supplied by an IDD.
//! A Display is a topology snapshot. Re-enumerate after reconnecting, moving,
//! resizing or changing the primary monitor; stale snapshots fail closed.
use crate::{Error, Point, Rect, Result, Screenshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    id: u32,
    name: String,
    friendly_name: String,
    bounds: Rect,
    primary: bool,
}

impl Display {
    /// Enumerate active Windows displays. An installed but disabled driver or
    /// disconnected monitor does not constitute a capture target.
    pub fn all() -> Result<Vec<Self>> {
        #[cfg(all(windows, feature = "os"))]
        {
            let _dpi = crate::dpi::DpiGuard::enter()?;
            xcap::Monitor::all()
                .map_err(capture_error)?
                .iter()
                .map(Self::describe)
                .collect()
        }
        #[cfg(not(all(windows, feature = "os")))]
        {
            Err(unavailable())
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }
    /// Windows device name, such as \\.\DISPLAY2. Select exact names, not indices.
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn friendly_name(&self) -> &str {
        &self.friendly_name
    }
    /// Physical pixels in the Windows virtual desktop; left/top may be negative.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }
    pub fn is_primary(&self) -> bool {
        self.primary
    }

    pub fn contains(&self, point: Point) -> bool {
        i64::from(point.x) >= i64::from(self.bounds.left)
            && i64::from(point.y) >= i64::from(self.bounds.top)
            && i64::from(point.x) < i64::from(self.bounds.left) + i64::from(self.bounds.width)
            && i64::from(point.y) < i64::from(self.bounds.top) + i64::from(self.bounds.height)
    }

    /// Reject disconnected displays and topology changes instead of capturing a
    /// different display that happens to reuse an identifier.
    pub fn validate(&self) -> Result<()> {
        #[cfg(all(windows, feature = "os"))]
        {
            let _dpi = crate::dpi::DpiGuard::enter()?;
            self.resolve().map(|_| ())
        }
        #[cfg(not(all(windows, feature = "os")))]
        {
            Err(unavailable())
        }
    }

    pub(crate) fn capture(&self, region: Rect) -> Result<Screenshot> {
        if region.width <= 0 || region.height <= 0 || region.intersect(&self.bounds) != Some(region)
        {
            return Err(Error::InvalidArgument(
                "capture region must fit inside the selected display",
            ));
        }
        #[cfg(all(windows, feature = "os"))]
        {
            let _dpi = crate::dpi::DpiGuard::enter()?;
            let monitor = self.resolve()?;
            let x = u32::try_from(i64::from(region.left) - i64::from(self.bounds.left))
                .map_err(|_| Error::InvalidArgument("display coordinate overflow"))?;
            let y = u32::try_from(i64::from(region.top) - i64::from(self.bounds.top))
                .map_err(|_| Error::InvalidArgument("display coordinate overflow"))?;
            let rgba = monitor
                .capture_region(x, y, region.width as u32, region.height as u32)
                .map_err(capture_error)?;
            self.resolve()?;
            if rgba.width() != region.width as u32 || rgba.height() != region.height as u32 {
                return Err(Error::Screenshot(
                    "display changed during capture; enumerate again".into(),
                ));
            }
            let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
            Screenshot::new(rgb.width(), rgb.height(), rgb.into_raw())
        }
        #[cfg(not(all(windows, feature = "os")))]
        {
            Err(unavailable())
        }
    }

    #[cfg(all(windows, feature = "os"))]
    fn describe(monitor: &xcap::Monitor) -> Result<Self> {
        let name = monitor.name().map_err(capture_error)?;
        // Friendly labels are optional metadata; device identity never falls back.
        let friendly_name = monitor.friendly_name().unwrap_or_else(|_| name.clone());
        let width = i32::try_from(monitor.width().map_err(capture_error)?)
            .map_err(|_| Error::InvalidArgument("display width overflow"))?;
        let height = i32::try_from(monitor.height().map_err(capture_error)?)
            .map_err(|_| Error::InvalidArgument("display height overflow"))?;
        if width <= 0 || height <= 0 {
            return Err(Error::Screenshot("display has no active mode".into()));
        }
        Ok(Self {
            id: monitor.id().map_err(capture_error)?,
            name,
            friendly_name,
            bounds: Rect::new(
                monitor.x().map_err(capture_error)?,
                monitor.y().map_err(capture_error)?,
                width,
                height,
            ),
            primary: monitor.is_primary().map_err(capture_error)?,
        })
    }

    #[cfg(all(windows, feature = "os"))]
    fn resolve(&self) -> Result<xcap::Monitor> {
        for monitor in xcap::Monitor::all().map_err(capture_error)? {
            let current = Self::describe(&monitor)?;
            if current.id == self.id
                && current.name == self.name
                && current.bounds == self.bounds
                && current.primary == self.primary
            {
                return Ok(monitor);
            }
        }
        Err(Error::Screenshot(
            "selected display disconnected or changed; enumerate displays again".into(),
        ))
    }
}

#[cfg(not(all(windows, feature = "os")))]
fn unavailable() -> Error {
    if cfg!(windows) {
        Error::FeatureDisabled("os")
    } else {
        Error::UnsupportedPlatform
    }
}

#[cfg(all(windows, feature = "os"))]
fn capture_error(error: xcap::XCapError) -> Error {
    Error::Screenshot(error.to_string())
}

#[cfg(all(windows, feature = "os"))]
pub(crate) fn move_cursor(point: Point) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput,
    };
    let _dpi = crate::dpi::DpiGuard::enter()?;
    // Use the same physical-pixel bounds as capture, including negative origins.
    // GetSystemMetrics is not DPI-aware under a per-monitor-aware thread.
    let displays = Display::all()?;
    if !displays.iter().any(|display| display.contains(point)) {
        return Err(Error::InvalidArgument("point is outside active displays"));
    }
    let (left, top, right, bottom) = displays.iter().fold(
        (i64::MAX, i64::MAX, i64::MIN, i64::MIN),
        |(left, top, right, bottom), display| {
            let bounds = display.bounds();
            (
                left.min(i64::from(bounds.left)),
                top.min(i64::from(bounds.top)),
                right.max(i64::from(bounds.left) + i64::from(bounds.width)),
                bottom.max(i64::from(bounds.top) + i64::from(bounds.height)),
            )
        },
    );
    let normalize = |value: i32, origin: i64, extent: i64| -> Result<i32> {
        let relative = i64::from(value) - origin;
        if extent <= 0 || relative < 0 || relative >= extent {
            return Err(Error::InvalidArgument(
                "point is outside the virtual desktop",
            ));
        }
        // Aim at the center of the requested pixel's normalized interval.
        // Exact cursor readback remains mandatory before sending a click.
        Ok(((relative * 65536 + 32768) / extent).min(65535) as i32)
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: normalize(point.x, left, right - left)?,
                dy: normalize(point.y, top, bottom - top)?,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    if unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) } != 1 {
        return Err(Error::Input(format!(
            "virtual desktop SendInput failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn display() -> Display {
        Display {
            id: 1,
            name: "test".into(),
            friendly_name: "test".into(),
            bounds: Rect::new(-1920, -1080, 1920, 1080),
            primary: false,
        }
    }
    #[test]
    fn negative_bounds_include_first_pixel_and_exclude_end() {
        let display = display();
        assert!(display.contains(Point::new(-1920, -1080)));
        assert!(display.contains(Point::new(-1, -1)));
        for point in [
            Point::new(-1921, -1),
            Point::new(-1, -1081),
            Point::new(0, -1),
            Point::new(-1, 0),
        ] {
            assert!(!display.contains(point));
        }
    }
    #[test]
    fn invalid_region_is_rejected_before_native_capture() {
        for region in [
            Rect::new(-1921, -1080, 2, 2),
            Rect::new(-1, -1, 2, 2),
            Rect::new(-1, -1, 0, 1),
        ] {
            assert!(matches!(
                display().capture(region),
                Err(Error::InvalidArgument(_))
            ));
        }
    }
}
