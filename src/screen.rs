use crate::Display;
use crate::error::{Error, Result};
use crate::gui::AutoGui;
use crate::locate::{self, LocateOptions};
use crate::types::{Point, Rect, Rgb, Screenshot, Size};

impl AutoGui {
    pub fn size(&self) -> Result<Size> {
        self.backend.screen_size()
    }

    pub fn position(&self) -> Result<Point> {
        self.backend.mouse_pos()
    }

    pub fn on_screen(&self, x: i32, y: i32) -> Result<bool> {
        let s = self.size()?;
        Ok(x >= 0 && y >= 0 && x < s.width && y < s.height)
    }

    pub fn screenshot(&mut self) -> Result<Screenshot> {
        self.before_read_heavy()?;
        let img = self.backend.capture(None)?;
        self.after_read_heavy()?;
        Ok(img)
    }

    pub fn screenshot_region(&mut self, region: Rect) -> Result<Screenshot> {
        self.before_read_heavy()?;
        let img = self.backend.capture(Some(region))?;
        self.after_read_heavy()?;
        Ok(img)
    }

    /// Capture an explicitly selected real or virtual Windows display. The
    /// default screenshot APIs and primary-monitor corner fail-safe are unchanged.
    pub fn screenshot_display(&mut self, display: &Display) -> Result<Screenshot> {
        self.screenshot_display_region(display, display.bounds())
    }

    /// Region uses global Windows physical pixels, including negative origins.
    /// It must fit entirely inside this display's enumerated bounds.
    pub fn screenshot_display_region(
        &mut self,
        display: &Display,
        region: Rect,
    ) -> Result<Screenshot> {
        self.before_read_heavy()?;
        let frame = display.capture(region)?;
        self.after_read_heavy()?;
        Ok(frame)
    }

    pub fn screenshot_to_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<Screenshot> {
        let img = self.screenshot()?;
        img.save(path)?;
        Ok(img)
    }

    pub fn pixel(&mut self, x: i32, y: i32) -> Result<Rgb> {
        self.before_read_heavy()?;
        let img = self.backend.capture(Some(Rect::new(x, y, 1, 1)))?;
        self.after_read_heavy()?;
        img.get_pixel(0, 0)
    }

    pub fn pixel_matches_color(
        &mut self,
        x: i32,
        y: i32,
        color: Rgb,
        tolerance: u8,
    ) -> Result<bool> {
        Ok(self.pixel(x, y)?.matches(color, tolerance))
    }

    pub fn locate(
        &self,
        needle: &Screenshot,
        haystack: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Rect> {
        self.before_read_heavy()?;
        let result = locate::locate_all_checked(needle, haystack, opt, true, &mut || {
            self.before_read_heavy()
        });
        self.after_read_heavy()?;
        result?.into_iter().next().ok_or(Error::ImageNotFound)
    }

    pub fn try_locate(
        &self,
        needle: &Screenshot,
        haystack: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Option<Rect>> {
        self.before_read_heavy()?;
        let result = locate::locate_all_checked(needle, haystack, opt, false, &mut || {
            self.before_read_heavy()
        });
        self.after_read_heavy()?;
        Ok(result?.into_iter().next())
    }

    pub fn locate_all(
        &self,
        needle: &Screenshot,
        haystack: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Vec<Rect>> {
        self.before_read_heavy()?;
        let result = locate::locate_all_checked(
            needle,
            haystack,
            opt,
            self.raise_image_not_found,
            &mut || self.before_read_heavy(),
        );
        self.after_read_heavy()?;
        result
    }

    pub fn locate_on_screen<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Rect> {
        let needle = Screenshot::load(image)?;
        self.locate_screenshot_on_screen(&needle, opt)
    }

    fn search_screen(
        &self,
        needle: &Screenshot,
        opt: &LocateOptions,
        raise: bool,
    ) -> Result<Vec<Rect>> {
        self.before_read_heavy()?;
        let result = locate::locate_until(
            || {
                self.before_read_heavy()?;
                let (region, offset) = if let Some(region) = opt.region {
                    let size = self.backend.screen_size()?;
                    let clipped = region
                        .intersect(&Rect::new(0, 0, size.width, size.height))
                        .ok_or(Error::InvalidArgument("region does not intersect screen"))?;
                    (Some(clipped), Point::new(clipped.left, clipped.top))
                } else {
                    (None, Point::new(0, 0))
                };
                let image = self.backend.capture(region)?;
                let local = LocateOptions {
                    region: None,
                    ..opt.clone()
                };
                let mut hits =
                    locate::locate_all_checked(needle, &image, &local, false, &mut || {
                        self.before_read_heavy()
                    })?;
                for hit in &mut hits {
                    hit.left += offset.x;
                    hit.top += offset.y;
                }
                Ok(hits)
            },
            opt.min_search_time,
            raise,
        );
        self.after_read_heavy()?;
        result
    }

    pub fn locate_screenshot_on_screen(
        &mut self,
        needle: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Rect> {
        self.search_screen(needle, opt, true)?
            .into_iter()
            .next()
            .ok_or(Error::ImageNotFound)
    }

    pub fn try_locate_screenshot_on_screen(
        &mut self,
        needle: &Screenshot,
        opt: &LocateOptions,
    ) -> Result<Option<Rect>> {
        Ok(self.search_screen(needle, opt, false)?.into_iter().next())
    }

    pub fn try_locate_on_screen<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Option<Rect>> {
        self.try_locate_screenshot_on_screen(&Screenshot::load(image)?, opt)
    }

    pub fn locate_all_on_screen<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Vec<Rect>> {
        self.search_screen(&Screenshot::load(image)?, opt, self.raise_image_not_found)
    }

    pub fn locate_center_on_screen<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Point> {
        Ok(crate::types::center(self.locate_on_screen(image, opt)?))
    }

    pub fn click_image<P: AsRef<std::path::Path>>(
        &mut self,
        image: P,
        opt: &LocateOptions,
    ) -> Result<Point> {
        let p = self.locate_center_on_screen(image, opt)?;
        self.click_xy(p.x, p.y)?;
        Ok(p)
    }
}
