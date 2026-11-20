//! The science steps every front end runs after registration (ADR-178).
//!
//! The command line and the application used to repeat these steps: choose
//! the grid's plate solution, read and calibrate the integration sources,
//! split them into masters, integrate, and publish each master with its
//! metadata, plate solution check, maps and quality. They now share this
//! module, so a master is made the same way whichever front end asks for it.
//! Front ends keep only their own concerns: console lines and the report
//! layout of the CLI, progress, previews and the Results page of the GUI.
use crate::astrometry::{GridSolution, MasterAstrometry};
use crate::calibration::Calibration;
use crate::integrate::{IntegrationConfig, IntegrationEngine, IntegrationResult};
use crate::master_groups::{MasterRun, MasterSplit};
use crate::master_quality::MasterQuality;
use crate::register::{Canvas, RegistrationReport, SkyOptions};
use fulgur_core::fits::ImageHeader;
use fulgur_core::output::{MasterMetadata, RgbExportStats};
use fulgur_core::{Error, Result};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// The grid's plate solution for the masters' WCS (ADR-155), or why a tried
/// solve gave none (ADR-158).
#[derive(Default)]
pub struct GridChoice {
    pub solution: Option<(GridSolution, SkyOptions)>,
    /// Set when a plate solution was wanted but could not be made.
    pub failure: Option<String>,
}

/// Finds the grid's plate solution: the sky registration's own grid, else
/// the reference frame solved now.
///
/// `options` is `None` when no catalogue was given (no WCS and no failure),
/// and `Some(Err(reason))` when the catalogue could not be prepared.
pub fn grid_solution(
    plan: &RegistrationReport,
    root: &Path,
    sets: &[crate::inspect::InputSet],
    options: Option<std::result::Result<SkyOptions, String>>,
    cancel: &AtomicBool,
) -> Result<GridChoice> {
    let options = match options {
        None => return Ok(GridChoice::default()),
        Some(Err(reason)) => {
            return Ok(GridChoice {
                solution: None,
                failure: Some(reason),
            });
        }
        Some(Ok(options)) => options,
    };
    let grid = match crate::astrometry::sky_grid(plan) {
        Some(grid) => Ok(grid),
        None => crate::astrometry::solve_grid(plan, root, sets, &options, cancel),
    };
    match grid {
        Ok(grid) => Ok(GridChoice {
            solution: Some((grid, options)),
            failure: None,
        }),
        Err(Error::Cancelled) => Err(Error::Cancelled),
        Err(error) => Ok(GridChoice {
            solution: None,
            failure: Some(error.to_string()),
        }),
    }
}

/// Reads and calibrates the accepted frames and splits them into masters.
pub fn plan_masters(
    plan: &RegistrationReport,
    root: &Path,
    calibrations: &[Option<Arc<Calibration>>],
    split: MasterSplit,
) -> Result<Vec<MasterRun>> {
    let sources = crate::integrate::calibrated_sources(root, plan, calibrations)?;
    fulgur_core::telemetry::timeline("integration sources read");
    let runs = crate::master_groups::runs(plan, sources, split)?;
    fulgur_core::telemetry::timeline("master groups planned");
    Ok(runs)
}

/// The calibration formulas of the light sets, each named once, for the
/// master's header.
pub fn calibration_formula(calibrations: &[Option<Arc<Calibration>>]) -> Option<String> {
    let mut formulas: Vec<&str> = vec![];
    for calibration in calibrations.iter().flatten() {
        let formula = calibration.summary.formula.as_str();
        if !formulas.contains(&formula) {
            formulas.push(formula);
        }
    }
    (!formulas.is_empty()).then(|| formulas.join("; "))
}

/// What every master of one stack shares.
pub struct MasterContext<'a> {
    pub plan: &'a RegistrationReport,
    pub root: &'a Path,
    pub canvas: &'a Canvas,
    pub config: &'a IntegrationConfig,
    pub engine: &'a dyn IntegrationEngine,
    pub grid: &'a GridChoice,
    pub calibration_formula: Option<String>,
    pub cancel: &'a AtomicBool,
}

/// Where one master and its optional maps go.
pub struct MasterTargets<'a> {
    pub master: &'a Path,
    pub coverage: Option<&'a Path>,
    pub high_rejection: Option<&'a Path>,
}

/// One integrated and published master.
pub struct PublishedMaster {
    pub maps: IntegrationResult,
    /// Header of the photometric anchor, whose metadata the master keeps.
    pub anchor: ImageHeader,
    pub calibrated_frames: usize,
    pub export: RgbExportStats,
    pub astrometry: Option<MasterAstrometry>,
    pub master_quality: Option<MasterQuality>,
    /// Writing the master and its maps.
    pub write_seconds: f64,
}

/// Median DATE-OBS of the stacked frames with a readable capture time, as
/// microsecond UTC text, and how many frames it comes from. For an even count
/// it is halfway between the two central times (ADR-185).
fn median_date_obs<'a>(dates: impl IntoIterator<Item = &'a str>) -> Option<(String, usize)> {
    let mut times: Vec<i64> = dates
        .into_iter()
        .filter_map(crate::register::date_micros)
        .collect();
    if times.is_empty() {
        return None;
    }
    times.sort_unstable();
    let middle = times.len() / 2;
    let median = if times.len() % 2 == 1 {
        times[middle]
    } else {
        times[middle - 1] + (times[middle] - times[middle - 1]) / 2
    };
    let date = chrono::DateTime::from_timestamp_micros(median)?
        .naive_utc()
        .format("%Y-%m-%dT%H:%M:%S%.6f")
        .to_string();
    Some((date, times.len()))
}

/// Integrates one master, checks its plate solution, writes it and the maps
/// asked for, and measures it.
///
/// `progress` receives completed output tiles; without it the integration
/// prints its own console diagnostics. `written` receives the number of
/// files written so far, starting with 0 when writing begins.
pub fn publish_master(
    context: &MasterContext<'_>,
    run: &MasterRun,
    targets: &MasterTargets<'_>,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    written: &dyn Fn(usize),
) -> Result<PublishedMaster> {
    let cancel = context.cancel;
    // Drizzle integrates onto a grid `scale` times finer over the same area
    // (ADR-188); everything after integration works in its pixels.
    let drizzle_scale = context.config.drizzle.map(|d| d.scale);
    let drizzled;
    let canvas = match drizzle_scale {
        Some(k) => {
            let k = usize::from(k);
            let c = context.canvas;
            drizzled = Canvas {
                origin_x_px: c.origin_x_px * k as i32,
                origin_y_px: c.origin_y_px * k as i32,
                width: c.width * k,
                height: c.height * k,
                minimum_effective_weight_fraction: c.minimum_effective_weight_fraction,
                coordinate_rule: c.coordinate_rule,
            };
            &drizzled
        }
        None => context.canvas,
    };
    let anchor = fulgur_core::fits::read_header(&context.root.join(&run.anchor))?;
    let calibrated_frames = run
        .sources
        .iter()
        .filter(|s| s.calibration.is_some())
        .count();
    fulgur_core::telemetry::timeline("integration start");
    // A drizzle master is written tile by tile as it is made and never held
    // whole; what follows integration (WCS check, quality, preview) works on
    // it binned back to reference pixels (ADR-189).
    let mut sink = match drizzle_scale {
        Some(k) => Some(MasterSink::new(
            targets,
            canvas,
            usize::from(k),
            crate::integrate::output_method(&run.sources),
            run.input_frames,
        )?),
        None => None,
    };
    let mut maps = match (sink.as_mut(), progress) {
        (Some(sink), progress) => context.engine.integrate_into(
            &run.sources,
            canvas,
            context.config,
            run.input_frames,
            cancel,
            progress,
            sink,
        )?,
        (None, Some(progress)) => context.engine.integrate_with_progress(
            &run.sources,
            canvas,
            context.config,
            run.input_frames,
            cancel,
            progress,
        )?,
        (None, None) => context.engine.integrate_with_maps(
            &run.sources,
            canvas,
            context.config,
            run.input_frames,
            cancel,
        )?,
    };
    if let Some(sink) = &mut sink {
        let empty = || fulgur_core::RgbImage {
            width: 0,
            height: 0,
            demosaic_method: maps.image.demosaic_method,
            pixels: vec![],
        };
        let [image, coverage, high_rejection] =
            std::mem::replace(&mut sink.binned, [empty(), empty(), empty()]);
        maps.image = image;
        maps.coverage = coverage;
        maps.high_rejection = high_rejection;
    }
    fulgur_core::telemetry::timeline("integration done");
    let origin = [canvas.origin_x_px, canvas.origin_y_px];
    let astrometry = match &context.grid.solution {
        Some((grid, options)) => Some(match drizzle_scale {
            // Checked on the binned master in reference pixels, written for
            // the drizzle grid.
            Some(k) => {
                let mut checked = crate::astrometry::check_master(
                    &maps.image,
                    [context.canvas.origin_x_px, context.canvas.origin_y_px],
                    grid,
                    options,
                    cancel,
                )?;
                crate::astrometry::refine_cards(&mut checked, grid, f64::from(k), origin);
                checked
            }
            None => crate::astrometry::check_master(&maps.image, origin, grid, options, cancel)?,
        }),
        None => None,
    };
    let metadata = MasterMetadata {
        geometric_correction_frames: run.geometric_correction_frames,
        external_geometry_scale: context
            .plan
            .external_reference
            .as_ref()
            .map(|r| r.anchor_to_reference.scale_factor),
        accepted_frames: run.accepted_frames,
        rejection: maps.stats.rejection,
        total_exposure_seconds: run.total_exposure_seconds,
        origin_x_px: context.canvas.origin_x_px,
        origin_y_px: context.canvas.origin_y_px,
        monochrome_frames: run.monochrome_frames,
        calibrated_frames,
        calibration_formula: context.calibration_formula.clone(),
        astrometry: match &astrometry {
            Some(a) => Some(a.cards.clone().unwrap_or_else(crate::astrometry::unsolved)),
            None => context
                .grid
                .failure
                .as_ref()
                .map(|_| crate::astrometry::unsolved()),
        },
        drizzle: context
            .config
            .drizzle
            .map(|d| (d.scale, maps.stats.drizzle_rule.clone())),
        median_date_obs: median_date_obs(
            run.sources
                .iter()
                .filter_map(|s| s.header.info.date_obs.as_deref()),
        ),
    };
    fulgur_core::telemetry::timeline("master astrometry checked");
    written(0);
    let write_start = Instant::now();
    let export = match sink {
        Some(sink) => sink.finish(&anchor, &metadata, written, cancel)?,
        None => {
            let export = fulgur_core::output::write_master_atomic(
                targets.master,
                &maps.image,
                &anchor,
                &metadata,
                cancel,
            )?;
            let mut count = 1;
            written(count);
            for (path, image, kind) in [
                (targets.coverage, &maps.coverage, "COVERAGE"),
                (targets.high_rejection, &maps.high_rejection, "HIGHREJ"),
            ] {
                if let Some(path) = path {
                    fulgur_core::output::write_fraction_map_atomic(
                        path,
                        image,
                        kind,
                        run.input_frames,
                        Some(&metadata),
                        cancel,
                    )?;
                    count += 1;
                    written(count);
                }
            }
            export
        }
    };
    let write_seconds = write_start.elapsed().as_secs_f64();
    fulgur_core::telemetry::timeline("master and maps written");
    let mono = metadata.monochrome_frames == metadata.accepted_frames;
    let master_quality = match drizzle_scale {
        // Star sizes need the drizzle resolution: measured on a central
        // window read back from the written master, given in reference
        // pixels (ADR-189).
        Some(k) => drizzle_quality(targets, canvas, usize::from(k), mono, context, run, cancel),
        None => crate::master_quality::measure_sets(
            &maps.image,
            &maps.coverage,
            mono,
            context.plan,
            &run.sets,
            cancel,
        )
        .ok(),
    };
    Ok(PublishedMaster {
        maps,
        anchor,
        calibrated_frames,
        export,
        astrometry,
        master_quality,
        write_seconds,
    })
}

/// Master quality of a drizzle master, read back from its file (whole up to
/// 64 Mpx, else a central 8192² window), star sizes divided by the scale.
fn drizzle_quality(
    targets: &MasterTargets<'_>,
    canvas: &Canvas,
    k: usize,
    mono: bool,
    context: &MasterContext<'_>,
    run: &MasterRun,
    cancel: &std::sync::atomic::AtomicBool,
) -> Option<MasterQuality> {
    use fulgur_core::output::{OUTPUT_ADU_DIVISOR, read_written_region};
    // The whole master up to 64 Mpx (as a master held in memory would be
    // measured), else its central 8192² window.
    let whole = canvas.width * canvas.height <= 64 << 20;
    let (width, height) = if whole {
        (canvas.width, canvas.height)
    } else {
        (canvas.width.min(8192), canvas.height.min(8192))
    };
    let region = [
        (canvas.width - width) / 2,
        (canvas.height - height) / 2,
        width,
        height,
    ];
    let image = read_written_region(targets.master, region, OUTPUT_ADU_DIVISOR).ok()?;
    let coverage = match targets.coverage {
        Some(path) => read_written_region(path, region, 1.).ok()?,
        None => fulgur_core::RgbImage {
            width,
            height,
            demosaic_method: image.demosaic_method,
            pixels: vec![[1.; 3]; width * height],
        },
    };
    let mut quality = crate::master_quality::measure_sets(
        &image,
        &coverage,
        mono,
        context.plan,
        &run.sets,
        cancel,
    )
    .ok()?;
    let k = k as f64;
    quality.hfr_px = quality.hfr_px.map(|v| v / k);
    quality.fwhm_px = quality.fwhm_px.map(|v| v / k);
    Some(quality)
}

/// The master and maps of a drizzle run, written as tiles arrive, and the
/// same images binned k × k back to reference pixels for what follows
/// integration (ADR-189).
struct MasterSink {
    master: fulgur_core::output::StreamedImage,
    coverage: Option<fulgur_core::output::StreamedImage>,
    high_rejection: Option<fulgur_core::output::StreamedImage>,
    k: usize,
    binned: [fulgur_core::RgbImage; 3],
}

impl MasterSink {
    fn new(
        targets: &MasterTargets<'_>,
        canvas: &Canvas,
        k: usize,
        method: fulgur_core::DemosaicMethod,
        input_frames: usize,
    ) -> Result<Self> {
        use fulgur_core::output::StreamedImage;
        let mono = method == fulgur_core::DemosaicMethod::None;
        let (width, height) = (canvas.width, canvas.height);
        if width % k != 0 || height % k != 0 {
            return Err(Error::Invalid(
                "Drizzle canvas is not a whole number of bins.",
            ));
        }
        let map = |path: Option<&Path>, kind| {
            path.map(|p| StreamedImage::map(p, width, height, mono, kind, input_frames))
                .transpose()
        };
        let binned = || fulgur_core::RgbImage {
            width: width / k,
            height: height / k,
            demosaic_method: method,
            pixels: vec![[0.; 3]; width / k * (height / k)],
        };
        Ok(Self {
            master: StreamedImage::master(targets.master, width, height, mono, method)?,
            coverage: map(targets.coverage, "COVERAGE")?,
            high_rejection: map(targets.high_rejection, "HIGHREJ")?,
            k,
            binned: [binned(), binned(), binned()],
        })
    }

    /// Writes the headers and moves the files to their destinations.
    fn finish(
        self,
        anchor: &ImageHeader,
        metadata: &MasterMetadata,
        written: &dyn Fn(usize),
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<RgbExportStats> {
        let export = self.master.finish(anchor, Some(metadata), cancel)?;
        let mut count = 1;
        written(count);
        for map in [self.coverage, self.high_rejection].into_iter().flatten() {
            map.finish(anchor, Some(metadata), cancel)?;
            count += 1;
            written(count);
        }
        Ok(export)
    }
}

impl crate::integrate::TileSink for MasterSink {
    fn tile(
        &mut self,
        at: [usize; 2],
        size: [usize; 2],
        tile: &[crate::integrate::IntegratedPixel],
    ) -> Result<()> {
        self.master.write_tile(at, size, |i| tile[i].0)?;
        if let Some(map) = &mut self.coverage {
            map.write_tile(at, size, |i| tile[i].1)?;
        }
        if let Some(map) = &mut self.high_rejection {
            map.write_tile(at, size, |i| tile[i].2)?;
        }
        let share = 1. / (self.k * self.k) as f32;
        let bins = self.binned[0].width;
        for y in 0..size[1] {
            let row = (at[1] + y) / self.k * bins;
            for x in 0..size[0] {
                let bin = row + (at[0] + x) / self.k;
                let (value, coverage, high) = tile[y * size[0] + x];
                for c in 0..3 {
                    self.binned[0].pixels[bin][c] += value[c] * share;
                    self.binned[1].pixels[bin][c] += coverage[c] * share;
                    self.binned[2].pixels[bin][c] += high[c] * share;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod median_time_tests {
    use super::median_date_obs;

    #[test]
    fn median_capture_time_of_stacked_frames() {
        assert_eq!(
            median_date_obs([
                "2026-10-06T22:00:00",
                "2026-10-06T21:00:00.25",
                "not a date",
                "2026-10-06T23:30:00Z",
            ]),
            Some(("2026-10-06T22:00:00.000000".into(), 3))
        );
        // Even count: halfway between the two central times.
        assert_eq!(
            median_date_obs(["2026-10-06T23:59:00", "2026-10-07T00:01:00.5"]),
            Some(("2026-10-07T00:00:00.250000".into(), 2))
        );
        assert_eq!(median_date_obs(["", "x"]), None);
    }
}
