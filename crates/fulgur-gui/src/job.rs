//! Single-job orchestration. Scientific arithmetic and writers belong to the pipeline.
use crate::preview::Thumbnail;
use fulgur_core::output::{atomic_write, ensure_new_destination};
use fulgur_core::{Error, Result, check_cancelled};
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

pub const PHASES: [&str; 4] = ["Inspect", "Register", "Integrate", "Write outputs"];

#[derive(Clone, Default, Debug)]
pub struct Progress {
    pub phase: &'static str,
    pub completed: usize,
    pub total: usize,
    pub phase_started: Option<Instant>,
    pub updated_at: Option<Instant>,
    pub integration_seconds: Option<f64>,
    pub phase_durations: [Option<Duration>; 4],
    pub inspection_reused: bool,
}
impl Progress {
    pub fn phase_times(&self) -> [Option<Duration>; 4] {
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

#[derive(Clone, Debug)]
pub struct StackRequest {
    pub inputs: Vec<PathBuf>,
    pub output: PathBuf,
    pub reference: Option<PathBuf>,
    pub gpu: bool,
    pub review: bool,
    pub save_coverage: bool,
    pub save_high_rejection: bool,
    pub config: integrate::IntegrationConfig,
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
        Self {
            master: master.into(),
            coverage: sibling("_coverage.fits"),
            high_rejection: sibling("_high_rejection.fits"),
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
        && Path::new(name).extension().is_some_and(|e| {
            ["fit", "fits", "fts"]
                .iter()
                .any(|x| e.eq_ignore_ascii_case(x))
        })
}

impl StackRequest {
    fn available_destinations(&self) -> Result<Destinations> {
        let original =
            Destinations::with_maps(&self.output, self.save_coverage, self.save_high_rejection);
        if original.available()? {
            return Ok(original);
        }
        let stem = self
            .output
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let versioned = stem.rsplit_once("_v").and_then(|(base, number)| {
            number
                .parse::<u64>()
                .ok()
                .filter(|&n| n >= 2)
                .map(|n| (base, n))
        });
        let (base, previous) = versioned.unwrap_or((&stem, 1));
        let extension = self
            .output
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        let mut version = previous;
        loop {
            version = version.checked_add(1).ok_or(Error::Invalid(
                "Output version limit reached. Choose another master filename.",
            ))?;
            let master = self
                .output
                .with_file_name(format!("{base}_v{version}.{extension}"));
            let paths =
                Destinations::with_maps(&master, self.save_coverage, self.save_high_rejection);
            if paths.available()? {
                return Ok(paths);
            }
        }
    }
    pub fn validate(&self) -> Result<Destinations> {
        self.config.validate()?;
        if self
            .reference
            .as_ref()
            .is_some_and(|p| p.as_os_str().is_empty())
        {
            return Err(Error::Invalid(
                "Choose an accepted or external reference, or select Automatic.",
            ));
        }
        if self.inputs.is_empty() {
            return Err(Error::Invalid("Add FITS files or folders first."));
        }
        for input in &self.inputs {
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
                "Choose a valid Windows filename ending in .fit, .fits or .fts.",
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
    pub exposure: Option<f64>,
    pub date: Option<String>,
    pub hfr: Option<f64>,
    pub stars: usize,
    pub eccentricity: Option<f64>,
    pub weight: f64,
    pub accepted: bool,
    pub automatic_accepted: bool,
    pub can_include: bool,
    pub reasons: String,
}
fn rows(report: &inspect::InspectReport, root: &Path) -> Vec<QualityRow> {
    report
        .frames
        .iter()
        .map(|f| QualityRow {
            file_name: f.file_name.clone(),
            path: root.join(&f.file_name),
            exposure: f.frame.as_ref().map(|h| h.exposure_seconds),
            date: f.frame.as_ref().and_then(|h| h.date_obs.clone()),
            hfr: f.hfr_px,
            stars: f.star_count,
            eccentricity: f.eccentricity,
            weight: f.provisional_weight,
            accepted: f.accepted,
            automatic_accepted: f.accepted,
            can_include: can_include(f),
            reasons: f.rejection_reasons.join(", "),
        })
        .collect()
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

fn discover(inputs: &[PathBuf], cancel: &AtomicBool) -> Result<Vec<PathBuf>> {
    let mut files = vec![];
    for input in inputs {
        check_cancelled(cancel)?;
        fulgur_pipeline::survey::collect_inputs(input, &mut files)?;
    }
    let mut files = files
        .into_iter()
        .map(|p| p.canonicalize())
        .collect::<std::io::Result<Vec<_>>>()?;
    files.sort();
    files.dedup();
    Ok(files)
}

#[derive(Debug)]
struct SourceIdentity {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}
impl SourceIdentity {
    fn read(path: PathBuf) -> Result<Self> {
        let m = path.metadata()?;
        Ok(Self {
            path,
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
}
impl InspectionSession {
    pub fn matches_inputs(&self, inputs: &[PathBuf], cancel: &AtomicBool) -> Result<bool> {
        let files = discover(inputs, cancel)?;
        if files.len() != self.identities.len()
            || files
                .iter()
                .zip(&self.identities)
                .any(|(path, id)| path != &id.path)
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
        for row in &mut rows {
            if let Some(choice) = selection.iter().find(|s| s.path == row.path) {
                row.accepted = choice.include && row.can_include;
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
                    "This frame cannot be included: its FITS data, compatibility or measurements are unusable.",
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
    pub fn inspect(inputs: &[PathBuf], cancel: &AtomicBool) -> Result<Self> {
        Self::inspect_with_progress(inputs, cancel, &|_, _| {})
    }
    pub fn inspect_with_progress(
        inputs: &[PathBuf],
        cancel: &AtomicBool,
        observer: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<Self> {
        let mut files = vec![];
        for path in inputs {
            fulgur_pipeline::survey::collect_inputs(path, &mut files)?;
        }
        let mut files = files
            .into_iter()
            .map(|p| p.canonicalize())
            .collect::<std::io::Result<Vec<_>>>()?;
        files.sort();
        files.dedup();
        let identities = files
            .into_iter()
            .map(SourceIdentity::read)
            .collect::<Result<Vec<_>>>()?;
        let (report, root) = inspect::inspect_inputs_with_progress(
            inputs,
            inspect::InspectConfig::default(),
            cancel,
            observer,
        )?;
        let session = Self {
            report,
            root,
            identities,
        };
        session.verify()?;
        Ok(session)
    }
    pub fn rows(&self) -> Vec<QualityRow> {
        rows(&self.report, &self.root)
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
        reference: Option<PathBuf>,
        selection: Vec<FrameSelection>,
    },
}
pub struct Completed {
    pub rows: Vec<QualityRow>,
    pub paths: Destinations,
    pub master: Thumbnail,
    pub coverage: Thumbnail,
    pub high: Thumbnail,
    pub wall_seconds: f64,
    pub peak_rss_bytes: Option<u64>,
    pub accepted: usize,
    pub total: usize,
    pub exposure: f64,
    pub reference: String,
    pub width: usize,
    pub height: usize,
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
        let c = cancel.clone();
        let s = state.clone();
        let handle = std::thread::spawn(move || {
            let terminal_state = s.clone();
            let published_destinations = selected.clone();
            let work = || -> Result<()> {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(
                        std::thread::available_parallelism()
                            .map(usize::from)
                            .unwrap_or(1)
                            .min(16),
                    )
                    .build()
                    .map_err(|_| Error::Invalid("Could not start CPU workers."))?;
                let events = tx.clone();
                pool.install(move || {
                    if inspect_only {
                        progress(&s, "Inspect", 0, 0);
                        let session = Arc::new(InspectionSession::inspect_with_progress(
                            &request.inputs,
                            &c,
                            &|n, total| progress(&s, "Inspect", n, total),
                        )?);
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
                    let start = Instant::now();
                    progress(&s, "Validate inputs", 0, 0);
                    let reused = cached
                        .as_ref()
                        .map(|previous| previous.matches_inputs(&request.inputs, &c))
                        .transpose()?
                        .unwrap_or(false);
                    let session = if reused {
                        if let Ok(mut p) = s.lock() {
                            p.inspection_reused = true;
                            p.phase_durations[0] = Some(Duration::ZERO);
                        }
                        cached.unwrap()
                    } else {
                        selection.clear();
                        progress(&s, "Inspect", 0, 0);
                        Arc::new(InspectionSession::inspect_with_progress(
                            &request.inputs,
                            &c,
                            &|n, total| progress(&s, "Inspect", n, total),
                        )?)
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
                                    reference: chosen,
                                    selection: selected,
                                }) => {
                                    reference = chosen;
                                    selection = selected;
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
                        .and_then(|p| p.as_ref().map(Destinations::published))
                        .unwrap_or_default(),
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
    session: Arc<InspectionSession>,
    reference: Option<PathBuf>,
    cancel: &AtomicBool,
    state: &SharedProgress,
    start: Instant,
    review_seconds: f64,
    reused: bool,
    selection: Vec<FrameSelection>,
) -> Result<Completed> {
    if !session.matches_inputs(&request.inputs, cancel)? {
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
    let (plan, root) = register::register_inspected_with_progress(
        inspection,
        session.root.clone(),
        0.8,
        reference.as_deref(),
        cancel,
        engine_start,
        &|n, total| progress(state, "Register", n, total),
    )?;
    let anchor = fulgur_core::fits::read_header(&root.join(&plan.reference_anchor))?;
    let canvas = register::Canvas {
        origin_x_px: 0,
        origin_y_px: 0,
        width: plan.reference_width,
        height: plan.reference_height,
        minimum_effective_weight_fraction: 0.,
        coordinate_rule: "exact reference FITS dimensions and sensor pixel grid; no automatic crop",
    };
    let sources = integrate::sources(&root, &plan)?;
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
    let maps = engine.integrate_with_progress(
        &sources,
        &canvas,
        &request.config,
        plan.frame_count,
        cancel,
        &|n, total| progress(state, "Integrate", n, total),
    )?;
    let output_count = paths.paths().len();
    progress(state, "Write outputs", 0, output_count);
    let metadata = fulgur_core::output::MasterMetadata {
        accepted_frames: plan.accepted_count,
        total_exposure_seconds: plan.accepted_exposure_seconds,
        origin_x_px: 0,
        origin_y_px: 0,
        external_geometry_scale: plan
            .external_reference
            .as_ref()
            .map(|e| e.anchor_to_reference.scale_factor),
    };
    let write = Instant::now();
    let export = fulgur_core::output::write_master_atomic(
        &paths.master,
        &maps.image,
        &anchor,
        &metadata,
        cancel,
    )?;
    let mut written = 1;
    progress(state, "Write outputs", written, output_count);
    if paths.save_coverage {
        fulgur_core::output::write_fraction_map_atomic(
            &paths.coverage,
            &maps.coverage,
            "COVERAGE",
            plan.frame_count,
            cancel,
        )?;
        written += 1;
        progress(state, "Write outputs", written, output_count);
    }
    if paths.save_high_rejection {
        fulgur_core::output::write_fraction_map_atomic(
            &paths.high_rejection,
            &maps.high_rejection,
            "HIGHREJ",
            plan.frame_count,
            cancel,
        )?;
        written += 1;
        progress(state, "Write outputs", written, output_count);
    }
    let write_seconds = write.elapsed().as_secs_f64();
    let wall_seconds = (start.elapsed().as_secs_f64() - review_seconds).max(0.);
    let mut final_rows = session.rows();
    for (row, registered) in final_rows.iter_mut().zip(&plan.frames) {
        if row.file_name != registered.file_name {
            return Err(Error::Invalid("Quality row identity mismatch."));
        }
        row.accepted = registered.accepted;
        row.weight = registered.final_weight;
        row.reasons = registered.rejection_reasons.join(", ");
    }
    let result = Completed {
        rows: final_rows,
        paths: paths.clone(),
        master: Thumbnail::from_image(&maps.image, true),
        coverage: Thumbnail::from_image(&maps.coverage, false),
        high: Thumbnail::from_image(&maps.high_rejection, false),
        wall_seconds,
        peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
        accepted: plan.accepted_count,
        total: plan.frame_count,
        exposure: plan.accepted_exposure_seconds,
        reference: reference
            .as_ref()
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| plan.reference_anchor.clone()),
        width: canvas.width,
        height: canvas.height,
    };
    // Same science/report fields as CLI. UI-specific timing is explicitly separate.
    let summary = serde_json::json!({"schema_version":2,"command":"stack","frontend":"windows_gui","output_file":paths.master.file_name().unwrap_or_default().to_string_lossy(),"coverage_file":paths.save_coverage.then(|| paths.coverage.file_name().unwrap_or_default().to_string_lossy()),"high_rejection_file":paths.save_high_rejection.then(|| paths.high_rejection.file_name().unwrap_or_default().to_string_lossy()),"map_denominator_frames":plan.frame_count,"canvas_mode":"reference","origin_x_px":0,"origin_y_px":0,"width":canvas.width,"height":canvas.height,"write_seconds":write_seconds,"wall_seconds":wall_seconds,"review_wait_seconds":review_seconds,"frames_per_second":result.total as f64/wall_seconds,"peak_rss_bytes":result.peak_rss_bytes,"output_bitpix":-32,"output_units":"linear reference-anchor source ADU / 65535; bounded 0..1","bias_subtracted":false,"gps_copied_to_fits":anchor.valid_gps_cards().is_some(),"intermediate_images_written":0,"coverage_definition":"RGB surviving sample count / all original input frames, after frame/geometry/low/high rejection; fraction 0..1 = 0..100%; not weight or exposure","high_rejection_definition":"RGB upper rejected sample count / all original input frames; fraction 0..1 = 0..100%; excludes lower rejection and invalid edges"});
    let mut summary = summary;
    summary["inspection_reused"] = serde_json::json!(reused);
    summary["reused_inspection_measurement_seconds"] =
        serde_json::json!(reused.then_some(session.report.wall_seconds));
    summary["manual_selection"] = serde_json::json!(manual_selection);
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
    progress(state, "Complete", output_count, output_count);
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
            inputs: vec![source],
            output: dir.join("master.fits"),
            reference: None,
            gpu: false,
            review: true,
            save_coverage: true,
            save_high_rejection: true,
            config: IntegrationConfig::default(),
        }
    }
    use integrate::IntegrationConfig;
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
            "",
        ] {
            assert!(!valid_filename(name), "{name}");
        }
        for name in ["M 33.fits", "com10.fit", "CONSOLE.fts", "Měsíc.fit"] {
            assert!(valid_filename(name), "{name}");
        }
        let dir = tempfile::tempdir().unwrap();
        let r = request(dir.path());
        let original = std::fs::read(&r.inputs[0]).unwrap();
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
        assert_eq!(std::fs::read(&r.inputs[0]).unwrap(), original);
    }
    #[test]
    fn inspection_session_rejects_source_mutation_and_redacts_location() {
        let dir = tempfile::tempdir().unwrap();
        let r = request(dir.path());
        let cancel = AtomicBool::new(false);
        let session = InspectionSession::inspect(&r.inputs, &cancel).unwrap();
        assert!(session.verify().is_ok());
        let json = serde_json::to_string(&session.rows()).unwrap();
        assert!(!json.contains("SITELAT") && !json.contains("SITELONG"));
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&r.inputs[0])
            .unwrap()
            .write_all(b"change")
            .unwrap();
        assert!(session.verify().is_err());
    }
    #[test]
    fn review_wait_is_cancellable_without_outputs_or_source_changes() {
        let dir = tempfile::tempdir().unwrap();
        let r = request(dir.path());
        let original = std::fs::read(&r.inputs[0]).unwrap();
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
}
