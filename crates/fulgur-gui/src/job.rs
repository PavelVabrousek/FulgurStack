//! Single-job orchestration. Scientific arithmetic and writers belong to the pipeline.
use crate::preview::Thumbnail;
use fulgur_core::output::{atomic_write, ensure_new_destination};
use fulgur_core::{Error, Result, check_cancelled};
use fulgur_pipeline::master_groups::{self, MasterSplit};
use fulgur_pipeline::{inspect, integrate, register};
use integrate::IntegrationEngine;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant, SystemTime};

pub const PHASES: [&str; 5] = [
    "Calibration masters",
    "Inspect",
    "Register",
    "Integrate",
    "Write outputs",
];

#[derive(Clone, Copy, Debug)]
pub struct PhaseOutcome {
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Clone, Default, Debug)]
pub struct Progress {
    pub phase: &'static str,
    pub completed: usize,
    pub total: usize,
    pub phase_started: Option<Instant>,
    pub updated_at: Option<Instant>,
    pub integration_seconds: Option<f64>,
    pub phase_durations: [Option<Duration>; 5],
    pub inspection_reused: bool,
    pub phase_outcomes: [Option<PhaseOutcome>; 5],
    /// Time of the earlier measurement whose results this run reuses.
    pub earlier: Duration,
}
impl Progress {
    pub fn phase_times(&self) -> [Option<Duration>; 5] {
        let mut times = self.phase_durations;
        if let Some(index) = PHASES.iter().position(|&phase| phase == self.phase) {
            if let Some(start) = self.phase_started {
                times[index] = Some(start.elapsed());
            }
        }
        times
    }
}
pub type SharedProgress = Arc<Mutex<Progress>>;

/// Borrow large catalogs/statistics directly; never clone them into a JSON tree.
#[derive(Serialize)]
struct BorrowedReport<'a> {
    #[serde(flatten)]
    summary: serde_json::Value,
    config: &'a integrate::IntegrationConfig,
    registration: &'a register::RegistrationReport,
    integration: &'a integrate::IntegrationStats,
    export: &'a fulgur_core::output::RgbExportStats,
}
fn progress(state: &SharedProgress, phase: &'static str, completed: usize, total: usize) {
    if let Ok(mut value) = state.lock() {
        let now = Instant::now();
        if value.phase != phase {
            if let Some(index) = PHASES.iter().position(|&phase| phase == value.phase) {
                value.phase_durations[index] = value.phase_started.map(|s| now.duration_since(s));
            }
            if value.phase == "Integrate" {
                value.integration_seconds = value
                    .phase_started
                    .map(|s| now.duration_since(s).as_secs_f64());
            }
            value.phase_started = Some(now);
        }
        if value.phase != phase || value.completed != completed || value.total != total {
            value.updated_at = Some(now);
        }
        value.phase = phase;
        value.completed = completed;
        value.total = total;
    }
}

fn freeze_progress(state: &SharedProgress) {
    if let Ok(mut value) = state.lock() {
        value.phase_durations = value.phase_times();
        value.phase_started = None;
    }
}

/// One calibration item of a light set, as the owner selected it.
#[derive(Clone, Debug, PartialEq)]
pub enum DarkSource {
    Frames(Vec<PathBuf>, fulgur_pipeline::calibration::CombineMethod),
    Master(PathBuf),
}

/// Switched-on calibration items of one light set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalibrationSources {
    pub bias: Option<DarkSource>,
    pub dark: Option<DarkSource>,
    pub flat: Option<DarkSource>,
    pub dark_flat: Option<DarkSource>,
    /// Darks or bias frames, or a saved map, for the hot pixel map (ADR-163).
    pub hot_pixels: Option<DarkSource>,
    /// Share of all pixels the map repairs, in percent.
    pub hot_percent: f64,
}

impl CalibrationSources {
    pub fn is_empty(&self) -> bool {
        self.bias.is_none()
            && self.dark.is_none()
            && self.flat.is_none()
            && self.hot_pixels.is_none()
    }
}

/// Hot pixel maps built or loaded in this session, keyed by their files'
/// identities and the share, reused while the files are unchanged.
type HotKey = (Vec<(PathBuf, u64, SystemTime)>, u64);
static HOT_MAPS: Mutex<Vec<(HotKey, Arc<fulgur_pipeline::calibration::HotPixelMap>)>> =
    Mutex::new(Vec::new());

fn hot_pixel_map(
    source: &DarkSource,
    percent: f64,
    cancel: &AtomicBool,
) -> Result<Arc<fulgur_pipeline::calibration::HotPixelMap>> {
    use fulgur_pipeline::calibration::{HOT_PIXEL_DEFAULT_PERCENT, HotPixelMap};
    let percent = if percent > 0. {
        percent
    } else {
        HOT_PIXEL_DEFAULT_PERCENT
    };
    let inputs: Vec<PathBuf> = match source {
        DarkSource::Frames(files, _) => files.clone(),
        DarkSource::Master(path) => vec![path.clone()],
    };
    let files = fulgur_pipeline::calibration::collect_frames(&inputs)?;
    let key: HotKey = (
        files
            .iter()
            .map(|p| {
                let m = p.metadata()?;
                Ok((p.clone(), m.len(), m.modified()?))
            })
            .collect::<Result<Vec<_>>>()?,
        percent.to_bits(),
    );
    if let Some((_, map)) = HOT_MAPS.lock().unwrap().iter().find(|(k, _)| *k == key) {
        return Ok(map.clone());
    }
    let map = Arc::new(HotPixelMap::from_inputs(&inputs, percent, cancel)?);
    let mut cache = HOT_MAPS.lock().unwrap();
    cache.push((key, map.clone()));
    if cache.len() > CACHED_CALIBRATIONS {
        cache.remove(0);
    }
    Ok(map)
}

/// Masters built or loaded in this session, keyed by their source files'
/// identities, the combine method ("file" for a master file), the master type
/// and (flats) the offset master used. They live in RAM and are reused by
/// later runs while files are unchanged.
type DarkKey = (
    Vec<(PathBuf, u64, SystemTime)>,
    &'static str,
    fulgur_pipeline::calibration::MasterKind,
    usize,
);

type CalibrationKey = [usize; 5];
static CALIBRATIONS: Mutex<
    Vec<(
        CalibrationKey,
        Arc<fulgur_pipeline::calibration::Calibration>,
    )>,
> = Mutex::new(Vec::new());

fn identity<T>(value: &Option<Arc<T>>) -> usize {
    value
        .as_ref()
        .map_or(0, |v| Arc::as_ptr(v) as *const u8 as usize)
}

/// Build (or reuse) a light set's calibration from its sources. The same
/// masters give the same `Arc`, so unchanged inputs keep reusable measurements.
pub fn calibration(
    sources: &CalibrationSources,
    cancel: &AtomicBool,
    progress: &dyn Fn(),
) -> Result<Option<Arc<fulgur_pipeline::calibration::Calibration>>> {
    use fulgur_pipeline::calibration::MasterKind;
    if sources.is_empty() {
        return Ok(None);
    }
    let build = |kind, source: &Option<DarkSource>, offset: Option<&Arc<_>>| -> Result<_> {
        let master = source
            .as_ref()
            .map(|s| calibration_master_offset(kind, s, offset, cancel, &|_| {}))
            .transpose()?;
        if master.is_some() {
            progress();
        }
        Ok(master)
    };
    let bias = build(MasterKind::Bias, &sources.bias, None)?;
    let dark = build(MasterKind::Dark, &sources.dark, None)?;
    let dark_flat = build(MasterKind::DarkFlat, &sources.dark_flat, None)?;
    let offset = if sources.flat.is_some() {
        dark_flat.clone().or_else(|| bias.clone())
    } else {
        None
    };
    let flat = build(MasterKind::Flat, &sources.flat, offset.as_ref())?;
    let hot = sources
        .hot_pixels
        .as_ref()
        .map(|s| hot_pixel_map(s, sources.hot_percent, cancel))
        .transpose()?;
    if hot.is_some() {
        progress();
    }
    let key = [
        identity(&bias),
        identity(&dark),
        identity(&flat),
        identity(&offset),
        identity(&hot),
    ];
    if let Some((_, c)) = CALIBRATIONS.lock().unwrap().iter().find(|(k, _)| *k == key) {
        return Ok(Some(c.clone()));
    }
    let mut calibration = fulgur_pipeline::calibration::Calibration::new(bias, dark, flat, offset)?;
    if let Some(map) = hot {
        calibration = calibration.with_hot_pixels(map)?;
    }
    let calibration = Arc::new(calibration);
    let mut cache = CALIBRATIONS.lock().unwrap();
    cache.push((key, calibration.clone()));
    if cache.len() > CACHED_CALIBRATIONS {
        cache.remove(0);
    }
    Ok(Some(calibration))
}
static MASTER_DARKS: Mutex<Vec<(DarkKey, Arc<fulgur_pipeline::calibration::MasterDark>)>> =
    Mutex::new(Vec::new());
/// Memory the in-RAM calibration masters may keep between phases and runs.
const CACHED_MASTER_BYTES: usize = 4 << 30;
/// Combined calibrations (and hot pixel maps) kept for reuse.
const CACHED_CALIBRATIONS: usize = 8;

pub fn master_dark(
    source: &DarkSource,
    cancel: &AtomicBool,
) -> Result<Arc<fulgur_pipeline::calibration::MasterDark>> {
    calibration_master(
        fulgur_pipeline::calibration::MasterKind::Dark,
        source,
        cancel,
    )
}

/// Build (or reuse) a master of any type in RAM. Master files are loaded for
/// darks only; other types are applied later and only previewed from files.
pub fn calibration_master(
    kind: fulgur_pipeline::calibration::MasterKind,
    source: &DarkSource,
    cancel: &AtomicBool,
) -> Result<Arc<fulgur_pipeline::calibration::MasterDark>> {
    calibration_master_with_progress(kind, source, cancel, &|_| {})
}

/// As `calibration_master`, reporting the stacked fraction (0..1).
pub fn calibration_master_with_progress(
    kind: fulgur_pipeline::calibration::MasterKind,
    source: &DarkSource,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Sync),
) -> Result<Arc<fulgur_pipeline::calibration::MasterDark>> {
    calibration_master_offset(kind, source, None, cancel, progress)
}

/// As `calibration_master_with_progress`; flats built from frames subtract
/// `offset` (master dark flat or bias) before normalization.
pub fn calibration_master_offset(
    kind: fulgur_pipeline::calibration::MasterKind,
    source: &DarkSource,
    offset: Option<&Arc<fulgur_pipeline::calibration::MasterDark>>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Sync),
) -> Result<Arc<fulgur_pipeline::calibration::MasterDark>> {
    let offset = offset.filter(|_| kind == fulgur_pipeline::calibration::MasterKind::Flat);
    let (files, method) = match source {
        DarkSource::Frames(files, method) => (
            fulgur_pipeline::calibration::collect_frames(files)?,
            Some(*method),
        ),
        DarkSource::Master(path) => (vec![path.canonicalize()?], None),
    };
    let key = (
        files
            .iter()
            .map(|p| {
                let m = p.metadata()?;
                Ok((p.clone(), m.len(), m.modified()?))
            })
            .collect::<Result<Vec<_>>>()?,
        method.map_or("file", |m| m.key()),
        kind,
        identity(&offset.cloned()),
    );
    if let Some((_, master)) = MASTER_DARKS.lock().unwrap().iter().find(|(k, _)| *k == key) {
        return Ok(master.clone());
    }
    let master = Arc::new(match method {
        Some(method) if kind == fulgur_pipeline::calibration::MasterKind::Flat => {
            fulgur_pipeline::calibration::build_flat_master(
                &files,
                offset.map(|o| o.as_ref()),
                method,
                cancel,
                progress,
            )?
        }
        Some(method) => fulgur_pipeline::calibration::build_master_with_progress(
            &files, kind, method, cancel, progress,
        )?,
        None => fulgur_pipeline::calibration::load_master(&files[0], kind, cancel)?,
    });
    let mut cache = MASTER_DARKS.lock().unwrap();
    cache.push((key, master.clone()));
    // Oldest masters go first, but only when the cache outgrows its memory
    // budget: one run with several light sets keeps all of its masters, so
    // the next phase finds them instead of building them again.
    while cache.len() > 1
        && cache
            .iter()
            .map(|(_, m)| m.pixels.len() * std::mem::size_of::<f32>())
            .sum::<usize>()
            > CACHED_MASTER_BYTES
    {
        cache.remove(0);
    }
    Ok(master)
}

#[derive(Clone, Debug)]
pub struct StackRequest {
    /// Included light sets with their declared sensor layouts.
    pub sets: Vec<inspect::InputSet>,
    /// Calibration per light set (same order as `sets`); masters are built in
    /// RAM when the job starts and attached to the sets.
    pub calibration: Vec<CalibrationSources>,
    /// Owner-facing light/calibration set description recorded in the report.
    pub provenance: serde_json::Value,
    pub output: PathBuf,
    pub reference: Option<PathBuf>,
    pub reference_strategy: register::ReferenceStrategy,
    pub gpu: bool,
    pub cpu_threads: usize,
    pub review: bool,
    pub geometric_correction: bool,
    pub partial_frames: bool,
    /// Whole-field limits every frame must meet (ADR-121).
    pub accuracy: register::AccuracyLimits,
    pub diagnose_excluded: bool,
    pub save_coverage: bool,
    pub save_high_rejection: bool,
    pub config: integrate::IntegrationConfig,
    /// Masters of this run, all on one pixel grid (ADR-101).
    pub master_split: MasterSplit,
    /// Sky registration through Gaia plate solutions instead of star
    /// matching (ADR-137, ADR-138); None for the star registration.
    pub sky: Option<fulgur_catalog::sky::SkyChoice>,
    /// Gaia catalogue and field centre for the master's WCS when the stars
    /// register it (ADR-155); sky registration uses its own solution.
    pub astrometry: Option<fulgur_catalog::sky::SkyChoice>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Destinations {
    pub master: PathBuf,
    pub coverage: PathBuf,
    pub high_rejection: PathBuf,
    pub report: PathBuf,
    pub save_coverage: bool,
    pub save_high_rejection: bool,
}
impl Destinations {
    pub fn new(master: &Path) -> Self {
        let stem = master.file_stem().unwrap_or_default().to_os_string();
        let sibling = |suffix: &str| {
            let mut name = stem.clone();
            name.push(suffix);
            master.with_file_name(name)
        };
        // Maps take the master's format (ADR-142).
        let maps = fulgur_core::output::OutputFormat::of(master)
            .unwrap_or_default()
            .extension();
        Self {
            master: master.into(),
            coverage: sibling(&format!("_coverage.{maps}")),
            high_rejection: sibling(&format!("_high_rejection.{maps}")),
            report: sibling("_report.json"),
            save_coverage: true,
            save_high_rejection: true,
        }
    }
    fn with_maps(master: &Path, coverage: bool, high_rejection: bool) -> Self {
        Self {
            save_coverage: coverage,
            save_high_rejection: high_rejection,
            ..Self::new(master)
        }
    }
    pub fn paths(&self) -> Vec<&Path> {
        let mut paths = vec![self.master.as_path()];
        if self.save_coverage {
            paths.push(&self.coverage);
        }
        if self.save_high_rejection {
            paths.push(&self.high_rejection);
        }
        paths.push(&self.report);
        paths
    }
    fn available(&self) -> Result<bool> {
        for path in self.paths() {
            let mut tmp = path.as_os_str().to_os_string();
            tmp.push(".tmp");
            for candidate in [path, Path::new(&tmp)] {
                match std::fs::symlink_metadata(candidate) {
                    Ok(_) => return Ok(false),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(true)
    }
    pub fn published(&self) -> Vec<PathBuf> {
        self.paths()
            .into_iter()
            .filter(|p| p.is_file())
            .map(Path::to_path_buf)
            .collect()
    }
}

/// Validate even on non-Windows hosts so project names remain portable.
pub fn valid_filename(name: &str) -> bool {
    if name.is_empty()
        || name.ends_with([' ', '.'])
        || name.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c))
    {
        return false;
    }
    let base = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    !matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) && !(base.chars().count() == 4
        && (base.starts_with("COM") || base.starts_with("LPT"))
        && matches!(base.chars().last(), Some('1'..='9' | '¹' | '²' | '³')))
        && fulgur_core::output::OutputFormat::of(Path::new(name)).is_some()
}

impl StackRequest {
    /// Build or reuse each set's in-RAM calibration before any measurement.
    pub fn prepare_darks(&mut self, cancel: &AtomicBool, state: &SharedProgress) -> Result<()> {
        let total: usize = self
            .calibration
            .iter()
            .map(|c| {
                [&c.bias, &c.dark, &c.flat, &c.dark_flat]
                    .iter()
                    .filter(|s| s.is_some())
                    .count()
            })
            .sum();
        if total == 0 {
            return Ok(());
        }
        progress(state, "Calibration masters", 0, total);
        let built = std::cell::Cell::new(0);
        for (set, sources) in self.sets.iter_mut().zip(&self.calibration) {
            set.calibration = calibration(sources, cancel, &|| {
                built.set(built.get() + 1);
                progress(state, "Calibration masters", built.get(), total);
            })?;
        }
        Ok(())
    }

    fn available_destinations(&self) -> Result<Destinations> {
        self.free_destinations(&self.output)
    }

    /// `output`, or its first free version, with this request's maps.
    fn free_destinations(&self, output: &Path) -> Result<Destinations> {
        let original =
            Destinations::with_maps(output, self.save_coverage, self.save_high_rejection);
        if original.available()? {
            return Ok(original);
        }
        let stem = output.file_stem().unwrap_or_default().to_string_lossy();
        let versioned = stem.rsplit_once("_v").and_then(|(base, number)| {
            number
                .parse::<u64>()
                .ok()
                .filter(|&n| n >= 2)
                .map(|n| (base, n))
        });
        let (base, previous) = versioned.unwrap_or((&stem, 1));
        let extension = output.extension().unwrap_or_default().to_string_lossy();
        let mut version = previous;
        loop {
            version = version.checked_add(1).ok_or(Error::Invalid(
                "Output version limit reached. Choose another master filename.",
            ))?;
            let master = output.with_file_name(format!("{base}_v{version}.{extension}"));
            let paths =
                Destinations::with_maps(&master, self.save_coverage, self.save_high_rejection);
            if paths.available()? {
                return Ok(paths);
            }
        }
    }

    /// Files of one master of a split run, named after its group.
    fn group_destinations(&self, base: &Destinations, label: &str) -> Result<Destinations> {
        let stem = base
            .master
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let extension = base
            .master
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        self.free_destinations(
            &base
                .master
                .with_file_name(format!("{stem}_{label}.{extension}")),
        )
    }

    pub fn validate(&self) -> Result<Destinations> {
        self.config.validate()?;
        if ![0, 12, 16, 24].contains(&self.cpu_threads) {
            return Err(Error::Invalid(
                "Choose automatic, 12, 16 or 24 CPU workers.",
            ));
        }
        if self
            .reference
            .as_ref()
            .is_some_and(|p| p.as_os_str().is_empty())
        {
            return Err(Error::Invalid(
                "Choose an accepted or external reference, or select Automatic.",
            ));
        }
        if self.sets.iter().all(|s| s.inputs.is_empty()) {
            return Err(Error::Invalid(
                "Include at least one light set with FITS or XISF files.",
            ));
        }
        for input in self.sets.iter().flat_map(|s| &s.inputs) {
            if !input.exists() {
                return Err(Error::Invalid(
                    "An input no longer exists. Add the source again.",
                ));
            }
        }
        if !valid_filename(
            &self
                .output
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
        ) {
            return Err(Error::Invalid(
                "Choose a valid Windows filename ending in .fit, .fits, .fts or .xisf.",
            ));
        }
        if self.gpu && !cfg!(feature = "gpu") {
            return Err(Error::Invalid(
                "GPU is not included in this build. Use CPU or a GPU-enabled build.",
            ));
        }
        let paths = self.available_destinations()?;
        for path in paths.paths() {
            ensure_new_destination(path)?;
            let mut tmp = path.as_os_str().to_os_string();
            tmp.push(".tmp");
            ensure_new_destination(Path::new(&tmp))?;
        }
        let parent = self
            .output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if !parent.is_dir() {
            return Err(Error::Invalid("Choose an existing output folder."));
        }
        // Only the allowed final .tmp path is probed, never an image intermediate.
        let mut tmp = paths.master.as_os_str().to_os_string();
        tmp.push(".tmp");
        let temporary = PathBuf::from(tmp);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        drop(file);
        std::fs::remove_file(temporary)?;
        Ok(paths)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct QualityRow {
    pub file_name: String,
    pub path: PathBuf,
    /// Index into the request's light sets, and that set's label.
    pub set: usize,
    pub set_name: String,
    pub exposure: Option<f64>,
    pub date: Option<String>,
    pub hfr: Option<f64>,
    pub stars: usize,
    pub eccentricity: Option<f64>,
    pub background: Option<f64>,
    pub noise: Option<f64>,
    pub weight: f64,
    pub accepted: bool,
    pub automatic_accepted: bool,
    pub can_include: bool,
    pub reasons: String,
    /// Sky registration: plate scale (arcsec/px), held-out RMS of the plate
    /// solution (px) and the worst 3 x 3 cell offset of the stacking map (px).
    pub sky_scale: Option<f64>,
    pub sky_held_out: Option<f64>,
    pub sky_check: Option<f64>,
}
fn rows(report: &inspect::InspectReport, root: &Path) -> Vec<QualityRow> {
    report
        .frames
        .iter()
        .map(|f| QualityRow {
            file_name: f.file_name.clone(),
            path: root.join(&f.file_name),
            set: f.light_set,
            set_name: report
                .light_sets
                .get(f.light_set)
                .map(|s| s.label.clone())
                .unwrap_or_default(),
            exposure: f.frame.as_ref().map(|h| h.exposure_seconds),
            date: f.frame.as_ref().and_then(|h| h.date_obs.clone()),
            hfr: f.hfr_px,
            stars: f.star_count,
            eccentricity: f.eccentricity,
            background: f.background_adu,
            noise: f.noise_adu_per_second,
            weight: f.provisional_weight,
            accepted: f.accepted,
            automatic_accepted: f.accepted,
            can_include: can_include(f),
            reasons: f
                .rejection_reasons
                .iter()
                .map(|r| reason_text(r))
                .collect::<Vec<_>>()
                .join(", "),
            sky_scale: None,
            sky_held_out: None,
            sky_check: None,
        })
        .collect()
}

/// The owner-facing wording of a rejection code; the report keeps the code.
fn reason_text(code: &str) -> &str {
    match code {
        "incompatible_target_filter_or_sensor" => {
            "another camera, gain or filter than most of its light set (split the set in Frames)"
        }
        code => code,
    }
}

fn can_include(frame: &inspect::QualityFrame) -> bool {
    frame.frame.is_some()
        && frame.measured_star_count >= 10
        && frame.hfr_px.is_some_and(|v| v.is_finite() && v > 0.)
        && frame
            .noise_adu_per_second
            .is_some_and(|v| v.is_finite() && v > 0.)
        && frame.background_adu.is_some_and(f64::is_finite)
        && frame
            .noise_adu_per_second
            .zip(frame.hfr_px)
            .is_some_and(|(noise, hfr)| {
                let weight = 1. / (noise * noise * hfr * hfr);
                weight.is_finite() && weight > 0.
            })
        && frame.rejection_reasons.iter().all(|reason| {
            matches!(
                reason.as_str(),
                "excessive_hfr" | "excessive_eccentricity" | "low_star_count_in_exposure_group"
            )
        })
}

#[derive(Clone, Debug)]
pub struct FrameSelection {
    pub path: PathBuf,
    pub include: bool,
}

#[derive(Serialize)]
struct ManualSelectionReport {
    file_name: String,
    included: bool,
    automatic_accepted: bool,
    automatic_reasons: Vec<String>,
}

#[derive(Clone, Debug)]
struct SourceIdentity {
    path: PathBuf,
    set: usize,
    len: u64,
    modified: SystemTime,
}
impl SourceIdentity {
    fn read((path, set): (PathBuf, usize)) -> Result<Self> {
        let m = path.metadata()?;
        Ok(Self {
            path,
            set,
            len: m.len(),
            modified: m.modified()?,
        })
    }
    fn verify(&self) -> Result<()> {
        let m = self.path.metadata()?;
        if m.len() != self.len || m.modified()? != self.modified {
            return Err(Error::Invalid(
                "Source changed after inspection. Start a new run.",
            ));
        }
        Ok(())
    }
}

pub struct InspectionSession {
    report: inspect::InspectReport,
    root: PathBuf,
    identities: Vec<SourceIdentity>,
    layouts: Vec<fulgur_core::LayoutChoice>,
    /// Owner exposure entries and derived-value confirmations (ADR-084).
    headers: Vec<(Option<f64>, bool)>,
    /// Calibrations the measurements were made with; reuse requires the same.
    darks: Vec<Option<Arc<fulgur_pipeline::calibration::Calibration>>>,
    /// Time spent on the calibration masters and on this inspection.
    phase_times: [Option<Duration>; 2],
    /// The bright-star lists sky registration solves with were kept.
    sky_stars: bool,
}
impl InspectionSession {
    fn record_phases(&mut self, state: &SharedProgress) {
        if let Ok(value) = state.lock() {
            let times = value.phase_times();
            self.phase_times = [times[0], times[1]];
        }
    }
    /// Same files in the same light sets with the same layouts, unchanged on disk.
    pub fn matches_inputs(&self, sets: &[inspect::InputSet], cancel: &AtomicBool) -> Result<bool> {
        if sets
            .iter()
            .map(|s| s.layout)
            .ne(self.layouts.iter().copied())
            || sets
                .iter()
                .map(|s| (s.exposure, s.accept_derived))
                .ne(self.headers.iter().copied())
            || sets.len() != self.darks.len()
            || sets
                .iter()
                .zip(&self.darks)
                .any(|(s, d)| match (&s.calibration, d) {
                    (None, None) => false,
                    (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                    _ => true,
                })
        {
            return Ok(false);
        }
        let files = inspect::set_files(sets, cancel)?;
        if files.len() != self.identities.len()
            || files
                .iter()
                .zip(&self.identities)
                .any(|((path, set), id)| path != &id.path || *set != id.set)
        {
            return Ok(false);
        }
        for id in &self.identities {
            check_cancelled(cancel)?;
            let metadata = match id.path.metadata() {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(e) => return Err(e.into()),
            };
            if metadata.len() != id.len || metadata.modified()? != id.modified {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub fn rows_with_selection(&self, selection: &[FrameSelection]) -> Vec<QualityRow> {
        let mut rows = self.rows();
        let mut choices = std::collections::BTreeMap::new();
        for choice in selection {
            choices.entry(&choice.path).or_insert(choice.include);
        }
        for row in &mut rows {
            if let Some(&include) = choices.get(&row.path) {
                row.accepted = include && row.can_include;
                row.reasons = format!(
                    "Manually {}. Automatic assessment: {}",
                    if row.accepted { "included" } else { "excluded" },
                    row.reasons
                );
            }
        }
        if !selection.is_empty() {
            let mut maximum = 0.0_f64;
            for (row, frame) in rows.iter_mut().zip(&self.report.frames) {
                row.weight = if row.accepted {
                    let noise = frame.noise_adu_per_second.unwrap();
                    let hfr = frame.hfr_px.unwrap();
                    1. / (noise * noise * hfr * hfr)
                } else {
                    0.
                };
                maximum = maximum.max(row.weight);
            }
            if maximum > 0. && maximum.is_finite() {
                for row in &mut rows {
                    row.weight /= maximum;
                }
            }
        }
        rows
    }
    fn selected_report(
        &self,
        selection: &[FrameSelection],
    ) -> Result<(inspect::InspectReport, Vec<ManualSelectionReport>)> {
        let mut report = self.report.clone();
        let mut decisions = vec![];
        for choice in selection {
            let frame = report
                .frames
                .iter_mut()
                .find(|f| self.root.join(&f.file_name) == choice.path)
                .ok_or(Error::Invalid(
                    "Manual selection no longer matches the inspected inputs.",
                ))?;
            if choice.include && !can_include(frame) {
                return Err(Error::Invalid(
                    "This frame cannot be included: its image data, compatibility or measurements are unusable.",
                ));
            }
            decisions.push(ManualSelectionReport {
                file_name: frame.file_name.clone(),
                included: choice.include,
                automatic_accepted: frame.accepted,
                automatic_reasons: frame.rejection_reasons.clone(),
            });
            frame.accepted = choice.include;
            if choice.include {
                frame.rejection_reasons.clear();
            } else {
                frame.rejection_reasons.push("manually_excluded".into());
            }
        }
        if !selection.is_empty() {
            let mut maximum = 0.0_f64;
            for frame in &mut report.frames {
                frame.provisional_weight = if frame.accepted {
                    let noise = frame.noise_adu_per_second.unwrap();
                    let hfr = frame.hfr_px.unwrap();
                    1. / (noise * noise * hfr * hfr)
                } else {
                    0.
                };
                if !frame.provisional_weight.is_finite()
                    || (frame.accepted && frame.provisional_weight <= 0.)
                {
                    return Err(Error::Invalid(
                        "Selected frame has unusable quality weight.",
                    ));
                }
                maximum = maximum.max(frame.provisional_weight);
            }
            if maximum > 0. {
                for frame in &mut report.frames {
                    frame.provisional_weight /= maximum;
                }
            }
            report.accepted_count = report.frames.iter().filter(|f| f.accepted).count();
            report.rejected_count = report.frame_count - report.accepted_count;
            report.accepted_exposure_seconds = report
                .frames
                .iter()
                .filter(|f| f.accepted)
                .filter_map(|f| f.frame.as_ref().map(|i| i.exposure_seconds))
                .sum();
        }
        if report.accepted_count == 0 {
            return Err(Error::Invalid(
                "Select at least one usable frame for stacking.",
            ));
        }
        Ok((report, decisions))
    }
    pub fn inspect(sets: &[inspect::InputSet], cancel: &AtomicBool) -> Result<Self> {
        Self::inspect_with_progress(sets, false, cancel, &|_, _| {})
    }
    /// `sky_stars` also keeps the bright-star lists of sky registration.
    pub fn inspect_with_progress(
        sets: &[inspect::InputSet],
        sky_stars: bool,
        cancel: &AtomicBool,
        observer: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<Self> {
        let identities = inspect::set_files(sets, cancel)?
            .into_iter()
            .map(SourceIdentity::read)
            .collect::<Result<Vec<_>>>()?;
        let (report, root) = inspect::inspect_sets(
            sets,
            inspect::InspectConfig {
                sky_stars,
                ..Default::default()
            },
            cancel,
            Some(observer),
        )?;
        let session = Self {
            report,
            root,
            identities,
            layouts: sets.iter().map(|s| s.layout).collect(),
            headers: sets
                .iter()
                .map(|s| (s.exposure, s.accept_derived))
                .collect(),
            darks: sets.iter().map(|s| s.calibration.clone()).collect(),
            phase_times: [None; 2],
            sky_stars,
        };
        session.verify()?;
        Ok(session)
    }
    /// This measurement with the bright-star lists sky registration needs,
    /// added without measuring again (ADR-154); the time is counted with the
    /// inspection.
    pub fn with_sky_stars(
        &self,
        sets: &[inspect::InputSet],
        cancel: &AtomicBool,
        observer: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<Self> {
        let start = Instant::now();
        let mut report = self.report.clone();
        inspect::add_sky_stars(&mut report, &self.root, sets, cancel, Some(observer))?;
        let [masters, inspection] = self.phase_times;
        let session = Self {
            report,
            root: self.root.clone(),
            identities: self.identities.clone(),
            layouts: self.layouts.clone(),
            headers: self.headers.clone(),
            darks: self.darks.clone(),
            phase_times: [
                masters,
                Some(inspection.unwrap_or_default() + start.elapsed()),
            ],
            sky_stars: true,
        };
        session.verify()?;
        Ok(session)
    }
    pub fn rows(&self) -> Vec<QualityRow> {
        rows(&self.report, &self.root)
    }
    /// The OBJECT card most usable frames carry.
    pub fn common_object(&self) -> Option<String> {
        fulgur_catalog::sky::common_object(&self.report)
    }
    /// Frames calibrated with a dark of another exposure or gain.
    pub fn calibration_warnings(&self) -> usize {
        self.report
            .frames
            .iter()
            .filter(|f| !f.calibration_warnings.is_empty())
            .count()
    }
    pub fn verify(&self) -> Result<()> {
        for id in &self.identities {
            id.verify()?;
        }
        Ok(())
    }
}

pub enum Event {
    Quality {
        rows: Vec<QualityRow>,
        waiting: bool,
        inspection: Arc<InspectionSession>,
        reused: bool,
    },
    Complete(Box<Completed>),
    Stopped {
        cancelled: bool,
        message: String,
        published: Vec<PathBuf>,
    },
}
pub enum ReviewDecision {
    Continue(Option<PathBuf>),
    ContinueWithSelection {
        reference_strategy: register::ReferenceStrategy,
        reference: Option<PathBuf>,
        selection: Vec<FrameSelection>,
    },
}
/// What calibration did to one light set, for the Results page.
pub struct SetCalibration {
    pub name: String,
    pub summary: Option<fulgur_pipeline::calibration::CalibrationSummary>,
    /// Frames of the set in the master, and those calibrated.
    pub stacked: usize,
    pub calibrated: usize,
    pub negative_samples: usize,
    pub repaired_pixels: usize,
    /// Frame warnings with the number of frames each applies to.
    pub warnings: Vec<(String, usize)>,
    /// Dark-flat files in use; the flats' offset falls back to the bias.
    pub dark_flat_files: Option<u64>,
}

fn set_calibration(
    plan: &register::RegistrationReport,
    provenance: &serde_json::Value,
) -> Vec<SetCalibration> {
    let inspection = &plan.inspection;
    inspection
        .light_sets
        .iter()
        .enumerate()
        .map(|(index, set)| {
            let frames: Vec<_> = plan
                .frames
                .iter()
                .zip(&inspection.frames)
                .filter(|(registered, f)| registered.accepted && f.light_set == index)
                .map(|(_, f)| f)
                .collect();
            let mut warnings: Vec<(String, usize)> = vec![];
            for warning in frames.iter().flat_map(|f| &f.calibration_warnings) {
                match warnings.iter_mut().find(|(text, _)| text == warning) {
                    Some((_, count)) => *count += 1,
                    None => warnings.push((warning.clone(), 1)),
                }
            }
            let dark_flat_files = provenance[index]["calibration_set"]["items"]
                .as_array()
                .and_then(|items| {
                    items.iter().find(|i| {
                        i["kind"] == "dark_flat" && i["enabled"] == serde_json::json!(true)
                    })
                })
                .and_then(|i| i["files"].as_u64());
            SetCalibration {
                name: set.label.clone(),
                summary: set.calibration.clone(),
                stacked: frames.len(),
                calibrated: frames.iter().filter(|f| f.calibrated).count(),
                negative_samples: frames.iter().map(|f| f.calibration_negative_samples).sum(),
                repaired_pixels: frames.iter().map(|f| f.calibration_repaired_pixels).sum(),
                warnings,
                dark_flat_files,
            }
        })
        .collect()
}

/// What differs between the masters of a split run; the Results page shows
/// one at a time by swapping it into `Completed`.
pub struct MasterView {
    pub label: Option<String>,
    pub calibrated_frames: usize,
    pub master_quality: Option<fulgur_pipeline::master_quality::MasterQuality>,
    pub rejection: &'static str,
    pub normalization: integrate::NormalizationSummary,
    pub paths: Destinations,
    pub master: Thumbnail,
    pub coverage: Thumbnail,
    pub high: Thumbnail,
    pub accepted: usize,
    pub total: usize,
    pub exposure: f64,
    pub reference: String,
}

pub struct Completed {
    /// Group shown now; None for a run with one master.
    pub label: Option<String>,
    /// The other masters of a split run.
    pub others: Vec<MasterView>,
    /// Accepted frames calibrated with a master dark.
    pub calibrated_frames: usize,
    /// One entry per light set of the stack.
    pub calibration: Vec<SetCalibration>,
    /// Star size, shape and sky level of the master (ADR-086).
    pub master_quality: Option<fulgur_pipeline::master_quality::MasterQuality>,
    /// Pixel rejection applied (after Auto resolution; ADR-087).
    pub rejection: &'static str,
    /// Scales and sky offsets applied before rejection (ADR-102).
    pub normalization: integrate::NormalizationSummary,
    /// Experimental partial frames: how many joined how many tiles (ADR-109).
    pub partial_summary: Option<String>,
    /// Stack from several cameras (ADR-110).
    pub camera_summary: Option<String>,
    /// Light sets measured on their own anchor (ADR-156).
    pub set_photometry_summary: Option<String>,
    /// Sky registration: solved frames, engines, field centre (ADR-138).
    pub sky_summary: Option<String>,
    /// The master's WCS: written, or why not (ADR-155).
    pub astrometry_summary: Option<String>,
    pub auto_reference_summary: Option<String>,
    pub geometric_correction: Option<[usize; 3]>,
    pub rows: Vec<QualityRow>,
    pub paths: Destinations,
    pub master: Thumbnail,
    pub coverage: Thumbnail,
    pub high: Thumbnail,
    pub wall_seconds: f64,
    /// This run plus the earlier measurement it reused.
    pub total_seconds: f64,
    pub peak_rss_bytes: Option<u64>,
    pub accepted: usize,
    pub total: usize,
    pub exposure: f64,
    pub reference: String,
    /// Native source frame that anchored registration.
    pub reference_anchor: PathBuf,
    pub width: usize,
    pub height: usize,
    pub phase_outcomes: [Option<PhaseOutcome>; 5],
}

impl Completed {
    /// Show the master of group `label` in the Results page.
    pub fn show(&mut self, label: &str) {
        let Some(other) = self
            .others
            .iter_mut()
            .find(|o| o.label.as_deref() == Some(label))
        else {
            return;
        };
        std::mem::swap(&mut self.label, &mut other.label);
        std::mem::swap(&mut self.calibrated_frames, &mut other.calibrated_frames);
        std::mem::swap(&mut self.master_quality, &mut other.master_quality);
        std::mem::swap(&mut self.rejection, &mut other.rejection);
        std::mem::swap(&mut self.normalization, &mut other.normalization);
        std::mem::swap(&mut self.paths, &mut other.paths);
        std::mem::swap(&mut self.master, &mut other.master);
        std::mem::swap(&mut self.coverage, &mut other.coverage);
        std::mem::swap(&mut self.high, &mut other.high);
        std::mem::swap(&mut self.accepted, &mut other.accepted);
        std::mem::swap(&mut self.total, &mut other.total);
        std::mem::swap(&mut self.exposure, &mut other.exposure);
        std::mem::swap(&mut self.reference, &mut other.reference);
    }
}

pub struct Worker {
    pub events: mpsc::Receiver<Event>,
    pub decision: mpsc::SyncSender<ReviewDecision>,
    pub cancel: Arc<AtomicBool>,
    pub progress: SharedProgress,
    pub destinations: Arc<Mutex<Option<Destinations>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub fn start(request: StackRequest, inspect_only: bool) -> Self {
        Self::start_with_inspection(request, inspect_only, None, vec![])
    }
    pub fn start_with_inspection(
        request: StackRequest,
        inspect_only: bool,
        cached: Option<Arc<InspectionSession>>,
        mut selection: Vec<FrameSelection>,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel(4);
        let (decision, review) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(Progress::default()));
        let destinations = Arc::new(Mutex::new(None::<Destinations>));
        let selected = destinations.clone();
        // Files of a split run, known once registration has grouped the frames.
        let split_destinations = Arc::new(Mutex::new(Vec::<Destinations>::new()));
        let split = split_destinations.clone();
        let c = cancel.clone();
        let s = state.clone();
        let handle = std::thread::spawn(move || {
            let terminal_state = s.clone();
            let published_destinations = selected.clone();
            let work = || -> Result<()> {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(if request.cpu_threads == 0 {
                        std::thread::available_parallelism()
                            .map(usize::from)
                            .unwrap_or(1)
                            .min(24)
                    } else {
                        // GPU pixel rejection keeps CPU reconstruction (ADR-090).
                        request.cpu_threads.min(24)
                    })
                    .build()
                    .map_err(|_| Error::Invalid("Could not start CPU workers."))?;
                let events = tx.clone();
                pool.install(move || {
                    let mut request = request;
                    let start = Instant::now();
                    request.prepare_darks(&c, &s)?;
                    if inspect_only {
                        progress(&s, "Inspect", 0, 0);
                        let mut session = InspectionSession::inspect_with_progress(
                            &request.sets,
                            request.sky.is_some(),
                            &c,
                            &|n, total| progress(&s, "Inspect", n, total),
                        )?;
                        session.record_phases(&s);
                        let session = Arc::new(session);
                        progress(&s, "Inspection complete", 1, 1);
                        let _ = events.send(Event::Quality {
                            rows: session.rows(),
                            waiting: false,
                            inspection: session,
                            reused: false,
                        });
                        return Ok(());
                    }
                    let paths = request.validate()?;
                    *selected.lock().unwrap() = Some(paths.clone());
                    progress(&s, "Validate inputs", 0, 0);
                    let reused = cached
                        .as_ref()
                        .map(|previous| previous.matches_inputs(&request.sets, &c))
                        .transpose()?
                        .unwrap_or(false);
                    // Sky registration needs the bright-star lists a measurement
                    // made for star registration did not keep: they are added,
                    // and the measurement and the owner's choices stay (ADR-154).
                    let cached = match cached {
                        Some(previous)
                            if reused && request.sky.is_some() && !previous.sky_stars =>
                        {
                            progress(&s, "Inspect", 0, 0);
                            Some(Arc::new(previous.with_sky_stars(
                                &request.sets,
                                &c,
                                &|n, total| progress(&s, "Inspect", n, total),
                            )?))
                        }
                        other => other,
                    };
                    let session = if reused {
                        let session = cached.unwrap();
                        if let Ok(mut p) = s.lock() {
                            // The measurement belongs to this master's processing time.
                            let [masters, inspection] = session.phase_times;
                            p.inspection_reused = true;
                            if let Some(masters) = masters {
                                p.phase_durations[0] =
                                    Some(p.phase_durations[0].unwrap_or_default() + masters);
                            }
                            p.phase_durations[1] = Some(inspection.unwrap_or_default());
                            p.earlier =
                                masters.unwrap_or_default() + inspection.unwrap_or_default();
                        }
                        session
                    } else {
                        selection.clear();
                        progress(&s, "Inspect", 0, 0);
                        let mut session = InspectionSession::inspect_with_progress(
                            &request.sets,
                            request.sky.is_some(),
                            &c,
                            &|n, total| progress(&s, "Inspect", n, total),
                        )?;
                        session.record_phases(&s);
                        Arc::new(session)
                    };
                    let waiting = request.review && !reused;
                    let _ = events.send(Event::Quality {
                        rows: session.rows_with_selection(&selection),
                        waiting,
                        inspection: session.clone(),
                        reused,
                    });
                    let mut reference = request.reference.clone();
                    let pause = Instant::now();
                    if waiting {
                        progress(&s, "Quality review", 0, 0);
                        loop {
                            check_cancelled(&c)?;
                            match review.recv_timeout(Duration::from_millis(100)) {
                                Ok(ReviewDecision::Continue(chosen)) => {
                                    reference = chosen;
                                    break;
                                }
                                Ok(ReviewDecision::ContinueWithSelection {
                                    reference_strategy,
                                    reference: chosen,
                                    selection: selected,
                                }) => {
                                    reference = chosen;
                                    selection = selected;
                                    request.reference_strategy = reference_strategy;
                                    break;
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => {
                                    return Err(Error::Cancelled);
                                }
                                Err(mpsc::RecvTimeoutError::Timeout) => {}
                            }
                        }
                    }
                    let review_seconds = if waiting {
                        pause.elapsed().as_secs_f64()
                    } else {
                        0.
                    };
                    let result = finish(
                        request,
                        paths,
                        &split,
                        session,
                        reference,
                        &c,
                        &s,
                        start,
                        review_seconds,
                        reused,
                        selection,
                    )?;
                    let _ = events.send(Event::Complete(Box::new(result)));
                    Ok(())
                })
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|payload| {
                    let detail = payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown worker panic".into());
                    Err(Error::Io(std::io::Error::other(format!(
                        "Processing worker stopped unexpectedly: {detail}"
                    ))))
                });
            freeze_progress(&terminal_state);
            if let Err(error) = outcome {
                // Existing outputs are never rolled back. Final files can be published individually.
                let _ = tx.send(Event::Stopped {
                    cancelled: matches!(error, Error::Cancelled),
                    message: error.to_string(),
                    published: published_destinations
                        .lock()
                        .ok()
                        .and_then(|p| p.clone())
                        .into_iter()
                        .chain(split_destinations.lock().map_or(vec![], |d| d.clone()))
                        .flat_map(|d| d.published())
                        .collect(),
                });
            }
        });
        Self {
            events: rx,
            decision,
            cancel,
            progress: state,
            destinations,
            handle: Some(handle),
        }
    }
    pub fn finished(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_finished())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[allow(clippy::too_many_arguments)] // One immutable job snapshot and its bounded control context.
fn finish(
    request: StackRequest,
    paths: Destinations,
    split_destinations: &Mutex<Vec<Destinations>>,
    session: Arc<InspectionSession>,
    reference: Option<PathBuf>,
    cancel: &AtomicBool,
    state: &SharedProgress,
    start: Instant,
    review_seconds: f64,
    reused: bool,
    selection: Vec<FrameSelection>,
) -> Result<Completed> {
    if !session.matches_inputs(&request.sets, cancel)? {
        return Err(Error::Invalid(
            "Inputs changed after inspection or review. Inspect quality again before stacking.",
        ));
    }
    let (mut inspection, manual_selection) = session.selected_report(&selection)?;
    if reused {
        inspection.wall_seconds = 0.;
        inspection.frames_per_second = 0.;
        inspection.timing_scope = "reused unchanged CFA measurements; no inspection computation in this run; original duration recorded separately";
    }
    progress(state, "Register", 0, 0);
    // Exclude human review time from processing telemetry and registration's envelope.
    let engine_start = Instant::now() - Duration::from_secs_f64(inspection.wall_seconds);
    let set_darks: Vec<_> = request.sets.iter().map(|s| s.calibration.clone()).collect();
    let options = register::RegistrationOptions {
        reference_strategy: request.reference_strategy,
        geometric_correction: request.geometric_correction,
        diagnose_excluded: request.diagnose_excluded,
        partial_frames: request.partial_frames,
        accuracy: request.accuracy,
    };
    let observer = |n, total| progress(state, "Register", n, total);
    let mut sky_position = None;
    let catalogue_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    // The catalogue for the master's WCS after a star registration (ADR-155);
    // without it, or when it cannot be opened, the master has no WCS.
    let mut astrometry_note = None;
    let star_astrometry = match (&request.sky, &request.astrometry) {
        (None, Some(choice)) => {
            let lines = catalogue_lines.clone();
            let say = Arc::new(move |line: String| {
                if let Ok(mut l) = lines.lock() {
                    l.push(line);
                }
            });
            match fulgur_catalog::sky::sky_options(choice, &inspection, say) {
                Ok((options, _)) => Some(options),
                Err(message) => {
                    astrometry_note = Some(format!("no WCS in the master: {message}"));
                    None
                }
            }
        }
        _ => None,
    };
    let mut sky_registration_options = None;
    let (plan, root) = match &request.sky {
        None => register::register_inspected_with_calibration(
            inspection,
            session.root.clone(),
            0.8,
            reference.as_deref(),
            options,
            cancel,
            engine_start,
            Some(&observer),
            &set_darks,
        )?,
        Some(choice) => {
            let lines = catalogue_lines.clone();
            let say = Arc::new(move |line: String| {
                if let Ok(mut l) = lines.lock() {
                    l.push(line);
                }
            });
            let (sky, position) = fulgur_catalog::sky::sky_options(choice, &inspection, say)
                .map_err(|message| Error::Io(std::io::Error::other(message)))?;
            sky_position = Some(position);
            let registered = register::register_sky(
                inspection,
                session.root.clone(),
                0.8,
                reference.as_deref(),
                options,
                &sky,
                cancel,
                engine_start,
                Some(&observer),
                &set_darks,
            )?;
            sky_registration_options = Some(sky);
            registered
        }
    };
    // The grid's plate solution: sky registration's own, or the reference
    // frame solved now.
    let astrometry = match (sky_registration_options, star_astrometry) {
        (Some(options), _) => fulgur_pipeline::astrometry::sky_grid(&plan).map(|g| (g, options)),
        (None, Some(options)) => {
            match fulgur_pipeline::astrometry::solve_grid(
                &plan,
                &root,
                &request.sets,
                &options,
                cancel,
            ) {
                Ok(grid) => Some((grid, options)),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(error) => {
                    astrometry_note = Some(format!("no WCS in the master: {error}"));
                    None
                }
            }
        }
        _ => None,
    };
    // Why a tried plate solve gave no WCS, for the report (ADR-158).
    let astrometry_failure = astrometry_note.clone();
    let mut astrometry_summary = astrometry_note;
    let canvas = register::Canvas {
        origin_x_px: 0,
        origin_y_px: 0,
        width: plan.reference_width,
        height: plan.reference_height,
        minimum_effective_weight_fraction: 0.,
        coordinate_rule: "exact reference FITS dimensions and sensor pixel grid; no automatic crop",
    };
    let sources = integrate::calibrated_sources(&root, &plan, &set_darks)?;
    let runs = master_groups::runs(&plan, sources, request.master_split)?;
    let destinations = runs
        .iter()
        .map(|run| match &run.label {
            Some(label) => request.group_destinations(&paths, label),
            None => Ok(paths.clone()),
        })
        .collect::<Result<Vec<_>>>()?;
    if let Ok(mut value) = split_destinations.lock() {
        value.clone_from(&destinations);
    }
    let formula = set_darks
        .iter()
        .flatten()
        .map(|c| c.summary.formula.clone())
        .collect::<Vec<_>>();
    let registered = PhaseOutcome {
        succeeded: plan.registration_success_count,
        failed: plan
            .registration_attempted_count
            .saturating_sub(plan.registration_success_count),
        skipped: plan.registration_skipped_count,
    };
    if let Ok(mut value) = state.lock() {
        value.phase_outcomes[2] = Some(registered);
    }
    progress(state, "Integrate", 0, 0);
    let engine: Box<dyn IntegrationEngine> = if request.gpu {
        #[cfg(feature = "gpu")]
        {
            Box::new(integrate::GpuIntegrationEngine::new()?)
        }
        #[cfg(not(feature = "gpu"))]
        {
            return Err(Error::Invalid("GPU is not compiled."));
        }
    } else {
        Box::new(integrate::CpuIntegrationEngine)
    };
    let stacked: usize = runs.iter().map(|r| r.sources.len()).sum();
    // Successful integration processes its complete source list. A source read/
    // reconstruction failure aborts the phase; pixel rejection is not a frame failure.
    let integrated = PhaseOutcome {
        succeeded: stacked,
        failed: 0,
        skipped: plan.frame_count.saturating_sub(stacked),
    };
    let (mut wall_seconds, mut total_seconds) = (0., 0.);
    let mut views = Vec::with_capacity(runs.len());
    for (index, (run, paths)) in runs.iter().zip(&destinations).enumerate() {
        let last = index + 1 == runs.len();
        let maps = engine.integrate_with_progress(
            &run.sources,
            &canvas,
            &request.config,
            run.input_frames,
            cancel,
            &|n, total| progress(state, "Integrate", index * total + n, runs.len() * total),
        )?;
        let output_count = paths.paths().len();
        // Earlier masters of a split run write within the Integrate phase.
        let written_progress = |written: usize| {
            if last {
                progress(state, "Write outputs", written, output_count);
            }
        };
        if last {
            if let Ok(mut value) = state.lock() {
                value.phase_outcomes[3] = Some(integrated);
            }
        }
        written_progress(0);
        let anchor = fulgur_core::fits::read_header(&root.join(&run.anchor))?;
        let calibrated_frames = run
            .sources
            .iter()
            .filter(|s| s.calibration.is_some())
            .count();
        let metadata = fulgur_core::output::MasterMetadata {
            geometric_correction_frames: run.geometric_correction_frames,
            accepted_frames: run.accepted_frames,
            rejection: maps.stats.rejection,
            total_exposure_seconds: run.total_exposure_seconds,
            origin_x_px: 0,
            origin_y_px: 0,
            external_geometry_scale: plan
                .external_reference
                .as_ref()
                .map(|e| e.anchor_to_reference.scale_factor),
            monochrome_frames: run.monochrome_frames,
            calibrated_frames,
            calibration_formula: (!formula.is_empty()).then(|| formula.join("; ")),
            astrometry: None,
        };
        let master_astrometry = match &astrometry {
            Some((grid, options)) => Some(fulgur_pipeline::astrometry::check_master(
                &maps.image,
                [0, 0],
                grid,
                options,
                cancel,
            )?),
            None => None,
        };
        let mut metadata = metadata;
        metadata.astrometry = match &master_astrometry {
            Some(a) => Some(
                a.cards
                    .clone()
                    .unwrap_or_else(fulgur_pipeline::astrometry::unsolved),
            ),
            None => astrometry_failure
                .as_ref()
                .map(|_| fulgur_pipeline::astrometry::unsolved()),
        };
        if let Some(a) = &master_astrometry {
            astrometry_summary = Some(a.summary());
        }
        let write = Instant::now();
        let export = fulgur_core::output::write_master_atomic(
            &paths.master,
            &maps.image,
            &anchor,
            &metadata,
            cancel,
        )?;
        let mut written = 1;
        written_progress(written);
        let master_quality = fulgur_pipeline::master_quality::measure_sets(
            &maps.image,
            &maps.coverage,
            metadata.monochrome_frames == metadata.accepted_frames,
            &plan,
            &run.sets,
            cancel,
        )
        .ok();
        if paths.save_coverage {
            fulgur_core::output::write_fraction_map_atomic(
                &paths.coverage,
                &maps.coverage,
                "COVERAGE",
                run.input_frames,
                cancel,
            )?;
            written += 1;
            written_progress(written);
        }
        if paths.save_high_rejection {
            fulgur_core::output::write_fraction_map_atomic(
                &paths.high_rejection,
                &maps.high_rejection,
                "HIGHREJ",
                run.input_frames,
                cancel,
            )?;
            written += 1;
            written_progress(written);
        }
        let write_seconds = write.elapsed().as_secs_f64();
        wall_seconds = (start.elapsed().as_secs_f64() - review_seconds).max(0.);
        let (earlier_seconds, masters_seconds) = state.lock().map_or((0., None), |p| {
            (
                p.earlier.as_secs_f64(),
                p.phase_durations[0].map(|d| d.as_secs_f64()),
            )
        });
        total_seconds = wall_seconds + earlier_seconds;
        let peak_rss_bytes = fulgur_core::telemetry::peak_rss_bytes();
        let view = MasterView {
            label: run.label.clone(),
            calibrated_frames,
            master_quality,
            rejection: maps.stats.rejection,
            normalization: integrate::NormalizationSummary::of(&run.sources),
            paths: paths.clone(),
            master: Thumbnail::from_image(&maps.image, true),
            coverage: Thumbnail::from_map(&maps.coverage),
            high: Thumbnail::from_map(&maps.high_rejection),
            accepted: run.accepted_frames,
            total: run.input_frames,
            exposure: run.total_exposure_seconds,
            reference: match (&run.label, &reference) {
                (None, Some(path)) => path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                _ => run.anchor.clone(),
            },
        };
        // Same science/report fields as CLI. UI-specific timing is explicitly separate.
        let summary = serde_json::json!({"schema_version":2,"command":"stack","frontend":"windows_gui","output_file":paths.master.file_name().unwrap_or_default().to_string_lossy(),"coverage_file":paths.save_coverage.then(|| paths.coverage.file_name().unwrap_or_default().to_string_lossy()),"high_rejection_file":paths.save_high_rejection.then(|| paths.high_rejection.file_name().unwrap_or_default().to_string_lossy()),"map_denominator_frames":run.input_frames,"canvas_mode":"reference","origin_x_px":0,"origin_y_px":0,"width":canvas.width,"height":canvas.height,"write_seconds":write_seconds,"wall_seconds":wall_seconds,"review_wait_seconds":review_seconds,"frames_per_second":plan.frame_count as f64/wall_seconds,"peak_rss_bytes":peak_rss_bytes,"output_bitpix":-32,"output_format":fulgur_core::output::OutputFormat::of(&paths.master).unwrap_or_default().key(),"output_units":"linear reference-anchor source ADU / 65535; bounded 0..1","bias_subtracted":false,"gps_copied_to_fits":anchor.valid_gps_cards().is_some(),"intermediate_images_written":0,"coverage_definition":"RGB surviving sample count / all original input frames, after frame/geometry/low/high rejection; fraction 0..1 = 0..100%; not weight or exposure","high_rejection_definition":"RGB upper rejected sample count / all original input frames; fraction 0..1 = 0..100%; excludes lower rejection and invalid edges"});
        let mut summary = summary;
        if let Some(label) = &run.label {
            summary["master_group"] = serde_json::json!({
                "label": label,
                "light_sets": run.sets.iter().map(|&s| plan.inspection.light_sets[s].label.clone()).collect::<Vec<_>>(),
                "photometric_anchor": run.anchor,
            });
        }
        summary["inspection_reused"] = serde_json::json!(reused);
        summary["calibration_masters_seconds"] = serde_json::json!(masters_seconds);
        summary["earlier_measurement_seconds"] = serde_json::json!(earlier_seconds);
        summary["total_processing_seconds"] = serde_json::json!(total_seconds);
        summary["reused_inspection_measurement_seconds"] =
            serde_json::json!(reused.then_some(session.report.wall_seconds));
        summary["manual_selection"] = serde_json::json!(manual_selection);
        summary["light_sets"] = request.provenance.clone();
        summary["calibration_applied"] = serde_json::json!(calibrated_frames > 0);
        summary["calibrated_frames"] = serde_json::json!(calibrated_frames);
        summary["master_quality"] = serde_json::json!(view.master_quality);
        if let Some(a) = &master_astrometry {
            summary["astrometry"] = serde_json::json!(a);
        } else if let Some(reason) = &astrometry_failure {
            summary["astrometry"] = serde_json::json!({ "written": false, "reason": reason });
        }
        summary["normalization"] = serde_json::json!(view.normalization);
        summary["calibration"] = serde_json::json!(
            set_darks
                .iter()
                .map(|c| c.as_ref().map(|c| &c.summary))
                .collect::<Vec<_>>()
        );
        summary["bias_subtracted"] =
            serde_json::json!(set_darks.iter().flatten().any(|c| c.bias.is_some()));
        let report = BorrowedReport {
            summary,
            config: &request.config,
            registration: &plan,
            integration: &maps.stats,
            export: &export,
        };
        atomic_write(&paths.report, cancel, |w| {
            serde_json::to_writer_pretty(&mut *w, &report)
                .map_err(|_| Error::Invalid("Could not serialize report."))?;
            w.write_all(b"\n")?;
            Ok(())
        })?;
        views.push(view);
    }
    let mut final_rows = session.rows();
    for (row, registered) in final_rows.iter_mut().zip(&plan.frames) {
        if row.file_name != registered.file_name {
            return Err(Error::Invalid("Quality row identity mismatch."));
        }
        row.accepted = registered.accepted;
        row.weight = registered.final_weight;
        row.reasons = registered
            .rejection_reasons
            .iter()
            .map(|r| reason_text(r))
            .collect::<Vec<_>>()
            .join(", ");
        if let Some(sky) = &registered.sky {
            row.sky_scale = sky.arcsec_per_px;
            row.sky_held_out = sky.held_out_rms_px;
            row.sky_check = sky
                .check
                .as_ref()
                .filter(|_| registered.registration_success)
                .map(|c| c.worst_cell_offset_target_px);
        }
    }
    let mut views = views.into_iter();
    let first = views
        .next()
        .ok_or(Error::Invalid("A stack needs at least one master."))?;
    let result = Completed {
        label: first.label,
        others: views.collect(),
        calibrated_frames: first.calibrated_frames,
        calibration: set_calibration(&plan, &request.provenance),
        master_quality: first.master_quality,
        rejection: first.rejection,
        normalization: first.normalization,
        partial_summary: plan.partial_summary(),
        camera_summary: plan.camera_summary(),
        set_photometry_summary: plan.set_photometry_summary(),
        astrometry_summary,
        sky_summary: plan.sky.as_ref().map(|s| {
            let centre = sky_position.as_ref().map_or(String::new(), |p| {
                format!(
                    " · field centre RA {:.3}° Dec {:+.3}° from {}",
                    p.ra,
                    p.dec,
                    fulgur_catalog::sky::source_label(p)
                )
            });
            let catalogue = catalogue_lines
                .lock()
                .ok()
                .and_then(|l| l.last().cloned())
                .map_or(String::new(), |l| format!(" · {l}"));
            let fallback = s
                .fallback_note
                .as_ref()
                .map_or(String::new(), |n| format!(" · {n}"));
            format!(
                "Sky: {}/{} frames plate-solved in {:.1} s · {} registered through the sky, {} by stars{centre}{catalogue}{fallback}",
                s.solved, s.frames_to_solve, s.solve_seconds, s.registered_by_sky, s.registered_by_stars
            )
        }),
        auto_reference_summary: plan.auto_reference.as_ref().map(|a| {
            let warning = a.warning().map_or(String::new(), |w| format!(" · ⚠ {w}"));
            format!(
                "Auto: {} nights · est. >=80% Use area {:.0} × {:.0} px · mean {:.0}%{warning}",
                a.nights,
                a.predicted_rectangle_width_px,
                a.predicted_rectangle_height_px,
                100. * a.predicted_mean_selected_coverage
            )
        }),
        geometric_correction: plan.geometric_correction_enabled.then(|| {
            let corrected = plan
                .frames
                .iter()
                .filter(|f| {
                    f.accepted
                        && f.geometric_correction
                            .as_ref()
                            .is_some_and(|d| d.model.is_some())
                })
                .count();
            [
                corrected,
                plan.accepted_count - corrected,
                plan.quality_accepted_count - plan.accepted_count,
            ]
        }),
        rows: final_rows,
        paths: first.paths,
        master: first.master,
        coverage: first.coverage,
        high: first.high,
        wall_seconds,
        total_seconds,
        peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
        accepted: first.accepted,
        total: first.total,
        exposure: first.exposure,
        reference: first.reference,
        reference_anchor: root.join(&plan.reference_anchor),
        width: canvas.width,
        height: canvas.height,
        phase_outcomes: [None, None, Some(registered), Some(integrated), None],
    };
    progress(state, "Complete", 1, 1);
    Ok(result)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    fn input(path: &Path) {
        let mut bytes = Vec::new();
        for (key, value) in [
            ("SIMPLE", "T"),
            ("BITPIX", "16"),
            ("NAXIS", "2"),
            ("NAXIS1", "64"),
            ("NAXIS2", "64"),
            ("BZERO", "32768"),
            ("BAYERPAT", "'GRBG'"),
            ("EXPTIME", "10"),
            ("DATE-OBS", "'2026-10-06T22:00:00'"),
        ] {
            bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
        }
        bytes.extend(format!("{:<80}", "END").as_bytes());
        bytes.resize(2880, b' ');
        for _ in 0..4096 {
            bytes.extend((-31768_i16).to_be_bytes());
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        std::fs::write(path, bytes).unwrap();
    }
    pub(crate) fn request(dir: &Path) -> StackRequest {
        let source = dir.join("source.fit");
        input(&source);
        StackRequest {
            sets: vec![inspect::single_set(&[source], Default::default())],
            calibration: vec![CalibrationSources::default()],
            provenance: serde_json::json!([]),
            output: dir.join("master.fits"),
            reference: None,
            reference_strategy: register::ReferenceStrategy::Auto,
            gpu: false,
            cpu_threads: 0,
            review: true,
            geometric_correction: false,
            partial_frames: false,
            accuracy: register::AccuracyLimits::default(),
            diagnose_excluded: false,
            save_coverage: true,
            save_high_rejection: true,
            config: IntegrationConfig::default(),
            master_split: MasterSplit::Combined,
            sky: None,
            astrometry: None,
        }
    }
    use integrate::IntegrationConfig;

    /// A grid of stars on a level sky; `gain` scales the stars only.
    fn star_frame(path: &Path, filter: &str, gain: f64) {
        let mut bytes = Vec::new();
        let filter = format!("'{filter}'");
        for (key, value) in [
            ("SIMPLE", "T"),
            ("BITPIX", "16"),
            ("NAXIS", "2"),
            ("NAXIS1", "288"),
            ("NAXIS2", "288"),
            ("BZERO", "32768"),
            ("BAYERPAT", "'GRBG'"),
            ("EXPTIME", "10"),
            ("OBJECT", "'M33'"),
            ("DATE-OBS", "'2026-10-06T22:00:00.000000'"),
            ("FILTER", filter.as_str()),
        ] {
            bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
        }
        bytes.extend(format!("{:<80}", "END").as_bytes());
        bytes.resize(2880, b' ');
        for y in 0..288 {
            for x in 0..288 {
                let mut value = 2000. + ((x * 71 + y * 23 + x * y * 11) % 29) as f64 - 14.;
                for cy in [40.3, 88.3, 136.3, 184.3, 232.3] {
                    for cx in [40.2, 88.2, 136.2, 184.2, 232.2] {
                        let r = ((f64::from(x) - cx).powi(2) + (f64::from(y) - cy).powi(2)) / 1.96;
                        value += gain * 10000. * (-0.5 * r).exp();
                    }
                }
                bytes.extend(((value.round() as i32 - 32768) as i16).to_be_bytes());
            }
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn per_filter_run_publishes_one_master_each_and_results_switch_between_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = request(dir.path());
        r.sets = [("IRCUT", 1.), ("Ha", 3.)]
            .into_iter()
            .map(|(filter, gain)| {
                let frames: Vec<PathBuf> = (0..3)
                    .map(|i| {
                        let path = dir.path().join(format!("{filter}_{i}.fit"));
                        star_frame(&path, filter, gain);
                        path
                    })
                    .collect();
                let mut set = inspect::single_set(&frames, Default::default());
                set.label = filter.into();
                set
            })
            .collect();
        r.calibration = vec![CalibrationSources::default(); 2];
        r.review = false;
        r.master_split = MasterSplit::PerFilter;
        let worker = Worker::start(r, false);
        let mut result = loop {
            match worker
                .events
                .recv_timeout(Duration::from_secs(120))
                .unwrap()
            {
                Event::Complete(result) => break result,
                Event::Stopped { message, .. } => panic!("{message}"),
                Event::Quality { .. } => {}
            }
        };
        assert_eq!(result.others.len(), 1);
        assert_eq!(result.label.as_deref(), Some("IRCUT"));
        assert_eq!((result.accepted, result.total), (3, 3));
        assert!(!dir.path().join("master.fits").exists());
        for name in [
            "master_IRCUT.fits",
            "master_IRCUT_report.json",
            "master_Ha.fits",
            "master_Ha_coverage.fits",
            "master_Ha_report.json",
        ] {
            assert!(dir.path().join(name).is_file(), "{name}");
        }
        let first = result.paths.master.clone();
        result.show("Ha");
        assert_eq!(result.label.as_deref(), Some("Ha"));
        assert_eq!(result.paths.master, dir.path().join("master_Ha.fits"));
        assert_eq!(result.others[0].paths.master, first);
        result.show("missing");
        assert_eq!(result.label.as_deref(), Some("Ha"));
    }

    #[test]
    fn sky_star_lists_are_added_without_measuring_again() {
        let dir = tempfile::tempdir().unwrap();
        let r = request(dir.path());
        let cancel = AtomicBool::new(false);
        let session = InspectionSession::inspect(&r.sets, &cancel).unwrap();
        assert!(!session.sky_stars);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let added = session
            .with_sky_stars(&r.sets, &cancel, &|_, _| {
                calls.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();
        assert!(added.sky_stars);
        assert!(calls.load(Ordering::Relaxed) > 0);
        // The measurement itself is the same; only the lists were added.
        assert_eq!(
            serde_json::to_string(&added.report.frames).unwrap(),
            serde_json::to_string(&session.report.frames).unwrap()
        );
        assert!(added.matches_inputs(&r.sets, &cancel).unwrap());
        let with =
            InspectionSession::inspect_with_progress(&r.sets, true, &cancel, &|_, _| {}).unwrap();
        for (a, e) in added.report.frames.iter().zip(&with.report.frames) {
            assert_eq!(a.sky_stars, e.sky_stars, "{}", a.file_name);
        }
    }

    #[test]
    fn windows_validation_no_clobber_and_preflight_leaves_no_temporary_files() {
        for name in [
            "CON.fits",
            "com1.fit",
            "aux.foo.fits",
            "LPT².fits",
            "foo?.fits",
            "foo.fits ",
            "foo.fits.",
            "../bad.fits",
            "master.png",
            "CON.xisf",
            "",
        ] {
            assert!(!valid_filename(name), "{name}");
        }
        for name in [
            "M 33.fits",
            "com10.fit",
            "CONSOLE.fts",
            "Měsíc.fit",
            "M 33.xisf",
            "M42.XISF",
        ] {
            assert!(valid_filename(name), "{name}");
        }
        // Maps take the master's format (ADR-142).
        let xisf = Destinations::new(Path::new("out/M 33.xisf"));
        assert_eq!(xisf.coverage, Path::new("out/M 33_coverage.xisf"));
        assert_eq!(
            xisf.high_rejection,
            Path::new("out/M 33_high_rejection.xisf")
        );
        assert_eq!(xisf.report, Path::new("out/M 33_report.json"));
        let fits = Destinations::new(Path::new("out/M 33.fit"));
        assert_eq!(fits.coverage, Path::new("out/M 33_coverage.fits"));
        let dir = tempfile::tempdir().unwrap();
        let r = request(dir.path());
        let original = std::fs::read(&r.sets[0].inputs[0]).unwrap();
        assert!(r.validate().is_ok());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        std::fs::write(Destinations::new(&r.output).coverage, b"existing").unwrap();
        assert_eq!(
            r.validate().unwrap().master,
            dir.path().join("master_v2.fits")
        );
        assert_eq!(
            std::fs::read(Destinations::new(&r.output).coverage).unwrap(),
            b"existing"
        );
        assert_eq!(std::fs::read(&r.sets[0].inputs[0]).unwrap(), original);
    }
    #[test]
    fn inspection_session_rejects_source_mutation_and_redacts_location() {
        let dir = tempfile::tempdir().unwrap();
        let r = request(dir.path());
        let cancel = AtomicBool::new(false);
        let session = InspectionSession::inspect(&r.sets, &cancel).unwrap();
        assert!(session.verify().is_ok());
        assert!(session.matches_inputs(&r.sets, &cancel).unwrap());
        let mut mono = r.sets.clone();
        mono[0].layout = fulgur_core::LayoutChoice::Mono;
        assert!(
            !session.matches_inputs(&mono, &cancel).unwrap(),
            "a changed layout invalidates the inspection"
        );
        let json = serde_json::to_string(&session.rows()).unwrap();
        assert!(!json.contains("SITELAT") && !json.contains("SITELONG"));
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&r.sets[0].inputs[0])
            .unwrap()
            .write_all(b"change")
            .unwrap();
        assert!(session.verify().is_err());
    }
    #[test]
    fn review_wait_is_cancellable_without_outputs_or_source_changes() {
        let dir = tempfile::tempdir().unwrap();
        let r = request(dir.path());
        let original = std::fs::read(&r.sets[0].inputs[0]).unwrap();
        let destinations = Destinations::new(&r.output);
        let worker = Worker::start(r, false);
        let event = worker.events.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(matches!(event, Event::Quality { waiting: true, .. }));
        assert!(destinations.published().is_empty());
        worker.cancel.store(true, Ordering::Relaxed);
        assert!(matches!(
            worker.events.recv_timeout(Duration::from_secs(10)).unwrap(),
            Event::Stopped {
                cancelled: true,
                ..
            }
        ));
        drop(worker);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(
            std::fs::read(dir.path().join("source.fit")).unwrap(),
            original
        );
    }

    /// A star-registered run of a real light folder with a Gaia catalogue
    /// writes the master's WCS, as the app runs it (ADR-155).
    #[test]
    #[ignore = "needs FULGURSTACK_TEST_ASTROMETRY_DIR, _GAIA and _OUT"]
    fn real_star_run_writes_the_master_wcs() {
        let var = |name: &str| PathBuf::from(std::env::var_os(name).unwrap());
        let out = var("FULGURSTACK_TEST_ASTROMETRY_OUT");
        let folder = var("FULGURSTACK_TEST_ASTROMETRY_DIR");
        let new_request = || {
            let mut r = real_request(&out, &folder);
            r.astrometry = Some(fulgur_catalog::sky::SkyChoice {
                sources: fulgur_catalog::GaiaSources {
                    pixinsight: Some(var("FULGURSTACK_TEST_ASTROMETRY_GAIA")),
                    cache: out.join("gaia_cache"),
                    download: true,
                },
                centre: None,
                object: std::env::var("FULGURSTACK_TEST_ASTROMETRY_OBJECT").ok(),
                online: true,
            });
            r
        };
        let run = |worker: Worker| {
            let mut session = None;
            let result = loop {
                match worker
                    .events
                    .recv_timeout(Duration::from_secs(1200))
                    .unwrap()
                {
                    Event::Complete(result) => break result,
                    Event::Stopped { message, .. } => panic!("{message}"),
                    Event::Quality { inspection, .. } => session = Some(inspection),
                }
            };
            (result, session)
        };
        let (first, session) = run(Worker::start(new_request(), false));
        let summary = first.astrometry_summary.clone().unwrap_or_default();
        println!("Astrometry, fresh inspection: {summary}");
        // The same inspection reused with the owner's selection, as a second
        // Stack in the app does.
        let selection = std::env::var_os("FULGURSTACK_TEST_ASTROMETRY_SELECTION")
            .map(|report| {
                let report: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
                report["manual_selection"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| {
                        let name = row["file_name"].as_str().unwrap();
                        let session = session.as_ref().unwrap();
                        let frame = session
                            .report
                            .frames
                            .iter()
                            .find(|f| f.file_name.ends_with(name))
                            .unwrap();
                        FrameSelection {
                            path: session.root.join(&frame.file_name),
                            include: row["included"].as_bool().unwrap(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (second, _) = run(Worker::start_with_inspection(
            new_request(),
            false,
            session,
            selection,
        ));
        let reused = second.astrometry_summary.clone().unwrap_or_default();
        println!("Astrometry, reused inspection: {reused}");
        assert!(summary.starts_with("WCS written"), "{summary}");
        assert!(reused.starts_with("WCS written"), "{reused}");
    }

    fn real_request(out: &Path, folder: &Path) -> StackRequest {
        let mut r = request(out);
        r.sets = vec![inspect::single_set(
            &[folder.to_path_buf()],
            Default::default(),
        )];
        r.output = out.join("master.xisf");
        r.review = false;
        r.geometric_correction = true;
        r.partial_frames = true;
        r.save_coverage = false;
        r.save_high_rejection = false;
        r.master_split = MasterSplit::PerSet;
        r
    }
}
