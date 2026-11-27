//! Frame difference (SPEC section 7.3, 21).
use crate::error::{Error, Result};
use crate::types::{Rect, Screenshot};

/// How much of a frame changed between two screenshots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameDiff {
    /// Differing pixels divided by total pixels (0.0..=1.0; 0.0 for empty frames).
    pub ratio: f64,
    /// Bounding box of the differing pixels in image coordinates, if any.
    pub bounds: Option<Rect>,
}

impl Screenshot {
    /// Compares two same-sized frames. A pixel differs when any channel
    /// differs by more than `tolerance`. Different sizes yield
    /// [`Error::InvalidArgument`].
    pub fn diff(&self, other: &Screenshot, tolerance: u8) -> Result<FrameDiff> {
        if self.width != other.width || self.height != other.height {
            return Err(Error::InvalidArgument("frames differ in size"));
        }
        let width = self.width as usize;
        let total = width * self.height as usize;
        if total == 0 || self.pixels.len() != total * 3 || other.pixels.len() != total * 3 {
            return Ok(FrameDiff {
                ratio: 0.0,
                bounds: None,
            });
        }
        let tolerance = i16::from(tolerance);
        let (mut count, mut min_x, mut max_x) = (0usize, usize::MAX, 0usize);
        let (mut min_y, mut max_y) = (usize::MAX, 0usize);
        let rows = self.pixels.chunks_exact(width * 3);
        let other_rows = other.pixels.chunks_exact(width * 3);
        for (y, (a, b)) in rows.zip(other_rows).enumerate() {
            if a == b {
                continue;
            }
            for (x, (p, q)) in a
                .as_chunks::<3>()
                .0
                .iter()
                .zip(b.as_chunks::<3>().0)
                .enumerate()
            {
                let differs = p
                    .iter()
                    .zip(q)
                    .any(|(&u, &v)| (i16::from(u) - i16::from(v)).abs() > tolerance);
                if differs {
                    count += 1;
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        let bounds = (count > 0).then(|| {
            Rect::new(
                min_x as i32,
                min_y as i32,
                (max_x - min_x + 1) as i32,
                (max_y - min_y + 1) as i32,
            )
        });
        Ok(FrameDiff {
            ratio: count as f64 / total as f64,
            bounds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Rgb;

    fn set(shot: &mut Screenshot, x: usize, y: usize, v: [u8; 3]) {
        let i = (y * shot.width as usize + x) * 3;
        shot.pixels[i..i + 3].copy_from_slice(&v);
    }

    #[test]
    fn identical_frames_have_no_diff() {
        let a = Screenshot::solid(4, 3, Rgb::new(9, 9, 9));
        let d = a.diff(&a.clone(), 0).unwrap();
        assert_eq!(d.ratio, 0.0);
        assert_eq!(d.bounds, None);
    }

    #[test]
    fn ratio_and_bounds_cover_differing_pixels() {
        let a = Screenshot::solid(10, 10, Rgb::new(0, 0, 0));
        let mut b = a.clone();
        set(&mut b, 2, 3, [255, 0, 0]);
        set(&mut b, 5, 7, [0, 255, 0]);
        let d = a.diff(&b, 0).unwrap();
        assert!((d.ratio - 0.02).abs() < 1e-12);
        assert_eq!(d.bounds, Some(Rect::new(2, 3, 4, 5)));
    }

    #[test]
    fn tolerance_is_per_channel_and_exclusive() {
        let a = Screenshot::solid(2, 2, Rgb::new(100, 100, 100));
        let mut b = a.clone();
        set(&mut b, 0, 0, [108, 92, 100]);
        assert_eq!(a.diff(&b, 8).unwrap().ratio, 0.0);
        assert!((a.diff(&b, 7).unwrap().ratio - 0.25).abs() < 1e-12);
    }

    #[test]
    fn size_mismatch_is_invalid_argument() {
        let a = Screenshot::solid(2, 2, Rgb::new(0, 0, 0));
        let b = Screenshot::solid(3, 2, Rgb::new(0, 0, 0));
        assert!(matches!(a.diff(&b, 0), Err(Error::InvalidArgument(_))));
    }
}
