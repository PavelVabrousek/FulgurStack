//! Compact Desk over the accepted pipeline and bounded image viewers.
mod compact;
mod quality_table;
use crate::{
    image_view::{DetailSource, ImageView},
    job::{self, Completed, Event, QualityRow, StackRequest, Worker},
    preview::{DisplayOptions, PreviewTask, Thumbnail},
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
    Setup,
    Quality,
    Processing,
    Results,
}
pub struct Workspace {
    inputs: Vec<PathBuf>,
    view: View,
    setup_tab: usize,
    show_preview: bool,
    show_quality_chart: bool,
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
    diagnose_excluded: bool,
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
    processing_open: bool,
    processing_saved: bool,
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
            setup_tab: 0,
            show_preview: true,
            show_quality_chart: false,
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
            diagnose_excluded: false,
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
            processing_open: false,
            processing_saved: false,
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
            diagnose_excluded: self.diagnose_excluded,
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
        self.diagnose_excluded = saved.diagnose_excluded;
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
        self.processing_saved = false;
        if let Some(request) = &mut self.active_request {
            request.reference = (self.reference_mode != 0).then(|| PathBuf::from(&self.reference));
        }
        self.persist_settings(None, true);
        self.preview_task = None;
        self.preview_view.stop_loading();
        self.result_view.stop_loading();
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
            app.inputs = inputs.into_iter().map(|p| settings::absolute(&p)).collect();
        }
        app
    }
    fn busy(&self) -> bool {
        self.worker.is_some()
    }
    fn add_inputs(&mut self, paths: Vec<PathBuf>) {
        self.preview_task = None;
        self.preview_view.reset();
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
        self.quality_table.reset_rows();
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
            diagnose_excluded: self.diagnose_excluded,
            save_coverage: self.save_coverage,
            save_high_rejection: self.save_high_rejection,
            config,
        }
    }
    fn start(&mut self, inspect_only: bool) {
        self.processing_saved = false;
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
        self.preview_view.stop_loading();
        self.result_view.stop_loading();
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
        self.active_request = Some(request.clone());
        if inspect_only {
            self.inspection = None;
            self.manual_selection.clear();
            self.quality_table.reset_rows();
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
                    self.processing_open = false;
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
                    self.preview_view.reset();
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
        self.preview_view.reset();
        self.preview = None;
        self.preview_texture = None;
        self.preview_task = None;
        if !self.busy() || self.waiting {
            self.preview_task = Some(PreviewTask::start(self.rows[index].path.clone()));
        }
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        self.compact_navigation(ctx);
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
                    self.quality_table.reset_rows();
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
            .default_width(258.)
            .min_width(220.)
            .max_width(380.)
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
                    ui.label(timestamp_seconds(Some(date)));
                }
                ui.label(format!(
                    "{} s · {} stars · HFR {} px",
                    whole_seconds(row.exposure),
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
                ui.label(RichText::new("Unlinked RGB · display only").small().weak());
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
                        self.preview_view.show(
                            ui,
                            texture,
                            preview,
                            (!self.busy() || self.waiting)
                                .then(|| DetailSource::Cfa(self.rows[index].path.clone())),
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
                } else if self.preview_task.is_none()
                    && ui.button("Load selected subexposure").clicked()
                {
                    self.preview_task = Some(PreviewTask::start(self.rows[index].path.clone()));
                }
            });
    }
    fn quality(&mut self, ui: &mut egui::Ui) {
        self.quality_table_ui(ui);
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
    fn processing(&mut self, ui: &mut egui::Ui) {
        self.compact_processing(ui);
    }
    fn processing_modal(&mut self, ctx: &egui::Context) {
        self.compact_processing_modal(ctx);
    }
    fn results(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.compact_results(ui, ctx);
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
        self.compact_footer(ctx);
        self.sidebar(ctx);
        if self.view == View::Quality && self.show_preview {
            self.source_panel(ctx);
        }
        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Frames => self.frames(ui),
            View::Setup => self.compact_setup(ui),
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
    fn review_is_static_and_footer_continue_button_really_resumes_worker() {
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
            view: View::Quality,
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
                .any(|(_, s)| s.contains("Inspection complete · waiting for review"))
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
            background: Some(1200.),
            noise: Some(0.8),
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
