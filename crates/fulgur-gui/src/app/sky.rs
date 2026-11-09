//! Sky registration settings (plan phase 4, ADR-138): the method switch, the
//! folder of the local Gaia catalogue with a test of the installation, and
//! the field centre. Checks and look-ups run on their own threads.
use super::blind::BlindSolve;
use super::compact::{GOOD, WARN, one_row_right_to_left};
use super::notice::{Choice, Fix, Severity, accepted_line, callout, fix_card};
use super::settings_page::setting_row;
use super::*;
use fulgur_catalog::guess::TargetGuess;
use fulgur_catalog::sky::{SkyChoice, parse_dec, parse_ra};
use fulgur_catalog::xpsd::InstallationCheck;
use std::sync::Mutex;

const MUTED: Color32 = Color32::from_rgb(159, 176, 191);
const SUBPANEL: Color32 = Color32::from_rgb(15, 36, 56);
const SUBPANEL_LINE: Color32 = Color32::from_rgb(36, 60, 82);

const STARS_INFO: &str = "Matches the stars of each frame with the reference frame. One camera and one optical train at a time; other cameras join through their own anchor.";
const SKY_INFO: &str = "Plate-solves every frame against the Gaia DR3 catalogue and registers it through the sky, whatever camera, optics, filter or orientation took it. Frames it cannot solve fall back to Stars. Distortion is always corrected.";
const METHOD_INFO: &str = "Stars (recommended): matches the stars of each frame with the reference frame. One camera and one optical train at a time; other cameras join through their own anchor.\n\n\
Plate solving (selected automatically when the light sets differ in camera, colour layout or sensor size): solves every frame against the Gaia DR3 catalogue and registers it through the sky, whatever camera, optics, filter or orientation took it. Frames it cannot solve fall back to Stars. Distortion is always corrected.";
const GAIA_INFO: &str = "The folder of the PixInsight Gaia DR3 files (gdr3-…xpsd). The catalogue is read from disk; nothing is copied. Test reads every file and checks that the installation is complete.\n\n\
Stars the folder does not hold can be downloaded from the ESA or Heidelberg (GAVO) Gaia archives; they are kept next to the settings for later runs.\n\n\
With either registration method the master gets a plate solution (WCS) from the catalogue, written in FITS and XISF once the master's own stars confirm it.";
const FIELD_INFO: &str = "The plate solver searches around this position.\n\n\
From the reference frame: when its FITS or XISF header holds the pointing (WCS or RA and DEC), the coordinates are entered here for you.\n\n\
From the target name: left empty, the OBJECT name most frames carry. Messier, NGC, IC, LBN, Sharpless, Barnard and common names are built in.\n\n\
Coordinates: RA in hours (h:m:s) or degrees, Dec in degrees.";

/// Field centre from the object name or from coordinates.
pub(super) const FIELD_FROM_OBJECT: usize = 0;
pub(super) const FIELD_FROM_COORDINATES: usize = 1;

pub(super) enum Task<T> {
    Idle,
    Running,
    Done(T),
}

// Not derived: a derive would ask `T: Default` of every result type.
#[allow(clippy::derivable_impls)]
impl<T> Default for Task<T> {
    fn default() -> Self {
        Self::Idle
    }
}

/// A result shared with the thread that computes it.
pub(super) type Shared<T> = Arc<Mutex<Task<T>>>;

#[derive(Default)]
pub(super) struct SkyPanel {
    pub enabled: bool,
    pub gaia_folder: String,
    pub download: bool,
    pub field_mode: usize,
    pub object: String,
    pub ra: String,
    pub dec: String,
    pub online: bool,
    /// The owner chose the method in this session: no automatic switch.
    pub method_chosen: bool,
    /// What the headers of the reference gave (ADR-162): the file and the
    /// position, with the key of the inputs it was read for.
    header: Option<(PathBuf, (f64, f64))>,
    header_key: String,
    /// Texts this panel entered itself, to tell them from the owner's.
    filled: Option<(String, String)>,
    /// Coordinates and target name saved by an earlier session, while the
    /// owner has not touched them.
    restored: Option<(String, String, String)>,
    /// A target the names around the frames suggest (ADR-167); the owner
    /// confirms it with a button or enters the target.
    pub guess: Option<TargetGuess>,
    /// The blind-solving index and Auto-solve (ADR-169).
    pub blind: BlindSolve,
    /// Warnings accepted on purpose; kept with the preferences (ADR-171).
    pub accepted: std::collections::BTreeSet<String>,
    check: Shared<InstallationCheck>,
    /// The checked folder, so a changed path asks for a new test.
    checked_folder: String,
    lookup: Shared<Result<(f64, f64, String), String>>,
}

impl SkyPanel {
    pub(super) fn save(&self, settings: &mut AppSettings) {
        settings.sky_registration = self.enabled;
        settings.gaia_folder = self.gaia_folder.clone();
        settings.gaia_download = self.download;
        settings.sky_field_mode = self.field_mode;
        settings.sky_object = self.object.clone();
        settings.sky_ra = self.ra.clone();
        settings.sky_dec = self.dec.clone();
        settings.sky_online_lookup = self.online;
        settings.solve_index_folder = self.blind.folder.clone();
        settings.solve_field_min = self.blind.field_min;
        settings.solve_field_max = self.blind.field_max;
    }

    pub(super) fn restore(&mut self, saved: &AppSettings) {
        self.enabled = saved.sky_registration;
        self.gaia_folder = saved.gaia_folder.clone();
        self.download = saved.gaia_download;
        self.field_mode = saved.sky_field_mode.min(FIELD_FROM_COORDINATES);
        self.object = saved.sky_object.clone();
        self.ra = saved.sky_ra.clone();
        self.dec = saved.sky_dec.clone();
        self.online = saved.sky_online_lookup;
        self.blind.folder = saved.solve_index_folder.clone();
        self.blind.field_min = saved.solve_field_min;
        self.blind.field_max = saved.solve_field_max;
        self.accepted = saved.accepted_warnings.iter().cloned().collect();
        self.restored = Some((self.ra.clone(), self.dec.clone(), self.object.clone()));
    }

    /// The suggested target while plate solving has no field centre at all:
    /// no position in the headers and no name or coordinates entered.
    pub(super) fn unconfirmed_guess(&self) -> Option<&TargetGuess> {
        if !self.enabled || self.header.is_some() {
            return None;
        }
        let none_entered = if self.field_mode == FIELD_FROM_OBJECT {
            self.object.trim().is_empty()
        } else {
            self.ra.trim().is_empty() && self.dec.trim().is_empty()
        };
        self.guess.as_ref().filter(|_| none_entered)
    }

    /// What the owner reads before confirming a suggested target.
    pub(super) fn guess_text(guess: &TargetGuess) -> String {
        format!(
            "No target or position in the headers. The built-in catalogue found {} ({}) in the {}. Use it only if it is the target of these frames, or enter the target yourself.",
            guess.matched, guess.catalogue, guess.from
        )
    }

    /// A field centre that nothing confirms for these frames, in words: saved
    /// coordinates the owner has not looked at again while the headers carry
    /// no position, or a saved target name that differs from the frames'
    /// OBJECT. None when the centre is fine or sky registration is off.
    /// What an accepted saved centre was accepted for.
    pub(super) fn centre_key(&self) -> Option<String> {
        let (ra, dec, object) = self.restored.as_ref()?;
        Some(format!("saved_centre|{ra}|{dec}|{object}"))
    }

    /// The notice, unless the owner accepted this very centre.
    pub(super) fn centre_notice(&self, header_object: Option<&str>) -> Option<String> {
        if self
            .centre_key()
            .is_some_and(|key| self.accepted.contains(&key))
        {
            return None;
        }
        self.centre_notice_raw(header_object)
    }

    fn centre_notice_raw(&self, header_object: Option<&str>) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let (ra, dec, object) = self.restored.as_ref()?;
        if self.field_mode == FIELD_FROM_COORDINATES {
            let untouched = *ra == self.ra && *dec == self.dec && !ra.trim().is_empty();
            // The headers were read (a key exists) and gave no position.
            if untouched && !self.header_key.is_empty() && self.header.is_none() {
                return Some(format!(
                    "The field centre RA {} · Dec {} was saved from an earlier session and the frames carry no position: nothing confirms they show this field. A wrong centre finds no solution.",
                    ra.trim(),
                    dec.trim()
                ));
            }
        } else {
            let name = |s: &str| {
                s.chars()
                    .filter(|c| c.is_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>()
            };
            let saved = object.trim();
            if *object == self.object && !saved.is_empty() {
                if let Some(own) = header_object.filter(|own| name(own) != name(saved)) {
                    return Some(format!(
                        "The target name “{saved}” was saved from an earlier session; the frames say “{own}”."
                    ));
                }
            }
        }
        None
    }

    /// Downloaded Gaia cones live next to the settings.
    fn cache() -> PathBuf {
        settings::path()
            .and_then(|p| p.parent().map(|d| d.join("gaia_cache")))
            .unwrap_or_else(|| std::env::temp_dir().join("fulgurstack_gaia_cache"))
    }

    /// What a run uses: None for star registration, Err with what is missing.
    pub(super) fn choice(&self) -> Result<Option<SkyChoice>, String> {
        if !self.enabled {
            return Ok(None);
        }
        let folder = self.gaia_folder.trim();
        if folder.is_empty() && !self.download {
            return Err("Sky registration: choose the Gaia catalogue folder in Settings, or allow downloading.".into());
        }
        let centre = if self.field_mode == FIELD_FROM_COORDINATES {
            match (parse_ra(&self.ra), parse_dec(&self.dec)) {
                (Some(ra), Some(dec)) => Some((ra, dec)),
                _ => {
                    return Err("Sky registration: enter the field centre as RA (degrees or hours, 01:33:50.9) and Dec (degrees, +30:39:36) in Settings.".into());
                }
            }
        } else {
            None
        };
        Ok(Some(SkyChoice {
            sources: fulgur_catalog::GaiaSources {
                pixinsight: (!folder.is_empty()).then(|| PathBuf::from(folder)),
                cache: Self::cache(),
                download: self.download,
            },
            centre,
            object: (self.field_mode == FIELD_FROM_OBJECT && !self.object.trim().is_empty())
                .then(|| self.object.trim().to_string()),
            online: self.online,
        }))
    }

    /// The Gaia catalogue and field centre for the master's WCS, whichever
    /// registration method is chosen (ADR-155); None without a catalogue or
    /// with coordinates that are not valid.
    pub(super) fn catalogue_choice(&self) -> Option<SkyChoice> {
        let folder = self.gaia_folder.trim();
        if folder.is_empty() && !self.download {
            return None;
        }
        let centre = if self.field_mode == FIELD_FROM_COORDINATES {
            Some((parse_ra(&self.ra)?, parse_dec(&self.dec)?))
        } else {
            None
        };
        Some(SkyChoice {
            sources: fulgur_catalog::GaiaSources {
                pixinsight: (!folder.is_empty()).then(|| PathBuf::from(folder)),
                cache: Self::cache(),
                download: self.download,
            },
            centre,
            object: (self.field_mode == FIELD_FROM_OBJECT && !self.object.trim().is_empty())
                .then(|| self.object.trim().to_string()),
            online: self.online,
        })
    }

    fn start_check(&mut self, ctx: &egui::Context) {
        let folder = PathBuf::from(self.gaia_folder.trim());
        self.checked_folder = self.gaia_folder.clone();
        *self.check.lock().unwrap() = Task::Running;
        let (state, ctx) = (self.check.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = fulgur_catalog::xpsd::check_installation(&folder, false);
            *state.lock().unwrap() = Task::Done(result);
            ctx.request_repaint();
        });
    }

    fn start_lookup(&mut self, ctx: &egui::Context, fallback: Option<String>) {
        let name = Some(self.object.trim().to_string())
            .filter(|n| !n.is_empty())
            .or(fallback);
        let Some(name) = name else {
            *self.lookup.lock().unwrap() = Task::Done(Err(
                "Enter a target name, or add frames whose headers name it.".into(),
            ));
            return;
        };
        *self.lookup.lock().unwrap() = Task::Running;
        let (state, ctx, online) = (self.lookup.clone(), ctx.clone(), self.online);
        std::thread::spawn(move || {
            let getter: Option<&fulgur_catalog::Getter> = if online {
                Some(&fulgur_catalog::lookup::http_get)
            } else {
                None
            };
            let result = fulgur_catalog::position(None, None, Some(&name), getter)
                .map(|p| (p.ra, p.dec, fulgur_catalog::sky::source_label(&p)))
                .map_err(|notes| format!("“{name}” not found: {}.", notes.join("; ")));
            *state.lock().unwrap() = Task::Done(result);
            ctx.request_repaint();
        });
    }

    /// The method switch, a row of the Registration panel.
    pub(super) fn method_row(&mut self, ui: &mut egui::Ui, editable: bool, mixed: Option<&str>) {
        setting_row(ui, "Method", METHOD_INFO, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                let before = self.enabled;
                egui::ComboBox::from_id_salt("registration_method")
                    .selected_text(if self.enabled {
                        "Plate solving · any camera"
                    } else {
                        "Stars · one camera"
                    })
                    .width(260.)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.enabled, false, "Stars · one camera")
                            .on_hover_text(STARS_INFO);
                        ui.selectable_value(&mut self.enabled, true, "Plate solving · any camera")
                            .on_hover_text(SKY_INFO);
                    });
                self.method_chosen |= before != self.enabled;
            });
            if let Some(reason) = mixed {
                callout(
                    ui,
                    if self.enabled {
                        Severity::Note
                    } else {
                        Severity::Warning
                    },
                    &format!(
                        "The light sets differ ({reason}): plate solving registers them through the sky."
                    ),
                    None,
                );
            }
        });
    }

    /// A Gaia folder is named or downloading is allowed.
    pub(super) fn has_catalogue(&self) -> bool {
        !self.gaia_folder.trim().is_empty() || self.download
    }

    pub(super) fn header_key(&self) -> &str {
        &self.header_key
    }

    /// The pointing in the header of the reference (or, without a manual
    /// reference, of the first included frame) becomes the field centre
    /// unless the owner entered another (ADR-162).
    pub(super) fn take_header_position(&mut self, reference: Option<PathBuf>, key: String) {
        if self.header_key != key {
            self.header_key = key;
            self.header = reference.and_then(|file| {
                fulgur_core::fits::read_sky_position(&file).map(|position| (file, position))
            });
        }
        let Some((_, (ra, dec))) = &self.header else {
            return;
        };
        let entered = (format!("{ra:.5}"), format!("{dec:+.5}"));
        let untouched = (self.ra.trim().is_empty() && self.dec.trim().is_empty())
            || self.filled.as_ref() == Some(&(self.ra.clone(), self.dec.clone()));
        if untouched && self.filled.as_ref() != Some(&entered) {
            self.ra = entered.0.clone();
            self.dec = entered.1.clone();
            self.filled = Some(entered);
            self.field_mode = FIELD_FROM_COORDINATES;
        }
    }

    /// The Gaia catalogue and the field centre, under the method: required by
    /// plate solving, optional with stars, for the master's WCS (ADR-155).
    pub(super) fn plate_solving(
        &mut self,
        ui: &mut egui::Ui,
        editable: bool,
        header_object: Option<String>,
    ) {
        egui::Frame::new()
            .fill(SUBPANEL)
            .stroke(egui::Stroke::new(1_f32, SUBPANEL_LINE))
            .corner_radius(6.)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !self.enabled {
                    // Star registration uses the catalogue only for the
                    // master's plate solution (ADR-155).
                    ui.label(
                        RichText::new("Optional: with a Gaia catalogue the reference frame is plate-solved and the master gets a WCS. Leave the folder empty for none.")
                            .small()
                            .color(MUTED),
                    );
                    ui.add_space(4.);
                }
                self.catalogue_row(ui, editable);
                let gaia = self.gaia_folder.clone();
                ui.add_space(2.);
                ui.separator();
                egui::CollapsingHeader::new(RichText::new("Blind-solving index").strong())
                    .id_salt("blind_solving_index")
                    .default_open(false)
                    .show_unindented(ui, |ui| {
                        ui.add_space(2.);
                        self.blind.index_section(ui, editable, &gaia);
                    });
                ui.add_space(6.);
                self.field_row(ui, editable, header_object);
            });
        ui.add_space(6.);
    }

    fn catalogue_row(&mut self, ui: &mut egui::Ui, editable: bool) {
        setting_row(ui, "Gaia DR3 folder", GAIA_INFO, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                one_row_right_to_left(ui, |ui| {
                    let testing = matches!(*self.check.lock().unwrap(), Task::Running);
                    if ui
                        .add_enabled(
                            !testing && !self.gaia_folder.trim().is_empty(),
                            egui::Button::new("Test"),
                        )
                        .on_hover_text(
                            "Reads every catalogue file and checks that the installation is complete.",
                        )
                        .clicked()
                    {
                        self.start_check(ui.ctx());
                    }
                    if ui.button("Browse…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.gaia_folder = path.to_string_lossy().into_owned();
                            self.start_check(ui.ctx());
                        }
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.gaia_folder)
                            .hint_text("e.g. the PixInsight “Gaia DR3” folder")
                            .desired_width(ui.available_width()),
                    );
                });
            });
            if let Some(message) = self.check_status(ui) {
                let key = format!("gaia_untested|{}", self.gaia_folder.trim());
                if self.accepted.contains(&key) {
                    if accepted_line(ui, "Accepted: the catalogue folder is used untested.") {
                        self.accepted.remove(&key);
                    }
                } else {
                    let mut fix = Fix::new(message, "Test the catalogue");
                    fix.enabled = editable;
                    fix.keep = Some("Skip the test");
                    fix.result = Some((
                        "Reads:",
                        vec!["every catalogue file".to_owned(), "about 10 s".to_owned()],
                    ));
                    match fix_card(ui, &fix) {
                        Choice::Primary => self.start_check(ui.ctx()),
                        Choice::Keep => {
                            self.accepted.insert(key);
                        }
                        _ => {}
                    }
                }
            }
            ui.add_enabled_ui(editable, |ui| {
                ui.checkbox(
                    &mut self.download,
                    "Download stars the folder does not hold",
                )
                .on_hover_text(format!(
                    "From the ESA or Heidelberg Gaia archives, kept in {} for later runs.",
                    display_path(&Self::cache())
                ));
            });
        });
    }

    /// The status of the last test. Some(text) when a test is wanted: the
    /// folder was never tested, or it changed since.
    fn check_status(&mut self, ui: &mut egui::Ui) -> Option<&'static str> {
        let state = self.check.lock().unwrap();
        match &*state {
            Task::Idle => {
                if !self.gaia_folder.trim().is_empty() {
                    return Some(
                        "This folder has not been tested. Plate solving fails when a catalogue file is missing or damaged.",
                    );
                }
            }
            Task::Running => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Checking the catalogue files…")
                            .small()
                            .color(MUTED),
                    );
                });
            }
            Task::Done(check) => {
                if self.checked_folder != self.gaia_folder {
                    return Some("The folder changed since the last test.");
                }
                let complete = check.verdict == "complete";
                let coverage = check
                    .magnitude
                    .map_or(String::new(), |(a, b)| format!(", G {a:.1} to {b:.1}"));
                let text = if complete {
                    format!(
                        "✔ Complete: {} files{coverage}, {} stars, checked in {:.1} s.",
                        check.files.len(),
                        thousands(check.sources),
                        check.seconds
                    )
                } else {
                    format!(
                        "✖ {}: {} files{coverage}.",
                        match check.verdict {
                            "incomplete" => "Incomplete",
                            "damaged" => "Damaged",
                            _ => "No Gaia catalogue here",
                        },
                        check.files.len()
                    )
                };
                ui.label(RichText::new(text).color(if complete { GOOD } else { WARN }));
                for line in check.problems.iter().chain(&check.notes).take(6) {
                    ui.label(RichText::new(format!("· {line}")).small().color(MUTED));
                }
            }
        }
        None
    }

    /// Where the plate solver searches.
    /// A finished Auto-solve becomes the field centre: coordinates mode with
    /// the position it found.
    fn apply_auto_solve(&mut self) {
        let Some(result) = self.blind.take_auto() else {
            return;
        };
        if let Ok(found) = &result {
            let entered = (format!("{:.5}", found.ra), format!("{:+.5}", found.dec));
            self.ra = entered.0.clone();
            self.dec = entered.1.clone();
            self.filled = Some(entered);
            self.field_mode = FIELD_FROM_COORDINATES;
            self.restored = None;
        }
        self.blind.last = Some(result);
    }

    /// Auto-solve: finds the field of the first frames with no coordinates
    /// and no name, through the blind-solving index.
    fn auto_solve_button(&mut self, ui: &mut egui::Ui) {
        let gaia = self.gaia_folder.clone();
        let has_index = self.blind.exists(&gaia);
        let has_frames = !self.blind.files.is_empty();
        let ready = has_index && has_frames && !self.blind.busy();
        let hint = if !has_frames {
            "Add light frames first."
        } else if !has_index {
            "Build the blind-solving index first (Blind-solving index, above)."
        } else if self.blind.busy() {
            "Busy."
        } else {
            "Finds the field of the first frames by itself, from their stars, when no coordinates or target name are known. Takes a few seconds."
        };
        if ui
            .add_enabled(ready, egui::Button::new("Auto-solve"))
            .on_hover_text(hint)
            .on_disabled_hover_text(hint)
            .clicked()
        {
            self.blind.start_auto(&gaia, ui.ctx());
        }
    }

    fn field_row(&mut self, ui: &mut egui::Ui, editable: bool, header_object: Option<String>) {
        let notice = self.centre_notice(header_object.as_deref());
        let suggestion = self.unconfirmed_guess().cloned();
        let gaia_folder = self.gaia_folder.clone();
        let auto_ready =
            self.blind.exists(&gaia_folder) && !self.blind.files.is_empty() && !self.blind.busy();
        setting_row(ui, "Field centre", FIELD_INFO, |ui| {
            if let Some(guess) = &suggestion {
                let message = Self::guess_text(guess);
                let primary = format!("Use {}", guess.matched);
                let mut fix = Fix::new(&message, &primary);
                fix.enabled = editable;
                if auto_ready {
                    fix.secondary = Some("Auto-solve instead");
                }
                if let Some(found) = fulgur_catalog::dso::find(&guess.matched) {
                    fix.result = Some((
                        "Centre:",
                        vec![fulgur_pipeline::sky::catalogue::position_text(
                            found.ra, found.dec,
                        )],
                    ));
                }
                match fix_card(ui, &fix) {
                    Choice::Primary => {
                        self.field_mode = FIELD_FROM_OBJECT;
                        self.object = guess.matched.clone();
                        self.restored = None;
                    }
                    Choice::Secondary => self.blind.start_auto(&gaia_folder, ui.ctx()),
                    Choice::None | Choice::Keep => {}
                }
            }
            if let Some(notice) = &notice {
                let by_name = self.field_mode == FIELD_FROM_OBJECT;
                let action = if by_name {
                    "Clear the saved name"
                } else {
                    "Clear the saved coordinates"
                };
                let mut fix = Fix::new(notice, action);
                fix.enabled = editable;
                fix.keep = Some("Keep this centre");
                fix.result = Some((
                    "Then:",
                    vec![
                        "target name from the headers".to_owned(),
                        "or Auto-solve".to_owned(),
                    ],
                ));
                match fix_card(ui, &fix) {
                    Choice::Primary => {
                        if by_name {
                            self.object.clear();
                        } else {
                            self.ra.clear();
                            self.dec.clear();
                            self.field_mode = FIELD_FROM_OBJECT;
                        }
                        self.restored = None;
                    }
                    Choice::Keep => {
                        if let Some(key) = self.centre_key() {
                            self.accepted.insert(key);
                        }
                    }
                    _ => {}
                }
            } else if let Some(key) = self.centre_key().filter(|k| {
                self.accepted.contains(k)
                    && self.centre_notice_raw(header_object.as_deref()).is_some()
            }) {
                if accepted_line(ui, "Accepted: the saved field centre is kept.") {
                    self.accepted.remove(&key);
                }
            }
            self.apply_auto_solve();
            self.blind.auto_note(ui);
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("sky_field_mode")
                        .selected_text(if self.field_mode == FIELD_FROM_OBJECT {
                            "From the target name"
                        } else {
                            "Coordinates"
                        })
                        .width(260.)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.field_mode,
                                FIELD_FROM_OBJECT,
                                "From the target name",
                            );
                            ui.selectable_value(
                                &mut self.field_mode,
                                FIELD_FROM_COORDINATES,
                                "Coordinates",
                            );
                        });
                    self.auto_solve_button(ui);
                });
                if self.field_mode == FIELD_FROM_OBJECT {
                    one_row_right_to_left(ui, |ui| {
                        let running = matches!(*self.lookup.lock().unwrap(), Task::Running);
                        if ui
                            .add_enabled(!running, egui::Button::new("Look up"))
                            .clicked()
                        {
                            self.start_lookup(ui.ctx(), header_object.clone());
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.object)
                                .hint_text(header_object.as_deref().map_or(
                                    "M 33, NGC 7000, LBN 437, Sh2-155…".to_string(),
                                    |o| format!("{o} (from the headers)"),
                                ))
                                .desired_width(ui.available_width()),
                        );
                    });
                    ui.checkbox(&mut self.online, "Look names up online (NED, SIMBAD)")
                        .on_hover_text("Only for names that are not built in.");
                } else {
                    ui.horizontal(|ui| {
                        ui.label("RA");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ra)
                                .hint_text("01:33:50.9 or 23.462")
                                .desired_width(150.),
                        );
                        ui.add_space(8.);
                        ui.label("Dec");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.dec)
                                .hint_text("+30:39:36 or 30.66")
                                .desired_width(150.),
                        );
                    });
                }
            });
            if self.field_mode == FIELD_FROM_OBJECT {
                match &*self.lookup.lock().unwrap() {
                    Task::Idle => {}
                    Task::Running => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Looking up…").small().color(MUTED));
                        });
                    }
                    Task::Done(Ok((ra, dec, source))) => {
                        ui.label(
                            RichText::new(format!(
                                "{} · {source}",
                                fulgur_pipeline::sky::catalogue::position_text(*ra, *dec)
                            ))
                            .small()
                            .color(GOOD),
                        );
                    }
                    Task::Done(Err(message)) => {
                        ui.label(RichText::new(message).small().color(WARN));
                    }
                }
            } else {
                let mut adopt = None;
                let mut keep: Option<String> = None;
                let mut undo: Option<String> = None;
                if let Some((file, (ra, dec))) = &self.header {
                    let ours = self.filled.as_ref() == Some(&(self.ra.clone(), self.dec.clone()));
                    let far = match (parse_ra(&self.ra), parse_dec(&self.dec)) {
                        (Some(r), Some(d)) => {
                            let dr = (r - ra + 180.).rem_euclid(360.) - 180.;
                            dr.abs() * d.to_radians().cos() > 0.5 || (d - dec).abs() > 0.5
                        }
                        _ => true,
                    };
                    if !ours && far {
                        let key = format!(
                            "header_position|{}|{}|{}",
                            file.file_name().unwrap_or_default().to_string_lossy(),
                            self.ra,
                            self.dec
                        );
                        if self.accepted.contains(&key) {
                            if accepted_line(
                                ui,
                                "Accepted: your coordinates differ from the header.",
                            ) {
                                undo = Some(key);
                            }
                        } else {
                            let position = (format!("{ra:.5}"), format!("{dec:+.5}"));
                            let message = format!(
                                "The header of {} points to {}, far from the centre entered here.",
                                file.file_name().unwrap_or_default().to_string_lossy(),
                                fulgur_pipeline::sky::catalogue::position_text(*ra, *dec)
                            );
                            let mut fix = Fix::new(&message, "Use the header position");
                            fix.enabled = editable;
                            fix.keep = Some("Keep my coordinates");
                            let entered = match (parse_ra(&self.ra), parse_dec(&self.dec)) {
                                (Some(r), Some(d)) => {
                                    fulgur_pipeline::sky::catalogue::position_text(r, d)
                                }
                                _ => "nothing valid entered".to_owned(),
                            };
                            fix.result = Some(("Replaces:", vec![entered]));
                            match fix_card(ui, &fix) {
                                Choice::Primary => adopt = Some(position),
                                Choice::Keep => keep = Some(key),
                                _ => {}
                            }
                        }
                    }
                    if ours {
                        ui.label(
                            RichText::new(format!(
                                "From the header of {}",
                                file.file_name().unwrap_or_default().to_string_lossy()
                            ))
                            .small()
                            .color(MUTED),
                        );
                    }
                }
                if let Some(position) = adopt {
                    (self.ra, self.dec) = position.clone();
                    self.filled = Some(position);
                }
                if let Some(key) = keep {
                    self.accepted.insert(key);
                }
                if let Some(key) = undo {
                    self.accepted.remove(&key);
                }
                match (parse_ra(&self.ra), parse_dec(&self.dec)) {
                    (Some(ra), Some(dec)) => {
                        ui.label(
                            RichText::new(fulgur_pipeline::sky::catalogue::position_text(ra, dec))
                                .small()
                                .color(GOOD),
                        );
                    }
                    _ if self.ra.trim().is_empty() && self.dec.trim().is_empty() => {}
                    _ => {
                        ui.label(
                            RichText::new("Not a valid position yet.")
                                .small()
                                .color(WARN),
                        );
                    }
                }
            }
        });
    }
}

fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}
