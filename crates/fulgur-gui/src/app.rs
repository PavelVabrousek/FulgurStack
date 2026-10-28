//! Four-step workspace (Frames, Settings, Processing, Results) over the
//! accepted pipeline, light/calibration sets and bounded image viewers.
mod compact;
mod frames;
mod quality_table;
use crate::{
    image_view::{DetailSource, ImageView},
    job::{self, Completed, Event, QualityRow, StackRequest, Worker},
    preview::{DisplayOptions, PreviewTask, Thumbnail},
    sets::{
        self, CalibrationItem, CalibrationKind, CalibrationSet, CalibrationSource, Level, LightSet,
        ScanResult, Scanner,
    },
    settings::{self, AppSettings},
};
use eframe::egui::{self, Color32, RichText, TextureHandle};
use fulgur_core::LayoutChoice;
use fulgur_pipeline::integrate::{IntegrationConfig, TileMode};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    Frames,
    Settings,
    Processing,
    Results,
}

/// What the Frames step shows in its main area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Selection {
    All,
    Light(u64),
    Calibration(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Action,
    ConfirmMixed,
}

/// Workspace sets to remove (never files on disk).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClearKind {
    Lights,
    Calibration,
    All,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RunKind {
    Measure,
    Stack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LastRun {
    Measured,
    Done,
    Stopped,
}

pub struct Workspace {
    view: View,
    light_sets: Vec<LightSet>,
    calibration_sets: Vec<CalibrationSet>,
    next_id: u64,
    selection: Selection,
    scanner: Scanner,
    /// Light set ids in the order of the current inspection's sets.
    inspected_ids: Vec<u64>,
    show_preview: bool,
    show_quality_chart: bool,
    advanced_open: bool,
    menu: Option<Menu>,
    pending_clear: Option<ClearKind>,
    run_kind: Option<RunKind>,
    last_run: Option<LastRun>,
    /// Blink the selected frame against the registration reference every second.
    blink: bool,
    blink_epoch: Instant,
    reference_task: Option<PreviewTask>,
    reference_preview: Option<(PathBuf, Thumbnail)>,
    reference_texture: Option<TextureHandle>,
    reference_view: ImageView,
    active_request: Option<StackRequest>,
    result_request: Option<StackRequest>,
    result_phase_times: [Option<Duration>; 4],
    result_inspection_reused: bool,
    rows: Vec<QualityRow>,
    inspection: Option<Arc<job::InspectionSession>>,
    manual_selection: BTreeMap<PathBuf, bool>,
    final_weights: bool,
    selected: Option<usize>,
    quality_table: quality_table::QualityTable,
    folder: String,
    filename: String,
    reference_mode: usize,
    reference: String,
    review: bool,
    geometric_correction: bool,
    diagnose_excluded: bool,
    save_coverage: bool,
    save_high_rejection: bool,
    gpu: bool,
    tile: usize,
    cpu_threads: usize,
    memory: usize,
    worker: Option<Worker>,
    preview_task: Option<PreviewTask>,
    preview: Option<Thumbnail>,
    preview_texture: Option<TextureHandle>,
    result_texture: Option<TextureHandle>,
    preview_view: ImageView,
    result_view: ImageView,
    stretch: bool,
    result_stretch: bool,
    map_display_max: [f32; 2],
    layer: usize,
    channel: usize,
    result: Option<Box<Completed>>,
    waiting: bool,
    message: String,
    started: Option<Instant>,
    review_started: Option<Instant>,
    review_wait: Duration,
    finished_elapsed: Option<Duration>,
    run_complete: bool,
    last_progress: Option<job::Progress>,
    output_plan: Option<job::Destinations>,
    settings_path: Option<PathBuf>,
    settings_last: Option<AppSettings>,
    settings_pending: Option<(AppSettings, Instant)>,
    settings_notice: Option<String>,
    settings_save_failed: bool,
    confirm_close: bool,
    exit_after_cancel: bool,
    smoke: Option<NativeSmoke>,
}

struct NativeSmoke {
    folder: PathBuf,
    stage: usize,
    pending: Option<String>,
    started: Instant,
    captures: Vec<String>,
    last_trace: String,
    quick: bool,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            view: View::Frames,
            light_sets: vec![],
            calibration_sets: vec![],
            next_id: 1,
            selection: Selection::All,
            scanner: Scanner::default(),
            inspected_ids: vec![],
            show_preview: true,
            show_quality_chart: false,
            advanced_open: false,
            menu: None,
            pending_clear: None,
            run_kind: None,
            last_run: None,
            blink: false,
            blink_epoch: Instant::now(),
            reference_task: None,
            reference_preview: None,
            reference_texture: None,
            reference_view: ImageView::default(),
            active_request: None,
            result_request: None,
            result_phase_times: [None; 4],
            result_inspection_reused: false,
            rows: vec![],
            inspection: None,
            manual_selection: BTreeMap::new(),
            final_weights: false,
            selected: None,
            quality_table: quality_table::QualityTable::default(),
            folder: std::env::current_dir()
                .unwrap_or_default()
                .join("outputs")
                .to_string_lossy()
                .into_owned(),
            filename: "master.fits".into(),
            reference_mode: 0,
            reference: String::new(),
            review: false,
            geometric_correction: false,
            diagnose_excluded: false,
            save_coverage: false,
            save_high_rejection: false,
            gpu: false,
            tile: 0,
            cpu_threads: 0,
            memory: 12288,
            worker: None,
            preview_task: None,
            preview: None,
            preview_texture: None,
            result_texture: None,
            preview_view: ImageView::default(),
            result_view: ImageView::default(),
            stretch: true,
            result_stretch: true,
            map_display_max: [1., 0.05],
            layer: 0,
            channel: 1,
            result: None,
            waiting: false,
            message: "Add light frames to start. Originals remain unchanged.".into(),
            started: None,
            review_started: None,
            review_wait: Duration::ZERO,
            finished_elapsed: None,
            run_complete: false,
            last_progress: None,
            output_plan: None,
            settings_path: None,
            settings_last: None,
            settings_pending: None,
            settings_notice: None,
            settings_save_failed: false,
            confirm_close: false,
            exit_after_cancel: false,
            smoke: None,
        }
    }
}

fn layout_key(layout: LayoutChoice) -> String {
    layout.name().to_owned()
}

impl Workspace {
    fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn light(&self, id: u64) -> Option<&LightSet> {
        self.light_sets.iter().find(|s| s.id == id)
    }
    fn light_mut(&mut self, id: u64) -> Option<&mut LightSet> {
        self.light_sets.iter_mut().find(|s| s.id == id)
    }
    fn calibration(&self, id: u64) -> Option<&CalibrationSet> {
        self.calibration_sets.iter().find(|s| s.id == id)
    }
    fn calibration_mut(&mut self, id: u64) -> Option<&mut CalibrationSet> {
        self.calibration_sets.iter_mut().find(|s| s.id == id)
    }

    fn included_sets(&self) -> impl Iterator<Item = &LightSet> {
        self.light_sets
            .iter()
            .filter(|s| s.include && !s.sources.is_empty())
    }

    /// Light set id of an inspected quality row.
    fn row_set_id(&self, row: &QualityRow) -> Option<u64> {
        self.inspected_ids.get(row.set).copied()
    }
    fn layout_of_row(&self, index: usize) -> LayoutChoice {
        self.rows
            .get(index)
            .and_then(|r| self.row_set_id(r))
            .and_then(|id| self.light(id))
            .map_or(LayoutChoice::Auto, |s| s.layout)
    }

    /// Distinct filters of the included sets (mixed filters are allowed with a warning).
    fn included_filters(&self) -> Vec<String> {
        let mut filters = vec![];
        for set in self.included_sets() {
            for f in set.summary.iter().flat_map(|s| s.filters.iter()) {
                if !filters.contains(f) {
                    filters.push(f.clone());
                }
            }
        }
        filters
    }
    fn mixed_filters(&self) -> bool {
        self.included_filters().len() > 1
    }
    fn mixed_layouts(&self) -> bool {
        let mono = self.included_sets().filter(|s| s.is_mono()).count();
        mono > 0 && mono < self.included_sets().count()
    }
    /// Largest included set; others must match its sensor size.
    fn reference_size(&self) -> Option<(usize, usize)> {
        self.included_sets()
            .filter_map(|s| s.summary.as_ref().map(|m| (m.files, (m.width, m.height))))
            .max_by_key(|(files, _)| *files)
            .map(|(_, size)| size)
    }
    fn size_differs(&self, set: &LightSet) -> bool {
        match (self.reference_size(), set.summary.as_ref()) {
            (Some(reference), Some(s)) => (s.width, s.height) != reference && s.width > 0,
            _ => false,
        }
    }

    /// Reason the primary action cannot start, in owner language.
    fn blocker(&self) -> Option<String> {
        if self.light_sets.is_empty() {
            return Some("Add a folder of light frames to begin.".into());
        }
        if self.included_sets().next().is_none() {
            return Some("Include at least one light set in the stack.".into());
        }
        if self.included_sets().any(|s| s.summary.is_none()) {
            return Some("Reading FITS headers…".into());
        }
        if let Some(set) = self.included_sets().find(|s| s.needs_layout()) {
            return Some(format!(
                "Choose colour or mono for light set “{}” (no Bayer pattern in its headers).",
                set.name
            ));
        }
        None
    }

    fn frame_selection(&self) -> Vec<job::FrameSelection> {
        let mut choices = self.manual_selection.clone();
        for path in &self.quality_table.removed {
            choices.insert(path.clone(), false);
        }
        choices
            .iter()
            .map(|(path, &include)| job::FrameSelection {
                path: path.clone(),
                include,
            })
            .collect()
    }
    fn refresh_quality_selection(&mut self) {
        if let Some(inspection) = &self.inspection {
            self.rows = inspection.rows_with_selection(&self.frame_selection());
            self.final_weights = false;
        }
    }

    /// Light-set membership or layouts changed: measurements no longer apply.
    fn invalidate_measurements(&mut self, message: &str) {
        if self.rows.is_empty() && self.inspection.is_none() {
            return;
        }
        self.preview_task = None;
        self.preview_view.reset();
        self.rows.clear();
        self.inspection = None;
        self.inspected_ids.clear();
        self.manual_selection.clear();
        self.quality_table.reset_rows();
        self.final_weights = false;
        self.selected = None;
        self.preview = None;
        self.preview_texture = None;
        self.message = message.into();
    }

    fn saved_sets(
        &self,
    ) -> (
        Vec<settings::SavedLightSet>,
        Vec<settings::SavedCalibrationSet>,
    ) {
        let calibration: Vec<_> = self
            .calibration_sets
            .iter()
            .map(|c| settings::SavedCalibrationSet {
                name: c.name.clone(),
                items: CalibrationKind::ALL
                    .iter()
                    .filter_map(|&k| {
                        c.item(k).map(|i| settings::SavedCalibrationItem {
                            kind: k.key().into(),
                            master: match &i.source {
                                CalibrationSource::Master(p) => Some(settings::absolute(p)),
                                CalibrationSource::Frames(_) => None,
                            },
                            frames: match &i.source {
                                CalibrationSource::Frames(f) => {
                                    f.iter().map(|p| settings::absolute(p)).collect()
                                }
                                CalibrationSource::Master(_) => vec![],
                            },
                            enabled: i.enabled,
                        })
                    })
                    .collect(),
            })
            .collect();
        let lights = self
            .light_sets
            .iter()
            .map(|s| settings::SavedLightSet {
                name: s.name.clone(),
                sources: s.sources.iter().map(|p| settings::absolute(p)).collect(),
                layout: layout_key(s.layout),
                include: s.include,
                calibration: s
                    .calibration
                    .and_then(|id| self.calibration_sets.iter().position(|c| c.id == id)),
            })
            .collect();
        (lights, calibration)
    }

    fn settings_snapshot(&self) -> AppSettings {
        let reference_path = if !self.reference.trim().is_empty()
            && std::path::Path::new(&self.reference).exists()
        {
            settings::absolute(std::path::Path::new(&self.reference))
                .to_string_lossy()
                .into_owned()
        } else {
            self.reference.clone()
        };
        let (light_sets, calibration_sets) = self.saved_sets();
        AppSettings {
            inputs: vec![],
            light_sets,
            calibration_sets,
            output_folder: if self.folder.trim().is_empty() {
                self.folder.clone()
            } else {
                settings::absolute(std::path::Path::new(&self.folder))
                    .to_string_lossy()
                    .into_owned()
            },
            master_filename: self.filename.clone(),
            reference_mode: self.reference_mode,
            reference_path,
            save_coverage: self.save_coverage,
            save_high_rejection: self.save_high_rejection,
            review_quality: false,
            geometric_correction: self.geometric_correction,
            diagnose_excluded: self.diagnose_excluded,
            gpu: self.gpu,
            tile_size: self.tile,
            cpu_threads: self.cpu_threads,
            memory_budget_mib: self.memory,
            ..AppSettings::default()
        }
    }

    fn push_light_set(&mut self, name: String, sources: Vec<PathBuf>) -> u64 {
        let id = self.new_id();
        self.light_sets.push(LightSet {
            id,
            name,
            sources,
            layout: LayoutChoice::Auto,
            include: true,
            calibration: None,
            summary: None,
        });
        id
    }

    fn add_light_set(
        &mut self,
        name: String,
        sources: Vec<PathBuf>,
        ctx: Option<&egui::Context>,
    ) -> u64 {
        let id = self.push_light_set(name, sources.clone());
        self.scan_light(id, sources, ctx);
        id
    }

    fn scan_light(&self, id: u64, sources: Vec<PathBuf>, ctx: Option<&egui::Context>) {
        let ctx = ctx.cloned();
        self.scanner.light(id, sources, move || {
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        });
    }

    fn restore_settings(&mut self, saved: AppSettings) {
        let mut calibration_ids = vec![];
        for set in &saved.calibration_sets {
            let id = self.new_id();
            calibration_ids.push(id);
            let mut items: [Option<CalibrationItem>; 4] = Default::default();
            for item in &set.items {
                let Some(kind) = CalibrationKind::from_key(&item.kind) else {
                    continue;
                };
                let source = match &item.master {
                    Some(master) => CalibrationSource::Master(master.clone()),
                    None if !item.frames.is_empty() => {
                        CalibrationSource::Frames(item.frames.clone())
                    }
                    None => continue,
                };
                items[kind.index()] = Some(CalibrationItem {
                    source,
                    enabled: item.enabled,
                    summary: None,
                });
            }
            self.calibration_sets.push(CalibrationSet {
                id,
                name: set.name.clone(),
                items,
            });
        }
        let mut lights = saved.light_sets.clone();
        if lights.is_empty() {
            // Earlier versions kept a flat input list: one light set per entry.
            lights = saved
                .inputs
                .iter()
                .map(|p| settings::SavedLightSet {
                    name: default_set_name(p),
                    sources: vec![p.clone()],
                    layout: "auto".into(),
                    include: true,
                    calibration: None,
                })
                .collect();
        }
        for saved_set in lights {
            let id = self.new_id();
            self.light_sets.push(LightSet {
                id,
                name: saved_set.name,
                sources: saved_set.sources,
                layout: LayoutChoice::parse(&saved_set.layout).unwrap_or_default(),
                include: saved_set.include,
                calibration: saved_set
                    .calibration
                    .and_then(|i| calibration_ids.get(i).copied()),
                summary: None,
            });
        }
        if !saved.output_folder.trim().is_empty() {
            self.folder = saved.output_folder;
        }
        if job::valid_filename(&saved.master_filename) {
            self.filename = saved.master_filename;
        }
        self.reference_mode = if saved.reference_mode <= 3 {
            saved.reference_mode
        } else {
            0
        };
        self.reference = saved.reference_path;
        self.save_coverage = saved.save_coverage;
        self.save_high_rejection = saved.save_high_rejection;
        self.diagnose_excluded = saved.diagnose_excluded;
        self.geometric_correction = saved.geometric_correction;
        self.gpu = saved.gpu;
        self.tile = if [0, 128, 256, 512].contains(&saved.tile_size) {
            saved.tile_size
        } else {
            0
        };
        self.memory = saved.memory_budget_mib.clamp(1024, 12288);
        self.cpu_threads = if [0, 12, 16, 24].contains(&saved.cpu_threads) {
            saved.cpu_threads
        } else {
            0
        };
        self.message = "Last light sets and settings restored.".into();
    }

    /// Start header scans of restored sets (needs a repaint context).
    fn rescan_all(&self, ctx: &egui::Context) {
        for set in &self.light_sets {
            self.scan_light(set.id, set.sources.clone(), Some(ctx));
        }
        for set in &self.calibration_sets {
            for kind in CalibrationKind::ALL {
                if let Some(path) = set.item(kind).and_then(|i| i.first_path()) {
                    let ctx = ctx.clone();
                    self.scanner
                        .item(set.id, kind, path.to_path_buf(), move || {
                            ctx.request_repaint()
                        });
                }
            }
        }
    }

    fn persist_settings(&mut self, ctx: Option<&egui::Context>, force: bool) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        if !force && self.busy() && !self.waiting {
            return;
        }
        let current = self.settings_snapshot();
        if self.settings_last.as_ref() == Some(&current) && !(force && self.settings_save_failed) {
            self.settings_pending = None;
            return;
        }
        let changed_at = match &self.settings_pending {
            Some((previous, at)) if previous == &current => *at,
            _ => {
                let now = Instant::now();
                self.settings_pending = Some((current.clone(), now));
                now
            }
        };
        let delay = Duration::from_millis(600);
        if !force && changed_at.elapsed() < delay {
            if let Some(ctx) = ctx {
                ctx.request_repaint_after(delay.saturating_sub(changed_at.elapsed()));
            }
            return;
        }
        let result = settings::save(&path, &current);
        self.settings_last = Some(current);
        self.settings_pending = None;
        self.settings_save_failed = result.is_err();
        self.settings_notice = result.err().map(|_| {
            "Settings could not be saved. Check access to the local application data folder.".into()
        });
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    }
    fn continue_run(&mut self) {
        let strategy = if self.reference_mode == 3 {
            fulgur_pipeline::register::ReferenceStrategy::ChronologicalMedian
        } else {
            fulgur_pipeline::register::ReferenceStrategy::Auto
        };
        if let Some(request) = &mut self.active_request {
            request.reference =
                matches!(self.reference_mode, 1 | 2).then(|| PathBuf::from(&self.reference));
            request.reference_strategy = strategy;
        }
        self.persist_settings(None, true);
        self.preview_task = None;
        self.preview_view.stop_loading();
        self.result_view.stop_loading();
        if let Some(worker) = &self.worker {
            let reference = if matches!(self.reference_mode, 0 | 3) {
                None
            } else {
                Some(PathBuf::from(&self.reference))
            };
            if worker
                .decision
                .try_send(job::ReviewDecision::ContinueWithSelection {
                    reference_strategy: strategy,
                    reference,
                    selection: self.frame_selection(),
                })
                .is_ok()
            {
                self.finish_review_wait();
                self.waiting = false;
                self.view = View::Processing;
                self.message = "Continuing with registration…".into();
            }
        }
    }
    fn finish_review_wait(&mut self) {
        if let Some(start) = self.review_started.take() {
            self.review_wait += start.elapsed();
        }
    }
    fn active_elapsed(&self) -> Duration {
        if let Some(elapsed) = self.finished_elapsed {
            return elapsed;
        }
        self.started
            .map(|start| {
                start
                    .elapsed()
                    .saturating_sub(self.review_wait)
                    .saturating_sub(self.review_started.map(|s| s.elapsed()).unwrap_or_default())
            })
            .unwrap_or_default()
    }

    fn remaining_text(&self, state: Option<&job::Progress>) -> String {
        if self.waiting {
            "Paused for quality review".into()
        } else if self.run_complete {
            clock(0)
        } else if !self.busy() {
            "Not running".into()
        } else if self
            .worker
            .as_ref()
            .is_some_and(|w| w.cancel.load(Ordering::Relaxed))
        {
            "Cancelling…".into()
        } else if let Some(seconds) = state.and_then(remaining_seconds) {
            format!("~{}", clock(seconds.ceil() as u64))
        } else {
            "Calculating…".into()
        }
    }
    fn cancel_run(&mut self) {
        self.finish_review_wait();
        if let Some(worker) = &self.worker {
            worker.cancel.store(true, Ordering::Relaxed);
        }
        self.waiting = false;
        self.message = "Cancelling… waiting for workers and output cleanup.".into();
    }
    pub fn enable_smoke(&mut self, folder: PathBuf, reference: PathBuf) {
        if !folder.is_dir()
            || [
                "quality.png",
                "master.png",
                "coverage.png",
                "high-rejections.png",
                "native_smoke.json",
                "waiting.png",
            ]
            .iter()
            .any(|name| folder.join(name).exists())
        {
            self.message =
                "UI smoke capture requires an existing folder with unused output names.".into();
            return;
        }
        self.folder = folder.to_string_lossy().into_owned();
        self.filename = "native_master.fits".into();
        self.reference_mode = 2;
        self.reference = reference.to_string_lossy().into_owned();
        self.review = true;
        self.save_coverage = true;
        self.save_high_rejection = true;
        self.smoke = Some(NativeSmoke {
            folder,
            stage: 0,
            pending: None,
            started: Instant::now(),
            captures: vec![],
            last_trace: String::new(),
            quick: false,
        });
    }
    /// Explicit local QA mode for the normal no-review flow.
    pub fn enable_quick_smoke(&mut self, folder: PathBuf, reference: PathBuf) {
        self.enable_smoke(folder, reference);
        self.review = false;
        if let Some(smoke) = &mut self.smoke {
            smoke.quick = true;
        }
    }
    fn smoke_tick(&mut self, ctx: &egui::Context) {
        let Some(mut smoke) = self.smoke.take() else {
            return;
        };
        let captured = ctx.input(|i| {
            i.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = captured {
            if let Some(name) = smoke.pending.take() {
                let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(error) = image::save_buffer(
                    smoke.folder.join(&name),
                    &bytes,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    self.message = format!("UI capture failed: {error}");
                    smoke.stage = 99;
                } else {
                    smoke.captures.push(name);
                }
            }
        }
        let mut capture = None;
        let review_captures = if smoke.quick { 0 } else { 2 };
        match smoke.stage {
            0 if self.blocker().is_none() => {
                self.start(false);
                smoke.stage = 1;
            }
            1 if self.waiting => {
                if let Some(index) = self.rows.iter().position(|r| r.accepted) {
                    self.select(index);
                }
                self.view = View::Frames;
                smoke.stage = 2;
            }
            1 if smoke.quick && self.result.is_some() && !self.busy() => {
                smoke.stage = 4;
            }
            2 if self.preview.is_some() => {
                capture = Some("quality.png");
                smoke.stage = 3;
            }
            3 if smoke.captures.len() == 1 => {
                self.view = View::Processing;
                capture = Some("waiting.png");
                smoke.stage = 8;
            }
            8 if smoke.captures.len() == 2 => {
                self.continue_run();
                smoke.stage = 4;
            }
            4 if self.result.is_some() && !self.busy() => {
                self.view = View::Results;
                self.layer = 0;
                self.result_texture = None;
                capture = Some("master.png");
                smoke.stage = 5;
            }
            5 if smoke.captures.len() == review_captures + 1 => {
                self.layer = 1;
                self.result_texture = None;
                capture = Some("coverage.png");
                smoke.stage = 6;
            }
            6 if smoke.captures.len() == review_captures + 2 => {
                self.layer = 2;
                self.result_texture = None;
                capture = Some("high-rejections.png");
                smoke.stage = 7;
            }
            7 if smoke.captures.len() == review_captures + 3 => {
                smoke.stage = 100;
            }
            _ => {}
        }
        if let Some(name) = capture {
            smoke.pending = Some(name.into());
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let phase = self
            .worker
            .as_ref()
            .and_then(|w| {
                w.progress
                    .lock()
                    .ok()
                    .map(|p| format!("{} {}/{}", p.phase, p.completed, p.total))
            })
            .unwrap_or_else(|| "idle".into());
        let trace = format!(
            "stage={} captures={} waiting={} busy={} phase={phase}",
            smoke.stage,
            smoke.captures.len(),
            self.waiting,
            self.busy()
        );
        if trace != smoke.last_trace {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(smoke.folder.join("native_trace.jsonl"))
            {
                let _ = writeln!(
                    f,
                    "{}",
                    serde_json::json!({"elapsed_s":smoke.started.elapsed().as_secs_f64(),"state":trace})
                );
            }
            smoke.last_trace = trace;
        }
        if smoke.started.elapsed() > Duration::from_secs(60)
            || self.message.starts_with("Unable to finish")
        {
            smoke.stage = 99;
        }
        if smoke.stage >= 99 {
            let report = serde_json::json!({"passed":smoke.stage==100,"captures":smoke.captures,"message":self.message,"measured_rows":self.rows.len(),"quality_scroll":"virtualized continuous; no pagination","workflow":"Frames, Settings, Processing, Results","subexposure_preview":"right panel; one immutable source frame","source_preview_max_edge":crate::preview::PREVIEW_EDGE});
            let path = smoke.folder.join("native_smoke.json");
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let _ = fulgur_core::output::atomic_write(&path, &cancel, |w| {
                w.write_all(report.to_string().as_bytes())?;
                Ok(())
            });
            self.exit_after_cancel = true;
            if self.busy() {
                self.cancel_run();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else {
            self.smoke = Some(smoke);
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    pub fn new(cc: &eframe::CreationContext<'_>, inputs: Vec<PathBuf>) -> Self {
        Self::create(cc, inputs, true)
    }
    /// Explicit QA launches never read or replace the owner's preferences.
    pub fn new_transient(cc: &eframe::CreationContext<'_>, inputs: Vec<PathBuf>) -> Self {
        Self::create(cc, inputs, false)
    }
    fn create(cc: &eframe::CreationContext<'_>, inputs: Vec<PathBuf>, persistent: bool) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(7, 21, 35);
        style.visuals.window_fill = Color32::from_rgb(11, 28, 45);
        style.visuals.extreme_bg_color = Color32::from_rgb(4, 13, 23);
        style.visuals.faint_bg_color = Color32::from_rgb(11, 28, 45);
        style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(11, 28, 45);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(18, 39, 59);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(18, 39, 59);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(27, 55, 79);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(27, 55, 79);
        style.animation_time = 0.;
        style.spacing.item_spacing = egui::vec2(7., 6.);
        style.spacing.button_padding = egui::vec2(9., 5.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(13.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(13.));
        cc.egui_ctx.set_style(style);
        #[cfg(windows)]
        if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/segoeui.ttf") {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("Segoe UI".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, "Segoe UI".into());
            cc.egui_ctx.set_fonts(fonts);
        }
        let mut app = Self::default();
        if persistent {
            app.settings_path = settings::path();
            if let Some(path) = &app.settings_path {
                match settings::load(path) {
                    Ok(Some(saved)) => app.restore_settings(saved),
                    Ok(None) => {},
                    Err(_) => app.settings_notice = Some("Saved settings could not be restored. Defaults are in use; changes will be saved normally.".into()),
                }
            } else {
                app.settings_notice = Some(
                    "Local application data is unavailable; settings cannot be remembered.".into(),
                );
            }
            // A broken/future settings file is not replaced just by opening
            // and closing the app; only an actual configuration change saves it.
            app.settings_last = Some(app.settings_snapshot());
        }
        if !inputs.is_empty() {
            // Command-line inputs replace restored sets: one light set each.
            app.light_sets.clear();
            for input in inputs {
                let path = settings::absolute(&input);
                app.push_light_set(default_set_name(&path), vec![path]);
            }
        }
        app.rescan_all(&cc.egui_ctx);
        app
    }
    fn busy(&self) -> bool {
        self.worker.is_some()
    }
    fn measured(&self) -> bool {
        !self.rows.is_empty()
    }

    fn add_light_sources(&mut self, paths: Vec<PathBuf>, ctx: Option<&egui::Context>) {
        // Each folder becomes its own set; loose files dropped together form one set.
        let (folders, files): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| p.is_dir());
        let mut last = None;
        for folder in folders {
            last = Some(self.add_light_set(default_set_name(&folder), vec![folder], ctx));
        }
        if !files.is_empty() {
            let name = files
                .first()
                .and_then(|p| p.parent())
                .map(default_set_name)
                .unwrap_or_else(|| "Files".into());
            last = Some(self.add_light_set(format!("{name} (files)"), files, ctx));
        }
        if let Some(id) = last {
            self.selection = Selection::Light(id);
            self.view = View::Frames;
            self.invalidate_measurements("Light sets changed · measure quality again.");
            self.message = "Light set added · reading FITS headers.".into();
        }
    }
    fn add_light_folder(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Add light-frame folder")
            .pick_folder()
        {
            self.add_light_sources(vec![path], Some(ctx));
        }
    }
    fn add_light_files(&mut self, ctx: &egui::Context) {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Add light frames")
            .add_filter("FITS", &["fit", "fits", "fts"])
            .pick_files()
        {
            self.add_light_sources(paths, Some(ctx));
        }
    }
    /// Remove sets from the workspace only; files stay untouched on disk.
    fn clear_sets(&mut self, kind: ClearKind) {
        if matches!(kind, ClearKind::Lights | ClearKind::All) {
            self.light_sets.clear();
            self.invalidate_measurements("Light sets cleared. Files stay on disk.");
            self.rows.clear();
            self.inspection = None;
            self.inspected_ids.clear();
            self.final_weights = false;
        }
        if matches!(kind, ClearKind::Calibration | ClearKind::All) {
            self.calibration_sets.clear();
            for set in &mut self.light_sets {
                set.calibration = None;
            }
        }
        self.selection = Selection::All;
        self.message = match kind {
            ClearKind::Lights => "Light sets cleared. Files stay on disk.",
            ClearKind::Calibration => "Calibration sets cleared. Files stay on disk.",
            ClearKind::All => "All sets cleared. Files stay on disk.",
        }
        .into();
    }

    /// Registration reference among measured rows: manual choice, the
    /// completed run's anchor, or the predicted chronological median of
    /// Use-on frames (Auto equals it for one night). Bool: manual.
    fn registration_reference(&self) -> Option<(usize, bool, &'static str)> {
        let same = |a: &Path, b: &Path| display_path(a).eq_ignore_ascii_case(&display_path(b));
        match self.reference_mode {
            1 if !self.reference.trim().is_empty() => {
                let chosen = PathBuf::from(&self.reference);
                self.rows
                    .iter()
                    .position(|r| same(&r.path, &chosen))
                    .map(|i| (i, true, "Registration reference · chosen manually"))
            }
            0 | 3 => {
                if let Some(anchor) = self
                    .result
                    .as_ref()
                    .filter(|_| self.final_weights)
                    .map(|r| r.reference_anchor.clone())
                {
                    return self
                        .rows
                        .iter()
                        .position(|r| same(&r.path, &anchor))
                        .map(|i| (i, false, "Registration reference · used by the last stack"));
                }
                let mut accepted: Vec<usize> = (0..self.rows.len())
                    .filter(|&i| self.rows[i].accepted && self.rows[i].date.is_some())
                    .collect();
                accepted.sort_by(|&a, &b| {
                    self.rows[a]
                        .date
                        .cmp(&self.rows[b].date)
                        .then_with(|| self.rows[a].file_name.cmp(&self.rows[b].file_name))
                });
                accepted.get(accepted.len().saturating_sub(1) / 2).map(|&i| {
                    (
                        i,
                        false,
                        "Registration reference · automatic (chronological median of Use-on frames; several nights may choose another frame)",
                    )
                })
            }
            _ => None,
        }
    }

    fn remove_light_set(&mut self, id: u64) {
        self.light_sets.retain(|s| s.id != id);
        self.selection = Selection::All;
        self.invalidate_measurements("Light set removed · measure quality again.");
    }
    fn set_include(&mut self, id: u64, include: bool) {
        if let Some(set) = self.light_mut(id) {
            set.include = include;
        }
        self.invalidate_measurements("Light sets in the stack changed · measure quality again.");
    }
    fn set_layout(&mut self, id: u64, layout: LayoutChoice) {
        if let Some(set) = self.light_mut(id) {
            if set.layout == layout {
                return;
            }
            set.layout = layout;
        }
        self.invalidate_measurements("Sensor layout changed · measure quality again.");
    }

    fn add_calibration_folder(&mut self, ctx: &egui::Context) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Add calibration folder")
            .pick_folder()
        else {
            return;
        };
        let id = self.new_id();
        self.calibration_sets.push(CalibrationSet {
            id,
            name: default_set_name(&folder),
            items: Default::default(),
        });
        self.selection = Selection::Calibration(id);
        self.message = "Sorting calibration files by their FITS headers…".into();
        let ctx = ctx.clone();
        self.scanner
            .calibration(id, folder, move || ctx.request_repaint());
    }
    fn add_empty_calibration(&mut self) {
        let id = self.new_id();
        let n = self.calibration_sets.len() + 1;
        self.calibration_sets.push(CalibrationSet {
            id,
            name: format!("Calibration set {n}"),
            items: Default::default(),
        });
        self.selection = Selection::Calibration(id);
    }
    fn remove_calibration_set(&mut self, id: u64) {
        self.calibration_sets.retain(|c| c.id != id);
        for set in &mut self.light_sets {
            if set.calibration == Some(id) {
                set.calibration = None;
            }
        }
        self.selection = Selection::All;
    }
    fn set_calibration_item(
        &mut self,
        id: u64,
        kind: CalibrationKind,
        source: Option<CalibrationSource>,
        ctx: &egui::Context,
    ) {
        let first = source.as_ref().and_then(|s| match s {
            CalibrationSource::Frames(f) => f.first().cloned(),
            CalibrationSource::Master(p) => Some(p.clone()),
        });
        let seestar = self.light_sets.iter().any(|l| {
            l.calibration == Some(id) && l.summary.as_ref().is_some_and(|s| s.is_seestar())
        });
        if let Some(set) = self.calibration_mut(id) {
            set.items[kind.index()] = source.map(|source| CalibrationItem {
                source,
                // Seestar lights are already bias/dark corrected in the telescope.
                enabled: !(seestar
                    && matches!(kind, CalibrationKind::Bias | CalibrationKind::Dark)),
                summary: None,
            });
        }
        if let Some(path) = first {
            let ctx = ctx.clone();
            self.scanner
                .item(id, kind, path, move || ctx.request_repaint());
        }
    }

    /// Best compatible calibration set for a light set (or none).
    fn best_calibration(&self, light: &LightSet) -> Option<u64> {
        let summary = light.summary.as_ref()?;
        self.calibration_sets
            .iter()
            .filter_map(|c| {
                let (level, _) = sets::compatibility(summary, c);
                (level != Level::Bad && c.items.iter().flatten().next().is_some())
                    .then_some((level, c.id))
            })
            .min_by_key(|(level, _)| *level)
            .map(|(_, id)| id)
    }

    fn poll_scans(&mut self) {
        let results: Vec<_> = self.scanner.receiver.try_iter().collect();
        for result in results {
            match result {
                ScanResult::Light { id, summary } => {
                    let assign = self.light(id).is_some_and(|s| s.calibration.is_none());
                    if let Some(set) = self.light_mut(id) {
                        set.summary = Some(summary);
                    }
                    if assign {
                        let best = self.light(id).and_then(|s| self.best_calibration(s));
                        if let Some(set) = self.light_mut(id) {
                            set.calibration = best;
                        }
                    }
                }
                ScanResult::Calibration { id, scan } => {
                    let found = scan.items.iter().flatten().count();
                    if let Some(set) = self.calibration_mut(id) {
                        set.items = scan.items;
                        // Seestar lights are bias/dark corrected in the telescope.
                        if set.is_seestar() {
                            for kind in [CalibrationKind::Bias, CalibrationKind::Dark] {
                                if let Some(item) = set.items[kind.index()].as_mut() {
                                    item.enabled = false;
                                }
                            }
                        }
                    }
                    self.message = format!(
                        "Calibration folder sorted: {found} kinds found, {} files without a calibration IMAGETYP ignored.",
                        scan.ignored
                    );
                    // Offer the new set to light sets that have none yet.
                    let unassigned: Vec<u64> = self
                        .light_sets
                        .iter()
                        .filter(|l| l.calibration.is_none())
                        .map(|l| l.id)
                        .collect();
                    for light_id in unassigned {
                        let fits = self
                            .light(light_id)
                            .and_then(|l| l.summary.as_ref())
                            .zip(self.calibration(id))
                            .is_some_and(|(s, c)| sets::compatibility(s, c).0 != Level::Bad);
                        if fits {
                            if let Some(l) = self.light_mut(light_id) {
                                l.calibration = Some(id);
                            }
                        }
                    }
                }
                ScanResult::Item { id, kind, summary } => {
                    if let Some(item) = self
                        .calibration_mut(id)
                        .and_then(|s| s.items[kind.index()].as_mut())
                    {
                        item.summary = summary;
                    }
                }
            }
        }
    }

    fn provenance(&self) -> serde_json::Value {
        let sets: Vec<_> = self
            .included_sets()
            .map(|s| {
                let calibration = s.calibration.and_then(|id| self.calibration(id)).map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "items": CalibrationKind::ALL.iter().filter_map(|&k| c.item(k).map(|i| serde_json::json!({
                            "kind": k.key(),
                            "source": if matches!(i.source, CalibrationSource::Master(_)) { "master" } else { "frames" },
                            "files": i.count(),
                            "enabled": i.enabled,
                        }))).collect::<Vec<_>>(),
                    })
                });
                serde_json::json!({
                    "name": s.name,
                    "layout": s.layout.name(),
                    "layout_label": s.layout_label(),
                    "filters": s.summary.as_ref().map(|m| m.filters.clone()).unwrap_or_default(),
                    "exposures_seconds": s.summary.as_ref().map(|m| m.exposures.clone()).unwrap_or_default(),
                    "calibration_set": calibration,
                    "calibration_applied": false,
                })
            })
            .collect();
        serde_json::json!(sets)
    }

    fn request(&self) -> StackRequest {
        let mut config = IntegrationConfig {
            memory_budget_mib: self.memory,
            ..IntegrationConfig::default()
        };
        if self.tile != 0 {
            config.tile_mode = TileMode::Manual;
            config.tile_width = self.tile;
            config.tile_height = self.tile;
        }
        StackRequest {
            sets: self
                .included_sets()
                .map(|s| fulgur_pipeline::inspect::InputSet {
                    label: s.name.clone(),
                    inputs: s.sources.clone(),
                    layout: s.layout,
                })
                .collect(),
            provenance: self.provenance(),
            output: PathBuf::from(&self.folder).join(&self.filename),
            reference: if matches!(self.reference_mode, 0 | 3) {
                None
            } else {
                Some(PathBuf::from(&self.reference))
            },
            gpu: self.gpu,
            reference_strategy: if self.reference_mode == 3 {
                fulgur_pipeline::register::ReferenceStrategy::ChronologicalMedian
            } else {
                fulgur_pipeline::register::ReferenceStrategy::Auto
            },
            cpu_threads: self.cpu_threads,
            review: self.review,
            geometric_correction: self.geometric_correction,
            diagnose_excluded: self.diagnose_excluded,
            save_coverage: self.save_coverage,
            save_high_rejection: self.save_high_rejection,
            config,
        }
    }
    fn start(&mut self, inspect_only: bool) {
        self.menu = None;
        if self.busy() {
            return;
        }
        if let Some(reason) = self.blocker() {
            self.message = reason;
            return;
        }
        let mut request = self.request();
        if !inspect_only && self.folder.trim().is_empty() {
            self.message = "Choose an output folder in Settings before stacking.".into();
            return;
        }
        let output_plan = if inspect_only {
            None
        } else {
            match request.validate() {
                Ok(paths) => {
                    request.output = paths.master.clone();
                    Some(paths)
                }
                Err(error) => {
                    self.message = error.to_string();
                    return;
                }
            }
        };
        self.persist_settings(None, true);
        self.preview_task = None; // Cancels and joins before any integration buffers exist.
        self.preview_view.stop_loading();
        self.result_view.stop_loading();
        self.waiting = false;
        self.started = Some(Instant::now());
        self.review_started = None;
        self.review_wait = Duration::ZERO;
        self.finished_elapsed = None;
        self.run_complete = false;
        self.last_progress = None;
        self.last_run = None;
        self.output_plan = output_plan;
        self.run_kind = Some(if inspect_only {
            RunKind::Measure
        } else {
            RunKind::Stack
        });
        self.message = if inspect_only {
            "Inspecting original frames…"
        } else {
            "Processing immutable source frames…"
        }
        .into();
        if inspect_only {
            self.view = View::Frames;
            self.inspection = None;
            self.manual_selection.clear();
            self.quality_table.reset_rows();
        } else {
            self.view = View::Processing;
        }
        let ids: Vec<u64> = self.included_sets().map(|s| s.id).collect();
        self.active_request = Some(request.clone());
        let cached = (self.inspected_ids == ids)
            .then(|| self.inspection.clone())
            .flatten();
        self.inspected_ids = ids;
        self.worker = Some(Worker::start_with_inspection(
            request,
            inspect_only,
            cached,
            self.frame_selection(),
        ));
    }
    /// Stack, asking first when the included sets mix filters.
    fn request_stack(&mut self) {
        if self.mixed_filters() && self.menu != Some(Menu::ConfirmMixed) {
            self.menu = Some(Menu::ConfirmMixed);
        } else {
            self.start(false);
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        self.poll_scans();
        if let Some(paths) = self
            .worker
            .as_ref()
            .and_then(|w| w.destinations.lock().ok().and_then(|p| p.clone()))
        {
            self.output_plan = Some(paths);
        }
        if let Some(state) = self
            .worker
            .as_ref()
            .and_then(|w| w.progress.lock().ok().map(|p| p.clone()))
        {
            self.last_progress = Some(state);
        }
        let finished = self.worker.as_ref().is_some_and(Worker::finished);
        let events: Vec<_> = self
            .worker
            .as_ref()
            .map(|w| w.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Quality {
                    rows,
                    waiting,
                    inspection,
                    reused,
                } => {
                    let invalidated = self.inspection.is_some() && !reused;
                    self.inspection = Some(inspection);
                    if !reused {
                        self.manual_selection.clear();
                        self.quality_table.reset_rows();
                    }
                    self.rows = rows;
                    self.final_weights = false;
                    self.waiting = waiting;
                    self.selected = self
                        .rows
                        .iter()
                        .position(|r| !self.quality_table.removed.contains(&r.path));
                    self.preview = None;
                    self.preview_texture = None;
                    if invalidated {
                        self.message = "Inputs changed. Quality was measured again and the previous manual selection was cleared.".into();
                    }
                    if waiting {
                        self.review_started = Some(Instant::now());
                        self.view = View::Frames;
                        self.message =
                            "Waiting for your confirmation · inspection complete. Select Continue to begin registration."
                                .into();
                    }
                }
                Event::Complete(mut result) => {
                    self.finish_review_wait();
                    self.finished_elapsed = Some(self.active_elapsed());
                    self.run_complete = true;
                    self.last_run = Some(LastRun::Done);
                    self.result_request = self.active_request.take();
                    self.result_inspection_reused = self
                        .last_progress
                        .as_ref()
                        .is_some_and(|p| p.inspection_reused);
                    self.result_phase_times = self
                        .last_progress
                        .as_ref()
                        .map(job::Progress::phase_times)
                        .unwrap_or_default();
                    self.output_plan = Some(result.paths.clone());
                    self.rows = std::mem::take(&mut result.rows);
                    self.final_weights = true;
                    self.message = format!(
                        "Complete · {}/{} frames · {} s · originals preserved",
                        result.accepted,
                        result.total,
                        whole_seconds(Some(result.wall_seconds))
                    );
                    self.result = Some(result);
                    self.result_texture = None;
                    self.result_view.reset();
                    self.layer = 0;
                    self.view = View::Results;
                    self.waiting = false;
                    ctx.request_repaint();
                }
                Event::Stopped {
                    cancelled,
                    message,
                    published,
                } => {
                    self.finish_review_wait();
                    self.finished_elapsed = Some(self.active_elapsed());
                    self.waiting = false;
                    self.last_run = Some(LastRun::Stopped);
                    self.message = format!(
                        "{}: {}",
                        if cancelled {
                            "Cancelled"
                        } else {
                            "Unable to finish"
                        },
                        message
                    );
                    if !published.is_empty() {
                        self.message.push_str(" Published final files remain: ");
                        self.message.push_str(
                            &published
                                .iter()
                                .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                                .collect::<Vec<_>>()
                                .join(", "),
                        );
                    }
                    ctx.request_repaint();
                }
            }
        }
        if finished {
            if let Some(state) = self
                .worker
                .as_ref()
                .and_then(|w| w.progress.lock().ok().map(|p| p.clone()))
            {
                self.last_progress = Some(state);
            }
            self.finished_elapsed = Some(self.active_elapsed());
            if self.run_complete {
                self.result_inspection_reused = self
                    .last_progress
                    .as_ref()
                    .is_some_and(|p| p.inspection_reused);
                self.result_phase_times = self
                    .last_progress
                    .as_ref()
                    .map(job::Progress::phase_times)
                    .unwrap_or_default();
            }
            self.worker = None;
            if self.run_kind == Some(RunKind::Measure) && self.last_run != Some(LastRun::Stopped) {
                self.last_run = Some(LastRun::Measured);
                self.message = format!(
                    "Inspection complete · {} accepted / {} inputs · provisional weights",
                    self.rows.iter().filter(|f| f.accepted).count(),
                    self.rows.len()
                );
            }
            if self.last_run == Some(LastRun::Stopped) && self.run_kind == Some(RunKind::Measure) {
                self.inspected_ids.clear();
            }
            ctx.request_repaint();
        }
        let ready = self
            .preview_task
            .as_ref()
            .and_then(|t| t.result.try_recv().ok());
        if let Some(result) = ready {
            self.preview_task = None;
            match result {
                Ok(p) => {
                    self.preview = Some(p);
                    self.preview_texture = None;
                    self.preview_view.stop_loading();
                }
                Err(error) => self.message = format!("Preview: {error}"),
            }
        }
        let reference_ready = self
            .reference_task
            .as_ref()
            .and_then(|t| t.result.try_recv().ok().map(|r| (t.path.clone(), r)));
        if let Some((path, result)) = reference_ready {
            self.reference_task = None;
            match result {
                Ok(p) => {
                    self.reference_preview = Some((path, p));
                    self.reference_texture = None;
                }
                Err(error) => {
                    self.blink = false;
                    self.message = format!("Reference preview: {error}");
                }
            }
        }
        if self.exit_after_cancel && !self.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if (self.busy() && !self.waiting)
            || self.preview_task.is_some()
            || self.reference_task.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
    fn select(&mut self, index: usize) {
        if self.selected == Some(index) && self.preview.is_some() {
            return;
        }
        self.selected = Some(index);
        // Zoom and pan carry over to the next frame of the same size.
        self.preview_view.stop_loading();
        self.preview = None;
        self.preview_texture = None;
        self.preview_task = None;
        if !self.busy() || self.waiting {
            self.preview_task = Some(PreviewTask::start(
                self.rows[index].path.clone(),
                self.layout_of_row(index),
            ));
        }
    }
    fn source_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("sub_preview")
            .default_width(250.)
            .min_width(210.)
            .max_width(380.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(RichText::new("SELECTED FRAME").small().weak());
                let Some(index) = self.selected.filter(|&i| i < self.rows.len()) else {
                    ui.label("Select a measured frame in the list.");
                    return;
                };
                let row = &self.rows[index];
                ui.add(egui::Label::new(RichText::new(&row.file_name).strong()).wrap());
                ui.label(
                    RichText::new(format!(
                        "{} · {}",
                        timestamp_seconds(row.date.as_deref()),
                        row.set_name
                    ))
                    .weak(),
                );
                ui.label(format!(
                    "{} s · {} stars · HFR {} px",
                    whole_seconds(row.exposure),
                    row.stars,
                    number(row.hfr)
                ));
                ui.label(if row.accepted { "Accepted" } else { "Rejected" });
                if !row.reasons.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&row.reasons).small()).wrap());
                }
                if ui
                    .add_enabled(
                        row.accepted && (!self.busy() || self.waiting),
                        egui::Button::new("Use as reference"),
                    )
                    .clicked()
                {
                    self.reference_mode = 1;
                    self.reference = row.path.to_string_lossy().into_owned();
                }
                let reference = self.registration_reference();
                if let Some((_, manual, text)) = reference.filter(|(i, _, _)| *i == index) {
                    ui.label(
                        RichText::new(text)
                            .small()
                            .color(if manual { REFERENCE_MANUAL_TEXT } else { REFERENCE_AUTO_TEXT }),
                    );
                }
                ui.separator();
                if ui
                    .checkbox(&mut self.stretch, "Auto stretch · view only")
                    .changed()
                {
                    self.preview_texture = None;
                    self.reference_texture = None;
                }
                let reference_index = reference.map(|(i, _, _)| i).filter(|&i| i != index);
                let blink_response = ui
                    .add_enabled(
                        reference_index.is_some() && (!self.busy() || self.waiting),
                        egui::Checkbox::new(&mut self.blink, "Blink against reference · 1 s"),
                    )
                    .on_hover_text("Alternates this frame and the registration reference every second, with the same zoom and position. Frames are shown as recorded, before registration.")
                    .on_disabled_hover_text("Available when a registration reference other than this frame is known.");
                if blink_response.changed() && self.blink {
                    self.blink_epoch = Instant::now();
                }
                // Keep the reference preview loaded for the current reference.
                let blinking = self.blink && reference_index.is_some() && (!self.busy() || self.waiting);
                if let Some(ri) = reference_index.filter(|_| blinking) {
                    let path = self.rows[ri].path.clone();
                    let loaded = self.reference_preview.as_ref().is_some_and(|(p, _)| *p == path);
                    let loading = self.reference_task.as_ref().is_some_and(|t| t.path == path);
                    if !loaded && !loading {
                        self.reference_preview = None;
                        self.reference_texture = None;
                        self.reference_task = Some(PreviewTask::start(path, self.layout_of_row(ri)));
                    }
                }
                let show_reference = blinking
                    && self.reference_preview.is_some()
                    && (self.blink_epoch.elapsed().as_millis() / 1000) % 2 == 1;
                if blinking {
                    let until_toggle = 1000 - (self.blink_epoch.elapsed().as_millis() % 1000) as u64;
                    ctx.request_repaint_after(Duration::from_millis(until_toggle.max(16)));
                    ui.label(
                        RichText::new(if self.reference_preview.is_none() {
                            "Loading reference…"
                        } else if show_reference {
                            "Showing: REFERENCE"
                        } else {
                            "Showing: this frame"
                        })
                        .strong()
                        .color(if show_reference { REFERENCE_MANUAL_TEXT } else { Color32::from_rgb(124, 196, 228) }),
                    );
                }
                if self.preview_task.is_some() {
                    ui.spinner();
                    ui.label("Reading one immutable source…");
                }
                if show_reference {
                    let ri = reference_index.unwrap();
                    let (path, preview) = self.reference_preview.as_ref().unwrap();
                    if self.reference_texture.is_none() {
                        self.reference_texture = Some(ctx.load_texture(
                            "reference_preview",
                            preview.display(self.stretch, None),
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    let texture = self.reference_texture.clone().unwrap();
                    let detail = DetailSource::Cfa(path.clone(), self.layout_of_row(ri));
                    self.reference_view.copy_geometry(&self.preview_view);
                    self.reference_view.show(
                        ui,
                        &texture,
                        preview,
                        (!self.busy() || self.waiting).then_some(detail),
                        DisplayOptions {
                            stretch: self.stretch,
                            ..DisplayOptions::default()
                        },
                    );
                    self.preview_view.copy_geometry(&self.reference_view);
                } else if let Some(preview) = &self.preview {
                    if self.preview_texture.is_none() {
                        self.preview_texture = Some(ctx.load_texture(
                            "source_preview",
                            preview.display(self.stretch, None),
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    if let Some(texture) = &self.preview_texture {
                        let layout = self.layout_of_row(index);
                        self.preview_view.show(
                            ui,
                            texture,
                            preview,
                            (!self.busy() || self.waiting)
                                .then(|| DetailSource::Cfa(self.rows[index].path.clone(), layout)),
                            DisplayOptions {
                                stretch: self.stretch,
                                ..DisplayOptions::default()
                            },
                        );
                    }
                } else if self.busy() && !self.waiting {
                    ui.label(
                        "Source loading is deferred while stacking to preserve the memory ceiling.",
                    );
                } else if self.preview_task.is_none() && ui.button("Load selected frame").clicked()
                {
                    self.preview_task = Some(PreviewTask::start(
                        self.rows[index].path.clone(),
                        self.layout_of_row(index),
                    ));
                }
            });
    }
    fn chart(&self, ui: &mut egui::Ui) {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.), egui::Sense::hover());
        let painter = ui.painter();
        let plot = rect.shrink2(egui::vec2(36., 20.));
        let max = self.rows.iter().filter_map(|r| r.hfr).fold(1., f64::max) * 1.1;
        painter.line_segment(
            [plot.left_bottom(), plot.left_top()],
            egui::Stroke::new(1_f32, ui.visuals().weak_text_color()),
        );
        painter.line_segment(
            [plot.left_bottom(), plot.right_bottom()],
            egui::Stroke::new(1_f32, ui.visuals().weak_text_color()),
        );
        painter.text(
            rect.left_top(),
            egui::Align2::LEFT_TOP,
            "HFR px",
            egui::FontId::proportional(12.),
            ui.visuals().text_color(),
        );
        painter.text(
            rect.center_bottom(),
            egui::Align2::CENTER_BOTTOM,
            "Acquisition order · Accepted / Rejected",
            egui::FontId::proportional(12.),
            ui.visuals().text_color(),
        );
        let mut order: Vec<_> = self.rows.iter().collect();
        order.sort_by(|a, b| {
            a.date
                .cmp(&b.date)
                .then_with(|| a.file_name.cmp(&b.file_name))
        });
        for (i, row) in order.iter().enumerate() {
            if let Some(hfr) = row.hfr {
                let point = egui::pos2(
                    plot.left()
                        + i as f32 / (order.len().saturating_sub(1).max(1)) as f32 * plot.width(),
                    plot.bottom() - hfr as f32 / max as f32 * plot.height(),
                );
                let color = if row.accepted {
                    Color32::from_rgb(65, 170, 153)
                } else {
                    Color32::from_rgb(221, 116, 100)
                };
                if row.accepted {
                    painter.circle_filled(point, 2., color);
                } else {
                    painter.line_segment(
                        [point - egui::vec2(2., 2.), point + egui::vec2(2., 2.)],
                        egui::Stroke::new(1.5_f32, color),
                    );
                    painter.line_segment(
                        [point + egui::vec2(-2., 2.), point + egui::vec2(2., -2.)],
                        egui::Stroke::new(1.5_f32, color),
                    );
                }
            }
        }
        painter.text(
            plot.left_top(),
            egui::Align2::RIGHT_TOP,
            format!("{max:.1} "),
            egui::FontId::proportional(11.),
            ui.visuals().text_color(),
        );
    }
}

/// Registration reference highlight: wine for automatic, lighter red for manual.
const REFERENCE_AUTO_FILL: Color32 = Color32::from_rgb(94, 22, 40);
const REFERENCE_MANUAL_FILL: Color32 = Color32::from_rgb(170, 52, 64);
const REFERENCE_AUTO_TEXT: Color32 = Color32::from_rgb(214, 120, 140);
const REFERENCE_MANUAL_TEXT: Color32 = Color32::from_rgb(255, 128, 128);

fn default_set_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn clock(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn phase_clock(duration: Duration) -> String {
    clock(duration.as_secs())
}

fn whole_seconds(value: Option<f64>) -> String {
    value
        .filter(|v| v.is_finite())
        .map(|v| format!("{:.0}", v.trunc()))
        .unwrap_or_else(|| "—".into())
}

/// Omit fractional seconds for display, retaining the date and timezone suffix.
fn timestamp_seconds(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "—".into();
    };
    let Some((whole, fraction)) = value.split_once('.') else {
        return value.into();
    };
    let time = whole
        .as_bytes()
        .get(whole.len().saturating_sub(8)..)
        .unwrap_or_default();
    let clock = time.len() == 8
        && time[2] == b':'
        && time[5] == b':'
        && time
            .iter()
            .enumerate()
            .all(|(i, b)| i == 2 || i == 5 || b.is_ascii_digit());
    let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
    if clock && digits > 0 {
        format!("{whole}{}", &fraction[digits..])
    } else {
        value.into()
    }
}

/// UI-only prediction. Early inspection/registration cannot predict future tile work.
fn remaining_seconds(state: &job::Progress) -> Option<f64> {
    let started = state.phase_started?;
    let phase_elapsed = started.elapsed().as_secs_f64();
    let sampled = state.updated_at?.duration_since(started).as_secs_f64();
    let saving_allowance = |integration: f64| (integration * 0.05).max(2.);
    let estimate = match state.phase {
        "Inspect" if state.total > 0 && state.completed > 0 && sampled >= 1. => {
            (sampled * state.total as f64 / state.completed as f64 - phase_elapsed).max(0.)
        }
        "Integrate" if state.total > 0 && state.completed > 0 && sampled >= 1. => {
            let average = sampled / state.completed as f64;
            let total = average * state.total as f64;
            // A slow next tile may exceed the current estimate; keep a live lower
            // bound instead of announcing zero while work remains.
            let unfinished = if state.completed < state.total {
                1.
            } else {
                0.
            };
            (total - phase_elapsed).max(unfinished) + saving_allowance(total)
        }
        "Write outputs" => {
            if state.total > 0 && state.completed > 0 && sampled > 0. {
                (sampled * state.total as f64 / state.completed as f64 - phase_elapsed).max(1.)
            } else {
                (saving_allowance(state.integration_seconds?) - phase_elapsed).max(1.)
            }
        }
        "Complete" => 0.,
        _ => return None,
    };
    estimate.is_finite().then_some(estimate.max(0.))
}

fn number(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.2}"))
        .unwrap_or_else(|| "—".into())
}

/// Readable Windows path without the extended-length `\\?\` prefix.
fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = text.strip_prefix("\\\\?\\") {
        rest.to_owned()
    } else {
        text.into_owned()
    }
}

fn open(path: &std::path::Path) {
    if !path.is_file() {
        return;
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}
fn reveal(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
}
impl eframe::App for Workspace {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.draw(ctx);
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.persist_settings(None, true);
    }
}
impl Workspace {
    fn draw(&mut self, ctx: &egui::Context) {
        self.poll(ctx);
        self.smoke_tick(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        let dropped: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        let workspace_available = !self.confirm_close;
        if workspace_available && !self.busy() && !dropped.is_empty() {
            self.add_light_sources(dropped, Some(ctx));
        }
        if workspace_available
            && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O))
            && !self.busy()
        {
            self.add_light_files(ctx);
        }
        if workspace_available && ctx.input(|i| i.key_pressed(egui::Key::F5)) && self.waiting {
            self.continue_run();
        } else if workspace_available
            && ctx.input(|i| i.key_pressed(egui::Key::F5))
            && !self.busy()
            && self.blocker().is_none()
        {
            if self.measured() {
                self.request_stack();
            } else {
                self.start(true);
            }
        }
        self.top_bar(ctx);
        self.status_bar(ctx);
        let preview = self.view == View::Frames
            && self.show_preview
            && self.measured()
            && !matches!(self.selection, Selection::Calibration(_));
        if self.view == View::Frames
            && (!self.light_sets.is_empty() || !self.calibration_sets.is_empty())
        {
            self.sets_rail(ctx);
        }
        if preview {
            self.source_panel(ctx);
        }
        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Frames => self.frames_view(ui, ctx),
            View::Settings => self.settings_view(ui),
            View::Processing => self.processing_view(ui),
            View::Results => self.results_view(ui, ctx),
        });
        if let Some(kind) = self.pending_clear {
            egui::Modal::new(egui::Id::new("confirm_clear")).show(ctx, |ui| {
                ui.heading(match kind {
                    ClearKind::Lights => "Remove all light sets?",
                    ClearKind::Calibration => "Remove all calibration sets?",
                    ClearKind::All => "Remove all light and calibration sets?",
                });
                ui.label("They are removed from this workspace only. No file on disk is changed or deleted.");
                if matches!(kind, ClearKind::Lights | ClearKind::All) && self.measured() {
                    ui.label(RichText::new("Quality measurements and the frame list are cleared too.").color(Color32::from_rgb(232, 162, 106)));
                }
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.pending_clear = None;
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("Remove").color(Color32::WHITE)).fill(Color32::from_rgb(156, 43, 54)))
                        .clicked()
                    {
                        self.pending_clear = None;
                        self.clear_sets(kind);
                    }
                });
            });
        }
        if self.confirm_close {
            egui::Modal::new(egui::Id::new("confirm_exit")).show(ctx, |ui| {
                ui.heading("Cancel the active run and exit?");
                ui.label("Originals stay unchanged. Already published final files remain.");
                ui.horizontal(|ui| {
                    if ui.button("Keep working").clicked() {
                        self.confirm_close = false;
                    }
                    if ui.button("Cancel run and exit").clicked() {
                        self.preview_task = None;
                        self.cancel_run();
                        self.exit_after_cancel = true;
                        self.confirm_close = false;
                    }
                });
            });
        }
        self.persist_settings(Some(ctx), false);
    }
}

#[cfg(test)]
mod tests;
