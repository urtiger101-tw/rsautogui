#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    pub fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// Screen rectangle. Public name is `Rect`; `Box` is a PyAutoGUI alias.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

pub type Box = Rect;

impl Rect {
    pub fn new(left: i32, top: i32, width: i32, height: i32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.left.saturating_add(self.width)
    }

    pub fn bottom(&self) -> i32 {
        self.top.saturating_add(self.height)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && y >= self.top && x < self.right() && y < self.bottom()
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        if self.width <= 0 || self.height <= 0 || other.width <= 0 || other.height <= 0 {
            return None;
        }
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right > left && bottom > top {
            Some(Rect::new(left, top, right - left, bottom - top))
        } else {
            None
        }
    }

    pub fn iou(&self, other: &Rect) -> f32 {
        match self.intersect(other) {
            None => 0.0,
            Some(i) => {
                let a = (self.width as f32 * self.height as f32).max(1.0);
                let b = (other.width as f32 * other.height as f32).max(1.0);
                let inter = i.width as f32 * i.height as f32;
                inter / (a + b - inter)
            }
        }
    }
}

pub fn center(b: Rect) -> Point {
    Point::new(b.left + b.width / 2, b.top + b.height / 2)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn matches(&self, other: Rgb, tolerance: u8) -> bool {
        fn near(a: u8, b: u8, t: u8) -> bool {
            (a as i16 - b as i16).unsigned_abs() as u8 <= t
        }
        near(self.r, other.r, tolerance)
            && near(self.g, other.g, tolerance)
            && near(self.b, other.b, tolerance)
    }

    pub fn luma(&self) -> u8 {
        // Rec. 601 integer approximation
        ((self.r as u16 * 299 + self.g as u16 * 587 + self.b as u16 * 114) / 1000) as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollDirection {
    Vertical,
    Horizontal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
    /// RGB8 row-major
    pub pixels: Vec<u8>,
}

impl Screenshot {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> crate::error::Result<Self> {
        let need = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or(crate::error::Error::InvalidArgument(
                "image dimensions overflow",
            ))?;
        if pixels.len() != need {
            return Err(crate::error::Error::InvalidArgument(
                "pixel buffer length does not match width*height*3",
            ));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn solid(width: u32, height: u32, color: Rgb) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 3);
        for _ in 0..(width * height) {
            pixels.push(color.r);
            pixels.push(color.g);
            pixels.push(color.b);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn get_pixel(&self, x: i32, y: i32) -> crate::error::Result<Rgb> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return Err(crate::error::Error::InvalidArgument("pixel out of bounds"));
        }
        let i = (y as usize)
            .checked_mul(self.width as usize)
            .and_then(|row| row.checked_add(x as usize))
            .and_then(|pixel| pixel.checked_mul(3))
            .ok_or(crate::error::Error::InvalidArgument("pixel index overflow"))?;
        let pixel = self
            .pixels
            .get(i..i + 3)
            .ok_or(crate::error::Error::InvalidArgument("invalid pixel buffer"))?;
        Ok(Rgb::new(pixel[0], pixel[1], pixel[2]))
    }

    pub fn set_pixel(&mut self, x: i32, y: i32, color: Rgb) -> crate::error::Result<()> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return Err(crate::error::Error::InvalidArgument("pixel out of bounds"));
        }
        let i = (y as usize)
            .checked_mul(self.width as usize)
            .and_then(|row| row.checked_add(x as usize))
            .and_then(|pixel| pixel.checked_mul(3))
            .ok_or(crate::error::Error::InvalidArgument("pixel index overflow"))?;
        let pixel = self
            .pixels
            .get_mut(i..i + 3)
            .ok_or(crate::error::Error::InvalidArgument("invalid pixel buffer"))?;
        pixel.copy_from_slice(&[color.r, color.g, color.b]);
        Ok(())
    }

    pub fn region(&self, rect: Rect) -> crate::error::Result<Screenshot> {
        let width = i32::try_from(self.width)
            .map_err(|_| crate::error::Error::InvalidArgument("image width is too large"))?;
        let height = i32::try_from(self.height)
            .map_err(|_| crate::error::Error::InvalidArgument("image height is too large"))?;
        let bounds = Rect::new(0, 0, width, height);
        let clipped = rect
            .intersect(&bounds)
            .ok_or(crate::error::Error::InvalidArgument(
                "region does not intersect image",
            ))?;
        let mut pixels = Vec::with_capacity(clipped.width as usize * clipped.height as usize * 3);
        for y in clipped.top..clipped.bottom() {
            for x in clipped.left..clipped.right() {
                let pixel = self.get_pixel(x, y)?;
                pixels.extend_from_slice(&[pixel.r, pixel.g, pixel.b]);
            }
        }
        Screenshot::new(clipped.width as u32, clipped.height as u32, pixels)
    }

    pub fn save<P: AsRef<std::path::Path>>(&self, path: P) -> crate::error::Result<()> {
        let image = image::RgbImage::from_raw(self.width, self.height, self.pixels.clone())
            .ok_or(crate::error::Error::InvalidArgument("invalid pixel buffer"))?;
        image
            .save(path)
            .map_err(|error| crate::error::Error::Screenshot(error.to_string()))
    }

    pub fn load<P: AsRef<std::path::Path>>(path: P) -> crate::error::Result<Self> {
        let path = path.as_ref();
        match image::open(path) {
            Ok(image) => {
                let image = image.to_rgb8();
                Screenshot::new(image.width(), image.height(), image.into_raw())
            }
            Err(_error)
                if path
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("ppm")) =>
            {
                parse_ppm(&std::fs::read(path)?)
            }
            Err(error) => Err(crate::error::Error::Screenshot(error.to_string())),
        }
    }
}

fn parse_ppm(data: &[u8]) -> crate::error::Result<Screenshot> {
    let mut i = 0;
    let next_token = |i: &mut usize, data: &[u8]| -> crate::error::Result<String> {
        loop {
            while *i < data.len() && data[*i].is_ascii_whitespace() {
                *i += 1;
            }
            if *i < data.len() && data[*i] == b'#' {
                while *i < data.len() && data[*i] != b'\n' {
                    *i += 1;
                }
                continue;
            }
            break;
        }
        let start = *i;
        while *i < data.len() && !data[*i].is_ascii_whitespace() {
            *i += 1;
        }
        if start == *i {
            return Err(crate::error::Error::Screenshot("truncated ppm".into()));
        }
        Ok(String::from_utf8_lossy(&data[start..*i]).into_owned())
    };
    let magic = next_token(&mut i, data)?;
    if magic != "P6" {
        return Err(crate::error::Error::Screenshot(
            "only binary PPM P6 supported".into(),
        ));
    }
    let w: u32 = next_token(&mut i, data)?
        .parse()
        .map_err(|_| crate::error::Error::Screenshot("bad width".into()))?;
    let h: u32 = next_token(&mut i, data)?
        .parse()
        .map_err(|_| crate::error::Error::Screenshot("bad height".into()))?;
    let max: u32 = next_token(&mut i, data)?
        .parse()
        .map_err(|_| crate::error::Error::Screenshot("bad maxval".into()))?;
    if max != 255 {
        return Err(crate::error::Error::Screenshot("maxval must be 255".into()));
    }
    if i < data.len() && data[i].is_ascii_whitespace() {
        i += 1;
    }
    let pixels = data[i..].to_vec();
    Screenshot::new(w, h, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_round_trips_png() {
        let path =
            std::env::temp_dir().join(format!("autogui-roundtrip-{}.png", std::process::id()));
        let mut screenshot = Screenshot::solid(2, 1, Rgb::new(10, 20, 30));
        screenshot.set_pixel(1, 0, Rgb::new(40, 50, 60)).unwrap();
        screenshot.save(&path).unwrap();

        let loaded = Screenshot::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded, screenshot);
    }

    #[test]
    fn screenshot_region_clips_to_image_bounds() {
        let screenshot = Screenshot::solid(4, 3, Rgb::new(1, 2, 3));
        let cropped = screenshot.region(Rect::new(-1, 1, 3, 5)).unwrap();
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert_eq!(cropped.get_pixel(1, 1).unwrap(), Rgb::new(1, 2, 3));
    }
}
