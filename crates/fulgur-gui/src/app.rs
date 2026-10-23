//! Workspace: navigation and settings on the left, source preview on the right.
use crate::{
    job::{self, Completed, Event, QualityRow, StackRequest, Worker},
    preview::{PreviewTask, Thumbnail},
    settings::{self, AppSettings},
};
use eframe::egui::{self, Color32, RichText, TextureHandle};
use fulgur_pipeline::integrate::{IntegrationConfig, TileMode};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Frames,
    Quality,
    Processing,
    Results,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Accepted,
    Rejected,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Name,
    Exposure,
    Hfr,
    Stars,
    Eccentricity,
    Weight,
}

pub struct Workspace {
    inputs: Vec<PathBuf>,
    view: View,
    rows: Vec<QualityRow>,
    inspection: Option<Arc<job::InspectionSession>>,
    manual_selection: BTreeMap<PathBuf, bool>,
    final_weights: bool,
    selected: Option<usize>,
    filter: Filter,
    sort: Sort,
    descending: bool,
    folder: String,
    filename: String,
    reference_mode: usize,
    reference: String,
    review: bool,
    save_coverage: bool,
    save_high_rejection: bool,
    gpu: bool,
    tile: usize,
    memory: usize,
    worker: Option<Worker>,
    preview_task: Option<PreviewTask>,
    preview: Option<Thumbnail>,
    preview_texture: Option<TextureHandle>,
    result_texture: Option<TextureHandle>,
    stretch: bool,
    result_stretch: bool,
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
    processing_open: bool,
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
            inputs: vec![],
            view: View::Frames,
            rows: vec![],
            inspection: None,
            manual_selection: BTreeMap::new(),
            final_weights: false,
            selected: None,
            filter: Filter::All,
            sort: Sort::Name,
            descending: false,
            folder: std::env::current_dir()
                .unwrap_or_default()
                .join("outputs")
                .to_string_lossy()
                .into_owned(),
            filename: "master.fits".into(),
            reference_mode: 0,
            reference: String::new(),
            review: false,
            save_coverage: false,
            save_high_rejection: false,
            gpu: false,
            tile: 0,
            memory: 12288,
            worker: None,
            preview_task: None,
            preview: None,
            preview_texture: None,
            result_texture: None,
            stretch: true,
            result_stretch: true,
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
            processing_open: false,
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
impl Workspace {
    fn frame_selection(&self) -> Vec<job::FrameSelection> {
        self.manual_selection
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
        AppSettings {
            inputs: self.inputs.iter().map(|p| settings::absolute(p)).collect(),
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
            review_quality: self.review,
            gpu: self.gpu,
            tile_size: self.tile,
            memory_budget_mib: self.memory,
            ..AppSettings::default()
        }
    }
    fn restore_settings(&mut self, saved: AppSettings) {
        self.inputs = saved.inputs;
        if !saved.output_folder.trim().is_empty() {
            self.folder = saved.output_folder;
        }
        if job::valid_filename(&saved.master_filename) {
            self.filename = saved.master_filename;
        }
        self.reference_mode = if saved.reference_mode <= 2 {
            saved.reference_mode
        } else {
            0
        };
        self.reference = saved.reference_path;
        self.save_coverage = saved.save_coverage;
        self.save_high_rejection = saved.save_high_rejection;
        self.review = saved.review_quality;
        self.gpu = saved.gpu;
        self.tile = if [0, 128, 256, 512].contains(&saved.tile_size) {
            saved.tile_size
        } else {
            0
        };
        self.memory = saved.memory_budget_mib.clamp(1024, 12288);
        self.message = "Last settings restored. Inspect quality or stack to continue.".into();
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
        self.persist_settings(None, true);
        self.preview_task = None;
        if let Some(worker) = &self.worker {
            let reference = if self.reference_mode == 0 {
                None
            } else {
                Some(PathBuf::from(&self.reference))
            };
            if worker
                .decision
                .try_send(job::ReviewDecision::ContinueWithSelection {
                    reference,
                    selection: self.frame_selection(),
                })
                .is_ok()
            {
                self.finish_review_wait();
                self.waiting = false;
                self.view = View::Quality;
                self.processing_open = true;
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

    fn processing_timing(&self, ui: &mut egui::Ui, state: Option<&job::Progress>) {
        let elapsed = if self.started.is_some() {
            clock(self.active_elapsed().as_secs())
        } else {
            "—".into()
        };
        let remaining = if self.waiting {
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
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Elapsed: {elapsed}")).strong());
            ui.separator();
            ui.label(format!("Estimated remaining: {remaining}"))
                .on_hover_text("Approximate time until the stack and output files are ready. Uses measured integration speed and refines during saving. Initial saving allowance: 5% of integration time, at least 2 seconds. Quality review wait is excluded; this is not a guaranteed completion time.");
        });
        ui.add_space(8.);
    }
    fn cancel_run(&mut self) {
        self.finish_review_wait();
        if let Some(worker) = &self.worker {
            worker.cancel.store(true, Ordering::Relaxed);
        }
        self.waiting = false;
        self.processing_open = true;
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
            0 => {
                self.start(false);
                smoke.stage = 1;
            }
            1 if self.waiting => {
                if let Some(index) = self.rows.iter().position(|r| r.accepted) {
                    self.select(index);
                }
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
            let report = serde_json::json!({"passed":smoke.stage==100,"captures":smoke.captures,"message":self.message,"measured_rows":self.rows.len(),"quality_scroll":"virtualized continuous; no pagination","settings_location":"left navigation panel","subexposure_preview":"right panel; one immutable VNG frame","source_preview_max_edge":crate::preview::PREVIEW_EDGE});
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
        style.visuals.window_fill = Color32::from_rgb(7, 21, 35);
        style.visuals.extreme_bg_color = Color32::from_rgb(4, 13, 23);
        style.visuals.faint_bg_color = Color32::from_rgb(11, 28, 45);
        style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(11, 28, 45);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(18, 39, 59);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(18, 39, 59);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(27, 55, 79);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(27, 55, 79);
        style.animation_time = 0.;
        style.spacing.item_spacing = egui::vec2(9., 9.);
        style.spacing.button_padding = egui::vec2(12., 8.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
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
            app.inputs = inputs.into_iter().map(|p| settings::absolute(&p)).collect();
        }
        app
    }
    fn busy(&self) -> bool {
        self.worker.is_some()
    }
    fn add_inputs(&mut self, paths: Vec<PathBuf>) {
        self.preview_task = None;
        let previous_count = self.inputs.len();
        for path in paths {
            if !self.inputs.contains(&path) {
                self.inputs.push(path);
            }
        }
        if self.inputs.len() == previous_count {
            return;
        }
        self.rows.clear();
        self.inspection = None;
        self.manual_selection.clear();
        self.final_weights = false;
        self.selected = None;
        self.preview = None;
        self.preview_texture = None;
        self.message = "Inputs changed. Inspect or stack to measure quality.".into();
    }
    fn add_folder(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Add light-frame folder")
            .pick_folder()
        {
            self.add_inputs(vec![path]);
        }
    }
    fn add_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Add light frames")
            .add_filter("FITS", &["fit", "fits", "fts"])
            .pick_files()
        {
            self.add_inputs(paths);
        }
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
            inputs: self.inputs.clone(),
            output: PathBuf::from(&self.folder).join(&self.filename),
            reference: if self.reference_mode == 0 {
                None
            } else {
                Some(PathBuf::from(&self.reference))
            },
            gpu: self.gpu,
            review: self.review,
            save_coverage: self.save_coverage,
            save_high_rejection: self.save_high_rejection,
            config,
        }
    }
    fn start(&mut self, inspect_only: bool) {
        if self.busy() {
            return;
        }
        let mut request = self.request();
        if !inspect_only && self.folder.trim().is_empty() {
            self.message = "Choose an output folder before stacking.".into();
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
        self.waiting = false;
        self.started = Some(Instant::now());
        self.review_started = None;
        self.review_wait = Duration::ZERO;
        self.finished_elapsed = None;
        self.run_complete = false;
        self.last_progress = None;
        self.output_plan = output_plan;
        self.processing_open = true;
        self.message = if inspect_only {
            "Inspecting original CFA frames…"
        } else {
            "Processing immutable source frames…"
        }
        .into();
        self.view = View::Quality;
        if inspect_only {
            self.inspection = None;
            self.manual_selection.clear();
        }
        self.worker = Some(Worker::start_with_inspection(
            request,
            inspect_only,
            self.inspection.clone(),
            self.frame_selection(),
        ));
    }
    fn poll(&mut self, ctx: &egui::Context) {
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
                    }
                    self.rows = rows;
                    self.final_weights = false;
                    self.waiting = waiting;
                    self.selected = (!self.rows.is_empty()).then_some(0);
                    self.preview = None;
                    self.preview_texture = None;
                    if invalidated {
                        self.message = "Inputs changed. Quality was measured again and the previous manual selection was cleared.".into();
                    }
                    if waiting {
                        self.processing_open = false;
                        self.review_started = Some(Instant::now());
                        self.view = View::Quality;
                        self.message =
                            "Waiting for your confirmation · inspection complete. Select Continue stacking to begin registration."
                                .into();
                    }
                }
                Event::Complete(mut result) => {
                    self.finish_review_wait();
                    self.finished_elapsed = Some(self.active_elapsed());
                    self.run_complete = true;
                    self.output_plan = Some(result.paths.clone());
                    self.processing_open = false;
                    self.rows = std::mem::take(&mut result.rows);
                    self.final_weights = true;
                    self.message = format!(
                        "Complete · {}/{} frames · {:.3} s · originals preserved",
                        result.accepted, result.total, result.wall_seconds
                    );
                    self.result = Some(result);
                    self.result_texture = None;
                    self.layer = 0;
                    self.view = View::Results;
                    self.waiting = false;
                    // Remove the modal/backdrop and lay out Results immediately,
                    // even if the worker has already ended and no heartbeat remains.
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
            self.worker = None;
            if self.message.starts_with("Inspecting") {
                self.processing_open = false;
                self.view = View::Quality;
                self.message = format!(
                    "Inspection complete · {} accepted / {} inputs · provisional weights",
                    self.rows.iter().filter(|f| f.accepted).count(),
                    self.rows.len()
                );
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
                }
                Err(error) => self.message = format!("Preview: {error}"),
            }
        }
        if self.exit_after_cancel && !self.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if (self.busy() && !self.waiting) || self.preview_task.is_some() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
    fn select(&mut self, index: usize) {
        if self.selected == Some(index) && self.preview.is_some() {
            return;
        }
        self.selected = Some(index);
        self.preview = None;
        self.preview_texture = None;
        self.preview_task = None;
        if !self.busy() || self.waiting {
            self.preview_task = Some(PreviewTask::start(self.rows[index].path.clone()));
        }
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation_settings").default_width(270.).min_width(235.).max_width(380.).resizable(true).show(ctx,|ui| {
            ui.heading("FulgurStack");ui.label(RichText::new("Local astrophotography stacking").small().weak());ui.separator();
            for (view,label) in [(View::Frames,"Frames"),(View::Quality,"Quality"),(View::Processing,"Processing"),(View::Results,"Results")] {if ui.selectable_label(if view==View::Processing{self.processing_open}else{self.view==view},label).clicked(){if view==View::Processing{self.processing_open=true;}else{self.view=view;}}}
            ui.separator();
            ui.add_enabled_ui(!self.busy(),|ui|{
                ui.checkbox(&mut self.review,"Review quality before stacking").on_hover_text("Pause after inspection for your approval. Leave off to stack without stopping.");
            });
            if self.waiting {
                ui.label(RichText::new("Waiting for your confirmation").strong());
                if ui.add_sized([ui.available_width(),38.],egui::Button::new("Continue stacking").fill(ui.visuals().selection.bg_fill)).clicked(){
                    self.continue_run();
                }
                if ui.button("STOP").clicked(){self.cancel_run();}
            } else if self.busy() {
                if ui.add_sized([ui.available_width(),38.],egui::Button::new("STOP")).clicked(){self.cancel_run();}
            } else if ui.add_enabled(!self.inputs.is_empty(),egui::Button::new("Stack images").min_size(egui::vec2(ui.available_width(),38.))).clicked(){self.start(false);}
            ui.separator();
            egui::ScrollArea::vertical().id_salt("settings_scroll").show(ui,|ui|{
                ui.heading("Stack settings");
                let mutable=!self.busy();
                ui.add_enabled_ui(mutable,|ui| {
                    ui.label("Processing engine");
                    egui::ComboBox::from_id_salt("engine").selected_text(if self.gpu{"GPU"}else{"CPU · default"}).width(ui.available_width()-12.).show_ui(ui,|ui|{ui.selectable_value(&mut self.gpu,false,"CPU · default");ui.selectable_value(&mut self.gpu,true,if cfg!(feature="gpu"){"GPU"}else{"GPU · requires GPU-enabled build"});});
                });
                ui.add_enabled_ui(mutable||self.waiting,|ui|{
                    ui.label("Reference");egui::ComboBox::from_id_salt("reference_mode").selected_text(["Automatic · accepted median","Selected accepted frame","External FITS"][self.reference_mode]).width(ui.available_width()-12.).show_ui(ui,|ui|{for (id,label) in ["Automatic · accepted median","Selected accepted frame","External FITS"].iter().enumerate(){ui.selectable_value(&mut self.reference_mode,id,*label);}});
                    if self.reference_mode!=0 {
                        if self.reference_mode==2 && ui.button("Choose reference…").clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("FITS",&["fit","fits","fts"]).pick_file(){self.reference=path.to_string_lossy().into_owned();}}
                        ui.add(egui::TextEdit::singleline(&mut self.reference).hint_text("Select a reference in Quality"));
                    }
                });
                ui.label(RichText::new("Reference size · no crop").strong());ui.label(RichText::new("Exact reference dimensions and pixel grid.").small().weak());ui.separator();
                ui.add_enabled_ui(mutable,|ui| {
                    ui.heading("Output");ui.label("Folder");ui.text_edit_singleline(&mut self.folder);
                    if ui.button("Choose output folder…").clicked(){if let Some(path)=rfd::FileDialog::new().pick_folder(){self.folder=path.to_string_lossy().into_owned();}}
                    ui.label("Master filename");ui.text_edit_singleline(&mut self.filename);
                    ui.checkbox(&mut self.save_coverage,"Save coverage map");
                    ui.checkbox(&mut self.save_high_rejection,"Save high rejection map");
                    ui.label(RichText::new("Master + JSON report are always saved. Existing names get _v2, _v3, …; originals are preserved.").small().weak());
                    ui.collapsing("Advanced",|ui|{
                        ui.label("Memory ceiling (MiB)");ui.add(egui::DragValue::new(&mut self.memory).range(1024..=12288));
                        egui::ComboBox::from_id_salt("tiles").selected_text(if self.tile==0{"Tiles: Automatic".into()}else{format!("Tiles: {} × {}",self.tile,self.tile)}).show_ui(ui,|ui|{for size in [0,128,256,512]{ui.selectable_value(&mut self.tile,size,if size==0{"Automatic · RAM reserve".into()}else{format!("{size} × {size}")});}});
                        ui.label("FITS cache: Off");ui.label("Pixel rejection: 3.0 low / 2.5 high");
                    });
                });
            });
        });
    }
    fn frames(&mut self, ui: &mut egui::Ui) {
        ui.heading("Your next stack");
        ui.label("One target and filter. Different exposure lengths are supported.");
        ui.add_enabled_ui(!self.busy(), |ui| {
            ui.horizontal(|ui| {
                if ui.button("Add folder…").clicked() {
                    self.add_folder();
                }
                if ui.button("Add files…").clicked() {
                    self.add_files();
                }
                if ui
                    .add_enabled(
                        !self.inputs.is_empty(),
                        egui::Button::new("Inspect quality"),
                    )
                    .clicked()
                {
                    self.start(true);
                }
                if ui.button("Clear inputs").clicked() {
                    self.inputs.clear();
                    self.rows.clear();
                    self.inspection = None;
                    self.manual_selection.clear();
                    self.selected = None;
                    self.preview = None;
                    self.preview_texture = None;
                }
            });
        });
        ui.separator();
        ui.label("Drag FITS files or folders into the window to add them.");
        egui::ScrollArea::vertical().show(ui, |ui| {
            for path in &self.inputs {
                ui.label(path.to_string_lossy());
            }
        });
        if !self.rows.is_empty() {
            ui.separator();
            ui.label(format!(
                "{} measured frames · {} quality accepted",
                self.rows.len(),
                self.rows.iter().filter(|f| f.accepted).count()
            ));
        }
    }
    fn source_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("sub_preview")
            .default_width(345.)
            .min_width(250.)
            .max_width(800.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Subexposure preview");
                let Some(index) = self.selected.filter(|&i| i < self.rows.len()) else {
                    ui.label("Select a measured frame in Quality.");
                    return;
                };
                let row = &self.rows[index];
                ui.label(RichText::new(&row.file_name).strong());
                if let Some(date) = &row.date {
                    ui.label(date);
                }
                ui.label(format!(
                    "{} s · {} stars · HFR {} px",
                    number(row.exposure),
                    row.stars,
                    number(row.hfr)
                ));
                ui.label(if row.accepted { "Accepted" } else { "Rejected" });
                if !row.reasons.is_empty() {
                    ui.label(&row.reasons);
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
                ui.separator();
                if ui
                    .checkbox(&mut self.stretch, "Auto stretch · view only")
                    .changed()
                {
                    self.preview_texture = None;
                }
                ui.label(
                    RichText::new("Unlinked RGB · fitted downsampled preview")
                        .small()
                        .weak(),
                );
                if self.preview_task.is_some() {
                    ui.spinner();
                    ui.label("Reading one immutable source…");
                }
                if let Some(preview) = &self.preview {
                    if self.preview_texture.is_none() {
                        self.preview_texture = Some(ctx.load_texture(
                            "source_preview",
                            preview.display(self.stretch, None),
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    if let Some(texture) = &self.preview_texture {
                        ui.add(
                            egui::Image::new(texture)
                                .fit_to_exact_size(fit(texture, ui.available_size())),
                        );
                    }
                } else if self.busy() && !self.waiting {
                    ui.label(
                        "Source loading is deferred while stacking to preserve the memory ceiling.",
                    );
                } else if self.preview_task.is_none()
                    && ui.button("Load selected subexposure").clicked()
                {
                    self.preview_task = Some(PreviewTask::start(self.rows[index].path.clone()));
                }
            });
    }
    fn quality(&mut self, ui: &mut egui::Ui) {
        ui.heading("Quality");
        if !self.waiting {
            if let Some(state) = self
                .worker
                .as_ref()
                .and_then(|w| w.progress.lock().ok().map(|p| p.clone()))
                .filter(|p| p.phase == "Inspect")
            {
                if state.total > 0 {
                    let fraction = (state.completed as f32 / state.total as f32).clamp(0., 1.);
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .text(format!("Inspect quality: {:.0} %", fraction * 100.)),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Preparing quality inspection…");
                    });
                }
                ui.separator();
            }
        }
        if self.waiting {
            self.review_prompt(ui);
            ui.separator();
        }
        ui.label(if self.final_weights {
            "Original CFA measurements · final registered weights"
        } else {
            "Measurements from original CFA frames · provisional weights before registration"
        });
        ui.horizontal(|ui| {
            for (value, label) in [
                (Filter::All, "All"),
                (Filter::Accepted, "Accepted"),
                (Filter::Rejected, "Rejected"),
            ] {
                ui.selectable_value(&mut self.filter, value, label);
            }
            ui.label(format!(
                "{} frames · {} selected",
                self.rows.len(),
                self.rows
                    .iter()
                    .filter(|r| self
                        .manual_selection
                        .get(&r.path)
                        .copied()
                        .unwrap_or(r.automatic_accepted))
                    .count()
            ));
        });
        let editable = self.inspection.is_some() && (!self.busy() || self.waiting);
        ui.horizontal_wrapped(|ui| {
            ui.label("Use selects frames for stacking. Filters only change the table view.");
            if ui
                .add_enabled(
                    editable && !self.manual_selection.is_empty(),
                    egui::Button::new("Use automatic selection"),
                )
                .clicked()
            {
                self.manual_selection.clear();
                self.refresh_quality_selection();
            }
        });
        if self.rows.is_empty() {
            ui.label("Add inputs, then inspect quality or start stacking.");
            return;
        }
        self.chart(ui);
        ui.separator();
        let mut indices: Vec<_> = (0..self.rows.len())
            .filter(|&i| {
                self.filter == Filter::All
                    || (self.filter == Filter::Accepted) == self.rows[i].accepted
            })
            .collect();
        indices.sort_by(|&a, &b| {
            let a = &self.rows[a];
            let b = &self.rows[b];
            let order = match self.sort {
                Sort::Name => a.file_name.cmp(&b.file_name),
                Sort::Exposure => a
                    .exposure
                    .partial_cmp(&b.exposure)
                    .unwrap_or(std::cmp::Ordering::Equal),
                Sort::Hfr => a
                    .hfr
                    .partial_cmp(&b.hfr)
                    .unwrap_or(std::cmp::Ordering::Equal),
                Sort::Stars => a.stars.cmp(&b.stars),
                Sort::Eccentricity => a
                    .eccentricity
                    .partial_cmp(&b.eccentricity)
                    .unwrap_or(std::cmp::Ordering::Equal),
                Sort::Weight => a.weight.total_cmp(&b.weight),
            };
            let order = order.then_with(|| a.file_name.cmp(&b.file_name));
            if self.descending {
                order.reverse()
            } else {
                order
            }
        });
        let mut selection_change = None;
        egui::ScrollArea::horizontal()
            .id_salt("quality_columns")
            .show(ui, |ui| {
                let widths = [175., 58., 58., 55., 56., 58., 78.];
                ui.horizontal(|ui| {
                    ui.add_sized([38., 24.], egui::Label::new("Use"));
                    for ((sort, label), width) in [
                        (Sort::Name, "Frame"),
                        (Sort::Exposure, "Exp. s"),
                        (Sort::Hfr, "HFR px"),
                        (Sort::Stars, "Stars"),
                        (Sort::Eccentricity, "Ecc."),
                        (Sort::Weight, "Weight"),
                    ]
                    .into_iter()
                    .zip(widths)
                    {
                        if ui
                            .add_sized([width, 24.], egui::Button::new(label))
                            .clicked()
                        {
                            if self.sort == sort {
                                self.descending = !self.descending;
                            } else {
                                self.sort = sort;
                                self.descending = false;
                            }
                        }
                    }
                    ui.label("Status");
                });
                egui::ScrollArea::vertical()
                    .id_salt("quality_rows")
                    .auto_shrink([false, false])
                    .show_rows(ui, 26., indices.len(), |ui, range| {
                        for position in range {
                            let index = indices[position];
                            let row = &self.rows[index];
                            let name: String = if row.file_name.chars().count() > 24 {
                                row.file_name.chars().take(21).chain("…".chars()).collect()
                            } else {
                                row.file_name.clone()
                            };
                            let values = [
                                name,
                                number(row.exposure),
                                number(row.hfr),
                                row.stars.to_string(),
                                number(row.eccentricity),
                                format!("{:.3}", row.weight),
                                if self.manual_selection.get(&row.path) == Some(&false) {
                                    "Excluded · manual".into()
                                } else if self.final_weights && !row.accepted {
                                    "Rejected · final".into()
                                } else if self.manual_selection.get(&row.path) == Some(&true) {
                                    "Included · manual".into()
                                } else if row.accepted { "Accepted".into() } else { "Rejected".into() },
                            ];
                            let mut clicked = false;
                            ui.horizontal(|ui| {
                                let mut included = self.manual_selection.get(&row.path).copied().unwrap_or(row.automatic_accepted);
                                ui.add_enabled_ui(editable && (included || row.can_include), |ui| {
                                    if ui.add_sized([38., 26.], egui::Checkbox::without_text(&mut included))
                                        .on_hover_text(if row.can_include { "Include or exclude this frame from stacking." } else { "Cannot include unusable or incompatible input data." })
                                        .changed() {
                                        selection_change = Some((row.path.clone(), included));
                                    }
                                });
                                for (column, (text, width)) in values.iter().zip(widths).enumerate()
                                {
                                    let response = if column == 0 {
                                        ui.add_sized(
                                            [width, 26.],
                                            egui::Button::new(text)
                                                .selected(self.selected == Some(index)),
                                        )
                                    } else {
                                        ui.add_sized(
                                            [width, 26.],
                                            egui::Label::new(text).sense(egui::Sense::click()),
                                        )
                                    };
                                    clicked |= response.clicked();
                                    response.on_hover_text(format!(
                                        "{}\n{}",
                                        row.file_name, row.reasons
                                    ));
                                }
                            });
                            if clicked {
                                self.select(index);
                            }
                        }
                    });
            });
        if let Some((path, include)) = selection_change {
            self.manual_selection.insert(path, include);
            self.refresh_quality_selection();
        }
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
    fn review_prompt(&mut self, ui: &mut egui::Ui) {
        ui.heading("Waiting for your confirmation");
        ui.label(
            "Quality inspection is complete. No processing is running while you review the frames.",
        );
        ui.label(format!(
            "{} accepted / {} measured frames. Continue to begin registration and stacking.",
            self.rows.iter().filter(|r| r.accepted).count(),
            self.rows.len()
        ));
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(
                    egui::Button::new("Continue stacking")
                        .fill(ui.visuals().selection.bg_fill)
                        .min_size(egui::vec2(180., 40.)),
                )
                .clicked()
            {
                self.continue_run();
            }
            if ui.button("Review frames").clicked() {
                self.view = View::Quality;
                self.processing_open = false;
            }
            if ui.button("STOP").clicked() {
                self.cancel_run();
            }
        });
    }
    fn processing(&mut self, ui: &mut egui::Ui) {
        let state = self
            .worker
            .as_ref()
            .and_then(|w| w.progress.lock().ok().map(|p| p.clone()))
            .or_else(|| self.last_progress.clone());
        self.processing_timing(ui, state.as_ref());
        if let Some(paths) = &self.output_plan {
            ui.add(
                egui::Label::new(format!(
                    "Output: {}",
                    paths
                        .master
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ))
                .truncate(),
            )
            .on_hover_text(paths.master.display().to_string());
            ui.label(
                RichText::new(format!(
                    "Coverage map: {} · High rejection map: {}",
                    if paths.save_coverage { "save" } else { "off" },
                    if paths.save_high_rejection {
                        "save"
                    } else {
                        "off"
                    }
                ))
                .small()
                .weak(),
            );
            ui.add_space(8.);
        }
        let times = state
            .as_ref()
            .map(job::Progress::phase_times)
            .unwrap_or_default();
        egui::Grid::new("processing_phase_times")
            .num_columns(3)
            .spacing([24., 6.])
            .show(ui, |ui| {
                ui.strong("Phase");
                ui.strong("Time");
                ui.strong("Status");
                ui.end_row();
                for (index, phase) in job::PHASES.iter().enumerate() {
                    let current = state.as_ref().is_some_and(|s| s.phase == *phase);
                    let active =
                        current && self.busy() && !self.waiting && self.finished_elapsed.is_none();
                    ui.label(if active {
                        RichText::new(*phase).strong()
                    } else {
                        RichText::new(*phase)
                    });
                    ui.monospace(times[index].map(phase_clock).unwrap_or_else(|| "—".into()));
                    ui.label(
                        if index == 0 && state.as_ref().is_some_and(|s| s.inspection_reused) {
                            "Reused"
                        } else if active {
                            "Running"
                        } else if current && self.finished_elapsed.is_some() && !self.run_complete {
                            "Stopped"
                        } else if times[index].is_some() {
                            "Complete"
                        } else {
                            "Not started"
                        },
                    );
                    ui.end_row();
                }
            });
        ui.add_space(12.);
        if self.waiting {
            self.review_prompt(ui);
            return;
        }
        if let Some(state) = state.filter(|_| self.busy() && self.finished_elapsed.is_none()) {
            if state.total > 0 {
                let fraction = (state.completed as f32 / state.total as f32).clamp(0., 1.);
                ui.add(egui::ProgressBar::new(fraction).text(format!(
                    "{}: {:.0} %",
                    state.phase,
                    (fraction * 100.).floor()
                )));
            } else {
                ui.spinner();
                ui.label(if state.phase == "Quality review" {
                    "Starting registration"
                } else {
                    state.phase
                });
            }
        }
        ui.separator();
        ui.add(egui::Label::new(&self.message).truncate())
            .on_hover_text(&self.message);
        ui.label(
            RichText::new("Completed output files are kept if the run is stopped.")
                .small()
                .weak(),
        );
    }
    fn processing_modal(&mut self, ctx: &egui::Context) {
        if !self.processing_open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("processing_modal")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 80.).clamp(280., 640.));
            ui.spacing_mut().item_spacing.y = 6.;
            // Keep the primary action visible above the compact, non-scrolling body.
            ui.horizontal(|ui| {
                ui.heading("Processing");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let active = self.busy() && self.finished_elapsed.is_none();
                    if active {
                        let stopping = self.worker.as_ref().is_some_and(|w| w.cancel.load(Ordering::Relaxed));
                        if ui.add_enabled(!stopping,
                            egui::Button::new(RichText::new(if stopping { "Stopping…" } else { "STOP" }).strong().color(Color32::WHITE))
                                .fill(Color32::from_rgb(156, 43, 54)).min_size(egui::vec2(130., 40.)))
                            .on_hover_text("Stop this run and wait for worker cleanup. Completed output files are kept.")
                            .clicked() {
                            self.cancel_run();
                        }
                    } else if ui.add_sized([130., 40.], egui::Button::new("Close")).clicked() {
                        self.processing_open = false;
                        ctx.request_repaint();
                    }
                });
            });
            ui.separator();
            self.processing(ui);
        });
        // An active run can only be interrupted explicitly; backdrop/Escape must
        // never hide the only progress/cancellation control or cancel by accident.
        if response.should_close()
            && (!self.busy() || self.waiting || self.finished_elapsed.is_some())
        {
            self.processing_open = false;
            ctx.request_repaint();
        }
    }
    fn results(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Results");
        let Some(result) = &self.result else {
            ui.label("Complete a stack to view the master and diagnostic maps.");
            return;
        };
        ui.label(format!(
            "{}/{} accepted · {:.3} s · {} × {} · no crop",
            result.accepted, result.total, result.wall_seconds, result.width, result.height
        ));
        ui.label(format!(
            "Accepted exposure {:.0} s · peak RAM {} · reference {}",
            result.exposure,
            result
                .peak_rss_bytes
                .map(|b| format!("{:.1} MiB", b as f64 / 1048576.))
                .unwrap_or_else(|| "unavailable".into()),
            result.reference
        ));
        ui.label(
            RichText::new("Saved run · changing settings does not recompute this result")
                .small()
                .weak(),
        );
        ui.label(format!(
            "Master: {}",
            result
                .paths
                .master
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        ui.horizontal(|ui| {
            for (id, label) in ["Master", "Coverage", "High rejections"].iter().enumerate() {
                if ui.selectable_value(&mut self.layer, id, *label).changed() {
                    self.result_texture = None;
                }
            }
        });
        if self.layer == 0 {
            if ui
                .checkbox(
                    &mut self.result_stretch,
                    "Auto stretch · unlinked RGB · view only",
                )
                .changed()
            {
                self.result_texture = None;
            }
        } else {
            if (self.layer == 1 && !result.paths.save_coverage)
                || (self.layer == 2 && !result.paths.save_high_rejection)
            {
                ui.label(
                    RichText::new("Preview only · this map was not saved to a file")
                        .small()
                        .weak(),
                );
            }
            ui.horizontal(|ui| {
                ui.label("Channel");
                for (id, label) in ["R", "G", "B"].iter().enumerate() {
                    if ui.selectable_value(&mut self.channel, id, *label).changed() {
                        self.result_texture = None;
                    }
                }
                ui.label("Fixed range: 0% — 100%");
            });
            ui.label(if self.layer == 1 {
                "Surviving samples / ALL original inputs (after rejection)"
            } else {
                "Upper rejected samples / ALL original inputs"
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Reveal output folder").clicked() {
                reveal(&result.paths.master);
            }
            for (label, path, saved) in [
                ("Master", &result.paths.master, true),
                (
                    "Coverage",
                    &result.paths.coverage,
                    result.paths.save_coverage,
                ),
                (
                    "High rejections",
                    &result.paths.high_rejection,
                    result.paths.save_high_rejection,
                ),
                ("Report", &result.paths.report, true),
            ] {
                if saved && ui.button(format!("Open {label}")).clicked() {
                    open(path);
                }
            }
        });
        let preview = match self.layer {
            0 => &result.master,
            1 => &result.coverage,
            _ => &result.high,
        };
        if self.result_texture.is_none() {
            self.result_texture = Some(ctx.load_texture(
                "result_preview",
                preview.display(
                    self.layer == 0 && self.result_stretch,
                    (self.layer != 0).then_some(self.channel),
                ),
                egui::TextureOptions::LINEAR,
            ));
        }
        if let Some(texture) = &self.result_texture {
            ui.add(egui::Image::new(texture).fit_to_exact_size(fit(texture, ui.available_size())));
        }
    }
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
    let seconds = duration.as_secs();
    format!(
        "{:02}:{:02}:{:02}.{}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        duration.subsec_millis() / 100
    )
}

/// UI-only prediction. Early inspection/registration cannot predict future tile work.
fn remaining_seconds(state: &job::Progress) -> Option<f64> {
    let started = state.phase_started?;
    let phase_elapsed = started.elapsed().as_secs_f64();
    let sampled = state.updated_at?.duration_since(started).as_secs_f64();
    let saving_allowance = |integration: f64| (integration * 0.05).max(2.);
    let estimate = match state.phase {
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

fn fit(texture: &TextureHandle, available: egui::Vec2) -> egui::Vec2 {
    let size = texture.size_vec2();
    size * (available.x / size.x)
        .min(available.y.max(80.) / size.y)
        .max(0.01)
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
        if self.view == View::Processing {
            // Legacy internal page requests must not reopen an automatic
            // progress dialog after completion. Navigation opens summaries explicitly.
            self.view = if self.run_complete {
                View::Results
            } else {
                View::Quality
            };
            self.processing_open = !self.run_complete;
        }
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
        let workspace_available = !self.processing_open && !self.confirm_close;
        if workspace_available && !self.busy() && !dropped.is_empty() {
            self.add_inputs(dropped);
        }
        if workspace_available
            && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O))
            && !self.busy()
        {
            self.add_files();
        }
        if workspace_available && ctx.input(|i| i.key_pressed(egui::Key::F5)) && self.waiting {
            self.continue_run();
        } else if workspace_available
            && ctx.input(|i| i.key_pressed(egui::Key::F5))
            && !self.busy()
            && !self.inputs.is_empty()
        {
            self.start(false);
        }
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(&self.message);
                if let Some(notice) = &self.settings_notice {
                    ui.label(RichText::new(notice).color(ui.visuals().warn_fg_color));
                }
                if self.busy() && !self.waiting {
                    ui.spinner();
                }
            });
        });
        self.sidebar(ctx);
        if self.view == View::Quality {
            self.source_panel(ctx);
        }
        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Frames => self.frames(ui),
            View::Quality => self.quality(ui),
            View::Processing => unreachable!("Processing is displayed in a modal"),
            View::Results => self.results(ui, ctx),
        });
        self.processing_modal(ctx);
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
mod tests {
    use super::*;
    fn render(
        app: &mut Workspace,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.style_mut(|style| style.animation_time = 0.);
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 760.),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.draw(ctx),
        )
    }
    #[test]
    fn review_is_static_and_processing_continue_button_really_resumes_worker() {
        let dir = tempfile::tempdir().unwrap();
        let request = crate::job::tests::request(dir.path());
        let worker = Worker::start(request, false);
        let Event::Quality {
            rows,
            waiting: true,
            ..
        } = worker.events.recv_timeout(Duration::from_secs(10)).unwrap()
        else {
            panic!("Expected review");
        };
        let now = Instant::now();
        let mut app = Workspace {
            worker: Some(worker),
            waiting: true,
            rows,
            view: View::Processing,
            started: Some(now - Duration::from_secs(183)),
            review_started: Some(now - Duration::from_secs(180)),
            ..Workspace::default()
        };
        let ctx = egui::Context::default();
        let _ = render(&mut app, &ctx, vec![]);
        let output = render(&mut app, &ctx, vec![]);
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::epaint::Shape::Text(t) = &s.shape {
                    Some((t.pos, t.galley.job.text.as_str()))
                } else {
                    None
                }
            })
            .collect();
        assert!(
            texts
                .iter()
                .any(|(_, s)| *s == "Waiting for your confirmation")
        );
        assert!(!texts.iter().any(|(_, s)| s.contains("elapsed")));
        assert!(
            app.active_elapsed() < Duration::from_secs(4),
            "Review wait is not processing time"
        );
        // No spinner/heartbeat should request another repaint while the worker
        // is explicitly parked for review (the context may retain one layout pass).
        let output = render(&mut app, &ctx, vec![]);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
        );
        let point = texts
            .iter()
            .filter(|(_, s)| *s == "Continue stacking")
            .max_by(|(a, _), (b, _)| a.x.total_cmp(&b.x))
            .map(|(p, _)| *p + egui::vec2(10., 8.))
            .unwrap();
        let button = |pressed| egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let _ = render(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(point), button(true)],
        );
        let _ = render(&mut app, &ctx, vec![button(false)]);
        assert!(!app.waiting);
        assert!(app.review_started.is_none());
        // The intentionally starless fixture must reach registration and fail
        // explicitly, rather than wait for a second review confirmation.
        assert!(matches!(
            app.worker
                .as_ref()
                .unwrap()
                .events
                .recv_timeout(Duration::from_secs(10))
                .unwrap(),
            Event::Stopped {
                cancelled: false,
                ..
            }
        ));
    }
    #[test]
    fn quick_stack_needs_no_review_decision_and_idle_views_do_not_animate() {
        let dir = tempfile::tempdir().unwrap();
        let mut request = crate::job::tests::request(dir.path());
        request.review = false;
        let worker = Worker::start(request, false);
        assert!(matches!(
            worker.events.recv_timeout(Duration::from_secs(10)).unwrap(),
            Event::Quality { waiting: false, .. }
        ));
        assert!(matches!(
            worker.events.recv_timeout(Duration::from_secs(10)).unwrap(),
            Event::Stopped {
                cancelled: false,
                ..
            }
        ));
        let mut app = Workspace::default();
        assert!(!app.review);
        for view in [View::Frames, View::Quality, View::Processing, View::Results] {
            app.view = view;
            let ctx = egui::Context::default();
            let _ = render(&mut app, &ctx, vec![]);
            let _ = render(&mut app, &ctx, vec![]);
            let output = render(&mut app, &ctx, vec![]);
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1)
            );
        }
    }
    fn row(i: usize) -> QualityRow {
        QualityRow {
            file_name: format!("frame_{i:05}.fit"),
            path: format!("frame_{i:05}.fit").into(),
            exposure: Some(10.),
            date: Some(format!("2026-10-06T22:{:02}:00", i % 60)),
            hfr: Some(1.5 + (i % 10) as f64 / 10.),
            stars: 100,
            eccentricity: Some(0.4),
            weight: 0.7,
            accepted: i % 5 != 0,
            automatic_accepted: i % 5 != 0,
            can_include: true,
            reasons: if i % 5 == 0 {
                "tracking".into()
            } else {
                String::new()
            },
        }
    }
    #[test]
    fn quality_virtualizes_large_sets_and_renders_all_views_at_windows_scales() {
        let ctx = egui::Context::default();
        let mut app = Workspace {
            rows: (0..10000).map(row).collect(),
            view: View::Quality,
            ..Workspace::default()
        };
        for scale in [1., 1.25, 1.5, 2.] {
            ctx.set_pixels_per_point(scale);
            for view in [View::Frames, View::Quality, View::Processing, View::Results] {
                app.view = view;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280., 800.),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.draw(ctx),
                );
                if view == View::Quality {
                    // The overview chart deliberately draws all observations. Count table
                    // text rather than unrelated chart marks to test row virtualization.
                    let visible_rows=output.shapes.iter().filter(|s|matches!(&s.shape,egui::epaint::Shape::Text(t) if t.galley.job.text.starts_with("frame_"))).count();
                    assert!(
                        (1..100).contains(&visible_rows),
                        "Only visible table rows may be drawn: {visible_rows}"
                    );
                }
                assert!(!output.shapes.is_empty());
            }
        }
        assert_eq!(app.rows.len(), 10000);
        assert!(app.worker.is_none());
        assert_eq!(app.request().config.fits_cache_mib, 0);
        assert!(app.request().reference.is_none());
    }
    #[test]
    fn input_changes_discard_source_preview_and_settings_keep_reference_grid() {
        let mut app = Workspace {
            rows: vec![row(0)],
            selected: Some(0),
            ..Workspace::default()
        };
        app.add_inputs(vec!["setA".into(), "setA".into()]);
        assert_eq!(app.inputs.len(), 1);
        assert!(app.rows.is_empty() && app.selected.is_none() && app.preview.is_none());
        app.tile = 256;
        let config = app.request().config;
        assert_eq!((config.tile_width, config.tile_height), (256, 256));
        assert_eq!(config.tile_mode, TileMode::Manual);
    }
}
