//! The Settings step (ADR-141): one column of panels with a label and a
//! control per row. Explanations sit behind an info marker; the page itself
//! keeps only warnings and live summaries such as the next stack's method.
use super::compact::{WARN, card, label_with_actions, one_row_right_to_left};
use super::notice::{Choice, Fix, fix_card};
use super::*;
use fulgur_core::output::OutputFormat;
use fulgur_pipeline::integrate::{Drizzle, Rejection, TrailMode};

const MUTED: Color32 = Color32::from_rgb(159, 176, 191);
const LIVE: Color32 = Color32::from_rgb(124, 196, 228);
/// Width of the label column.
const LABEL_WIDTH: f32 = 180.;
/// Narrower panels put each label above its control.
const TWO_COLUMNS: f32 = 520.;
/// The page reads top to bottom in one column of this width at most.
const PAGE_WIDTH: f32 = 900.;

/// A small circled "i" that holds the explanation: hovering shows it as a
/// tooltip, a click keeps it open until the next click outside or Escape.
/// It works in disabled panels too (a disabled widget reports no clicks, so
/// the click is read from the pointer). It is drawn, so it never depends on
/// a font holding the glyph.
pub(super) fn info(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let size = 15.;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let popup = response.id.with("info_popup");
    let pointer_on = ui.rect_contains_pointer(rect);
    let clicked = response.clicked()
        || (!ui.is_enabled() && pointer_on && ui.input(|i| i.pointer.primary_clicked()));
    if clicked {
        ui.memory_mut(|m| m.toggle_popup(popup));
    }
    let open = ui.memory(|m| m.is_popup_open(popup));
    let colour = if pointer_on || open {
        Color32::WHITE
    } else {
        MUTED
    };
    let painter = ui.painter();
    painter.circle_stroke(rect.center(), size * 0.45, egui::Stroke::new(1_f32, colour));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "i",
        egui::FontId::proportional(size * 0.72),
        colour,
    );
    egui::popup_below_widget(
        ui,
        popup,
        &response,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(320.);
            ui.set_max_width(420.);
            ui.label(text);
        },
    );
    if open {
        return response;
    }
    response
        .on_hover_cursor(egui::CursorIcon::Help)
        .on_hover_text(text)
        .on_disabled_hover_text(text)
}

/// One setting: its label (with an info marker when there is an explanation)
/// in the left column, the control and any live line in the right one.
pub(super) fn setting_row(
    ui: &mut egui::Ui,
    label: &str,
    explanation: &str,
    add: impl FnOnce(&mut egui::Ui),
) {
    let title = |ui: &mut egui::Ui| {
        ui.label(label);
        if !explanation.is_empty() {
            info(ui, explanation);
        }
    };
    if ui.available_width() >= TWO_COLUMNS {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(LABEL_WIDTH, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(LABEL_WIDTH);
                    title(ui);
                },
            );
            ui.vertical(add);
        });
    } else {
        ui.horizontal(title);
        add(ui);
    }
    ui.add_space(6.);
}

const REFERENCE_MODES: [(usize, &str, &str); 4] = [
    (
        0,
        "Auto (recommended)",
        "One night: the chronological median. Several nights: the frame with the best shared coverage.",
    ),
    (
        3,
        "Chronological median",
        "The middle frame of the accepted sequence.",
    ),
    (
        1,
        "A frame I choose",
        "Pick it in Frames with “Use as reference”.",
    ),
    (
        2,
        "External image · geometry only",
        "Uses another image’s pixel grid; science data still comes from your frames.",
    ),
];

const REFERENCE_INFO: &str = "The frame whose pixel grid every other frame is mapped onto.\n\n\
Auto: one night, the chronological median; several nights, the frame with the best shared coverage.\n\
Chronological median: the middle frame of the accepted sequence.\n\
A frame I choose: pick it in Frames with “Use as reference”.\n\
External image: uses another image’s pixel grid; science data still comes from your frames.";

const DISTORTION_INFO: &str = "Recommended. Also corrects lens distortion and small scale changes between nights, checked on stars kept aside. Each frame must pass whole-field accuracy checks; frames that fail are rejected. Takes longer.\n\n\
With plate solving it is always on: the map from the plate solutions corrects distortion by itself.";

const ACCURACY_INFO: &str = "How closely each frame must fit the reference to join the stack, in pixels of the master. Looser limits keep more frames, but each looser frame can widen stars.\n\n\
Applies to every whole-field check: frames, cameras joining the reference and the tiles of partial frames. The report records the limits used.";

const STAR_SHARE_INFO: &str = "Of the stars that the weaker of frame and reference shows where their fields overlap. Narrowband frames (Ha, OIII, SII) show fewer stars than a broadband reference; the share follows the weaker side, whichever it is.";

const REJECTION_METHODS: [(Rejection, &str); 5] = [
    (
        Rejection::Auto,
        "Recommended. Median below 6 frames, winsorized sigma clipping below 25, ESD from 25 frames.",
    ),
    (
        Rejection::Esd,
        "Generalized ESD test (significance 0.05, at most 30% per pixel). Lowest master noise on all tested sets of 27–856 frames; about 40–50% more stacking time than winsorized.",
    ),
    (
        Rejection::Winsorized,
        "Winsorized sigma clipping, low 3.0 / high 2.5 sigma. Fast; for 6–25 frames. Rejects about 3% of samples, so slightly more noise on large sets. Reproduces masters made before this choice existed.",
    ),
    (
        Rejection::LinearFit,
        "Linear fit clipping, low 5 / high 4 sigma. Tolerates sky-level differences between frames; the slowest method.",
    ),
    (
        Rejection::Median,
        "Median of all frames, no weights. Robust for very few frames, but about 20% more noise.",
    ),
];

const DRIZZLE_INFO: &str = "Integrates onto a grid with half-size pixels: each input pixel is shrunk to a square drop and its overlap with the output pixels weights it. With many dithered frames this recovers detail lost to large pixels, and Bayer drizzle builds colour from the raw sensor samples without demosaicing. It helps most when stars are under about 2 pixels across (2 × HFR); with wider stars the master is larger and noisier per pixel but hardly sharper. The master and its maps have 4× more pixels.";

const TRAIL_INFO: &str = "Generalized ESD only. ESD judges every sample on its own, so an asteroid or satellite loses only its bright core and its faint edges leave a soft trace in the master. Around each sample ESD rejects above the rest, the trail mask also rejects samples of the same frame within the radius: Hysteresis only those raised above the pixel's mean by the threshold, Grow all of them. Stars and compact bright structure are left alone. The high-rejection map shows the wider trails. Pixel rejection then runs on the CPU, and a stack takes a little longer.";

/// The next stack of `frames` frames uses ESD (the trail mask applies).
fn applied_esd(rejection: Rejection, frames: usize) -> bool {
    if frames == 0 {
        matches!(rejection, Rejection::Esd | Rejection::Auto)
    } else {
        rejection.resolve(frames) == Rejection::Esd
    }
}

const REJECTION_INFO: &str = "How the registered frames are combined pixel by pixel. Outliers (satellites, planes, cosmic rays, hot pixels) are removed first; the remaining samples are averaged with each frame's quality weight. The method used is shown in Results and recorded in the report.";

const NORMALIZATION_INFO: &str = "Before rejection every frame is scaled by the median flux of its matched stars, one scale for all colours, so clouds, extinction and exposure differences cancel. Its sky is then matched to the reference frame in each colour channel. Results and the report list the values used.";

const PARTIAL_INFO: &str = "Experimental. A refused frame still joins the processing tiles around which its stars pass the same accuracy limits. Steps can show at tile edges, and the master depends on tile size; Coverage shows where these frames count.\n\n\
Needs geometric distortion correction and the Stars method.";

const FORMAT_INFO: &str = "File format of the master and the diagnostic maps. Both hold the same 32-bit float samples, bounded 0..1, and the same keywords and history.

FITS: the format most astronomy programs read.
XISF: the native format of PixInsight.

Choosing a format sets the extension of the master file name; typing .fits or .xisf there chooses it too.";

fn format_label(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Fits => "FITS (.fits)",
        OutputFormat::Xisf => "XISF (.xisf)",
    }
}

const TILES_INFO: &str = "Automatic uses 1024 × 256 on frames wider than 4000 px and 512 × 256 on others, and smaller tiles when memory is short. Wider tiles read less from wide sensors; taller tiles only use more memory. The master does not change, except with partial frames.";

fn threads_label(count: usize) -> String {
    if count == 0 {
        "Automatic (up to 24)".into()
    } else {
        format!("{count} threads")
    }
}

impl Workspace {
    pub(super) fn settings_view(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.set_max_width(PAGE_WIDTH);
            let editable = !self.busy() || self.waiting;
            let busy = self.busy();
            card(ui, |ui| self.registration_panel(ui, editable, busy));
            ui.add_space(10.);
            card(ui, |ui| self.stacking_panel(ui, editable, busy));
            ui.add_space(10.);
            card(ui, |ui| self.output_panel(ui, busy));
            ui.add_space(10.);
            card(ui, |ui| self.advanced_panel(ui, busy));
            ui.add_space(6.);
            ui.label(
                RichText::new("Settings and light sets are remembered between sessions.")
                    .small()
                    .color(MUTED),
            );
        });
    }

    fn registration_panel(&mut self, ui: &mut egui::Ui, editable: bool, busy: bool) {
        ui.heading("Registration");
        ui.add_space(6.);
        self.reference_row(ui, editable);
        let header_object = self.inspection.as_ref().and_then(|s| s.common_object());
        let mixed = self.mixed_light_sets();
        self.sky.method_row(ui, !busy, mixed.as_deref());
        if !busy {
            self.update_field_centre();
        }
        self.sky.plate_solving(ui, !busy, header_object);
        let sky = self.sky.enabled;
        let correction = &mut self.stack.geometric_correction;
        setting_row(ui, "Geometric distortion", DISTORTION_INFO, |ui| {
            if sky {
                ui.add_enabled_ui(false, |ui| {
                    ui.checkbox(
                        &mut true,
                        "Correct geometric distortion · always on with plate solving",
                    );
                });
            } else {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.checkbox(correction, "Correct geometric distortion (recommended)");
                });
            }
        });
        accuracy_section(ui, &mut self.stack.accuracy, !busy, sky);
    }

    fn reference_row(&mut self, ui: &mut egui::Ui, editable: bool) {
        let (mode, reference, view) = (
            &mut self.stack.reference_mode,
            &mut self.stack.reference,
            &mut self.view,
        );
        setting_row(ui, "Reference frame", REFERENCE_INFO, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                let selected = REFERENCE_MODES
                    .iter()
                    .find(|m| m.0 == *mode)
                    .map_or(REFERENCE_MODES[0].1, |m| m.1);
                egui::ComboBox::from_id_salt("reference_mode")
                    .selected_text(selected)
                    .width(260.)
                    .show_ui(ui, |ui| {
                        for (value, label, hint) in REFERENCE_MODES {
                            ui.selectable_value(mode, value, label).on_hover_text(hint);
                        }
                    });
                if *mode == 1 {
                    if reference.trim().is_empty() {
                        let mut fix = Fix::new(
                            "No reference frame is chosen yet. A run cannot start without one.",
                            "Choose in Frames",
                        );
                        fix.secondary = Some("Use Auto instead");
                        fix.note = Some("Right-click a frame, “Use as reference”.");
                        match fix_card(ui, &fix) {
                            Choice::Primary => *view = View::Frames,
                            Choice::Secondary => *mode = 0,
                            Choice::None | Choice::Keep => {}
                        }
                    } else {
                        let name = Path::new(reference.as_str())
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let hover = display_path(Path::new(reference.as_str()));
                        label_with_actions(ui, format!("Reference: {name}"), Some(hover), |ui| {
                            if ui
                                .small_button("✕")
                                .on_hover_text("Clear reference")
                                .clicked()
                            {
                                reference.clear();
                            }
                        });
                    }
                }
                if *mode == 2 {
                    let text = if reference.trim().is_empty() {
                        RichText::new("No file chosen").color(WARN)
                    } else {
                        RichText::new(
                            Path::new(reference.as_str())
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy(),
                        )
                    };
                    label_with_actions(ui, text, None, |ui| {
                        if ui.button("Choose FITS or XISF…").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("FITS or XISF", &fulgur_core::fits::INPUT_EXTENSIONS)
                                .pick_file()
                            {
                                *reference = path.to_string_lossy().into_owned();
                            }
                        }
                    });
                }
            });
        });
    }

    fn stacking_panel(&mut self, ui: &mut egui::Ui, editable: bool, busy: bool) {
        let (frames, measured) = (self.next_stack_frames(), self.measured());
        ui.heading("Stacking");
        ui.add_space(6.);
        let rejection = &mut self.stack.rejection;
        setting_row(ui, "Outlier rejection", REJECTION_INFO, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                let hint = REJECTION_METHODS
                    .iter()
                    .find(|m| m.0 == *rejection)
                    .map_or("", |m| m.1);
                egui::ComboBox::from_id_salt("rejection")
                    .selected_text(rejection.label())
                    .width(260.)
                    .show_ui(ui, |ui| {
                        for (method, hint) in REJECTION_METHODS {
                            ui.selectable_value(rejection, method, method.label())
                                .on_hover_text(hint);
                        }
                    })
                    .response
                    .on_hover_text(hint);
            });
            let applied = rejection.resolve(frames);
            let text = if frames == 0 {
                "Add light frames to see which method the next stack uses.".to_owned()
            } else {
                format!(
                    "Next stack: {frames} frames{} → {}.",
                    if measured && *rejection == Rejection::Auto {
                        " in use"
                    } else {
                        ""
                    },
                    applied.label()
                )
            };
            ui.label(RichText::new(text).color(LIVE));
        });
        let esd = applied_esd(*rejection, frames);
        let drizzle = &mut self.stack.drizzle;
        setting_row(ui, "Drizzle", DRIZZLE_INFO, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                let mut on = drizzle.is_some();
                if ui
                    .checkbox(&mut on, "Drizzle 2× (finer output grid)")
                    .changed()
                {
                    *drizzle = on.then(Drizzle::default);
                }
                if let Some(d) = drizzle {
                    ui.horizontal(|ui| {
                        ui.label("Drop");
                        ui.add(
                            egui::DragValue::new(&mut d.pixfrac)
                                .range(0.3..=1.)
                                .speed(0.01)
                                .suffix(" px"),
                        );
                        ui.checkbox(&mut d.bayer, "Bayer drizzle (colour from raw CFA)");
                    });
                }
            });
            if drizzle.is_some() {
                ui.label(
                    RichText::new(
                        "Master 4× more pixels; runs on the CPU, the trail mask is not used.",
                    )
                    .color(MUTED),
                );
            }
        });
        let trail = &mut self.stack.trail;
        setting_row(ui, "Trail mask", TRAIL_INFO, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                egui::ComboBox::from_id_salt("trail_mask")
                    .selected_text(trail.mode.label())
                    .width(260.)
                    .show_ui(ui, |ui| {
                        for mode in TrailMode::ALL {
                            ui.selectable_value(&mut trail.mode, mode, mode.label());
                        }
                    });
                if trail.mode != TrailMode::Off {
                    ui.horizontal(|ui| {
                        ui.label("Radius");
                        ui.add(
                            egui::DragValue::new(&mut trail.radius_px)
                                .range(1.0..=16.)
                                .speed(0.1)
                                .suffix(" px"),
                        );
                        if trail.mode == TrailMode::Hysteresis {
                            ui.label("Threshold");
                            ui.add(
                                egui::DragValue::new(&mut trail.sigma)
                                    .range(0.5..=5.)
                                    .speed(0.05)
                                    .suffix(" σ"),
                            );
                        }
                    });
                }
            });
            if trail.mode != TrailMode::Off && !esd {
                ui.label(
                    RichText::new(
                        "Used only with Generalized ESD; the next stack does not use it.",
                    )
                    .color(MUTED),
                );
            }
        });
        setting_row(ui, "Normalization", NORMALIZATION_INFO, |ui| {
            ui.label(RichText::new("Additive with scaling · always on").color(MUTED));
        });
        let usable = self.stack.geometric_correction && !self.sky.enabled;
        let why = if self.sky.enabled {
            "Not used with plate solving."
        } else {
            "Needs geometric distortion correction (Registration)."
        };
        let partial = &mut self.stack.partial_frames;
        setting_row(ui, "Refused frames", PARTIAL_INFO, |ui| {
            ui.add_enabled_ui(!busy && usable, |ui| {
                ui.checkbox(
                    partial,
                    "Keep accurate parts of refused frames · experimental",
                )
                .on_disabled_hover_text(why);
            });
        });
    }

    fn output_panel(&mut self, ui: &mut egui::Ui, busy: bool) {
        let split_preview = self.split_preview();
        ui.heading("Output");
        ui.add_space(6.);
        let (folder, filename) = (&mut self.stack.folder, &mut self.stack.filename);
        let master_split = &mut self.stack.master_split;
        let (save_coverage, save_high_rejection) = (
            &mut self.stack.save_coverage,
            &mut self.stack.save_high_rejection,
        );
        ui.add_enabled_ui(!busy, |ui| {
            setting_row(ui, "Output folder", "", |ui| {
                one_row_right_to_left(ui, |ui| {
                    if ui.button("Browse…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            *folder = path.to_string_lossy().into_owned();
                        }
                    }
                    ui.add(egui::TextEdit::singleline(folder).desired_width(ui.available_width()));
                });
            });
            setting_row(
                ui,
                "Master file name",
                "A JSON report is saved next to the master. Existing names get _v2, _v3 …",
                |ui| {
                    ui.add(egui::TextEdit::singleline(filename).desired_width(ui.available_width()));
                },
            );
            setting_row(ui, "Output format", FORMAT_INFO, |ui| {
                // The file name's extension holds the choice (ADR-142).
                let current = OutputFormat::of(Path::new(filename.as_str())).unwrap_or_default();
                let mut chosen = current;
                egui::ComboBox::from_id_salt("output_format")
                    .selected_text(format_label(current))
                    .width(260.)
                    .show_ui(ui, |ui| {
                        for format in OutputFormat::ALL {
                            ui.selectable_value(&mut chosen, format, format_label(format));
                        }
                    });
                if chosen != current {
                    *filename = Path::new(filename.as_str())
                        .with_extension(chosen.extension())
                        .to_string_lossy()
                        .into_owned();
                }
            });
            setting_row(
                ui,
                "Masters",
                "One combined master, one per filter or one per light set. The line below lists the masters the next stack makes.",
                |ui| {
                    egui::ComboBox::from_id_salt("master_split")
                        .selected_text(master_split.label())
                        .width(260.)
                        .show_ui(ui, |ui| {
                            for split in fulgur_pipeline::master_groups::MasterSplit::ALL {
                                ui.selectable_value(master_split, split, split.label());
                            }
                        });
                    ui.label(RichText::new(&split_preview).small().color(MUTED));
                },
            );
            setting_row(
                ui,
                "Diagnostic maps",
                "Coverage and high-rejection maps as files next to the master, in its output format. Both are always shown in Results.",
                |ui| {
                    let mut maps = *save_coverage && *save_high_rejection;
                    let format = OutputFormat::of(Path::new(filename.as_str())).unwrap_or_default();
                    if ui.checkbox(&mut maps, format!("Save as {}", format.label())).changed() {
                        *save_coverage = maps;
                        *save_high_rejection = maps;
                    }
                },
            );
        });
    }

    fn advanced_panel(&mut self, ui: &mut egui::Ui, busy: bool) {
        let summary = format!(
            "{} · {} · {} MiB · tiles {}",
            if self.stack.gpu { "GPU" } else { "CPU" },
            threads_label(self.stack.cpu_threads),
            self.stack.memory,
            tile_label(self.stack.tile)
        );
        egui::CollapsingHeader::new(RichText::new("Advanced").heading())
            .id_salt("advanced_settings")
            .default_open(false)
            .show(ui, |ui| {
                ui.add_space(4.);
                ui.add_enabled_ui(!busy, |ui| {
                    setting_row(
                        ui,
                        "Processing engine",
                        "GPU: pixel rejection and averaging run on the graphics card while the CPU prepares the next tile; calibration, debayering and registration stay on the CPU.",
                        |ui| {
                            let gpu_label = if cfg!(feature = "gpu") {
                                "GPU"
                            } else {
                                "GPU (not in this build)"
                            };
                            egui::ComboBox::from_id_salt("processing_engine")
                                .selected_text(if self.stack.gpu { gpu_label } else { "CPU" })
                                .width(260.)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.stack.gpu, false, "CPU");
                                    ui.selectable_value(&mut self.stack.gpu, true, gpu_label);
                                });
                        },
                    );
                    setting_row(ui, "CPU threads", "", |ui| {
                        egui::ComboBox::from_id_salt("cpu_threads")
                            .selected_text(threads_label(self.stack.cpu_threads))
                            .show_ui(ui, |ui| {
                                for count in [0, 12, 16, 24] {
                                    ui.selectable_value(&mut self.stack.cpu_threads, count, threads_label(count));
                                }
                            });
                    });
                    setting_row(ui, "Memory ceiling (MiB)", "", |ui| {
                        ui.add(egui::DragValue::new(&mut self.stack.memory).range(1024..=12288));
                    });
                    setting_row(ui, "Output tiles", TILES_INFO, |ui| {
                        egui::ComboBox::from_id_salt("tiles")
                            .selected_text(tile_label(self.stack.tile))
                            .show_ui(ui, |ui| {
                                for shape in TILE_SHAPES {
                                    ui.selectable_value(&mut self.stack.tile, shape, tile_label(shape));
                                }
                            });
                    });
                    setting_row(ui, "Unused frames", "Diagnostics only; they never enter the stack.", |ui| {
                        ui.checkbox(&mut self.stack.diagnose_excluded, "Also register them");
                    });
                });
            });
        ui.label(RichText::new(summary).small().color(MUTED));
    }
}

/// How closely each frame must fit the reference to join the stack, folded
/// under a one-line summary. The presets set the RMS limit, the largest
/// single error and the share of stars together; editing one value leaves
/// them as Custom.
fn accuracy_section(ui: &mut egui::Ui, accuracy: &mut super::Accuracy, editable: bool, sky: bool) {
    let current = accuracy.preset();
    let name = current.map_or("Custom", |index| super::Accuracy::PRESETS[index].0);
    let summary = format!(
        "{name} · {} px RMS · {} % of stars · at least {} stars",
        accuracy.rms_px, accuracy.star_percent, accuracy.min_stars
    );
    let rms_info = format!(
        "RMS of all matched stars, in pixels of the master. No single star farther than {} px; the same limit applies in each corner of the field.",
        accuracy.maximum_px()
    );
    let min_info = format!(
        "A frame needs at least {} matched stars: the larger of this minimum and {} % of the weaker side. Fewer stars measure accuracy less reliably. Below 30 a frame is aligned without distortion correction.",
        accuracy.min_stars, accuracy.star_percent
    );
    egui::CollapsingHeader::new(RichText::new(format!("Registration accuracy · {summary}")))
        .id_salt("registration_accuracy")
        .default_open(false)
        .show(ui, |ui| {
                    if sky {
                        ui.label(
                            RichText::new("Plate solving checks its maps with its own fixed rule; these limits apply to frames that fall back to Stars.")
                                .small()
                                .color(MUTED),
                        );
                        ui.add_space(4.);
                    }
                    ui.add_enabled_ui(editable, |ui| {
                        setting_row(ui, "Balance", ACCURACY_INFO, |ui| {
                            egui::ComboBox::from_id_salt("accuracy_balance")
                                .selected_text(name)
                                .width(260.)
                                .show_ui(ui, |ui| {
                                    for (index, (label, hint, values)) in
                                        super::Accuracy::PRESETS.iter().enumerate()
                                    {
                                        if ui
                                            .selectable_label(current == Some(index), *label)
                                            .on_hover_text(*hint)
                                            .clicked()
                                        {
                                            *accuracy = *values;
                                        }
                                    }
                                });
                        });
                        setting_row(ui, "Accuracy (RMS)", &rms_info, |ui| {
                            egui::ComboBox::from_id_salt("accuracy_rms")
                                .selected_text(format!("{} px", accuracy.rms_px))
                                .width(260.)
                                .show_ui(ui, |ui| {
                                    for rms in super::Accuracy::RMS_CHOICES {
                                        ui.selectable_value(
                                            &mut accuracy.rms_px,
                                            rms,
                                            format!("{rms} px"),
                                        );
                                    }
                                });
                        });
                        setting_row(ui, "Stars that must match", STAR_SHARE_INFO, |ui| {
                            ui.add(
                                egui::Slider::new(&mut accuracy.star_percent, super::Accuracy::STAR_PERCENT)
                                    .step_by(5.)
                                    .suffix(" %"),
                            );
                        });
                        setting_row(ui, "Minimum matched stars", &min_info, |ui| {
                            egui::ComboBox::from_id_salt("accuracy_min_stars")
                                .selected_text(accuracy.min_stars.to_string())
                                .width(260.)
                                .show_ui(ui, |ui| {
                                    for stars in super::Accuracy::MIN_STAR_CHOICES {
                                        ui.selectable_value(
                                            &mut accuracy.min_stars,
                                            stars,
                                            stars.to_string(),
                                        );
                                    }
                                });
                        });
                    });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame with an info marker; returns its rect and whether its
    /// explanation is held open.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, enabled: bool) -> (egui::Rect, bool) {
        let mut out = (egui::Rect::NOTHING, false);
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800., 600.),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    let response = info(ui, "Explanation");
                    let open = ui.memory(|m| m.is_popup_open(response.id.with("info_popup")));
                    out = (response.rect, open);
                });
            });
        });
        out
    }

    fn click(ctx: &egui::Context, pos: egui::Pos2, enabled: bool) {
        frame(ctx, vec![egui::Event::PointerMoved(pos)], enabled);
        for pressed in [true, false] {
            let event = egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            };
            frame(ctx, vec![event], enabled);
        }
    }

    #[test]
    fn a_click_on_the_info_marker_keeps_its_explanation_open_in_enabled_and_disabled_panels() {
        for enabled in [true, false] {
            let ctx = egui::Context::default();
            let (rect, open) = frame(&ctx, vec![], enabled);
            assert!(!open);
            click(&ctx, rect.center(), enabled);
            assert!(
                frame(&ctx, vec![], enabled).1,
                "opened by a click, enabled {enabled}"
            );
            click(&ctx, egui::pos2(700., 500.), enabled);
            assert!(
                !frame(&ctx, vec![], enabled).1,
                "closed by a click outside, enabled {enabled}"
            );
        }
    }
}
