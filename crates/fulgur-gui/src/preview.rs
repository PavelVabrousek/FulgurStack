//! Display-only bounded samples. No preview is ever written back to FITS.
use fulgur_core::{DemosaicMethod, Result, RgbImage, check_cancelled};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

/// Long edge of an overview: enough for the full image on a large screen.
pub const PREVIEW_EDGE: usize = 2048;
/// Samples behind an auto stretch; more would only cost display time.
const STRETCH_SAMPLES: usize = 100_000;

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct DisplayOptions {
    pub stretch: bool,
    pub channel: Option<usize>,
    pub map_max: f32,
}
impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            stretch: false,
            channel: None,
            map_max: 1.,
        }
    }
}

#[derive(Clone, Copy)]
pub struct MapChannelStats {
    pub minimum: f32,
    pub maximum: f32,
    pub mean: f64,
    pub nonzero_pixels: usize,
}

pub struct Thumbnail {
    pub width: usize,
    pub height: usize,
    pub native_size: [usize; 2],
    pub pixels: Vec<[f32; 3]>,
    pub map_stats: Option<[MapChannelStats; 3]>,
    /// Auto stretch from unaveraged samples, so that it also fits native detail.
    stretch: Option<[(f32, f32); 3]>,
}
impl Thumbnail {
    /// Grey overview of one calibration plane, in ADU or normalized 0..1.
    pub fn from_plane(width: usize, height: usize, plane: &[f32]) -> Self {
        let factor = (width.max(height) as f64 / PREVIEW_EDGE as f64).max(1.);
        let (w, h) = (
            (width as f64 / factor).round().max(1.) as usize,
            (height as f64 / factor).round().max(1.) as usize,
        );
        let maximum = plane.iter().copied().fold(0_f32, f32::max);
        let divisor = if maximum > 1.5 { 65535. } else { 1. };
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let sx = ((x as f64 + 0.5) * width as f64 / w as f64).floor() as usize;
                let sy = ((y as f64 + 0.5) * height as f64 / h as f64).floor() as usize;
                let v =
                    (plane[sy.min(height - 1) * width + sx.min(width - 1)] / divisor).clamp(0., 1.);
                pixels.push([v; 3]);
            }
        }
        Self {
            width: w,
            height: h,
            native_size: [width, height],
            pixels,
            map_stats: None,
            stretch: None,
        }
    }

    /// Grey overview of a plane already mapped to 0..1; every source pixel
    /// counts, so nothing fine can beat against the sampling.
    pub fn from_display_plane(width: usize, height: usize, plane: &[f32]) -> Self {
        let factor = (width.max(height) as f64 / PREVIEW_EDGE as f64).max(1.);
        let (w, h) = (
            (width as f64 / factor).round().max(1.) as usize,
            (height as f64 / factor).round().max(1.) as usize,
        );
        let mut sums = vec![(0_f64, 0_u32); w * h];
        for (y, row) in plane.chunks_exact(width).enumerate() {
            let line = (y * h / height) * w;
            for (x, &value) in row.iter().enumerate() {
                let sum = &mut sums[line + x * w / width];
                sum.0 += f64::from(value);
                sum.1 += 1;
            }
        }
        Self {
            width: w,
            height: h,
            native_size: [width, height],
            pixels: sums
                .iter()
                .map(|(sum, count)| [(sum / f64::from((*count).max(1))) as f32; 3])
                .collect(),
            map_stats: None,
            stretch: None,
        }
    }

    /// Overview that averages every source pixel, so that fine structure stays
    /// truthful: neither lost between samples nor exaggerated by peaks.
    pub fn from_image(image: &RgbImage, sensor_units: bool) -> Self {
        let factor = (image.width.max(image.height) as f64 / PREVIEW_EDGE as f64).max(1.);
        let width = (image.width as f64 / factor).round().max(1.) as usize;
        let height = (image.height as f64 / factor).round().max(1.) as usize;
        let divisor = if sensor_units { 65535. } else { 1. };
        let mut sums = vec![([0_f64; 3], 0_u32); width * height];
        for (y, row) in image.pixels.chunks_exact(image.width).enumerate() {
            let line = (y * height / image.height) * width;
            for (x, pixel) in row.iter().enumerate() {
                let (sum, count) = &mut sums[line + x * width / image.width];
                for (total, value) in sum.iter_mut().zip(pixel) {
                    *total += f64::from((value / divisor).clamp(0., 1.));
                }
                *count += 1;
            }
        }
        let step = (image.pixels.len() / STRETCH_SAMPLES).max(1) | 1;
        let samples: Vec<[f32; 3]> = image
            .pixels
            .iter()
            .step_by(step)
            .map(|p| p.map(|v| (v / divisor).clamp(0., 1.)))
            .collect();
        Self {
            width,
            height,
            native_size: [image.width, image.height],
            pixels: sums
                .iter()
                .map(|(sum, count)| sum.map(|v| (v / f64::from((*count).max(1))) as f32))
                .collect(),
            map_stats: None,
            stretch: Some(stretch_of(&samples)),
        }
    }
    /// Overview and display-only summary of a map. The full scientific image
    /// is read without modifying or retaining it.
    pub fn from_map(image: &RgbImage) -> Self {
        let mut thumbnail = Self::from_image(image, false);
        let mut stats = [MapChannelStats {
            minimum: 1.,
            maximum: 0.,
            mean: 0.,
            nonzero_pixels: 0,
        }; 3];
        for pixel in &image.pixels {
            for channel in 0..3 {
                let value = pixel[channel].clamp(0., 1.);
                stats[channel].minimum = stats[channel].minimum.min(value);
                stats[channel].maximum = stats[channel].maximum.max(value);
                stats[channel].mean += f64::from(value);
                stats[channel].nonzero_pixels += usize::from(value > 0.);
            }
        }
        for channel in &mut stats {
            channel.mean /= image.pixels.len().max(1) as f64;
        }
        thumbnail.map_stats = Some(stats);
        thumbnail
    }
    pub fn stretch_parameters(&self) -> [(f32, f32); 3] {
        self.stretch.unwrap_or_else(|| {
            let step = (self.pixels.len() / STRETCH_SAMPLES).max(1) | 1;
            let samples: Vec<[f32; 3]> = self.pixels.iter().step_by(step).copied().collect();
            stretch_of(&samples)
        })
    }
    pub fn display(&self, stretch: bool, map_channel: Option<usize>) -> eframe::egui::ColorImage {
        self.display_with_options(DisplayOptions {
            stretch,
            channel: map_channel,
            ..DisplayOptions::default()
        })
    }
    pub(crate) fn display_with_options(&self, options: DisplayOptions) -> eframe::egui::ColorImage {
        let params = if options.stretch && options.channel.is_none() {
            self.stretch_parameters()
        } else {
            [(0., 0.5); 3]
        };
        let mut bytes = Vec::with_capacity(self.pixels.len() * 3);
        for rgb in &self.pixels {
            bytes.extend(display_pixel(*rgb, params, options));
        }
        eframe::egui::ColorImage::from_rgb([self.width, self.height], &bytes)
    }
}

fn stretch_of(samples: &[[f32; 3]]) -> [(f32, f32); 3] {
    let mut params = [(0., 0.5); 3];
    for (channel, p) in params.iter_mut().enumerate() {
        let mut values: Vec<f32> = samples.iter().map(|v| v[channel]).collect();
        values.sort_unstable_by(f32::total_cmp);
        let median = values.get(values.len() / 2).copied().unwrap_or(0.);
        let mut deviations: Vec<f32> = values.iter().map(|v| (v - median).abs()).collect();
        deviations.sort_unstable_by(f32::total_cmp);
        let sigma = deviations.get(deviations.len() / 2).copied().unwrap_or(0.) * 1.4826;
        let black = (median - 2.8 * sigma).clamp(0., 0.99);
        let x = ((median - black) / (1. - black)).clamp(0.000001, 0.999999);
        let target = 0.18;
        let mid = (x * (target - 1.)) / (2. * target * x - target - x);
        *p = (black, mid.clamp(0.000001, 0.999999));
    }
    params
}

/// Workers of a preview task: quick on a click, without starving the desktop.
pub(crate) fn preview_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).clamp(1, 8))
}
/// The same display transform is used for the overview and native-resolution crops.
pub(crate) fn display_pixel(
    rgb: [f32; 3],
    params: [(f32, f32); 3],
    options: DisplayOptions,
) -> [u8; 3] {
    std::array::from_fn(|channel| {
        let mut value = rgb[options.channel.unwrap_or(channel)];
        if options.channel.is_some() {
            value /= options.map_max.max(f32::EPSILON);
        }
        if options.stretch && options.channel.is_none() {
            let (black, mid) = params[channel];
            value = ((value - black) / (1. - black)).clamp(0., 1.);
            let denominator = (2. * mid - 1.) * value - mid;
            if denominator != 0. {
                value = (mid - 1.) * value / denominator;
            }
        }
        (value.clamp(0., 1.) * 255.).round() as u8
    })
}

pub fn subexposure(
    path: &Path,
    layout: fulgur_core::LayoutChoice,
    cancel: &AtomicBool,
) -> Result<Thumbnail> {
    check_cancelled(cancel)?;
    let header = fulgur_core::fits::read_header(path)?;
    // One worker, never concurrent with integration: a 62 Mpx full-frame
    // sensor needs about 1 GB transiently (raw, cosmetic copy and RGB).
    if header.info.width.saturating_mul(header.info.height) > 64_000_000 {
        return Err(fulgur_core::Error::Invalid(
            "Subexposure preview is limited to 64 million source pixels.",
        ));
    }
    if header.info.mosaic == fulgur_core::Mosaic::Rgb {
        let (width, height) = (header.info.width, header.info.height);
        let rgb = fulgur_core::fits::read_rgb_roi(&header, [0, 0, width, height], cancel, vec![])?;
        return Ok(Thumbnail::from_image(&rgb, true));
    }
    let mut raw = fulgur_core::fits::read_raw(path)?;
    raw.header.info.apply_layout(layout);
    check_cancelled(cancel)?;
    // A few workers, never concurrent with integration, own one full source.
    let (rgb, _) = fulgur_pipeline::debayer::debayer(&raw, cancel, DemosaicMethod::Vng)?;
    Ok(Thumbnail::from_image(&rgb, true))
}

pub struct PreviewTask {
    pub path: PathBuf,
    pub result: mpsc::Receiver<Result<Thumbnail>>,
    cancel: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl PreviewTask {
    pub fn start(path: PathBuf, layout: fulgur_core::LayoutChoice) -> Self {
        let (tx, result) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let c = cancel.clone();
        let handle = std::thread::spawn(move || {
            let result = rayon::ThreadPoolBuilder::new()
                .num_threads(preview_threads())
                .build()
                .map_err(|_| fulgur_core::Error::Invalid("Could not start preview worker."))
                .and_then(|pool| pool.install(|| subexposure(&p, layout, &c)));
            let _ = tx.send(result);
        });
        Self {
            path,
            result,
            cancel,
            handle: Some(handle),
        }
    }
}
impl Drop for PreviewTask {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs FULGURSTACK_TEST_LIGHT"]
    fn real_frame_preview_speed() {
        let path = PathBuf::from(std::env::var_os("FULGURSTACK_TEST_LIGHT").unwrap());
        let cancel = AtomicBool::new(false);
        for threads in [1, preview_threads()] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let start = std::time::Instant::now();
            let thumb = pool
                .install(|| subexposure(&path, fulgur_core::LayoutChoice::Auto, &cancel))
                .unwrap();
            println!(
                "{threads} workers: {:.2} s, overview {} x {}",
                start.elapsed().as_secs_f64(),
                thumb.width,
                thumb.height
            );
        }
    }

    #[test]
    fn overview_averages_every_pixel() {
        // One bright pixel in sixteen must show as its share, not vanish or fill.
        let mut pixels = vec![[0.1_f32; 3]; 4096 * 4096];
        for (i, p) in pixels.iter_mut().enumerate() {
            if i % 4 == 0 && (i / 4096) % 4 == 0 {
                *p = [0.9; 3];
            }
        }
        let image = RgbImage {
            width: 4096,
            height: 4096,
            demosaic_method: DemosaicMethod::Vng,
            pixels,
        };
        let thumb = Thumbnail::from_image(&image, false);
        assert_eq!((thumb.width, thumb.height), (2048, 2048));
        let total: f64 = thumb.pixels.iter().map(|p| f64::from(p[0])).sum();
        let mean = total / thumb.pixels.len() as f64;
        assert!((mean - 0.15).abs() < 1e-3, "{mean}");
    }

    #[test]
    fn display_preserves_linear_samples_dimensions_channels_and_bounds() {
        let image = RgbImage {
            width: 4096,
            height: 2048,
            demosaic_method: DemosaicMethod::Vng,
            pixels: vec![[6553.5, 13107., 19660.5]; 4096 * 2048],
        };
        let thumb = Thumbnail::from_image(&image, true);
        assert_eq!((thumb.width, thumb.height), (2048, 1024));
        let before = thumb.pixels.clone();
        let plain = thumb.display(false, None);
        let stretched = thumb.display(true, None);
        let blue = thumb.display(false, Some(2));
        assert_eq!(plain.size, stretched.size);
        assert_ne!(plain.pixels, stretched.pixels);
        assert_eq!(blue.pixels[0].r(), blue.pixels[0].b());
        assert_eq!(thumb.pixels, before);
        assert_eq!(image.pixels[0], [6553.5, 13107., 19660.5]);
    }
}
