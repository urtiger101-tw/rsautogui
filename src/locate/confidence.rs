//! Normalized per-channel correlation. Integral images supply patch statistics;
//! FFT supplies the cross term for large searches, with direct verification at
//! the acceptance threshold. No native OpenCV runtime is required.
use super::parallel::parallel;
use super::{luma, suppress_overlap};
use crate::error::Result;
use crate::types::{Rect, Screenshot};
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

type Check<'a> = &'a mut dyn FnMut() -> Result<()>;

struct Template {
    channels: usize,
    means: Vec<f64>,
    centered: Vec<Vec<f64>>,
    variance: f64,
}

fn value(image: &Screenshot, index: usize, channel: usize, gray: bool) -> f64 {
    let p = &image.pixels[index * 3..index * 3 + 3];
    if gray {
        f64::from(luma(p))
    } else {
        f64::from(p[channel])
    }
}

/// `len` pixels of one row of a single channel (or luma) from column `x`,
/// clipped to the image, without per-pixel bounds checks.
fn channel_span(
    image: &Screenshot,
    y: usize,
    x: usize,
    len: usize,
    channel: usize,
    gray: bool,
) -> impl Iterator<Item = u8> + '_ {
    let width = image.width as usize;
    let (start, end) = (x.min(width), (x + len).min(width));
    let row = image
        .pixels
        .get((y * width + start) * 3..(y * width + end) * 3)
        .unwrap_or_default();
    row.as_chunks::<3>()
        .0
        .iter()
        .map(move |p| if gray { luma(p) } else { p[channel] })
}

impl Template {
    fn new(needle: &Screenshot, gray: bool) -> Self {
        let channels = if gray { 1 } else { 3 };
        let count = needle.width as usize * needle.height as usize;
        let mut means = Vec::with_capacity(channels);
        let mut centered = Vec::with_capacity(channels);
        let mut variance = 0.0;
        for channel in 0..channels {
            let mean = (0..count)
                .map(|i| value(needle, i, channel, gray))
                .sum::<f64>()
                / count as f64;
            let plane: Vec<f64> = (0..count)
                .map(|i| value(needle, i, channel, gray) - mean)
                .collect();
            variance += plane.iter().map(|v| v * v).sum::<f64>();
            means.push(mean);
            centered.push(plane);
        }
        Self {
            channels,
            means,
            centered,
            variance,
        }
    }

    fn score_at(&self, hay: &Screenshot, needle: &Screenshot, x: usize, y: usize) -> f64 {
        let count = u64::from(needle.width) * u64::from(needle.height);
        let mut covariance = 0.0;
        let mut patch_variance = 0.0;
        let mut squared_error = 0.0;
        for channel in 0..self.channels {
            let mut sum = 0u64;
            let mut square_sum = 0u64;
            for ny in 0..needle.height as usize {
                for nx in 0..needle.width as usize {
                    let i = ny * needle.width as usize + nx;
                    let h = value(
                        hay,
                        (y + ny) * hay.width as usize + x + nx,
                        channel,
                        self.channels == 1,
                    );
                    let n = self.centered[channel][i];
                    covariance += (h - self.means[channel]) * n;
                    sum += h as u64;
                    square_sum += (h * h) as u64;
                    squared_error += (h - n - self.means[channel]).powi(2);
                }
            }
            patch_variance += (u128::from(square_sum) * u128::from(count) - u128::from(sum).pow(2))
                as f64
                / count as f64;
        }
        if self.variance <= 1e-9 {
            // A constant template has no normalized correlation. Compare color
            // distance instead of accepting every constant patch as score 1.
            1.0 - (squared_error / (count as f64 * self.channels as f64)).sqrt() / 255.0
        } else if patch_variance <= 1e-9 {
            0.0
        } else {
            (covariance / (self.variance * patch_variance).sqrt()).clamp(-1.0, 1.0)
        }
    }
}

pub(super) fn search(
    hay: &Screenshot,
    needle: &Screenshot,
    gray: bool,
    threshold: f32,
    check: Check<'_>,
) -> Result<Vec<Rect>> {
    let template = Template::new(needle, gray);
    let columns = (hay.width - needle.width + 1) as usize;
    let rows = (hay.height - needle.height + 1) as usize;
    let workload = (columns as u64)
        .saturating_mul(rows as u64)
        .saturating_mul(u64::from(needle.width))
        .saturating_mul(u64::from(needle.height))
        .saturating_mul(template.channels as u64);
    let mut candidates = Vec::new();
    if workload <= 2_000_000 {
        for y in 0..rows {
            check()?;
            for x in 0..columns {
                let score = template.score_at(hay, needle, x, y);
                if score + 1e-12 >= f64::from(threshold) {
                    candidates.push((
                        Rect::new(
                            x as i32,
                            y as i32,
                            needle.width as i32,
                            needle.height as i32,
                        ),
                        score,
                    ));
                }
            }
        }
    } else {
        let threshold = f64::from(threshold);
        let rect = |x: usize, y: usize| {
            Rect::new(
                x as i32,
                y as i32,
                needle.width as i32,
                needle.height as i32,
            )
        };
        let template = &template;
        type Band = (Vec<(Rect, f64)>, Windows, Vec<f64>);
        let found: Vec<Band> = if template.variance > 1e-9 {
            tiled_correlation(
                hay,
                needle,
                template,
                Band::default,
                |(found, windows, stats): &mut Band, x0, y0, w, h, cross: &[f64]| {
                    window_statistics(hay, needle, template, (x0, y0, w, h), windows, stats);
                    for (i, (&cross, &statistic)) in cross.iter().zip(stats.iter()).enumerate() {
                        let approximate = if statistic > 1e-9 {
                            cross / (template.variance * statistic).sqrt()
                        } else {
                            0.0
                        };
                        // FFT is a candidate filter. Verify every accepted correlation
                        // directly so numerical error cannot authorize an input action.
                        if approximate + 1e-8 >= threshold {
                            let (x, y) = (x0 + i % w, y0 + i / w);
                            let score = template.score_at(hay, needle, x, y);
                            if score + 1e-12 >= threshold {
                                found.push((rect(x, y), score));
                            }
                        }
                    }
                },
                check,
            )?
        } else {
            let pixels = f64::from(needle.width) * f64::from(needle.height);
            let scale = pixels * template.channels as f64;
            let band = (4 * needle.height as usize).max(32);
            parallel(
                (0..rows).step_by(band).collect(),
                rows * columns * template.channels,
                Band::default,
                |(found, windows, stats): &mut Band, y0: usize| {
                    let h = band.min(rows - y0);
                    window_statistics(hay, needle, template, (0, y0, columns, h), windows, stats);
                    for (i, &statistic) in stats.iter().enumerate() {
                        let score = (1.0 - (statistic / scale).sqrt() / 255.0).clamp(-1.0, 1.0);
                        if score + 1e-12 >= threshold {
                            found.push((rect(i % columns, y0 + i / columns), score));
                        }
                    }
                },
                check,
            )?
        };
        // Candidates are fully ordered below, so band order does not matter.
        candidates = found.into_iter().flat_map(|(found, _, _)| found).collect();
    }
    check()?;
    candidates.sort_unstable_by(|(a, sa), (b, sb)| {
        sb.total_cmp(sa)
            .then_with(|| a.top.cmp(&b.top))
            .then_with(|| a.left.cmp(&b.left))
    });
    suppress_overlap(
        candidates.into_iter().map(|(rect, _)| rect).collect(),
        check,
    )
}

/// Reusable buffers for `window_statistics`.
#[derive(Default)]
struct Windows {
    values: Vec<u8>,
    row: Vec<(u64, u64)>,
    totals: Vec<Vec<(u64, u64)>>,
}

/// Patch variance or, for flat templates, squared error against the template
/// mean, summed over channels, for the `w x h` positions from `(x0, y0)`,
/// row-major into `out`. Exact integer window sums slide down the rows, so
/// no integral table is stored and any rectangle can be computed on its own.
fn window_statistics(
    hay: &Screenshot,
    needle: &Screenshot,
    template: &Template,
    (x0, y0, w, h): (usize, usize, usize, usize),
    scratch: &mut Windows,
    out: &mut Vec<f64>,
) {
    let (nw, nh) = (needle.width as usize, needle.height as usize);
    let count = u64::from(needle.width) * u64::from(needle.height);
    // 255^2 * count^2 fits u64 below this; beyond it fall back to u128.
    let wide = count > 16_000_000;
    let gray = template.channels == 1;
    let flat = template.variance <= 1e-9;
    let statistic = |sum: u64, square: u64, mean: u64| -> f64 {
        match (flat, wide) {
            (true, false) => (square + count * mean * mean - 2 * mean * sum) as f64,
            (true, true) => {
                let (sum, square, count, mean) = (
                    u128::from(sum),
                    u128::from(square),
                    u128::from(count),
                    u128::from(mean),
                );
                (square + count * mean * mean - 2 * mean * sum) as f64
            }
            // Keep the subtraction exact for nearly flat, bright UI.
            (false, false) => (square * count - sum * sum) as f64 / count as f64,
            (false, true) => {
                let (sum, square, wide_count) =
                    (u128::from(sum), u128::from(square), u128::from(count));
                (square * wide_count - sum * sum) as f64 / count as f64
            }
        }
    };
    let Windows {
        values,
        row,
        totals,
    } = scratch;
    // Horizontal window sums of one haystack row.
    let windows = |y: usize, channel: usize, values: &mut Vec<u8>, row: &mut Vec<(u64, u64)>| {
        values.clear();
        values.extend(channel_span(hay, y, x0, w + nw - 1, channel, gray));
        row.clear();
        let (mut sum, mut square) = (0u64, 0u64);
        for &v in &values[..nw] {
            sum += u64::from(v);
            square += u64::from(v) * u64::from(v);
        }
        row.push((sum, square));
        for x in 1..w {
            let (add, remove) = (u64::from(values[x + nw - 1]), u64::from(values[x - 1]));
            sum = sum + add - remove;
            square = square + add * add - remove * remove;
            row.push((sum, square));
        }
    };
    totals.resize(template.channels, Vec::new());
    for total in totals.iter_mut() {
        total.clear();
        total.resize(w, (0, 0));
    }
    out.clear();
    out.resize(w * h, 0.0);
    for (v, out_row) in out.chunks_exact_mut(w).enumerate() {
        let y = y0 + v;
        for (channel, total) in totals.iter_mut().enumerate() {
            if v == 0 {
                for dy in 0..nh {
                    windows(y + dy, channel, values, row);
                    for (t, add) in total.iter_mut().zip(row.iter()) {
                        t.0 += add.0;
                        t.1 += add.1;
                    }
                }
            } else {
                windows(y + nh - 1, channel, values, row);
                for (t, add) in total.iter_mut().zip(row.iter()) {
                    t.0 += add.0;
                    t.1 += add.1;
                }
                windows(y - 1, channel, values, row);
                for (t, remove) in total.iter_mut().zip(row.iter()) {
                    t.0 -= remove.0;
                    t.1 -= remove.1;
                }
            }
            let mean = template.means[channel] as u64;
            for (cell, &(sum, square)) in out_row.iter_mut().zip(total.iter()) {
                *cell += statistic(sum, square, mean);
            }
        }
    }
}

// RustFFT is fastest with dimensions factored into 2 and 3. Valid-correlation
// positions do not wrap, so padding to the image extent is sufficient.
fn fft_len(minimum: usize) -> usize {
    let mut best = minimum.next_power_of_two();
    let mut two = 1usize;
    while two <= best {
        let mut length = two;
        while length < minimum {
            let Some(next) = length.checked_mul(3) else {
                break;
            };
            length = next;
        }
        if length >= minimum {
            best = best.min(length);
        }
        let Some(next) = two.checked_mul(2) else {
            break;
        };
        two = next;
    }
    best
}

/// Plans are reused across searches (and by `locate_until` polling loops).
fn plans(width: usize, height: usize) -> [Arc<dyn Fft<f64>>; 4] {
    static PLANNER: OnceLock<Mutex<FftPlanner<f64>>> = OnceLock::new();
    let mut planner = PLANNER
        .get_or_init(|| Mutex::new(FftPlanner::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    [
        planner.plan_fft_forward(width),
        planner.plan_fft_forward(height),
        planner.plan_fft_inverse(width),
        planner.plan_fft_inverse(height),
    ]
}

type Spectrum = Vec<Complex<f64>>;

/// FFT tile geometry. Each tile transforms `f x g` haystack pixels (zero
/// padded past the edge) and yields `tw x th` valid correlations, so the
/// working set stays in cache and tiles run in parallel. Valid positions never
/// wrap, so each tile's circular correlation equals the linear one.
#[derive(Clone, Copy)]
struct Tiles {
    f: usize,
    g: usize,
    tw: usize,
    th: usize,
}

impl Tiles {
    fn new(hay: &Screenshot, needle: &Screenshot) -> Self {
        let side = |needle: u32, hay: u32| {
            fft_len((4 * needle as usize).max(64)).min(fft_len(hay as usize))
        };
        let f = side(needle.width, hay.width);
        let g = side(needle.height, hay.height);
        Self {
            f,
            g,
            tw: f - needle.width as usize + 1,
            th: g - needle.height as usize + 1,
        }
    }
}

/// Per-thread buffers: row-major `z`, column-major `t` and accumulator `q`,
/// plus one tile of correlations.
struct Workspace {
    z: Spectrum,
    t: Spectrum,
    q: Spectrum,
    scratch: Spectrum,
    out: Vec<f64>,
}

struct Plans {
    tiles: Tiles,
    row_forward: Arc<dyn Fft<f64>>,
    column_forward: Arc<dyn Fft<f64>>,
    row_inverse: Arc<dyn Fft<f64>>,
    column_inverse: Arc<dyn Fft<f64>>,
}

impl Plans {
    fn new(tiles: Tiles) -> Self {
        let [row_forward, column_forward, row_inverse, column_inverse] = plans(tiles.f, tiles.g);
        Self {
            tiles,
            row_forward,
            column_forward,
            row_inverse,
            column_inverse,
        }
    }

    fn workspace(&self) -> Workspace {
        let scratch = [
            &self.row_forward,
            &self.column_forward,
            &self.row_inverse,
            &self.column_inverse,
        ]
        .iter()
        .map(|plan| plan.get_inplace_scratch_len())
        .max()
        .unwrap_or(0);
        let len = self.tiles.f * self.tiles.g;
        let zero = Complex::default();
        Workspace {
            z: vec![zero; len],
            t: vec![zero; len],
            q: vec![zero; len],
            scratch: vec![zero; scratch],
            out: Vec::new(),
        }
    }

    /// Row-major `z` to its column-major 2-D spectrum in `t`. Rows from
    /// `used` on are zero, and so is their transform.
    fn forward(&self, ws: &mut Workspace, used: usize) {
        let (f, g) = (self.tiles.f, self.tiles.g);
        for row in ws.z.chunks_exact_mut(f).take(used) {
            self.row_forward.process_with_scratch(row, &mut ws.scratch);
        }
        for (v, row) in ws.z.chunks_exact(f).enumerate() {
            for (column, value) in ws.t.chunks_exact_mut(g).zip(row) {
                column[v] = *value;
            }
        }
        for column in ws.t.chunks_exact_mut(g) {
            self.column_forward
                .process_with_scratch(column, &mut ws.scratch);
        }
    }

    /// Visit the spectra of the real (re) and imaginary (im) inputs packed
    /// into `t`, separated by conjugate symmetry, one column-major column
    /// `kx` at a time: `visit(kx, ky, [re_spectrum, im_spectrum])`.
    fn separate(&self, t: &[Complex<f64>], mut visit: impl FnMut(usize, usize, [Complex<f64>; 2])) {
        let (f, g) = (self.tiles.f, self.tiles.g);
        for (kx, own) in t.chunks_exact(g).enumerate() {
            let mirror_x = if kx == 0 { 0 } else { f - kx };
            let mirror = &t[mirror_x * g..(mirror_x + 1) * g];
            for (ky, &a) in own.iter().enumerate() {
                let b = mirror[if ky == 0 { 0 } else { g - ky }].conj();
                visit(kx, ky, [(a + b) * 0.5, (a - b) * Complex::new(0.0, -0.5)]);
            }
        }
    }
}

/// Valid cross-correlation of the haystack with the centered template, summed
/// over channels. Each tile's `w x h` correlations at `(x0, y0)` go to
/// `emit(state, x0, y0, w, h, values)` row-major. Real signals travel in pairs
/// through one complex FFT, and two tiles share one complex inverse.
fn tiled_correlation<S: Send>(
    hay: &Screenshot,
    needle: &Screenshot,
    template: &Template,
    init: impl Fn() -> S + Sync,
    emit: impl Fn(&mut S, usize, usize, usize, usize, &[f64]) + Sync,
    check: Check<'_>,
) -> Result<Vec<S>> {
    let tiles = Tiles::new(hay, needle);
    let (f, g) = (tiles.f, tiles.g);
    let plans = Plans::new(tiles);
    let zero = Complex::<f64>::default();
    let (nw, nh) = (needle.width as usize, needle.height as usize);
    let hay_h = hay.height as usize;
    let columns = hay.width as usize - nw + 1;
    let rows = hay_h - nh + 1;
    let gray = template.channels == 1;
    // Conjugated template spectra, column-major, two channels per transform;
    // channels with no variance contribute nothing and are skipped.
    let active: Vec<usize> = (0..template.channels)
        .filter(|&c| template.centered[c].iter().any(|v| v.abs() > 1e-12))
        .collect();
    let mut filters: Vec<(usize, Spectrum)> = Vec::with_capacity(active.len());
    let mut ws = plans.workspace();
    for two in active.chunks(2) {
        check()?;
        ws.z.fill(zero);
        for (part, &channel) in two.iter().enumerate() {
            for (y, source) in template.centered[channel].chunks_exact(nw).enumerate() {
                for (cell, v) in ws.z[y * f..y * f + nw].iter_mut().zip(source) {
                    if part == 0 {
                        cell.re = *v;
                    } else {
                        cell.im = *v;
                    }
                }
            }
        }
        plans.forward(&mut ws, nh);
        let mut spectra: Vec<Spectrum> = two.iter().map(|_| Vec::with_capacity(f * g)).collect();
        plans.separate(&ws.t, |_, _, parts| {
            for (spectrum, part) in spectra.iter_mut().zip(parts) {
                spectrum.push(part.conj());
            }
        });
        filters.extend(two.iter().copied().zip(spectra));
    }
    let mut origins = Vec::new();
    for y in (0..rows).step_by(tiles.th) {
        for x in (0..columns).step_by(tiles.tw) {
            origins.push((x, y));
        }
    }
    let pairs: Vec<&[(usize, usize)]> = origins.chunks(2).collect();
    let scale = (f * g) as f64;
    // Fresh pages are costly; the first band reuses the filter workspace.
    let pool = Mutex::new(vec![ws]);
    let (plans, filters, emit, pool) = (&plans, &filters, &emit, &pool);
    let states = parallel(
        pairs,
        columns * rows * template.channels,
        || {
            let reused = pool.lock().unwrap_or_else(PoisonError::into_inner).pop();
            (init(), reused.unwrap_or_else(|| plans.workspace()))
        },
        |(state, ws): &mut (S, Workspace), pair: &[(usize, usize)]| {
            ws.q.fill(zero);
            // (tile slot, filter index) for every real signal of this job.
            let signals: Vec<(usize, usize)> = (0..pair.len())
                .flat_map(|slot| (0..filters.len()).map(move |filter| (slot, filter)))
                .collect();
            for two in signals.chunks(2) {
                ws.z.fill(zero);
                for (part, &(slot, filter)) in two.iter().enumerate() {
                    let (x0, y0) = pair[slot];
                    let channel = filters[filter].0;
                    for (v, row) in ws.z.chunks_exact_mut(f).take(hay_h - y0).enumerate() {
                        let values = channel_span(hay, y0 + v, x0, f, channel, gray);
                        for (cell, value) in row.iter_mut().zip(values) {
                            if part == 0 {
                                cell.re = f64::from(value);
                            } else {
                                cell.im = f64::from(value);
                            }
                        }
                    }
                }
                let used = pair
                    .iter()
                    .map(|&(_, y0)| g.min(hay_h - y0))
                    .max()
                    .unwrap_or(g);
                plans.forward(ws, used);
                let q = &mut ws.q;
                plans.separate(&ws.t, |kx, ky, spectra| {
                    let k = kx * g + ky;
                    let mut sum = Complex::default();
                    for (spectrum, &(slot, filter)) in spectra.iter().zip(two) {
                        let product = spectrum * filters[filter].1[k];
                        // Tile 0 accumulates in re, tile 1 in im (times i).
                        sum += if slot == 0 {
                            product
                        } else {
                            Complex::new(-product.im, product.re)
                        };
                    }
                    q[k] += sum;
                });
            }
            for column in ws.q.chunks_exact_mut(g) {
                plans
                    .column_inverse
                    .process_with_scratch(column, &mut ws.scratch);
            }
            for (v, row) in ws.z.chunks_exact_mut(f).take(tiles.th).enumerate() {
                for (cell, column) in row.iter_mut().zip(ws.q.chunks_exact(g)) {
                    *cell = column[v];
                }
            }
            for row in ws.z.chunks_exact_mut(f).take(tiles.th) {
                plans.row_inverse.process_with_scratch(row, &mut ws.scratch);
            }
            for (slot, &(x0, y0)) in pair.iter().enumerate() {
                let (w, h) = (tiles.tw.min(columns - x0), tiles.th.min(rows - y0));
                ws.out.clear();
                for row in ws.z.chunks_exact(f).take(h) {
                    ws.out.extend(
                        row[..w]
                            .iter()
                            .map(|value| (if slot == 0 { value.re } else { value.im }) / scale),
                    );
                }
                emit(state, x0, y0, w, h, &ws.out);
            }
        },
        check,
    )?;
    Ok(states.into_iter().map(|(state, _)| state).collect())
}

/// The implementation before the threaded FFT rewrite, kept as the oracle.
#[cfg(test)]
mod reference {
    use super::{Check, Template, fft_len, suppress_overlap};
    use crate::error::Result;
    use crate::types::{Rect, Screenshot};
    use rustfft::{Fft, FftPlanner, num_complex::Complex};
    use std::sync::Arc;

    fn value(image: &Screenshot, index: usize, channel: usize, gray: bool) -> f64 {
        super::value(image, index, channel, gray)
    }

    pub(super) fn search(
        hay: &Screenshot,
        needle: &Screenshot,
        gray: bool,
        threshold: f32,
        check: Check<'_>,
    ) -> Result<Vec<Rect>> {
        let template = Template::new(needle, gray);
        let columns = (hay.width - needle.width + 1) as usize;
        let rows = (hay.height - needle.height + 1) as usize;
        let workload = (columns as u64)
            .saturating_mul(rows as u64)
            .saturating_mul(u64::from(needle.width))
            .saturating_mul(u64::from(needle.height))
            .saturating_mul(template.channels as u64);
        let mut candidates = Vec::new();
        if workload <= 2_000_000 {
            for y in 0..rows {
                check()?;
                for x in 0..columns {
                    let score = template.score_at(hay, needle, x, y);
                    if score + 1e-12 >= f64::from(threshold) {
                        candidates.push((
                            Rect::new(
                                x as i32,
                                y as i32,
                                needle.width as i32,
                                needle.height as i32,
                            ),
                            score,
                        ));
                    }
                }
            }
        } else {
            let stats = patch_statistics(hay, needle, &template, check)?;
            let cross = if template.variance > 1e-9 {
                Some(cross_correlation(hay, needle, &template, check)?)
            } else {
                None
            };
            for y in 0..rows {
                check()?;
                for x in 0..columns {
                    let index = y * columns + x;
                    let approximate = match &cross {
                        Some(cross) if stats[index] > 1e-9 => {
                            cross[index] / (template.variance * stats[index]).sqrt()
                        }
                        Some(_) => 0.0,
                        None => {
                            1.0 - (stats[index]
                                / (f64::from(needle.width)
                                    * f64::from(needle.height)
                                    * template.channels as f64))
                                .sqrt()
                                / 255.0
                        }
                    };
                    // FFT is a candidate filter. Verify every accepted correlation
                    // directly so numerical error cannot authorize an input action.
                    if approximate + 1e-8 >= f64::from(threshold) {
                        if x % 32 == 0 {
                            check()?;
                        }
                        let score = if cross.is_some() {
                            template.score_at(hay, needle, x, y)
                        } else {
                            approximate.clamp(-1.0, 1.0)
                        };
                        if score + 1e-12 >= f64::from(threshold) {
                            candidates.push((
                                Rect::new(
                                    x as i32,
                                    y as i32,
                                    needle.width as i32,
                                    needle.height as i32,
                                ),
                                score,
                            ));
                        }
                    }
                }
            }
        }
        check()?;
        candidates.sort_unstable_by(|(a, sa), (b, sb)| {
            sb.total_cmp(sa)
                .then_with(|| a.top.cmp(&b.top))
                .then_with(|| a.left.cmp(&b.left))
        });
        suppress_overlap(
            candidates.into_iter().map(|(rect, _)| rect).collect(),
            check,
        )
    }

    // One channel's integral tables at a time, so auxiliary memory is independent
    // of channel count. Output is patch variance or, for flat templates, squared error.
    pub(super) fn patch_statistics(
        hay: &Screenshot,
        needle: &Screenshot,
        template: &Template,
        check: Check<'_>,
    ) -> Result<Vec<f64>> {
        let stride = hay.width as usize + 1;
        let rows = hay.height as usize + 1;
        let columns = (hay.width - needle.width + 1) as usize;
        let output_rows = (hay.height - needle.height + 1) as usize;
        let mut output = vec![0.0; columns * output_rows];
        let mut sums = vec![0u64; stride * rows];
        let mut squares = vec![0u64; stride * rows];
        let count = u128::from(needle.width) * u128::from(needle.height);
        for channel in 0..template.channels {
            for y in 1..rows {
                if y % 32 == 0 {
                    check()?;
                }
                let mut sum = 0u64;
                let mut square = 0u64;
                for x in 1..stride {
                    let v = value(
                        hay,
                        (y - 1) * hay.width as usize + x - 1,
                        channel,
                        template.channels == 1,
                    ) as u64;
                    sum += v;
                    square += v * v;
                    sums[y * stride + x] = sums[(y - 1) * stride + x] + sum;
                    squares[y * stride + x] = squares[(y - 1) * stride + x] + square;
                }
            }
            for y in 0..output_rows {
                if y % 32 == 0 {
                    check()?;
                }
                for x in 0..columns {
                    let a = y * stride + x;
                    let b = a + needle.width as usize;
                    let c = (y + needle.height as usize) * stride + x;
                    let d = c + needle.width as usize;
                    let sum = u128::from((sums[d] - sums[b]) - (sums[c] - sums[a]));
                    let square = u128::from((squares[d] - squares[b]) - (squares[c] - squares[a]));
                    let statistic = if template.variance <= 1e-9 {
                        let mean = template.means[channel] as u128;
                        (square + count * mean * mean - 2 * mean * sum) as f64
                    } else {
                        // Keep the subtraction exact for nearly flat, bright UI.
                        (square * count - sum * sum) as f64 / count as f64
                    };
                    output[y * columns + x] += statistic;
                }
            }
        }
        Ok(output)
    }

    struct Transform {
        width: usize,
        height: usize,
        row: Arc<dyn Fft<f64>>,
        column: Arc<dyn Fft<f64>>,
    }

    impl Transform {
        fn run(&self, buffer: &mut [Complex<f64>], check: Check<'_>) -> Result<()> {
            let mut scratch = vec![
                Complex::default();
                self.row
                    .get_inplace_scratch_len()
                    .max(self.column.get_inplace_scratch_len())
            ];
            for (index, row) in buffer.chunks_exact_mut(self.width).enumerate() {
                if index % 32 == 0 {
                    check()?;
                }
                self.row.process_with_scratch(row, &mut scratch);
            }
            let mut column = vec![Complex::default(); self.height];
            for x in 0..self.width {
                if x % 32 == 0 {
                    check()?;
                }
                for y in 0..self.height {
                    column[y] = buffer[y * self.width + x];
                }
                self.column.process_with_scratch(&mut column, &mut scratch);
                for y in 0..self.height {
                    buffer[y * self.width + x] = column[y];
                }
            }
            Ok(())
        }
    }

    pub(super) fn cross_correlation(
        hay: &Screenshot,
        needle: &Screenshot,
        template: &Template,
        check: Check<'_>,
    ) -> Result<Vec<f64>> {
        let width = fft_len(hay.width as usize);
        let height = fft_len(hay.height as usize);
        let length = width * height;
        let mut planner = FftPlanner::<f64>::new();
        let forward = Transform {
            width,
            height,
            row: planner.plan_fft_forward(width),
            column: planner.plan_fft_forward(height),
        };
        let inverse = Transform {
            width,
            height,
            row: planner.plan_fft_inverse(width),
            column: planner.plan_fft_inverse(height),
        };
        let mut image = vec![Complex::default(); length];
        let mut filter = vec![Complex::default(); length];
        let mut result = vec![Complex::default(); length];
        for channel in 0..template.channels {
            check()?;
            if template.centered[channel].iter().all(|v| v.abs() <= 1e-12) {
                continue;
            }
            image.fill(Complex::default());
            filter.fill(Complex::default());
            for y in 0..hay.height as usize {
                for x in 0..hay.width as usize {
                    image[y * width + x].re = value(
                        hay,
                        y * hay.width as usize + x,
                        channel,
                        template.channels == 1,
                    );
                }
            }
            for y in 0..needle.height as usize {
                for x in 0..needle.width as usize {
                    filter[y * width + x].re =
                        template.centered[channel][y * needle.width as usize + x];
                }
            }
            forward.run(&mut image, check)?;
            forward.run(&mut filter, check)?;
            for i in 0..length {
                result[i] += image[i] * filter[i].conj();
            }
        }
        drop(image);
        drop(filter);
        inverse.run(&mut result, check)?;
        let columns = (hay.width - needle.width + 1) as usize;
        let rows = (hay.height - needle.height + 1) as usize;
        let mut output = Vec::with_capacity(columns * rows);
        for y in 0..rows {
            for x in 0..columns {
                output.push(result[y * width + x].re / length as f64);
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Rgb;

    fn noise(width: u32, height: u32, seed: u32) -> Screenshot {
        let mut state = seed;
        let pixels = (0..width * height * 3)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state >> 8) as u8
            })
            .collect();
        Screenshot {
            width,
            height,
            pixels,
        }
    }

    /// A flat background with low-contrast blocks, a planted copy of the
    /// needle, and a noisy near-copy, so thresholds separate real candidates.
    fn scene(width: u32, height: u32, needle: &Screenshot, gray_ui: bool) -> Screenshot {
        let mut hay = if gray_ui {
            Screenshot::solid(width, height, Rgb::new(236, 236, 236))
        } else {
            noise(width, height, 99)
        };
        for y in 0..needle.height as i32 {
            for x in 0..needle.width as i32 {
                let color = needle.get_pixel(x, y).unwrap();
                hay.set_pixel(x + 7, y + 5, color).unwrap();
                let noisy = Rgb::new(
                    color.r.saturating_add(((x * 7 + y) % 9) as u8),
                    color.g,
                    color.b.saturating_sub(((x + y * 3) % 5) as u8),
                );
                hay.set_pixel(
                    width as i32 - needle.width as i32 - 3 + x,
                    height as i32 - needle.height as i32 - 2 + y,
                    noisy,
                )
                .unwrap();
            }
        }
        hay
    }

    fn cases() -> Vec<(Screenshot, Screenshot)> {
        let needle = noise(20, 12, 7);
        let mut flat = Screenshot::solid(16, 9, Rgb::new(120, 130, 140));
        flat.set_pixel(3, 3, Rgb::new(121, 130, 140)).unwrap();
        let constant = Screenshot::solid(18, 10, Rgb::new(236, 236, 236));
        vec![
            (scene(210, 150, &needle, false), needle.clone()),
            (scene(333, 177, &needle, true), needle.clone()),
            (scene(150, 120, &flat, true), flat.clone()),
            (scene(160, 110, &constant, true), constant),
            // An odd tile count leaves one tile without an inverse partner.
            (scene(180, 60, &needle, false), needle.clone()),
            // Odd sizes exercise partial tiles and non power-of-two FFTs.
            (scene(401, 97, &needle, false), needle),
        ]
    }

    #[test]
    fn window_statistics_match_integral_tables_exactly() {
        for (hay, needle) in cases() {
            for gray in [false, true] {
                let template = Template::new(&needle, gray);
                let reference =
                    reference::patch_statistics(&hay, &needle, &template, &mut || Ok(())).unwrap();
                let (columns, rows) = (
                    (hay.width - needle.width + 1) as usize,
                    (hay.height - needle.height + 1) as usize,
                );
                let mut out = Vec::new();
                window_statistics(
                    &hay,
                    &needle,
                    &template,
                    (0, 0, columns, rows),
                    &mut Windows::default(),
                    &mut out,
                );
                assert_eq!(out, reference);
                // An interior rectangle matches the same positions.
                let mut part = Vec::new();
                window_statistics(
                    &hay,
                    &needle,
                    &template,
                    (3, 4, 17, 9),
                    &mut Windows::default(),
                    &mut part,
                );
                for (i, value) in part.iter().enumerate() {
                    assert_eq!(*value, reference[(4 + i / 17) * columns + 3 + i % 17]);
                }
            }
        }
    }

    #[test]
    fn tiled_correlation_matches_full_frame_fft() {
        for (hay, needle) in cases() {
            for gray in [false, true] {
                let template = Template::new(&needle, gray);
                if template.variance <= 1e-9 {
                    continue;
                }
                let reference =
                    reference::cross_correlation(&hay, &needle, &template, &mut || Ok(())).unwrap();
                let columns = (hay.width - needle.width + 1) as usize;
                let mut tiled = vec![f64::NAN; reference.len()];
                let tiles = tiled_correlation(
                    &hay,
                    &needle,
                    &template,
                    Vec::new,
                    |found: &mut Vec<(usize, f64)>, x0, y0, w, _, values: &[f64]| {
                        for (i, value) in values.iter().enumerate() {
                            found.push(((y0 + i / w) * columns + x0 + i % w, *value));
                        }
                    },
                    &mut || Ok(()),
                )
                .unwrap();
                for (index, value) in tiles.into_iter().flatten() {
                    assert!(tiled[index].is_nan(), "position emitted twice");
                    tiled[index] = value;
                }
                let magnitude = reference.iter().fold(1.0f64, |m, v| m.max(v.abs()));
                for (a, b) in tiled.iter().zip(&reference) {
                    assert!((a - b).abs() <= magnitude * 1e-9, "{a} vs {b}");
                }
            }
        }
    }

    #[test]
    fn search_matches_reference_implementation() {
        for (hay, needle) in cases() {
            for gray in [false, true] {
                for threshold in [0.5, 0.8, 0.95, 0.999, 1.0] {
                    let expected =
                        reference::search(&hay, &needle, gray, threshold, &mut || Ok(())).unwrap();
                    let actual = search(&hay, &needle, gray, threshold, &mut || Ok(())).unwrap();
                    assert_eq!(actual, expected, "gray={gray} threshold={threshold}");
                }
            }
        }
    }

    #[test]
    fn fft_search_stops_on_fail_safe() {
        let (hay, needle) = cases().remove(0);
        let mut calls = 0;
        let result = search(&hay, &needle, false, 0.9, &mut || {
            calls += 1;
            if calls == 4 {
                Err(crate::error::Error::FailSafe(crate::types::Point::new(
                    0, 0,
                )))
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(crate::error::Error::FailSafe(_))));
    }
}
