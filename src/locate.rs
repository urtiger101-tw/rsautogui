use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::types::Point;
use crate::types::{Rect, Screenshot, center};
use parallel::parallel;

#[cfg(feature = "opencv")]
mod confidence;
mod parallel;

#[derive(Clone, Debug)]
pub struct LocateOptions {
    pub grayscale: bool,
    pub confidence: Option<f32>,
    pub region: Option<Rect>,
    pub min_search_time: Duration,
}

impl Default for LocateOptions {
    fn default() -> Self {
        Self {
            grayscale: false,
            confidence: None,
            region: None,
            min_search_time: Duration::from_millis(0),
        }
    }
}

pub(crate) fn luma(pixel: &[u8]) -> u8 {
    ((pixel[0] as u32 * 299 + pixel[1] as u32 * 587 + pixel[2] as u32 * 114) / 1000) as u8
}

fn pixel_key(pixel: &[u8], grayscale: bool) -> u32 {
    if grayscale {
        luma(pixel) as u32
    } else {
        pixel[0] as u32 | (pixel[1] as u32) << 8 | (pixel[2] as u32) << 16
    }
}

/// Packed pixels for exact comparison: RGB bytes, or one luma byte per pixel.
fn plane<'a>(
    image: &'a Screenshot,
    gray: bool,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<std::borrow::Cow<'a, [u8]>> {
    if !gray {
        return Ok(std::borrow::Cow::Borrowed(&image.pixels));
    }
    let width = image.width as usize;
    let mut plane = vec![0u8; image.pixels.len() / 3];
    if width > 0 {
        let rows = plane
            .chunks_mut(width)
            .zip(image.pixels.chunks(width * 3))
            .collect();
        parallel(
            rows,
            image.pixels.len(),
            || (),
            |_, (out, pixels): (&mut [u8], &[u8])| {
                for (cell, pixel) in out.iter_mut().zip(pixels.as_chunks::<3>().0) {
                    *cell = luma(pixel);
                }
            },
            check,
        )?;
    }
    Ok(std::borrow::Cow::Owned(plane))
}

fn search_exact(
    hay: &Screenshot,
    needle: &Screenshot,
    gray: bool,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Rect>> {
    if needle.width == 0
        || needle.height == 0
        || needle.width > hay.width
        || needle.height > hay.height
    {
        return Ok(Vec::new());
    }
    let hay_plane = plane(hay, gray, check)?;
    let needle_plane = plane(needle, gray, check)?;
    if gray {
        scan::<1>(hay, needle, &hay_plane, &needle_plane, gray, check)
    } else {
        scan::<3>(hay, needle, &hay_plane, &needle_plane, gray, check)
    }
}

/// Scan each row for an anchor pixel (the needle's rarest colour), then compare
/// whole needle rows (memcmp), starting with the anchor's row. Rows run in
/// ordered bands, so hits stay in raster order.
fn scan<const N: usize>(
    hay: &Screenshot,
    needle: &Screenshot,
    hay_plane: &[u8],
    needle_plane: &[u8],
    gray: bool,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Rect>> {
    let mut hits = Vec::new();
    let (hay_w, nw, nh) = (
        hay.width as usize,
        needle.width as usize,
        needle.height as usize,
    );
    let columns = hay_w - nw + 1;
    let rows = hay.height as usize - nh + 1;
    let mut frequencies = HashMap::<u32, usize>::new();
    for pixel in needle.pixels.as_chunks::<3>().0.iter() {
        *frequencies.entry(pixel_key(pixel, gray)).or_default() += 1;
    }
    let anchor = needle
        .pixels
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
        .min_by_key(|(_, pixel)| {
            frequencies
                .get(&pixel_key(*pixel, gray))
                .copied()
                .unwrap_or(usize::MAX)
        });
    let Some((anchor_index, _)) = anchor else {
        return Ok(hits);
    };
    let (anchor_x, anchor_y) = (anchor_index % nw, anchor_index / nw);
    let Some(anchor_pixel) = needle_plane
        .get(anchor_index * N..anchor_index * N + N)
        .and_then(|p| <[u8; N]>::try_from(p).ok())
    else {
        return Ok(hits);
    };
    let (hay_stride, needle_stride) = (hay_w * N, nw * N);
    let needle_row = |ny: usize| needle_plane.get(ny * needle_stride..(ny + 1) * needle_stride);
    let matches = |x: usize, y: usize| {
        (anchor_y..nh).chain(0..anchor_y).all(|ny| {
            let start = (y + ny) * hay_stride + x * N;
            hay_plane.get(start..start + needle_stride) == needle_row(ny)
        })
    };
    let bands = parallel(
        (0..rows).collect(),
        rows * columns,
        Vec::new,
        |hits: &mut Vec<Rect>, y: usize| {
            let start = (y + anchor_y) * hay_stride + anchor_x * N;
            let Some(line) = hay_plane.get(start..start + columns * N) else {
                return;
            };
            for (x, pixel) in line.as_chunks::<N>().0.iter().enumerate() {
                if *pixel == anchor_pixel && matches(x, y) {
                    hits.push(Rect::new(x as i32, y as i32, nw as i32, nh as i32));
                }
            }
        },
        check,
    )?;
    hits.extend(bands.into_iter().flatten());
    suppress_overlap(hits, check)
}

fn suppress_overlap(hits: Vec<Rect>, check: &mut dyn FnMut() -> Result<()>) -> Result<Vec<Rect>> {
    if hits
        .first()
        .is_some_and(|rect| rect.width == 1 && rect.height == 1)
    {
        check()?;
        return Ok(hits);
    }
    let mut kept: Vec<Rect> = Vec::new();
    let mut grid = HashMap::<(i32, i32), Vec<usize>>::new();
    for (index, h) in hits.into_iter().enumerate() {
        if index % 1024 == 0 {
            check()?;
        }
        let cell = (h.left.div_euclid(h.width), h.top.div_euclid(h.height));
        let overlaps = (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                grid.get(&(cell.0 + dx, cell.1 + dy))
                    .is_some_and(|indices| indices.iter().any(|&i| kept[i].iou(&h) > 0.3))
            })
        });
        if overlaps {
            continue;
        }
        grid.entry(cell).or_default().push(kept.len());
        kept.push(h);
    }
    Ok(kept)
}

#[cfg(test)]
pub fn locate_in(
    needle: &Screenshot,
    haystack: &Screenshot,
    opt: &LocateOptions,
    raise_not_found: bool,
) -> Result<Option<Rect>> {
    Ok(locate_all_in(needle, haystack, opt, raise_not_found)?
        .into_iter()
        .next())
}

#[cfg(test)]
pub fn locate_all_in(
    needle: &Screenshot,
    haystack: &Screenshot,
    opt: &LocateOptions,
    raise_not_found: bool,
) -> Result<Vec<Rect>> {
    locate_all_checked(needle, haystack, opt, raise_not_found, &mut || Ok(()))
}

pub(crate) fn locate_all_checked(
    needle: &Screenshot,
    haystack: &Screenshot,
    opt: &LocateOptions,
    raise_not_found: bool,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Rect>> {
    validate_screenshot(needle)?;
    validate_screenshot(haystack)?;
    if opt.confidence.is_some_and(|c| !(0.0..=1.0).contains(&c)) {
        return Err(Error::InvalidArgument("confidence must be 0.0..=1.0"));
    }
    #[cfg(not(feature = "opencv"))]
    if opt.confidence.is_some() {
        return Err(Error::FeatureDisabled("opencv"));
    }
    check()?;
    let cropped;
    let (search, offset) = if let Some(region) = opt.region {
        let bounds = Rect::new(0, 0, haystack.width as i32, haystack.height as i32);
        let clipped = region
            .intersect(&bounds)
            .ok_or(Error::InvalidArgument("region does not intersect image"))?;
        cropped = haystack.region(clipped)?;
        (&cropped, Point::new(clipped.left, clipped.top))
    } else {
        (haystack, Point::new(0, 0))
    };
    if needle.width > search.width || needle.height > search.height {
        return Err(Error::InvalidArgument("needle larger than search region"));
    }
    let mut hits = match opt.confidence {
        #[cfg(feature = "opencv")]
        Some(threshold) => confidence::search(search, needle, opt.grayscale, threshold, check)?,
        _ => search_exact(search, needle, opt.grayscale, check)?,
    };
    for hit in &mut hits {
        hit.left += offset.x;
        hit.top += offset.y;
    }
    if hits.is_empty() && raise_not_found {
        Err(Error::ImageNotFound)
    } else {
        Ok(hits)
    }
}

fn validate_screenshot(image: &Screenshot) -> Result<()> {
    if image.width == 0 || image.height == 0 {
        return Err(Error::InvalidArgument("image dimensions must be positive"));
    }
    if image.width > i32::MAX as u32 || image.height > i32::MAX as u32 {
        return Err(Error::InvalidArgument(
            "image dimensions exceed coordinate range",
        ));
    }
    let expected = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or(Error::InvalidArgument("image dimensions overflow"))?;
    if expected != image.pixels.len() {
        return Err(Error::InvalidArgument("invalid screenshot pixel buffer"));
    }
    Ok(())
}

pub fn locate_until(
    mut scan: impl FnMut() -> Result<Vec<Rect>>,
    min_search_time: Duration,
    raise_not_found: bool,
) -> Result<Vec<Rect>> {
    let start = Instant::now();
    loop {
        let hits = scan()?;
        if !hits.is_empty() {
            return Ok(hits);
        }
        if start.elapsed() >= min_search_time {
            return if raise_not_found {
                Err(Error::ImageNotFound)
            } else {
                Ok(vec![])
            };
        }
        std::thread::sleep(
            Duration::from_millis(50).min(min_search_time.saturating_sub(start.elapsed())),
        );
    }
}

#[allow(dead_code)]
pub fn first_center(hits: &[Rect]) -> Option<crate::types::Point> {
    hits.first().copied().map(center)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Rgb;

    /// The exact search before the packed-row rewrite, kept as the oracle.
    mod reference {
        use super::super::{luma, pixel_key, suppress_overlap};
        use crate::error::Result;
        use crate::types::{Rect, Screenshot};
        use std::collections::HashMap;

        fn pixel_eq(hay: &Screenshot, needle: &Screenshot, x: i32, y: i32, gray: bool) -> bool {
            let hay_stride = hay.width as usize * 3;
            let needle_stride = needle.width as usize * 3;
            for ny in 0..needle.height as i32 {
                let hay_start = (y as usize + ny as usize) * hay_stride + x as usize * 3;
                let needle_start = ny as usize * needle_stride;
                for nx in 0..needle.width as i32 {
                    let h = hay_start + nx as usize * 3;
                    let n = needle_start + nx as usize * 3;
                    let (Some(hp), Some(np)) =
                        (hay.pixels.get(h..h + 3), needle.pixels.get(n..n + 3))
                    else {
                        return false;
                    };
                    if gray {
                        if luma(hp) != luma(np) {
                            return false;
                        }
                    } else if hp != np {
                        return false;
                    }
                }
            }
            true
        }

        pub(crate) fn search_exact(
            hay: &Screenshot,
            needle: &Screenshot,
            gray: bool,
            check: &mut dyn FnMut() -> Result<()>,
        ) -> Result<Vec<Rect>> {
            let mut hits = Vec::new();
            if needle.width == 0
                || needle.height == 0
                || needle.width > hay.width
                || needle.height > hay.height
            {
                return Ok(hits);
            }
            let max_x = hay.width as i32 - needle.width as i32;
            let max_y = hay.height as i32 - needle.height as i32;
            let mut frequencies = HashMap::<u32, usize>::new();
            for pixel in needle.pixels.as_chunks::<3>().0.iter() {
                *frequencies.entry(pixel_key(pixel, gray)).or_default() += 1;
            }
            let anchor = needle
                .pixels
                .as_chunks::<3>()
                .0
                .iter()
                .enumerate()
                .min_by_key(|(_, pixel)| {
                    frequencies
                        .get(&pixel_key(*pixel, gray))
                        .copied()
                        .unwrap_or(usize::MAX)
                });
            let Some((anchor_index, anchor_pixel)) = anchor else {
                return Ok(hits);
            };
            let anchor_x = anchor_index % needle.width as usize;
            let anchor_y = anchor_index / needle.width as usize;
            let anchor_key = pixel_key(anchor_pixel, gray);
            let hay_stride = hay.width as usize * 3;
            for y in 0..=max_y {
                check()?;
                for x in 0..=max_x {
                    let offset = (y as usize + anchor_y) * hay_stride + (x as usize + anchor_x) * 3;
                    let Some(pixel) = hay.pixels.get(offset..offset + 3) else {
                        continue;
                    };
                    if pixel_key(pixel, gray) != anchor_key {
                        continue;
                    }
                    if pixel_eq(hay, needle, x, y, gray) {
                        hits.push(Rect::new(x, y, needle.width as i32, needle.height as i32));
                    }
                }
            }
            suppress_overlap(hits, check)
        }
    }

    fn noise(width: u32, height: u32, seed: u32, levels: u32) -> Screenshot {
        let mut state = seed;
        let pixels = (0..width * height * 3)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                ((state >> 8) % levels * (255 / levels.max(2))) as u8
            })
            .collect();
        Screenshot::new(width, height, pixels).unwrap()
    }

    fn plant(hay: &mut Screenshot, needle: &Screenshot, left: i32, top: i32) {
        for y in 0..needle.height as i32 {
            for x in 0..needle.width as i32 {
                hay.set_pixel(left + x, top + y, needle.get_pixel(x, y).unwrap())
                    .unwrap();
            }
        }
    }

    #[test]
    fn exact_search_matches_reference_implementation() {
        let needle = noise(9, 6, 5, 4);
        let mut busy = noise(640, 400, 11, 4);
        for (left, top) in [(0, 0), (631, 394), (100, 40), (104, 42), (230, 199)] {
            plant(&mut busy, &needle, left, top);
        }
        let stripes = Screenshot::new(
            300,
            240,
            (0..300 * 240 * 3)
                .map(|i| ((i / 3) % 300 % 7) as u8 * 30)
                .collect(),
        )
        .unwrap();
        let cases = [
            (busy.clone(), needle.clone()),
            (busy.clone(), noise(1, 1, 3, 4)),
            (
                busy.region(Rect::new(50, 50, 30, 20)).unwrap(),
                busy.region(Rect::new(50, 50, 30, 20)).unwrap(),
            ),
            // Many overlapping hits: suppression order must be unchanged.
            (
                Screenshot::solid(260, 300, Rgb::new(4, 5, 6)),
                Screenshot::solid(3, 2, Rgb::new(4, 5, 6)),
            ),
            (
                stripes.clone(),
                stripes.region(Rect::new(3, 0, 10, 4)).unwrap(),
            ),
            (busy, noise(12, 7, 77, 255)),
        ];
        for (hay, needle) in &cases {
            for gray in [false, true] {
                let expected = reference::search_exact(hay, needle, gray, &mut || Ok(())).unwrap();
                let actual = search_exact(hay, needle, gray, &mut || Ok(())).unwrap();
                assert_eq!(
                    actual, expected,
                    "gray={gray} {}x{}",
                    needle.width, needle.height
                );
            }
        }
    }

    #[test]
    fn regression_grayscale_bright_pixels() {
        let mut hay = Screenshot::solid(6, 5, Rgb::new(255, 255, 255));
        let needle = Screenshot::solid(2, 2, Rgb::new(180, 240, 160));
        for y in 1..3 {
            for x in 3..5 {
                hay.set_pixel(x, y, Rgb::new(180, 240, 160)).unwrap();
            }
        }
        let opt = LocateOptions {
            grayscale: true,
            ..Default::default()
        };
        assert_eq!(
            locate_in(&needle, &hay, &opt, true).unwrap(),
            Some(Rect::new(3, 1, 2, 2))
        );
    }

    #[cfg(feature = "opencv")]
    #[test]
    fn regression_flat_template_rejects_different_color() {
        let needle = Screenshot::solid(3, 3, Rgb::new(255, 0, 0));
        let hay = Screenshot::solid(3, 3, Rgb::new(200, 0, 0));
        let opt = LocateOptions {
            confidence: Some(0.95),
            ..Default::default()
        };
        assert!(locate_in(&needle, &hay, &opt, false).unwrap().is_none());
    }

    #[cfg(feature = "opencv")]
    #[test]
    fn regression_flat_template_tolerates_small_pixel_noise() {
        let needle = Screenshot::solid(3, 3, Rgb::new(100, 100, 100));
        let mut hay = needle.clone();
        hay.set_pixel(1, 1, Rgb::new(101, 101, 101)).unwrap();
        let opt = LocateOptions {
            confidence: Some(0.99),
            grayscale: true,
            ..Default::default()
        };
        assert_eq!(
            locate_in(&needle, &hay, &opt, true).unwrap(),
            Some(Rect::new(0, 0, 3, 3))
        );
    }

    #[cfg(feature = "opencv")]
    #[test]
    fn fft_confidence_preserves_region_and_best_match_order() {
        let mut hay = Screenshot::solid(150, 110, Rgb::new(15, 30, 60));
        let mut needle = Screenshot::solid(18, 14, Rgb::new(0, 0, 0));
        for y in 0..14 {
            for x in 0..18 {
                let color = Rgb::new(
                    ((x * 73 + y * 29) % 220) as u8,
                    ((x * 31 + y * 91) % 220) as u8,
                    ((x * 47 + y * 13) % 220) as u8,
                );
                needle.set_pixel(x, y, color).unwrap();
                hay.set_pixel(x + 110, y + 80, color).unwrap();
                let delta = if (x + y) % 2 == 0 { 15 } else { 0 };
                hay.set_pixel(x + 20, y + 20, Rgb::new(color.r + delta, color.g, color.b))
                    .unwrap();
            }
        }
        let options = LocateOptions {
            confidence: Some(0.97),
            region: Some(Rect::new(10, 10, 130, 95)),
            ..Default::default()
        };
        let hits = locate_all_in(&needle, &hay, &options, true).unwrap();
        assert_eq!(
            hits,
            vec![Rect::new(110, 80, 18, 14), Rect::new(20, 20, 18, 14)]
        );
        let exact = LocateOptions {
            confidence: Some(1.0),
            ..options
        };
        assert_eq!(
            locate_in(&needle, &hay, &exact, true).unwrap(),
            Some(Rect::new(110, 80, 18, 14))
        );
    }

    #[cfg(feature = "opencv")]
    #[test]
    fn fft_low_contrast_template_keeps_a_perfect_match() {
        let mut hay = Screenshot::solid(150, 100, Rgb::new(255, 255, 255));
        let mut needle = Screenshot::solid(64, 32, Rgb::new(255, 255, 255));
        needle.set_pixel(21, 11, Rgb::new(254, 254, 254)).unwrap();
        hay.set_pixel(81, 51, Rgb::new(254, 254, 254)).unwrap();
        let options = LocateOptions {
            confidence: Some(1.0),
            grayscale: true,
            ..Default::default()
        };
        assert_eq!(
            locate_in(&needle, &hay, &options, true).unwrap(),
            Some(Rect::new(60, 40, 64, 32))
        );
    }

    #[test]
    fn invalid_pixels_and_empty_images_are_rejected() {
        let malformed = Screenshot {
            width: 3,
            height: 2,
            pixels: vec![0; 2],
        };
        let hay = Screenshot::solid(8, 8, Rgb::new(0, 0, 0));
        assert!(matches!(
            locate_in(&malformed, &hay, &LocateOptions::default(), false),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(
            locate_in(
                &Screenshot::solid(0, 0, Rgb::new(0, 0, 0)),
                &hay,
                &LocateOptions::default(),
                false
            ),
            Err(Error::InvalidArgument(_))
        ));
    }

    #[test]
    fn search_can_be_interrupted() {
        let hay = Screenshot::solid(40, 40, Rgb::new(0, 0, 0));
        let needle = Screenshot::solid(2, 2, Rgb::new(255, 0, 0));
        let mut checks = 0;
        let result =
            locate_all_checked(&needle, &hay, &LocateOptions::default(), false, &mut || {
                checks += 1;
                if checks == 3 {
                    Err(Error::FailSafe(Point::new(0, 0)))
                } else {
                    Ok(())
                }
            });
        assert!(matches!(result, Err(Error::FailSafe(_))));
        assert_eq!(checks, 3);
    }

    #[test]
    fn finds_block() {
        let mut hay = Screenshot::solid(200, 200, Rgb::new(255, 0, 0));
        let needle = Screenshot::solid(10, 10, Rgb::new(0, 0, 255));
        for y in 50..60 {
            for x in 40..50 {
                hay.set_pixel(x, y, Rgb::new(0, 0, 255)).unwrap();
            }
        }
        let box_ = locate_in(&needle, &hay, &LocateOptions::default(), true)
            .unwrap()
            .unwrap();
        assert_eq!(box_, Rect::new(40, 50, 10, 10));
        assert_eq!(center(box_), crate::types::Point::new(45, 55));
    }

    #[test]
    fn missing() {
        let hay = Screenshot::solid(20, 20, Rgb::new(0, 0, 0));
        let needle = Screenshot::solid(3, 3, Rgb::new(1, 2, 3));
        let err = locate_in(&needle, &hay, &LocateOptions::default(), true).unwrap_err();
        assert!(matches!(err, Error::ImageNotFound));
    }

    #[test]
    fn two_blocks() {
        let mut hay = Screenshot::solid(80, 40, Rgb::new(0, 0, 0));
        let needle = Screenshot::solid(4, 4, Rgb::new(9, 9, 9));
        for y in 2..6 {
            for x in 2..6 {
                hay.set_pixel(x, y, Rgb::new(9, 9, 9)).unwrap();
            }
        }
        for y in 2..6 {
            for x in 40..44 {
                hay.set_pixel(x, y, Rgb::new(9, 9, 9)).unwrap();
            }
        }
        let all = locate_all_in(&needle, &hay, &LocateOptions::default(), true).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[cfg(not(feature = "opencv"))]
    #[test]
    fn confidence_needs_feature() {
        let hay = Screenshot::solid(8, 8, Rgb::new(0, 0, 0));
        let needle = Screenshot::solid(2, 2, Rgb::new(0, 0, 0));
        let opt = LocateOptions {
            confidence: Some(0.9),
            ..Default::default()
        };
        let err = locate_in(&needle, &hay, &opt, true).unwrap_err();
        assert!(matches!(err, Error::FeatureDisabled(_)));
    }

    #[cfg(feature = "opencv")]
    #[test]
    fn confidence_finds_a_near_match() {
        let mut hay = Screenshot::solid(12, 10, Rgb::new(8, 8, 8));
        let mut needle = Screenshot::solid(4, 4, Rgb::new(0, 0, 0));
        for y in 0..4 {
            for x in 0..4 {
                let color = if (x + y) % 2 == 0 {
                    Rgb::new(240, 20, 10)
                } else {
                    Rgb::new(10, 30, 230)
                };
                needle.set_pixel(x, y, color).unwrap();
                hay.set_pixel(x + 5, y + 3, color).unwrap();
            }
        }
        hay.set_pixel(8, 4, Rgb::new(215, 35, 25)).unwrap();

        let options = LocateOptions {
            confidence: Some(0.9),
            ..LocateOptions::default()
        };
        let found = locate_in(&needle, &hay, &options, true).unwrap().unwrap();
        assert_eq!(found, Rect::new(5, 3, 4, 4));
    }
}
