//! Star size, shape and sky level of a stacked master (ADR-086), measured
//! with the same detector as the input frames so both can be compared.

use crate::register::RegistrationReport;
use crate::stars::{median, plane_sky, scan_plane};
use fulgur_core::{Result, RgbImage, check_cancelled};
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// Samples per channel for the covered-sky medians; bounded work on any size.
const SKY_SAMPLES: usize = 1 << 20;

#[derive(Clone, Debug, Serialize)]
pub struct MasterQuality {
    /// "green" (colour master) or "mono".
    pub plane: &'static str,
    pub method: &'static str,
    pub measured_stars: usize,
    pub detected_stars: usize,
    pub hfr_px: Option<f64>,
    pub fwhm_px: Option<f64>,
    pub eccentricity: Option<f64>,
    /// Local-sky median of the measurement plane, in reference-anchor ADU.
    pub background_adu: f64,
    /// The same on the 0..1 scale of the master FITS (ADU / 65535).
    pub background_normalized: f64,
    /// Median of covered pixels per channel (R, G, B; mono: three equal values).
    pub channel_median_adu: [f64; 3],
    /// Local sky level per channel (R, G, B), robust to stars and nebulae.
    pub channel_background_adu: [f64; 3],
    /// Per-pixel sky noise per channel (R, G, B): robust standard deviation
    /// around the local sky; for a mono master the three values are equal.
    pub channel_noise_adu: [f64; 3],
    /// The same on the 0..1 scale of the master FITS (ADU / 65535).
    pub channel_noise_normalized: [f64; 3],
    /// Sky noise of the 2x2-mean detection grid, as for the input frames.
    pub noise_adu: f64,
    /// Medians of the stacked frames in master pixels, for comparison.
    pub frames_hfr_px: Option<f64>,
    pub frames_fwhm_px: Option<f64>,
    pub frames_eccentricity: Option<f64>,
    pub seconds: f64,
}

/// Measure the master before export bounding. Pixels outside the stacked
/// footprint (no surviving sample in any channel) are filled with the sky
/// median so the canvas edge never looks like a star or a gradient.
pub fn measure(
    image: &RgbImage,
    coverage: &RgbImage,
    mono: bool,
    plan: &RegistrationReport,
    cancel: &AtomicBool,
) -> Result<MasterQuality> {
    let mut sets: Vec<usize> = plan.inspection.frames.iter().map(|f| f.light_set).collect();
    sets.sort_unstable();
    sets.dedup();
    measure_sets(image, coverage, mono, plan, &sets, cancel)
}

/// As `measure`, comparing with the frames of the given light sets only.
pub fn measure_sets(
    image: &RgbImage,
    coverage: &RgbImage,
    mono: bool,
    plan: &RegistrationReport,
    sets: &[usize],
    cancel: &AtomicBool,
) -> Result<MasterQuality> {
    let start = Instant::now();
    // Stars clip in the master where they clipped in its frames.
    let saturation = plan
        .inspection
        .frames
        .iter()
        .filter(|q| sets.contains(&q.light_set))
        .filter_map(|q| q.frame.as_ref()?.saturation_adu)
        .fold(None, |top: Option<f32>, v| {
            Some(top.map_or(v, |t| t.max(v)))
        });
    let mut quality = measure_image(image, coverage, mono, saturation, cancel)?;
    // Frame measurements are in source pixels; an external geometry
    // reference resamples them onto its own pixel grid.
    let scale = plan
        .external_reference
        .as_ref()
        .map_or(1., |r| r.anchor_to_reference.scale_factor);
    let accepted: Vec<_> = plan
        .frames
        .iter()
        .zip(&plan.inspection.frames)
        .filter(|(f, q)| f.accepted && sets.contains(&q.light_set))
        .map(|(_, q)| q)
        .collect();
    quality.frames_hfr_px = of(accepted.iter().filter_map(|q| q.hfr_px).map(|v| v * scale));
    quality.frames_fwhm_px = of(accepted
        .iter()
        .filter_map(|q| q.moment_fwhm_px)
        .map(|v| v * scale));
    quality.frames_eccentricity = of(accepted.iter().filter_map(|q| q.eccentricity));
    quality.seconds = start.elapsed().as_secs_f64();
    Ok(quality)
}

fn of(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values: Vec<f64> = values.filter(|v| v.is_finite()).collect();
    (!values.is_empty()).then(|| median(&mut values))
}

/// The master's own measurement, without the frame comparison.
pub(crate) fn measure_image(
    image: &RgbImage,
    coverage: &RgbImage,
    mono: bool,
    saturation: Option<f32>,
    cancel: &AtomicBool,
) -> Result<MasterQuality> {
    let start = Instant::now();
    let covered = |i: usize| coverage.pixels[i].iter().any(|&c| c > 0.);
    let stride = (image.pixels.len() / SKY_SAMPLES).max(1);
    let mut channel_median_adu = [0.; 3];
    for (channel, value) in channel_median_adu.iter_mut().enumerate() {
        check_cancelled(cancel)?;
        *value = of((0..image.pixels.len())
            .step_by(stride)
            .filter(|&i| covered(i))
            .map(|i| f64::from(image.pixels[i][channel])))
        .unwrap_or(0.);
    }
    let mut channel_background_adu = [0.; 3];
    let mut channel_noise_adu = [0.; 3];
    for channel in 0..3 {
        check_cancelled(cancel)?;
        let (level, noise) = plane_sky(image.width, image.height, |x, y| {
            let i = y * image.width + x;
            covered(i).then(|| f64::from(image.pixels[i][channel]))
        })
        .unwrap_or((channel_median_adu[channel], 0.));
        channel_background_adu[channel] = level;
        channel_noise_adu[channel] = noise;
    }
    let divisor = f64::from(fulgur_core::output::OUTPUT_ADU_DIVISOR);
    let fill = channel_median_adu[1] as f32;
    let plane: Vec<f32> = image
        .pixels
        .iter()
        .enumerate()
        .map(|(i, p)| if covered(i) { p[1] } else { fill })
        .collect();
    check_cancelled(cancel)?;
    let metrics = scan_plane(
        &plane,
        image.width,
        image.height,
        None,
        cancel,
        crate::stars::CATALOG_STARS,
        false,
        saturation,
        &mut Vec::new(),
    )?;
    let stars = &metrics.stars;
    Ok(MasterQuality {
        plane: if mono { "mono" } else { "green" },
        method: "frame detector on the master plane (2x2 detection grid, hot-pixel guard, 250-star catalogue); medians over measured stars; uncovered canvas filled with the sky median",
        measured_stars: stars.len(),
        detected_stars: metrics.detected_stars,
        hfr_px: of(stars.iter().map(|s| s.hfr_px)),
        fwhm_px: of(stars.iter().map(|s| s.moment_fwhm_px)),
        eccentricity: of(stars.iter().map(|s| s.eccentricity)),
        background_adu: metrics.background,
        background_normalized: metrics.background
            / f64::from(fulgur_core::output::OUTPUT_ADU_DIVISOR),
        channel_median_adu,
        channel_background_adu,
        channel_noise_adu,
        channel_noise_normalized: channel_noise_adu.map(|n| n / divisor),
        noise_adu: metrics.noise,
        frames_hfr_px: None,
        frames_fwhm_px: None,
        frames_eccentricity: None,
        seconds: start.elapsed().as_secs_f64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fulgur_core::DemosaicMethod;

    /// Gaussian stars of known size on a 1000 ADU sky with mild noise, an
    /// uncovered 20 px border at zero, and a colour cast in R and B.
    fn master(sigma_x: f64, sigma_y: f64) -> (RgbImage, RgbImage) {
        let (w, h) = (640, 480);
        let mut pixels = vec![[0_f32; 3]; w * h];
        let mut coverage = vec![[0_f32; 3]; w * h];
        for y in 20..h - 20 {
            for x in 20..w - 20 {
                let noise = ((x * 7919 + y * 104_729) % 97) as f64 / 97. * 6. - 3.;
                let mut v = 1000. + noise;
                for cy in (60..h - 40).step_by(60) {
                    for cx in (60..w - 40).step_by(60) {
                        let dx = (x as f64 - cx as f64 - 0.3) / sigma_x;
                        let dy = (y as f64 - cy as f64 - 0.2) / sigma_y;
                        v += 8000. * (-0.5 * (dx * dx + dy * dy)).exp();
                    }
                }
                pixels[y * w + x] = [v as f32 * 0.8, v as f32, v as f32 * 0.6];
                coverage[y * w + x] = [1.; 3];
            }
        }
        let image = |pixels| RgbImage {
            width: w,
            height: h,
            demosaic_method: DemosaicMethod::Vng,
            pixels,
        };
        (image(pixels), image(coverage))
    }

    #[test]
    fn known_gaussian_stars_give_their_size_shape_and_sky() {
        let cancel = AtomicBool::new(false);
        let (image, coverage) = master(1.6, 1.6);
        let q = measure_image(&image, &coverage, false, None, &cancel).unwrap();
        assert!(q.measured_stars >= 50, "{}", q.measured_stars);
        // Gaussian: FWHM = 2.355 sigma, HFR = 1.177 sigma.
        let fwhm = q.fwhm_px.unwrap();
        let hfr = q.hfr_px.unwrap();
        assert!((fwhm - 2.355 * 1.6).abs() < 0.4, "FWHM {fwhm}");
        assert!((hfr - 1.177 * 1.6).abs() < 0.4, "HFR {hfr}");
        assert!(q.eccentricity.unwrap() < 0.3, "{:?}", q.eccentricity);
        assert!(
            (q.background_adu - 1000.).abs() < 5.,
            "{}",
            q.background_adu
        );
        assert!((q.background_normalized - 1000. / 65535.).abs() < 1e-4);
        // The zero border is not sky: channel medians follow the colour cast.
        // Per-pixel noise: the synthetic sky is uniform in -3..3 ADU (sigma
        // 1.73 by MAD on a uniform law: 1.4826 * 1.5 = 2.2), scaled per channel.
        let [nr, ng, nb] = q.channel_noise_adu;
        assert!((ng - 2.2).abs() < 0.4, "noise G {ng}");
        assert!(
            (nr / ng - 0.8).abs() < 0.05 && (nb / ng - 0.6).abs() < 0.05,
            "{nr} {nb}"
        );
        let [br, bg, bb] = q.channel_background_adu;
        assert!((bg - 1000.).abs() < 3. && (br - 800.).abs() < 3. && (bb - 600.).abs() < 3.);
        let [r, g, b] = q.channel_median_adu;
        assert!((g - 1000.).abs() < 10. && (r - 800.).abs() < 10. && (b - 600.).abs() < 10.);
        assert_eq!(q.plane, "green");
        // Elongated stars: axis ratio 2 gives eccentricity sqrt(1 - 1/4) = 0.87.
        let (image, coverage) = master(2.4, 1.2);
        let e = measure_image(&image, &coverage, true, None, &cancel)
            .unwrap()
            .eccentricity
            .unwrap();
        assert!((e - 0.866).abs() < 0.1, "eccentricity {e}");
    }
}
