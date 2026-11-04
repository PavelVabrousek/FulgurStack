//! Sky registration settings (plan phase 4, ADR-138): the method switch, the
//! folder of the local Gaia catalogue with a test of the installation, and
//! the field centre. Checks and look-ups run on their own threads.
use super::compact::{GOOD, WARN, one_row_right_to_left};
use super::settings_page::setting_row;
use super::*;
use fulgur_catalog::sky::{SkyChoice, parse_dec, parse_ra};
use fulgur_catalog::xpsd::InstallationCheck;
use std::sync::Mutex;

const MUTED: Color32 = Color32::from_rgb(159, 176, 191);
const SUBPANEL: Color32 = Color32::from_rgb(15, 36, 56);
const SUBPANEL_LINE: Color32 = Color32::from_rgb(36, 60, 82);

const STARS_INFO: &str = "Matches the stars of each frame with the reference frame. One camera and one optical train at a time; other cameras join through their own anchor.";
const SKY_INFO: &str = "Plate-solves every frame against the Gaia DR3 catalogue and registers it through the sky, whatever camera, optics, filter or orientation took it. Frames it cannot solve fall back to Stars. Distortion is always corrected.";
const METHOD_INFO: &str = "Stars (recommended): matches the stars of each frame with the reference frame. One camera and one optical train at a time; other cameras join through their own anchor.\n\n\
Plate solving (experimental): solves every frame against the Gaia DR3 catalogue and registers it through the sky, whatever camera, optics, filter or orientation took it. Frames it cannot solve fall back to Stars. Distortion is always corrected.";
const GAIA_INFO: &str = "The folder of the PixInsight Gaia DR3 files (gdr3-…xpsd). The catalogue is read from disk; nothing is copied. Test reads every file and checks that the installation is complete.\n\n\
Stars the folder does not hold can be downloaded from the ESA or Heidelberg (GAVO) Gaia archives; they are kept next to the settings for later runs.";
const FIELD_INFO: &str = "The plate solver searches around this position.\n\n\
From the target name: left empty, the OBJECT name most frames carry. Messier, NGC, IC, LBN, Sharpless, Barnard and common names are built in.\n\n\
Coordinates: RA in hours (h:m:s) or degrees, Dec in degrees.";

/// Field centre from the object name or from coordinates.
pub(super) const FIELD_FROM_OBJECT: usize = 0;
pub(super) const FIELD_FROM_COORDINATES: usize = 1;

#[derive(Default)]
enum Task<T> {
    #[default]
    Idle,
    Running,
    Done(T),
}

/// A result shared with the thread that computes it.
type Shared<T> = Arc<Mutex<Task<T>>>;

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
    pub(super) fn method_row(&mut self, ui: &mut egui::Ui, editable: bool) {
        setting_row(ui, "Method", METHOD_INFO, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.enabled, false, "Stars (recommended)")
                        .on_hover_text(STARS_INFO);
                    ui.selectable_value(&mut self.enabled, true, "Plate solving · experimental")
                        .on_hover_text(SKY_INFO);
                });
            });
        });
    }

    /// The Gaia catalogue and the field centre, under the method while plate
    /// solving is chosen.
    pub(super) fn plate_solving(
        &mut self,
        ui: &mut egui::Ui,
        editable: bool,
        header_object: Option<String>,
    ) {
        if !self.enabled {
            return;
        }
        egui::Frame::new()
            .fill(SUBPANEL)
            .stroke(egui::Stroke::new(1_f32, SUBPANEL_LINE))
            .corner_radius(6.)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.catalogue_row(ui, editable);
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
            self.check_status(ui);
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

    fn check_status(&mut self, ui: &mut egui::Ui) {
        let state = self.check.lock().unwrap();
        match &*state {
            Task::Idle => {
                if !self.gaia_folder.trim().is_empty() {
                    ui.label(
                        RichText::new("Not tested yet: press Test.")
                            .small()
                            .color(WARN),
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
                    ui.label(
                        RichText::new("Folder changed: press Test.")
                            .small()
                            .color(WARN),
                    );
                    return;
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
    }

    /// Where the plate solver searches.
    fn field_row(&mut self, ui: &mut egui::Ui, editable: bool, header_object: Option<String>) {
        setting_row(ui, "Field centre", FIELD_INFO, |ui| {
            ui.add_enabled_ui(editable, |ui| {
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
                            RichText::new(format!("RA {ra:.4}° · Dec {dec:+.4}° · {source}"))
                                .small()
                                .color(GOOD),
                        );
                    }
                    Task::Done(Err(message)) => {
                        ui.label(RichText::new(message).small().color(WARN));
                    }
                }
            } else {
                match (parse_ra(&self.ra), parse_dec(&self.dec)) {
                    (Some(ra), Some(dec)) => {
                        ui.label(
                            RichText::new(format!("RA {ra:.4}° · Dec {dec:+.4}°"))
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
