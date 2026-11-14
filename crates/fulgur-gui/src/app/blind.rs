//! Blind solving in Settings (ADR-169): the quad index of the Gaia stars, kept
//! next to the Gaia files or in a folder the owner chose, and the auto-solve
//! of the field centre from the first frames. Building, testing and solving
//! run on their own threads.
use super::compact::{GOOD, WARN, one_row_right_to_left};
use super::notice::{Severity, callout};
use super::settings_page::setting_row;
use super::sky::{Shared, TASK_FAILED, Task, run_task};
use super::*;
use fulgur_solve::{
    Band, FIELD_LIMITS_DEG, Index, IndexStar, STANDARD_FIELDS_DEG, SizeEstimate, SolveOptions,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const MUTED: Color32 = Color32::from_rgb(159, 176, 191);

/// The index file's name; its folder is the Gaia folder unless chosen.
pub(super) const INDEX_FILE: &str = "fulgurstack_gaia_g13.fsidx";
/// The index holds the Gaia stars brighter than this.
const GMAX: f32 = 13.0;
/// Frames auto-solve looks at, at most.
const AUTO_FRAMES: usize = 3;

const FOLDER_INFO: &str = "Where the blind-solving index file lives. Empty: the folder of the Gaia DR3 files, so that one place holds the catalogue and what is built from it. Choose another folder when the Gaia folder is read-only or on a slow disk.";
const FILE_INFO: &str = "The index is built once from the Gaia DR3 files and read in a couple of seconds when it is used. Test reads the whole file and checks it; Delete removes only this file.";
const FIELDS_INFO: &str = "The sizes of sky the index can place, as the long side of a frame in degrees. A wider range needs more room: the estimate under it is for the whole sky with stars to G 13. A frame outside the range is not found blind.";
const BUILD_INFO: &str =
    "Reads the Gaia stars brighter than G 13 and writes the index. The Gaia files are only read.";

/// What a valid index file holds, for the status line.
#[derive(Clone, Debug)]
pub(super) struct IndexFacts {
    stars: usize,
    quads: usize,
    gmax: f32,
    coverage: Option<(f64, f64)>,
    file_bytes: u64,
    seconds: f64,
}

impl IndexFacts {
    fn of(index: &Index, file_bytes: u64, seconds: f64) -> Self {
        Self {
            stars: index.star_count(),
            quads: index.quad_count(),
            gmax: index.gmax(),
            coverage: index.coverage_deg(),
            file_bytes,
            seconds,
        }
    }

    fn line(&self) -> String {
        let fields = self.coverage.map_or(String::new(), |(a, b)| {
            format!("fields {} to {}° · ", trim(a), trim(b))
        });
        format!(
            "{fields}{} M quads · {} M stars to G {} · {} · read in {:.1} s",
            one_decimal(self.quads as f64 / 1e6),
            one_decimal(self.stars as f64 / 1e6),
            self.gmax,
            size(self.file_bytes),
            self.seconds
        )
    }
}

/// The field found by auto-solve and how sure the frames made it.
#[derive(Clone, Debug)]
pub(super) struct AutoSolved {
    pub ra: f64,
    pub dec: f64,
    pub arcsec_per_px: f64,
    pub field_deg: f64,
    pub solved: usize,
    pub tried: usize,
    pub seconds: f64,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Confirm {
    #[default]
    Nothing,
    Replace,
    Delete,
}

struct Build {
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<Vec<String>>>,
    started: Instant,
}

pub(super) struct BlindSolve {
    /// Folder of the index; empty means the Gaia folder.
    pub folder: String,
    /// Fields (long side, degrees) the next build covers.
    pub field_min: f64,
    pub field_max: f64,
    /// Frames auto-solve looks at, set by the application.
    pub files: Vec<PathBuf>,
    tested: Option<PathBuf>,
    test: Shared<Result<IndexFacts, String>>,
    build: Option<Build>,
    built: Shared<Result<IndexFacts, String>>,
    confirm: Confirm,
    auto: Shared<Result<AutoSolved, String>>,
    /// What the last auto-solve found, kept for the field centre row.
    pub last: Option<Result<AutoSolved, String>>,
}

impl Default for BlindSolve {
    fn default() -> Self {
        Self {
            folder: String::new(),
            field_min: STANDARD_FIELDS_DEG.0,
            field_max: STANDARD_FIELDS_DEG.1,
            files: vec![],
            tested: None,
            test: Shared::default(),
            build: None,
            built: Shared::default(),
            confirm: Confirm::Nothing,
            auto: Shared::default(),
            last: None,
        }
    }
}

fn trim(x: f64) -> String {
    let text = format!("{x:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn one_decimal(x: f64) -> String {
    format!("{x:.1}")
}

fn size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1e9)
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

impl BlindSolve {
    /// The index file: in the chosen folder, else next to the Gaia files.
    pub(super) fn path(&self, gaia_folder: &str) -> Option<PathBuf> {
        let folder = match self.folder.trim() {
            "" => gaia_folder.trim(),
            chosen => chosen,
        };
        (!folder.is_empty()).then(|| PathBuf::from(folder).join(INDEX_FILE))
    }

    /// An index file is there (it may not be valid).
    pub(super) fn exists(&self, gaia_folder: &str) -> bool {
        self.path(gaia_folder).is_some_and(|p| p.is_file())
    }

    /// An index build is still writing its result.
    pub(super) fn building(&self) -> bool {
        matches!(*self.built.lock().unwrap(), Task::Running)
    }

    /// Asks a running index build to stop; it removes its partial file.
    pub(super) fn cancel_build(&self) {
        if let Some(build) = &self.build {
            build.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn busy(&self) -> bool {
        self.build.is_some() || matches!(*self.auto.lock().unwrap(), Task::Running)
    }

    fn start_test(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.tested = Some(path.clone());
        self.test = run_task(
            ctx,
            move || {
                let started = Instant::now();
                Index::load(&path)
                    .map(|index| {
                        let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
                        IndexFacts::of(&index, bytes, started.elapsed().as_secs_f64())
                    })
                    .map_err(|e| e.to_string())
            },
            || Err(TASK_FAILED.into()),
        );
    }

    fn start_build(&mut self, gaia: PathBuf, path: PathBuf, ctx: &egui::Context) {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(vec!["Reading the Gaia stars…".to_owned()]));
        self.build = Some(Build {
            cancel: cancel.clone(),
            progress: progress.clone(),
            started: Instant::now(),
        });
        let (min, max) = (self.field_min, self.field_max);
        let repaint = ctx.clone();
        let tmp = Index::temporary_path(&path);
        self.built = run_task(
            ctx,
            move || {
                let ctx = repaint;
                let started = Instant::now();
                let say = |line: String| {
                    progress.lock().unwrap().push(line);
                    ctx.request_repaint();
                };
                let result = (|| -> Result<IndexFacts, String> {
                    let catalogue = fulgur_catalog::xpsd::XpsdCatalogue::open(&gaia)?;
                    let stars: Vec<IndexStar> = catalogue
                        .all_sky(f64::from(GMAX), &cancel)?
                        .into_iter()
                        .map(|(ra, dec, g)| IndexStar::new(ra, dec, g))
                        .collect();
                    say(format!("{} stars read.", stars.len()));
                    let index =
                        Index::build(stars, GMAX, &Band::for_fields(min, max), &say, &cancel)
                            .ok_or_else(|| "cancelled".to_owned())?;
                    say("Writing the file…".into());
                    if let Some(folder) = path.parent() {
                        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
                    }
                    index.save(&path).map_err(|e| e.to_string())?;
                    let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
                    Ok(IndexFacts::of(
                        &index,
                        bytes,
                        started.elapsed().as_secs_f64(),
                    ))
                })();
                if result.is_err() {
                    let _ = std::fs::remove_file(Index::temporary_path(&path));
                }
                result
            },
            move || {
                let _ = std::fs::remove_file(tmp);
                Err(TASK_FAILED.into())
            },
        );
    }

    /// Takes in a finished build: its facts become the index's status.
    fn collect_build(&mut self, path: &Path) {
        let finished = {
            let mut built = self.built.lock().unwrap();
            match std::mem::take(&mut *built) {
                Task::Done(result) => Some(result),
                other => {
                    *built = other;
                    None
                }
            }
        };
        let Some(result) = finished else {
            return;
        };
        self.build = None;
        self.tested = Some(path.to_owned());
        *self.test.lock().unwrap() = Task::Done(result);
    }

    fn delete(&mut self, path: &Path) {
        let _ = std::fs::remove_file(Index::temporary_path(path));
        // The partial-file name of version 0.14.
        let _ = std::fs::remove_file(path.with_extension("tmp"));
        *self.test.lock().unwrap() = match std::fs::remove_file(path) {
            Ok(()) => Task::Idle,
            Err(e) => Task::Done(Err(format!("could not delete: {e}"))),
        };
        self.tested = Some(path.to_owned());
    }

    /// Frames, folder, status, field range and Build, under the Gaia folder.
    pub(super) fn index_section(&mut self, ui: &mut egui::Ui, editable: bool, gaia_folder: &str) {
        let path = self.path(gaia_folder);
        if let Some(p) = &path {
            self.collect_build(p);
        }
        let building = self.build.is_some();
        let free = editable && !building;
        ui.label(
            RichText::new("Lets Auto-solve find the field with no coordinates and no target name.")
                .small()
                .color(MUTED),
        );
        ui.add_space(4.0);

        setting_row(ui, "Index folder", FOLDER_INFO, |ui| {
            ui.add_enabled_ui(free, |ui| {
                one_row_right_to_left(ui, |ui| {
                    if ui
                        .button("Default")
                        .on_hover_text("The folder of the Gaia files")
                        .clicked()
                    {
                        self.folder.clear();
                    }
                    if ui.button("Browse…").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.folder = folder.to_string_lossy().into_owned();
                        }
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.folder)
                            .hint_text("next to the Gaia DR3 files")
                            .desired_width(ui.available_width()),
                    );
                });
            });
            match &path {
                Some(p) => {
                    ui.label(
                        RichText::new(format!("File: {}", display_path(p)))
                            .small()
                            .color(MUTED),
                    );
                }
                None => {
                    ui.label(
                        RichText::new("Choose the Gaia DR3 folder above, or a folder here.")
                            .small()
                            .color(WARN),
                    );
                }
            }
        });

        setting_row(ui, "Index file", FILE_INFO, |ui| {
            self.status_line(ui, path.as_deref());
            ui.add_space(2.0);
            self.file_buttons(ui, free, path.as_deref());
        });

        setting_row(ui, "Fields to cover", FIELDS_INFO, |ui| {
            ui.add_enabled_ui(free, |ui| {
                ui.horizontal(|ui| {
                    let limits = FIELD_LIMITS_DEG;
                    ui.add(
                        egui::DragValue::new(&mut self.field_min)
                            .range(limits.0..=limits.1)
                            .speed(0.05)
                            .max_decimals(2)
                            .suffix(" °"),
                    );
                    ui.label("to");
                    ui.add(
                        egui::DragValue::new(&mut self.field_max)
                            .range(limits.0..=limits.1)
                            .speed(0.1)
                            .max_decimals(2)
                            .suffix(" °"),
                    );
                    if ui.small_button("Standard").clicked() {
                        (self.field_min, self.field_max) = STANDARD_FIELDS_DEG;
                    }
                });
            });
            if self.field_max < self.field_min * 1.5 {
                self.field_max = (self.field_min * 1.5).min(FIELD_LIMITS_DEG.1);
                self.field_min = self.field_min.min(self.field_max / 1.5);
            }
            let estimate = SizeEstimate::for_fields(self.field_min, self.field_max);
            ui.label(
                RichText::new(format!(
                    "Estimated file {} · {} in memory when used · building needs about {} of memory",
                    gigabytes(estimate.file_bytes),
                    gigabytes(estimate.file_bytes),
                    gigabytes(estimate.build_bytes)
                ))
                .small()
                .color(MUTED),
            );
        });

        setting_row(ui, "Build", BUILD_INFO, |ui| {
            self.build_controls(ui, editable, gaia_folder, path.as_deref());
        });
    }

    fn status_line(&self, ui: &mut egui::Ui, path: Option<&Path>) {
        let Some(path) = path else {
            ui.label(RichText::new("No index folder yet.").color(MUTED));
            return;
        };
        let testing = matches!(*self.test.lock().unwrap(), Task::Running);
        if testing {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Reading and checking the file…").color(MUTED));
            });
            return;
        }
        let bytes = std::fs::metadata(path).map(|m| m.len());
        match (
            &*self.test.lock().unwrap(),
            self.tested.as_deref() == Some(path),
        ) {
            (Task::Done(Ok(facts)), true) if bytes.is_ok() => {
                ui.label(RichText::new(format!("✔ Valid · {}", facts.line())).color(GOOD));
            }
            (Task::Done(Err(message)), true) => {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("✖ {message}"))
                            .color(Color32::from_rgb(255, 138, 128)),
                    )
                    .wrap(),
                );
            }
            _ => match bytes {
                Ok(bytes) => {
                    ui.label(
                        RichText::new(format!(
                            "File found · {} · press Test to check it",
                            size(bytes)
                        ))
                        .color(MUTED),
                    );
                }
                Err(_) => {
                    ui.label(RichText::new("Not built yet.").color(WARN));
                }
            },
        }
    }

    fn file_buttons(&mut self, ui: &mut egui::Ui, free: bool, path: Option<&Path>) {
        let exists = path.is_some_and(Path::is_file);
        let testing = matches!(*self.test.lock().unwrap(), Task::Running);
        ui.add_enabled_ui(free, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(exists && !testing, egui::Button::new("Test"))
                    .on_hover_text("Reads the whole file and checks it")
                    .clicked()
                {
                    if let Some(p) = path {
                        self.start_test(p.to_owned(), ui.ctx());
                    }
                }
                if self.confirm == Confirm::Delete {
                    ui.label(RichText::new("Delete the index file?").color(WARN));
                    if ui.button("Yes, delete").clicked() {
                        if let Some(p) = path {
                            self.delete(p);
                        }
                        self.confirm = Confirm::Nothing;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm = Confirm::Nothing;
                    }
                } else if ui
                    .add_enabled(exists, egui::Button::new("Delete"))
                    .on_hover_text("Removes only the index file")
                    .clicked()
                {
                    self.confirm = Confirm::Delete;
                }
            });
        });
    }

    fn build_controls(
        &mut self,
        ui: &mut egui::Ui,
        editable: bool,
        gaia: &str,
        path: Option<&Path>,
    ) {
        if let Some(build) = &self.build {
            let line = build
                .progress
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or_default();
            let elapsed = build.started.elapsed().as_secs();
            let cancel = build.cancel.clone();
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Building… {}:{:02}", elapsed / 60, elapsed % 60));
                if ui.button("Cancel").clicked() {
                    cancel.store(true, Ordering::Relaxed);
                }
            });
            ui.label(RichText::new(line).small().color(MUTED));
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
            return;
        }
        let gaia_ready = !gaia.trim().is_empty() && Path::new(gaia.trim()).is_dir();
        let ready = editable && gaia_ready && path.is_some();
        let exists = path.is_some_and(Path::is_file);
        if self.confirm == Confirm::Replace {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Replace the index that is there?").color(WARN));
                if ui.button("Yes, build").clicked() {
                    if let Some(p) = path {
                        self.start_build(PathBuf::from(gaia.trim()), p.to_owned(), ui.ctx());
                    }
                    self.confirm = Confirm::Nothing;
                }
                if ui.button("Cancel").clicked() {
                    self.confirm = Confirm::Nothing;
                }
            });
        } else if ui
            .add_enabled(ready, egui::Button::new("Build index"))
            .on_hover_text("Takes about a minute or two")
            .clicked()
        {
            if exists {
                self.confirm = Confirm::Replace;
            } else if let Some(p) = path {
                self.start_build(PathBuf::from(gaia.trim()), p.to_owned(), ui.ctx());
            }
        }
        if let Task::Done(Err(message)) = &*self.built.lock().unwrap() {
            ui.add(
                egui::Label::new(
                    RichText::new(if message == "cancelled" {
                        "Build cancelled.".to_owned()
                    } else {
                        format!("✖ Build failed: {message}")
                    })
                    .small()
                    .color(WARN),
                )
                .wrap(),
            );
        }
        if !gaia_ready {
            ui.label(
                RichText::new("Needs the Gaia DR3 folder above.")
                    .small()
                    .color(MUTED),
            );
        }
    }

    // ------------------------------------------------------------ auto-solve

    /// Looks at the first frames and finds their field with no hint.
    pub(super) fn start_auto(&mut self, gaia_folder: &str, ctx: &egui::Context) {
        let Some(path) = self.path(gaia_folder) else {
            return;
        };
        let files: Vec<PathBuf> = self.files.iter().take(AUTO_FRAMES).cloned().collect();
        self.last = None;
        self.auto = run_task(
            ctx,
            move || auto_solve(&path, &files),
            || Err(TASK_FAILED.into()),
        );
    }

    pub(super) fn auto_running(&self) -> bool {
        matches!(*self.auto.lock().unwrap(), Task::Running)
    }

    /// A finished auto-solve, once.
    pub(super) fn take_auto(&mut self) -> Option<Result<AutoSolved, String>> {
        let mut state = self.auto.lock().unwrap();
        match std::mem::take(&mut *state) {
            Task::Done(result) => Some(result),
            other => {
                *state = other;
                None
            }
        }
    }

    /// The note under the Auto-solve button.
    pub(super) fn auto_note(&self, ui: &mut egui::Ui) {
        if self.auto_running() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Solving the first frames blind…").color(MUTED));
            });
            return;
        }
        match &self.last {
            Some(Ok(found)) => {
                callout(
                    ui,
                    Severity::Note,
                    &format!(
                        "Found by blind solving {} of {} frames: {} · {:.2} arcsec/px · field {:.2}° · {:.1} s. Check it against the target if you know it.",
                        found.solved,
                        found.tried,
                        fulgur_pipeline::sky::catalogue::position_text(found.ra, found.dec),
                        found.arcsec_per_px,
                        found.field_deg,
                        found.seconds
                    ),
                    None,
                );
            }
            Some(Err(message)) => {
                callout(ui, Severity::Warning, message, None);
            }
            None => {}
        }
    }
}

/// Angle between two sky positions, in degrees.
fn separation(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (ra1, dec1, ra2, dec2) = (
        a.0.to_radians(),
        a.1.to_radians(),
        b.0.to_radians(),
        b.1.to_radians(),
    );
    let h = ((dec2 - dec1) / 2.0).sin().powi(2)
        + dec1.cos() * dec2.cos() * ((ra2 - ra1) / 2.0).sin().powi(2);
    2.0 * h.sqrt().min(1.0).asin().to_degrees()
}

/// Solves up to `AUTO_FRAMES` frames blind. The answer needs most of the
/// frames that were placed to agree within a degree.
fn auto_solve(index_file: &Path, files: &[PathBuf]) -> Result<AutoSolved, String> {
    let started = Instant::now();
    if files.is_empty() {
        return Err("There are no frames to solve yet.".into());
    }
    let index = Index::load(index_file).map_err(|e| format!("The index cannot be read: {e}"))?;
    let cancel = AtomicBool::new(false);
    let options = SolveOptions::default();
    let mut found = vec![];
    for file in files {
        let Ok(raw) = fulgur_core::fits::read_raw(file) else {
            continue;
        };
        let info = &raw.header.info;
        let mosaic = info.mosaic == fulgur_core::Mosaic::Bayer;
        let (mut stars, _) = fulgur_pipeline::sky::detect::bright_stars(
            &raw.pixels,
            info.width,
            info.height,
            mosaic,
        );
        stars.sort_by(|a, b| b[4].total_cmp(&a[4]));
        let points: Vec<[f64; 2]> = stars.iter().map(|s| [s[0], s[1]]).collect();
        if let Some(h) =
            fulgur_solve::solve(&index, info.width, info.height, &points, &options, &cancel)
        {
            found.push(h);
        }
    }
    if found.is_empty() {
        return Err("No place on the sky fits these frames: the index may not cover their field, or they hold too few stars. Enter the target or the coordinates instead.".into());
    }
    // The hypothesis most of the others agree with.
    let agree = |i: usize| {
        found
            .iter()
            .filter(|h| separation((h.ra, h.dec), (found[i].ra, found[i].dec)) < 1.0)
            .count()
    };
    let best = (0..found.len()).max_by_key(|&i| agree(i)).unwrap_or(0);
    let votes = agree(best);
    if votes * 2 <= found.len() {
        return Err("The frames give different places on the sky, so none is trusted. Enter the target or the coordinates.".into());
    }
    let h = &found[best];
    Ok(AutoSolved {
        ra: h.ra,
        dec: h.dec,
        arcsec_per_px: h.arcsec_per_px,
        field_deg: h.field_deg,
        solved: votes,
        tried: files.len(),
        seconds: started.elapsed().as_secs_f64(),
    })
}
