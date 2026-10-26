//! Display-only bounded samples. No preview is ever written back to FITS.
use fulgur_core::{DemosaicMethod, Result, RgbImage, check_cancelled};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub const PREVIEW_EDGE: usize = 640;

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
}
impl Thumbnail {
    pub fn from_image(image: &RgbImage, sensor_units: bool) -> Self {
        let factor = (image.width.max(image.height) as f64 / PREVIEW_EDGE as f64).max(1.);
        let width = (image.width as f64 / factor).round().max(1.) as usize;
        let height = (image.height as f64 / factor).round().max(1.) as usize;
        let divisor = if sensor_units { 65535. } else { 1. };
        let mut pixels = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let sx = ((x as f64 + 0.5) * image.width as f64 / width as f64).floor() as usize;
                let sy = ((y as f64 + 0.5) * image.height as f64 / height as f64).floor() as usize;
                pixels.push(
                    image.pixels[sy.min(image.height - 1) * image.width + sx.min(image.width - 1)]
                        .map(|v| (v / divisor).clamp(0., 1.)),
                );
            }
        }
        Self {
            width,
            height,
            native_size: [image.width, image.height],
            pixels,
            map_stats: None,
        }
    }
    /// Display-only map summary and, for rejection, peak-preserving overview bins.
    /// The full scientific image is read without modifying or retaining it.
    pub fn from_map(image: &RgbImage, preserve_peaks: bool) -> Self {
        let mut thumbnail = Self::from_image(image, false);
        let mut stats = [MapChannelStats {
            minimum: 1.,
            maximum: 0.,
            mean: 0.,
            nonzero_pixels: 0,
        }; 3];
        if preserve_peaks {
            thumbnail.pixels.fill([0.; 3]);
        }
        for y in 0..image.height {
            let destination_row = (y * thumbnail.height / image.height) * thumbnail.width;
            for x in 0..image.width {
                let pixel = image.pixels[y * image.width + x];
                let destination = destination_row + x * thumbnail.width / image.width;
                for channel in 0..3 {
                    let value = pixel[channel].clamp(0., 1.);
                    stats[channel].minimum = stats[channel].minimum.min(value);
                    stats[channel].maximum = stats[channel].maximum.max(value);
                    stats[channel].mean += f64::from(value);
                    stats[channel].nonzero_pixels += usize::from(value > 0.);
                    if preserve_peaks {
                        thumbnail.pixels[destination][channel] =
                            thumbnail.pixels[destination][channel].max(value);
                    }
                }
            }
        }
        for channel in &mut stats {
            channel.mean /= image.pixels.len().max(1) as f64;
        }
        thumbnail.map_stats = Some(stats);
        thumbnail
    }
    pub fn stretch_parameters(&self) -> [(f32, f32); 3] {
        let mut params = [(0., 0.5); 3];
        for (channel, p) in params.iter_mut().enumerate() {
            let mut values: Vec<f32> = self.pixels.iter().step_by(4).map(|v| v[channel]).collect();
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

pub fn subexposure(path: &Path, cancel: &AtomicBool) -> Result<Thumbnail> {
    check_cancelled(cancel)?;
    let header = fulgur_core::fits::read_header(path)?;
    if header.info.width.saturating_mul(header.info.height) > 16_000_000 {
        return Err(fulgur_core::Error::Invalid(
            "Subexposure preview is limited to 16 million source pixels.",
        ));
    }
    let raw = fulgur_core::fits::read_raw(path)?;
    check_cancelled(cancel)?;
    // A single worker, never concurrent with integration, owns one full source.
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
    pub fn start(path: PathBuf) -> Self {
        let (tx, result) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let c = cancel.clone();
        let handle = std::thread::spawn(move || {
            let result = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .map_err(|_| fulgur_core::Error::Invalid("Could not start preview worker."))
                .and_then(|pool| pool.install(|| subexposure(&p, &c)));
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
    fn display_preserves_linear_samples_dimensions_channels_and_bounds() {
        let image = RgbImage {
            width: 1000,
            height: 500,
            demosaic_method: DemosaicMethod::Vng,
            pixels: vec![[6553.5, 13107., 19660.5]; 500000],
        };
        let thumb = Thumbnail::from_image(&image, true);
        assert_eq!((thumb.width, thumb.height), (640, 320));
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
