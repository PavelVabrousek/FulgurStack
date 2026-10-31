//! Display-only views of one flat. They remove the Bayer pattern so vignetting
//! and dust can be judged at every zoom. Nothing here reaches a calibration or
//! a file.

/// Share of the field left out at each end of the vignetting range.
const VIGNETTING_TAIL: f64 = 0.002;
/// Long edge of the averaged image the vignetting range is read from.
const RANGE_EDGE: usize = 640;
/// Half of the dust view's display range, as a fraction of the local level.
const DUST_SPAN: f32 = 0.03;
/// Long edge of the coarse image that carries the field's large-scale shape.
const SHAPE_EDGE: usize = 390;
/// Blur radius as a share of the long edge. Three box passes of a radius make a
/// Gaussian of about that sigma, wider than dust shadows and narrower than the
/// falloff.
const SHAPE_SIGMA: f32 = 0.082;
const SHAPE_PASSES: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FlatView {
    #[default]
    Vignetting,
    Dust,
    Raw,
}

impl FlatView {
    pub const ALL: [Self; 3] = [Self::Vignetting, Self::Dust, Self::Raw];

    pub fn label(self, cfa: bool) -> &'static str {
        match self {
            Self::Vignetting => "Vignetting",
            Self::Dust => "Dust",
            Self::Raw if cfa => "Raw CFA",
            Self::Raw => "Raw",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Self::Vignetting => "The light falloff across the field, without the Bayer pattern",
            Self::Dust => "Dust shadows, with the large-scale falloff removed",
            Self::Raw => "The stored pixels, for checking single hot or dead pixels",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Vignetting => "vignetting",
            Self::Dust => "dust",
            Self::Raw => "raw",
        }
    }
}

/// A plane mapped to 0..1 for display, with a line that says how.
pub struct FlatRender {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f32>,
    pub note: String,
}

/// Mean of each 2 x 2 cell. Every cell holds all four Bayer positions, so the
/// pattern cannot survive, whatever the layout of the colour filter.
fn cells(width: usize, height: usize, plane: &[f32]) -> Vec<f32> {
    let (w, h) = (width / 2, height / 2);
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let top = &plane[2 * y * width..][..2 * w];
        let bottom = &plane[(2 * y + 1) * width..][..2 * w];
        for (a, b) in top.chunks_exact(2).zip(bottom.chunks_exact(2)) {
            out.push((a[0] + a[1] + b[0] + b[1]) * 0.25);
        }
    }
    out
}

/// Average of every source pixel of each coarse pixel.
fn coarse(width: usize, height: usize, plane: &[f32], block: usize) -> (usize, usize, Vec<f32>) {
    let (w, h) = (width.div_ceil(block), height.div_ceil(block));
    let mut sums = vec![0_f64; w * h];
    let mut counts = vec![0_u32; w * h];
    for (y, row) in plane.chunks_exact(width).enumerate() {
        for (x, &value) in row.iter().enumerate() {
            let at = y / block * w + x / block;
            sums[at] += f64::from(value);
            counts[at] += 1;
        }
    }
    let means = sums
        .iter()
        .zip(&counts)
        .map(|(s, &n)| (s / f64::from(n)) as f32)
        .collect();
    (w, h, means)
}

fn quantile(sorted: &[f32], share: f64) -> f32 {
    sorted[((sorted.len() - 1) as f64 * share).round() as usize]
}

fn blur_line(line: &[f32], radius: usize, out: &mut [f32]) {
    let (n, r) = (line.len() as isize, radius as isize);
    let at = |i: isize| f64::from(line[i.clamp(0, n - 1) as usize]);
    let mut sum: f64 = (-r..=r).map(at).sum();
    let width = (2 * r + 1) as f64;
    for i in 0..n {
        out[i as usize] = (sum / width) as f32;
        sum += at(i + r + 1) - at(i - r);
    }
}

fn blur(width: usize, height: usize, data: &mut [f32]) {
    let radius = (width.max(height) as f32 * SHAPE_SIGMA).round().max(1.) as usize;
    let mut line = vec![0.; width.max(height)];
    let mut out = vec![0.; width.max(height)];
    for _ in 0..SHAPE_PASSES {
        for row in data.chunks_exact_mut(width) {
            blur_line(row, radius, &mut out[..width]);
            row.copy_from_slice(&out[..width]);
        }
        for x in 0..width {
            for y in 0..height {
                line[y] = data[y * width + x];
            }
            blur_line(&line[..height], radius, &mut out[..height]);
            for y in 0..height {
                data[y * width + x] = out[y];
            }
        }
    }
}

/// The field's large-scale shape at full size, by bilinear reading of `shape`.
fn enlarge(shape: &[f32], w: usize, h: usize, block: usize, x: usize, y: usize) -> f32 {
    let position = |v: usize, n: usize| {
        let p = ((v as f32 + 0.5) / block as f32 - 0.5).clamp(0., (n - 1) as f32);
        let low = (p as usize).min(n.saturating_sub(2));
        (low, (low + 1).min(n - 1), p - low as f32)
    };
    let (x0, x1, fx) = position(x, w);
    let (y0, y1, fy) = position(y, h);
    let row = |line: usize| shape[line * w + x0] * (1. - fx) + shape[line * w + x1] * fx;
    row(y0) * (1. - fy) + row(y1) * fy
}

fn vignetting(width: usize, height: usize, plane: &[f32]) -> (Vec<f32>, String) {
    // The range comes from averaged pixels, so noise cannot widen it.
    let block = (width.max(height) / RANGE_EDGE).max(1);
    let (_, _, mut sorted) = coarse(width, height, plane, block);
    sorted.sort_unstable_by(f32::total_cmp);
    let median = quantile(&sorted, 0.5);
    let low = quantile(&sorted, VIGNETTING_TAIL);
    let high = quantile(&sorted, 1. - VIGNETTING_TAIL).max(low + f32::EPSILON);
    let pixels = plane
        .iter()
        .map(|v| ((v - low) / (high - low)).clamp(0., 1.))
        .collect();
    let percent = |v: f32| (f64::from(v) / f64::from(median).max(f64::MIN_POSITIVE) - 1.) * 100.;
    (
        pixels,
        format!(
            "Vignetting · {:+.1} to {:+.1} % of the median",
            percent(low),
            percent(high)
        ),
    )
}

fn dust(width: usize, height: usize, plane: &[f32]) -> (Vec<f32>, String) {
    let block = (width.max(height) / SHAPE_EDGE).max(1);
    let (w, h, mut shape) = coarse(width, height, plane, block);
    blur(w, h, &mut shape);
    let mut pixels = Vec::with_capacity(plane.len());
    for (y, row) in plane.chunks_exact(width).enumerate() {
        for (x, &value) in row.iter().enumerate() {
            let level = enlarge(&shape, w, h, block, x, y);
            let ratio = if level > 0. { value / level } else { 1. };
            pixels.push(((ratio - 1. + DUST_SPAN) / (2. * DUST_SPAN)).clamp(0., 1.));
        }
    }
    (
        pixels,
        format!(
            "Dust · large-scale falloff removed · ±{:.0} %",
            DUST_SPAN * 100.
        ),
    )
}

/// Render `plane` for `view`; None for the raw view or an unusable plane.
pub fn render(
    width: usize,
    height: usize,
    plane: &[f32],
    cfa: bool,
    view: FlatView,
) -> Option<FlatRender> {
    if view == FlatView::Raw || width < 2 || height < 2 || plane.len() != width * height {
        return None;
    }
    let binned = cfa.then(|| cells(width, height, plane));
    let (w, h, source) = match &binned {
        Some(cells) => (width / 2, height / 2, cells.as_slice()),
        None => (width, height, plane),
    };
    let (pixels, note) = match view {
        FlatView::Dust => dust(w, h, source),
        _ => vignetting(w, h, source),
    };
    Some(FlatRender {
        width: w,
        height: h,
        pixels,
        note: if cfa {
            format!("{note} · 2 × 2 average")
        } else {
            note
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RGGB flat with 2.5x channel differences, a corner falloff and a shadow.
    fn flat(width: usize, height: usize, shadow: f32) -> Vec<f32> {
        let mut plane = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let (dx, dy) = (
                    x as f32 / width as f32 - 0.5,
                    y as f32 / height as f32 - 0.5,
                );
                let falloff = 1. - 0.2 * (dx * dx + dy * dy);
                let (sx, sy) = (
                    x as f32 - width as f32 * 0.7,
                    y as f32 - height as f32 * 0.4,
                );
                let dust = if sx * sx + sy * sy < 10_000. {
                    1. - shadow
                } else {
                    1.
                };
                let level = [38_000., 31_000., 15_000., 38_000.][x % 2 + 2 * (y % 2)];
                plane.push(level * falloff * dust);
            }
        }
        plane
    }

    /// Largest step between neighbouring pixels, the mark of a fine pattern.
    fn roughness(width: usize, values: &[f32]) -> f32 {
        values
            .chunks_exact(width)
            .flat_map(|row| row.windows(2).map(|p| (p[1] - p[0]).abs()))
            .fold(0., f32::max)
    }

    #[test]
    fn views_have_no_bayer_pattern_and_keep_falloff_and_dust() {
        let (width, height) = (1200, 800);
        let plane = flat(width, height, 0.);
        let v = render(width, height, &plane, true, FlatView::Vignetting).unwrap();
        assert_eq!((v.width, v.height), (600, 400));
        assert!(v.pixels.iter().all(|p| (0. ..=1.).contains(p)));
        assert!(roughness(v.width, &v.pixels) < 0.02);
        let (corner, middle) = (v.pixels[0], v.pixels[200 * 600 + 300]);
        assert!(corner < 0.1 && middle > 0.9, "{corner} {middle}");

        let d = render(width, height, &plane, true, FlatView::Dust).unwrap();
        assert!(d.note.contains("2 × 2 average"));
        // The middle of the field is level; the blur runs out at the very edge.
        for y in 100..300 {
            for x in 150..450 {
                assert!((d.pixels[y * 600 + x] - 0.5).abs() < 0.1);
            }
        }

        // A 1 % shadow stands out from its surroundings in the dust view.
        let shadowed = flat(width, height, 0.01);
        let d = render(width, height, &shadowed, true, FlatView::Dust).unwrap();
        let inside = d.pixels[160 * 600 + 420];
        let outside = d.pixels[160 * 600 + 300];
        assert!(outside - inside > 0.1, "{inside} {outside}");
    }

    #[test]
    fn mono_keeps_full_size_and_raw_has_no_rendering() {
        let plane = vec![100.; 64 * 48];
        let v = render(64, 48, &plane, false, FlatView::Dust).unwrap();
        assert_eq!((v.width, v.height), (64, 48));
        assert!(!v.note.contains("2 × 2"));
        assert!(render(64, 48, &plane, true, FlatView::Raw).is_none());
    }

    #[test]
    #[ignore = "needs FULGURSTACK_TEST_FLAT and FULGURSTACK_TEST_FLAT_PNG"]
    fn real_flat_renders() {
        let path = std::env::var_os("FULGURSTACK_TEST_FLAT").unwrap();
        let out = std::path::PathBuf::from(std::env::var_os("FULGURSTACK_TEST_FLAT_PNG").unwrap());
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let plane = fulgur_core::fits::read_reference_plane(path.as_ref(), &cancel).unwrap();
        for view in [FlatView::Vignetting, FlatView::Dust] {
            let start = std::time::Instant::now();
            let r = render(plane.width, plane.height, &plane.pixels, true, view).unwrap();
            let seconds = start.elapsed().as_secs_f64();
            let thumbnail =
                crate::preview::Thumbnail::from_display_plane(r.width, r.height, &r.pixels);
            let bytes: Vec<u8> = thumbnail
                .pixels
                .iter()
                .map(|p| (p[0] * 255.).round() as u8)
                .collect();
            image::GrayImage::from_raw(thumbnail.width as u32, thumbnail.height as u32, bytes)
                .unwrap()
                .save(out.with_extension(format!("{}.png", view.key())))
                .unwrap();
            println!("{}: {seconds:.3} s, {}", view.key(), r.note);
        }
    }
}
