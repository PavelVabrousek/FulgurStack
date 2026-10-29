//! Workflow chrome (top bar, status bar) and the Settings, Processing and
//! Results steps. All actions use the existing worker and immutable results.
use super::*;
use egui::text::LayoutJob;

const ACCENT: Color32 = Color32::from_rgb(14, 106, 147);
const ACCENT_LINE: Color32 = Color32::from_rgb(43, 110, 145);
const SELECTED: Color32 = Color32::from_rgb(18, 50, 74);
const CARD: Color32 = Color32::from_rgb(11, 28, 45);
const CARD_LINE: Color32 = Color32::from_rgb(29, 48, 65);
const MUTED: Color32 = Color32::from_rgb(159, 176, 191);
pub(super) const WARN: Color32 = Color32::from_rgb(232, 162, 106);
pub(super) const GOOD: Color32 = Color32::from_rgb(65, 170, 153);
const STOP_RED: Color32 = Color32::from_rgb(156, 43, 54);

pub(super) fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(egui::Stroke::new(1_f32, CARD_LINE))
        .corner_radius(8.)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}

fn step_text(number: &str, label: &str, meta: &str, meta_color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = egui::FontId::proportional(13.);
    job.append(
        &format!("{number}  "),
        0.,
        egui::TextFormat::simple(font.clone(), Color32::from_rgb(124, 196, 228)),
    );
    job.append(
        label,
        0.,
        egui::TextFormat::simple(font.clone(), Color32::from_rgb(227, 234, 240)),
    );
    if !meta.is_empty() {
        job.append(
            &format!("   {meta}"),
            0.,
            egui::TextFormat::simple(font, meta_color),
        );
    }
    job
}

impl Workspace {
    fn run_percent(&self) -> Option<f32> {
        let progress = self.last_progress.as_ref()?;
        if !self.busy() || self.waiting {
            return None;
        }
        let phases = if self.run_kind == Some(RunKind::Measure) {
            1.
        } else {
            4.
        };
        let index = job::PHASES
            .iter()
            .position(|&p| p == progress.phase)
            .unwrap_or(0) as f32;
        let within = if progress.total > 0 {
            progress.completed as f32 / progress.total as f32
        } else {
            0.
        };
        Some(((index + within) / phases * 100.).clamp(0., 100.))
    }

    pub(super) fn top_bar(&mut self, ctx: &egui::Context) {
        let mut primary_rect = None;
        egui::TopBottomPanel::top("workflow_bar")
            .exact_height(54.)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("FulgurStack").strong().size(15.));
                    ui.add_space(10.);
                    let total: usize = self
                        .light_sets
                        .iter()
                        .filter_map(|s| s.summary.as_ref().map(|m| m.files))
                        .sum();
                    let frames_meta = if self.light_sets.is_empty() {
                        String::new()
                    } else if self.measured() {
                        let used = self.rows.iter().filter(|r| r.accepted).count();
                        format!("{used} / {} · {} sets", self.rows.len(), self.light_sets.len())
                    } else {
                        format!("{total} · {} sets", self.light_sets.len())
                    };
                    let reference = ["Auto", "Own frame", "External", "Median"][self.reference_mode];
                    let (processing_meta, processing_color) = if let Some(p) = self.run_percent() {
                        (format!("{p:.0} %"), Color32::from_rgb(124, 196, 228))
                    } else if self.waiting {
                        ("review".into(), WARN)
                    } else {
                        match self.last_run {
                            Some(LastRun::Done) => (
                                self.result
                                    .as_ref()
                                    .map(|r| clock(r.wall_seconds as u64))
                                    .unwrap_or_default(),
                                MUTED,
                            ),
                            Some(LastRun::Stopped) => ("stopped".into(), WARN),
                            _ => (String::new(), MUTED),
                        }
                    };
                    let steps = [
                        (View::Frames, "1", "Frames", frames_meta, MUTED),
                        (
                            View::Settings,
                            "2",
                            "Settings",
                            format!("{reference} · {}", if self.gpu { "GPU" } else { "CPU" }),
                            MUTED,
                        ),
                        (View::Processing, "3", "Processing", processing_meta, processing_color),
                        (
                            View::Results,
                            "4",
                            "Results",
                            if self.result.is_some() { "ready".into() } else { String::new() },
                            MUTED,
                        ),
                    ];
                    for (view, number, label, meta, color) in steps {
                        let on = self.view == view;
                        let button = egui::Button::new(step_text(number, label, &meta, color))
                            .fill(if on { SELECTED } else { Color32::TRANSPARENT })
                            .stroke(egui::Stroke::new(1_f32,
                                if on { ACCENT_LINE } else { Color32::TRANSPARENT },
                            ))
                            .min_size(egui::vec2(0., 32.));
                        if ui.add(button).clicked() {
                            self.view = view;
                            self.menu = None;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.busy() && !self.run_complete && self.finished_elapsed.is_none() {
                            let stopping = self
                                .worker
                                .as_ref()
                                .is_some_and(|w| w.cancel.load(Ordering::Relaxed));
                            if ui
                                .add_enabled(
                                    !stopping,
                                    egui::Button::new(
                                        RichText::new(if stopping { "Stopping…" } else { "STOP" })
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(STOP_RED)
                                    .min_size(egui::vec2(190., 34.)),
                                )
                                .on_hover_text(
                                    "Stop cooperatively and wait for worker cleanup. Published files remain.",
                                )
                                .clicked()
                            {
                                self.cancel_run();
                            }
                            if self.waiting && ui
                                .add(egui::Button::new("Continue stacking").min_size(egui::vec2(140., 34.)))
                                .clicked()
                            {
                                self.continue_run();
                            }
                            return;
                        }
                        let blocked = self.blocker();
                        let measured = self.measured();
                        let used = self.rows.iter().filter(|r| r.accepted).count();
                        let label = if measured {
                            format!("Stack {used} frames")
                        } else {
                            "Measure quality".into()
                        };
                        let has_menu = !self.light_sets.is_empty();
                        let chevron = ui
                            .add_enabled(
                                has_menu && blocked.is_none(),
                                egui::Button::new(RichText::new("▼").size(10.).color(Color32::WHITE))
                                    .fill(ACCENT)
                                    .min_size(egui::vec2(30., 34.)),
                            )
                            .on_hover_text(if measured {
                                "Measure quality again"
                            } else {
                                "Stack directly without reviewing quality"
                            });
                        if chevron.clicked() {
                            self.menu = if self.menu == Some(Menu::Action) {
                                None
                            } else {
                                Some(Menu::Action)
                            };
                        }
                        let primary = ui
                            .add_enabled(
                                blocked.is_none() && (!measured || used > 0),
                                egui::Button::new(
                                    RichText::new(label).strong().color(Color32::WHITE),
                                )
                                .fill(ACCENT)
                                .min_size(egui::vec2(160., 34.)),
                            )
                            .on_disabled_hover_text(blocked.unwrap_or_default());
                        if primary.clicked() {
                            if measured {
                                self.request_stack();
                            } else {
                                self.start(true);
                            }
                        }
                        primary_rect = Some(primary.rect.union(chevron.rect));
                    });
                });
            });
        if let (Some(menu), Some(anchor)) = (self.menu, primary_rect) {
            self.action_popup(ctx, menu, anchor);
        }
    }

    fn action_popup(&mut self, ctx: &egui::Context, menu: Menu, anchor: egui::Rect) {
        let width = if menu == Menu::ConfirmMixed {
            340.
        } else {
            270.
        };
        let area = egui::Area::new(egui::Id::new("action_popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(anchor.right() - width, anchor.bottom() + 6.))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .stroke(egui::Stroke::new(1_f32,
                        if menu == Menu::ConfirmMixed { WARN } else { CARD_LINE },
                    ))
                    .show(ui, |ui| {
                        ui.set_width(width - 16.);
                        match menu {
                            Menu::Action => {
                                let (label, hint) = if self.measured() {
                                    (
                                        "Measure quality again",
                                        "Re-inspect all included light sets; manual Use choices are cleared.",
                                    )
                                } else {
                                    (
                                        "Stack directly",
                                        "Measure and select frames automatically, without review.",
                                    )
                                };
                                if ui.button(RichText::new(label).strong()).clicked() {
                                    self.menu = None;
                                    if self.measured() {
                                        self.start(true);
                                    } else {
                                        self.request_stack();
                                    }
                                }
                                ui.label(RichText::new(hint).small().color(MUTED));
                            }
                            Menu::ConfirmMixed => {
                                ui.label(RichText::new("This stack mixes filters").strong().color(WARN));
                                ui.label(self.included_filters().join(" + "));
                                ui.add(
                                    egui::Label::new(
                                        RichText::new("Background level, colour balance and star profiles differ between filters. The result is allowed but needs a careful check.")
                                            .color(MUTED),
                                    )
                                    .wrap(),
                                );
                                ui.horizontal(|ui| {
                                    if ui.button("Review sets").clicked() {
                                        self.menu = None;
                                        self.view = View::Frames;
                                        self.selection = Selection::All;
                                    }
                                    if ui
                                        .add(egui::Button::new(RichText::new("Stack anyway").color(Color32::WHITE)).fill(ACCENT))
                                        .clicked()
                                    {
                                        self.start(false);
                                    }
                                });
                            }
                        }
                    });
            });
        let clicked_outside = ctx.input(|i| i.pointer.any_click())
            && !area.response.contains_pointer()
            && !anchor.contains(ctx.input(|i| i.pointer.interact_pos()).unwrap_or_default());
        if clicked_outside {
            self.menu = None;
        }
    }

    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(32.)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let (main, detail, color) = self.status_text();
                    ui.add(egui::Label::new(RichText::new(&main).strong().color(color)).truncate())
                        .on_hover_text(&main);
                    ui.add(
                        egui::Label::new(RichText::new(&detail).small().color(MUTED)).truncate(),
                    )
                    .on_hover_text(&detail);
                });
            });
    }

    fn status_text(&self) -> (String, String, Color32) {
        let text = Color32::from_rgb(227, 234, 240);
        let message = self
            .settings_notice
            .clone()
            .unwrap_or_else(|| self.message.clone());
        if self.busy() && !self.waiting {
            let main = if self.run_kind == Some(RunKind::Measure) {
                "Measuring quality…"
            } else {
                "Stacking in progress · details on the Processing screen"
            };
            return (main.into(), message, text);
        }
        if let Some(reason) = self.blocker().filter(|_| !self.light_sets.is_empty()) {
            return (reason, message, WARN);
        }
        if self.measured() {
            let (summary, detail) = self.quality_table.footer_status(
                &self.rows,
                &self.manual_selection,
                self.final_weights,
            );
            return (summary, format!("{detail} · {message}"), text);
        }
        if self.light_sets.is_empty() {
            return ("No frames yet".into(), message, text);
        }
        let frames: usize = self
            .included_sets()
            .filter_map(|s| s.summary.as_ref().map(|m| m.files))
            .sum();
        let sets = self.included_sets().count();
        (
            format!(
                "{frames} frames in {sets} light set{} · not measured yet",
                if sets == 1 { "" } else { "s" }
            ),
            message,
            text,
        )
    }

    pub(super) fn settings_view(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_max_width(1000.);
            let editable = !self.busy() || self.waiting;
            ui.columns(2, |cols| {
                card(&mut cols[0], |ui| {
                    ui.heading("Registration reference");
                    ui.add_enabled_ui(editable, |ui| {
                        for (mode, label, hint) in [
                            (0, "Auto (recommended)", "One night: chronological median. Several nights: the frame with the best shared coverage."),
                            (3, "Chronological median", "The middle frame of the accepted sequence."),
                            (1, "A frame I choose", "Pick it in Frames with “Use as reference”."),
                            (2, "External FITS · geometry only", "Uses another image’s pixel grid; science data still comes from your frames."),
                        ] {
                            ui.radio_value(&mut self.reference_mode, mode, RichText::new(label).strong());
                            ui.label(RichText::new(hint).small().color(MUTED));
                            ui.add_space(2.);
                        }
                        if self.reference_mode == 1 {
                            ui.separator();
                            if self.reference.trim().is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new("No frame chosen yet.").color(WARN));
                                    if ui.button("Choose in Frames").clicked() {
                                        self.view = View::Frames;
                                    }
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    let name = Path::new(&self.reference)
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned();
                                    ui.add(egui::Label::new(format!("Reference: {name}")).truncate())
                                        .on_hover_text(display_path(Path::new(&self.reference)));
                                    if ui.small_button("✕").on_hover_text("Clear reference").clicked() {
                                        self.reference.clear();
                                    }
                                });
                            }
                        }
                        if self.reference_mode == 2 {
                            ui.separator();
                            ui.horizontal(|ui| {
                                let text = if self.reference.trim().is_empty() {
                                    RichText::new("No file chosen").color(WARN)
                                } else {
                                    RichText::new(
                                        Path::new(&self.reference)
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy(),
                                    )
                                };
                                ui.add(egui::Label::new(text).truncate());
                                if ui.button("Choose FITS…").clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("FITS", &["fit", "fits", "fts"])
                                        .pick_file()
                                    {
                                        self.reference = path.to_string_lossy().into_owned();
                                    }
                                }
                            });
                        }
                    });
                });
                card(&mut cols[1], |ui| {
                    ui.heading("Output");
                    ui.add_enabled_ui(!self.busy(), |ui| {
                        ui.label(RichText::new("Output folder").color(MUTED));
                        ui.horizontal(|ui| {
                            let width = (ui.available_width() - 85.).max(60.);
                            ui.add(egui::TextEdit::singleline(&mut self.folder).desired_width(width));
                            if ui.button("Browse…").clicked() {
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    self.folder = path.to_string_lossy().into_owned();
                                }
                            }
                        });
                        ui.label(RichText::new("Master file name").color(MUTED));
                        ui.add(egui::TextEdit::singleline(&mut self.filename).desired_width(ui.available_width()));
                        ui.label(
                            RichText::new("A JSON report is saved next to it. Existing names get _v2, _v3 …")
                                .small()
                                .color(MUTED),
                        );
                        ui.add_space(4.);
                        let mut maps = self.save_coverage && self.save_high_rejection;
                        if ui.checkbox(&mut maps, "Save diagnostic maps").changed() {
                            self.save_coverage = maps;
                            self.save_high_rejection = maps;
                        }
                        ui.label(
                            RichText::new("Coverage and high-rejection FITS. Both are always shown in Results.")
                                .small()
                                .color(MUTED),
                        );
                    });
                });
            });
            ui.add_space(10.);
            card(ui, |ui| {
                let summary = format!(
                    "{} · {} · {} memory ceiling · distortion correction {}",
                    if self.gpu { "GPU" } else { "CPU" },
                    if self.cpu_threads == 0 {
                        "automatic threads".into()
                    } else {
                        format!("{} threads", self.cpu_threads)
                    },
                    if self.memory >= 12288 {
                        "12 GB".into()
                    } else {
                        format!("{:.1} GB", self.memory as f64 / 1024.)
                    },
                    if self.geometric_correction { "on" } else { "off" }
                );
                let header = egui::CollapsingHeader::new(RichText::new("Advanced").strong())
                    .id_salt("advanced_settings")
                    .open(Some(self.advanced_open))
                    .show(ui, |ui| {
                        ui.add_enabled_ui(!self.busy(), |ui| {
                            egui::Grid::new("advanced_grid")
                                .num_columns(2)
                                .spacing([24., 10.])
                                .show(ui, |ui| {
                                    ui.label("Processing engine");
                                    ui.horizontal(|ui| {
                                        ui.selectable_value(&mut self.gpu, false, "CPU");
                                        ui.selectable_value(
                                            &mut self.gpu,
                                            true,
                                            if cfg!(feature = "gpu") { "GPU" } else { "GPU (not in this build)" },
                                        );
                                    });
                                    ui.end_row();
                                    ui.label("CPU threads");
                                    ui.add_enabled_ui(!self.gpu, |ui| {
                                        egui::ComboBox::from_id_salt("cpu_threads")
                                            .selected_text(if self.cpu_threads == 0 {
                                                "Automatic (up to 24)".into()
                                            } else {
                                                format!("{} threads", self.cpu_threads)
                                            })
                                            .show_ui(ui, |ui| {
                                                for count in [0, 12, 16, 24] {
                                                    ui.selectable_value(
                                                        &mut self.cpu_threads,
                                                        count,
                                                        if count == 0 {
                                                            "Automatic (up to 24)".into()
                                                        } else {
                                                            format!("{count} threads")
                                                        },
                                                    );
                                                }
                                            });
                                    });
                                    ui.end_row();
                                    ui.label("Memory ceiling (MiB)");
                                    ui.add(egui::DragValue::new(&mut self.memory).range(1024..=12288));
                                    ui.end_row();
                                    ui.label("Output tiles");
                                    egui::ComboBox::from_id_salt("tiles")
                                        .selected_text(if self.tile == 0 {
                                            "Automatic".into()
                                        } else {
                                            format!("{} × {}", self.tile, self.tile)
                                        })
                                        .show_ui(ui, |ui| {
                                            for size in [0, 128, 256, 512] {
                                                ui.selectable_value(
                                                    &mut self.tile,
                                                    size,
                                                    if size == 0 {
                                                        "Automatic".into()
                                                    } else {
                                                        format!("{size} × {size}")
                                                    },
                                                );
                                            }
                                        });
                                    ui.end_row();
                                });
                            ui.checkbox(&mut self.geometric_correction, "Correct geometric distortion")
                                .on_hover_text("Each frame must pass whole-field accuracy checks; failed frames are rejected.");
                            ui.checkbox(&mut self.diagnose_excluded, "Also register unused frames")
                                .on_hover_text("Diagnostics only; they never enter the stack.");
                            ui.label(
                                RichText::new("Pixel rejection: winsorized sigma clipping, low 3.0 / high 2.5 (fixed, recorded in the report).")
                                    .small()
                                    .color(MUTED),
                            );
                            if self.gpu && self.included_sets().any(|s| s.is_mono()) {
                                ui.label(
                                    RichText::new("GPU reconstruction supports colour frames only; monochrome light sets need the CPU engine.")
                                        .color(WARN),
                                );
                            }
                        });
                    });
                if header.header_response.clicked() {
                    self.advanced_open = !self.advanced_open;
                }
                if !self.advanced_open {
                    ui.label(RichText::new(summary).small().color(MUTED));
                }
            });
            ui.add_space(6.);
            ui.label(RichText::new("Settings and light sets are remembered between sessions.").small().color(MUTED));
        });
    }

    pub(super) fn processing_view(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_max_width(1060.);
            ui.columns(2, |cols| {
                card(&mut cols[0], |ui| self.phase_card(ui));
                card(&mut cols[1], |ui| self.run_card(ui));
            });
        });
    }

    fn phase_card(&mut self, ui: &mut egui::Ui) {
        let completed = self.last_run == Some(LastRun::Done) && !self.busy();
        let progress = self
            .worker
            .as_ref()
            .and_then(|w| w.progress.lock().ok().map(|p| p.clone()))
            .or_else(|| self.last_progress.clone());
        let title = if self.busy() {
            if self.waiting {
                "Waiting for quality review"
            } else if self.run_kind == Some(RunKind::Measure) {
                "Measuring quality"
            } else {
                "Processing"
            }
        } else {
            match self.last_run {
                Some(LastRun::Done) => "Completed run",
                Some(LastRun::Measured) => "Quality measurement complete",
                Some(LastRun::Stopped) => "Stopped",
                None => "Not running",
            }
        };
        ui.horizontal(|ui| {
            ui.heading(title);
            if completed {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Open results").clicked() {
                        self.view = View::Results;
                    }
                });
            }
        });
        let elapsed = if self.started.is_some() {
            clock(self.active_elapsed().as_secs())
        } else {
            "—".into()
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Elapsed: {elapsed}")).strong());
            ui.separator();
            ui.label(format!("Estimated remaining: {}", self.remaining_text(progress.as_ref())))
                .on_hover_text("Approximate time until the stack and output files are ready. Uses measured integration speed. Quality review wait is excluded; this is not a guaranteed completion time.");
        });
        ui.separator();
        let times = if completed {
            self.result_phase_times
        } else {
            progress
                .as_ref()
                .map(job::Progress::phase_times)
                .unwrap_or_default()
        };
        let outcomes = if completed {
            self.result
                .as_ref()
                .map(|r| r.phase_outcomes)
                .unwrap_or_default()
        } else {
            progress
                .as_ref()
                .map(|p| p.phase_outcomes)
                .unwrap_or_default()
        };
        // Quality measurement shows only its own phase.
        let phase_count = if self.run_kind == Some(RunKind::Measure) && !completed {
            1
        } else {
            4
        };
        egui::Grid::new("phase_progress")
            .num_columns(4)
            .min_row_height(38.)
            .spacing([16., 6.])
            .show(ui, |ui| {
                for header in ["Phase", "Progress", "Time", "Status"] {
                    ui.strong(header);
                }
                ui.end_row();
                for (index, phase) in job::PHASES.iter().enumerate().take(phase_count) {
                    let current =
                        !completed && progress.as_ref().is_some_and(|p| p.phase == *phase);
                    let active =
                        current && self.busy() && !self.waiting && self.finished_elapsed.is_none();
                    let reused = index == 0
                        && if completed {
                            self.result_inspection_reused
                        } else {
                            progress.as_ref().is_some_and(|p| p.inspection_reused)
                        };
                    let finished_measure = self.last_run == Some(LastRun::Measured) && index == 0;
                    let fraction = if completed
                        || reused
                        || finished_measure
                        || (times[index].is_some() && !current)
                        || (self.run_complete && !completed)
                    {
                        Some(1.)
                    } else if current {
                        progress.as_ref().and_then(|p| {
                            (p.total > 0)
                                .then(|| (p.completed as f32 / p.total as f32).clamp(0., 1.))
                        })
                    } else {
                        None
                    };
                    ui.label(if active {
                        RichText::new(*phase).strong()
                    } else {
                        RichText::new(*phase)
                    });
                    if let Some(fraction) = fraction {
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_width(170.)
                                .text(format!("{:.0}%", (fraction * 100.).floor())),
                        );
                    } else {
                        ui.label("—");
                    }
                    ui.monospace(times[index].map(phase_clock).unwrap_or_else(|| "—".into()));
                    let (status, color) = if let Some(outcome) = outcomes[index] {
                        let mut text = format!(
                            "{} {} · {} failed",
                            outcome.succeeded,
                            if index == 1 { "registered" } else { "stacked" },
                            outcome.failed
                        );
                        if outcome.skipped > 0 {
                            text.push_str(&format!("\n{} excluded", outcome.skipped));
                        }
                        (text, GOOD)
                    } else if reused {
                        ("Reused from review".into(), GOOD)
                    } else if completed || finished_measure {
                        ("Complete".into(), GOOD)
                    } else if active {
                        ("Running".into(), Color32::from_rgb(124, 196, 228))
                    } else if current && self.finished_elapsed.is_some() && !self.run_complete {
                        ("Stopped".into(), WARN)
                    } else if times[index].is_some() {
                        ("Complete".into(), GOOD)
                    } else {
                        ("Not started".into(), MUTED)
                    };
                    ui.label(RichText::new(status).color(color));
                    ui.end_row();
                }
            });
        ui.separator();
        ui.add(egui::Label::new(&self.message).wrap());
        if let Some(paths) = &self.output_plan {
            ui.label(
                RichText::new(format!(
                    "Output: {}",
                    paths
                        .master
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ))
                .color(MUTED),
            )
            .on_hover_text(display_path(&paths.master));
        }
        ui.label(
            RichText::new(
                "Quality-review wait is excluded. Published files are kept on cancellation.",
            )
            .small()
            .color(MUTED),
        );
    }

    fn run_card(&self, ui: &mut egui::Ui) {
        ui.heading("This run");
        let request = if self.busy() {
            self.active_request.as_ref()
        } else {
            self.result_request
                .as_ref()
                .or(self.active_request.as_ref())
        };
        let reference = [
            "Auto",
            "Selected frame",
            "External FITS",
            "Chronological median",
        ][self.reference_mode];
        egui::Grid::new("run_info")
            .num_columns(2)
            .spacing([14., 4.])
            .show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.label(RichText::new(k).color(MUTED));
                    ui.label(v);
                    ui.end_row();
                };
                row("Reference", reference.into());
                row(
                    "Engine",
                    format!(
                        "{}{}",
                        if self.gpu { "GPU" } else { "CPU" },
                        if self.cpu_threads == 0 {
                            " · auto threads".into()
                        } else {
                            format!(" · {} threads", self.cpu_threads)
                        }
                    ),
                );
                row("Memory ceiling", format!("{} MiB", self.memory));
                row(
                    "Diagnostic maps",
                    if self.save_coverage || self.save_high_rejection {
                        "saved"
                    } else {
                        "preview only"
                    }
                    .into(),
                );
                row(
                    "Output folder",
                    Path::new(&self.folder)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            });
        ui.add_space(6.);
        ui.label(RichText::new("LIGHT SETS").small().color(MUTED));
        let names: Vec<String> = match request {
            Some(r) => r.sets.iter().map(|s| s.label.clone()).collect(),
            None => self.included_sets().map(|s| s.name.clone()).collect(),
        };
        for name in names {
            let set = self.light_sets.iter().find(|s| s.name == name);
            ui.separator();
            ui.label(RichText::new(&name).strong());
            if let Some(set) = set {
                ui.label(RichText::new(set.layout_label()).small().color(MUTED));
                let calibration = set
                    .calibration
                    .and_then(|id| self.calibration(id))
                    .map(|c| {
                        let applied: Vec<_> = CalibrationKind::ALL
                            .iter()
                            .filter(|&&k| c.enabled(k).is_some())
                            .map(|k| k.title().to_lowercase())
                            .collect();
                        if applied.is_empty() {
                            format!("Calibration: {} · nothing switched on", c.name)
                        } else {
                            format!(
                                "Calibration: {} · {} applied (masters in RAM)",
                                c.name,
                                applied.join(", ")
                            )
                        }
                    })
                    .unwrap_or_else(|| "No calibration".into());
                ui.label(RichText::new(calibration).small().color(MUTED));
            }
        }
        ui.add_space(6.);
        ui.label(
            RichText::new("Originals are only read. Registered frames stay in memory and are never written to disk.")
                .small()
                .color(MUTED),
        );
    }

    pub(super) fn results_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(result) = &self.result else {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                ui.heading("No stack yet");
                ui.label(RichText::new("The master, coverage and high-rejection maps appear here when a stack completes.").color(MUTED));
                if ui.button("Go to Frames").clicked() {
                    self.view = View::Frames;
                }
            });
            return;
        };
        let _ = result;
        let sidebar = 280_f32.min(ui.available_width() * 0.34);
        let image_width = (ui.available_width() - sidebar - 18.).max(100.);
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(image_width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.horizontal(|ui| {
                        for (layer, label) in
                            ["Master", "Coverage", "High rejection"].iter().enumerate()
                        {
                            if ui
                                .selectable_value(&mut self.layer, layer, *label)
                                .changed()
                            {
                                self.result_texture = None;
                                self.result_view.stop_loading();
                            }
                        }
                    });
                    self.result_image(ui, ctx);
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2(sidebar, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.result_sidebar(ui),
            );
        });
    }

    fn result_sidebar(&mut self, ui: &mut egui::Ui) {
        let Some(result) = self.result.as_ref() else {
            return;
        };
        ui.spacing_mut().item_spacing.y = 4.;
        ui.add(
            egui::Label::new(
                RichText::new(
                    result
                        .paths
                        .master
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                )
                .strong()
                .size(15.),
            )
            .wrap(),
        );
        ui.label(RichText::new("Completed · originals unchanged").color(GOOD));
        egui::Grid::new("result_stats")
            .num_columns(2)
            .spacing([14., 4.])
            .show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.label(RichText::new(k).color(MUTED));
                    ui.label(v);
                    ui.end_row();
                };
                row(
                    "Frames stacked",
                    format!("{} of {}", result.accepted, result.total),
                );
                row("Exposure", clock(result.exposure as u64));
                if let Some(request) = &self.result_request {
                    row("Light sets", request.sets.len().to_string());
                }
                row(
                    "Processing",
                    format!("{} s", whole_seconds(Some(result.wall_seconds))),
                );
                row(
                    "Peak RAM",
                    result
                        .peak_rss_bytes
                        .map(|b| format!("{:.1} GB", b as f64 / 1073741824.))
                        .unwrap_or_else(|| "unavailable".into()),
                );
                row(
                    "Output grid",
                    format!("{} × {} · no crop", result.width, result.height),
                );
                row(
                    "Calibration",
                    if result.calibrated_frames > 0 {
                        format!("calibrated · {} frames", result.calibrated_frames)
                    } else {
                        "none".into()
                    },
                );
            });
        if let Some(summary) = &result.auto_reference_summary {
            ui.label(RichText::new(summary).small()).on_hover_text("Catalog-only geometric prediction before pixel rejection; denominator is all Use-on frames.");
        }
        if let Some([applied, ordinary, failed]) = result.geometric_correction {
            ui.label(format!(
                "Geometry: {applied} corrected · {ordinary} linear · {failed} failed"
            ));
        }
        ui.add(
            egui::Label::new(
                RichText::new(format!("Reference: {}", result.reference))
                    .small()
                    .color(MUTED),
            )
            .truncate(),
        )
        .on_hover_text(&result.reference);
        ui.add_space(6.);
        let path = match self.layer {
            0 => Some(&result.paths.master),
            1 if result.paths.save_coverage => Some(&result.paths.coverage),
            2 if result.paths.save_high_rejection => Some(&result.paths.high_rejection),
            _ => None,
        };
        if ui
            .add_enabled(
                path.is_some(),
                egui::Button::new(
                    RichText::new(if self.layer == 0 {
                        "Open master FITS"
                    } else {
                        "Open map FITS"
                    })
                    .strong()
                    .color(Color32::WHITE),
                )
                .fill(ACCENT)
                .min_size(egui::vec2(ui.available_width(), 34.)),
            )
            .on_hover_text("Opens the FITS with its default application, e.g. PixInsight.")
            .clicked()
        {
            if let Some(path) = path {
                open(path);
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Open folder").clicked() {
                reveal(&result.paths.master);
            }
            if ui.button("Open report").clicked() {
                open(&result.paths.report);
            }
            if ui.button("Phase details").clicked() {
                self.view = View::Processing;
            }
        });
    }

    fn result_image(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let result = self.result.as_ref().unwrap();
        if self.layer == 0 {
            if ui
                .checkbox(
                    &mut self.result_stretch,
                    "Auto stretch · display only · linear FITS unchanged",
                )
                .changed()
            {
                self.result_texture = None;
            }
        } else {
            let saved = if self.layer == 1 {
                result.paths.save_coverage
            } else {
                result.paths.save_high_rejection
            };
            ui.horizontal(|ui| {
                ui.label("Channel");
                for (id, label) in ["R", "G", "B"].iter().enumerate() {
                    if ui.selectable_value(&mut self.channel, id, *label).changed() {
                        self.result_texture = None;
                    }
                }
                let max = &mut self.map_display_max[self.layer - 1];
                let previous = *max;
                egui::ComboBox::from_id_salt("map_display_max")
                    .selected_text(format!("Display 0–{:.0}%", *max * 100.))
                    .show_ui(ui, |ui| {
                        for value in [1., 0.1, 0.05, 0.01] {
                            ui.selectable_value(max, value, format!("0–{:.0}%", value * 100.));
                        }
                    });
                if *max != previous {
                    self.result_texture = None;
                    self.result_view.stop_loading();
                }
            });
            let what = if self.layer == 1 {
                "Surviving samples ÷ all inputs"
            } else {
                "Upper-rejected samples ÷ all inputs"
            };
            ui.label(
                RichText::new(if saved {
                    format!("{what} · saved as FITS")
                } else {
                    format!("{what} · preview only — turn on “Save diagnostic maps” in Settings to keep the FITS")
                })
                .small()
                .color(if saved { MUTED } else { WARN }),
            );
        }
        let preview = match self.layer {
            0 => &result.master,
            1 => &result.coverage,
            _ => &result.high,
        };
        let display = DisplayOptions {
            stretch: self.layer == 0 && self.result_stretch,
            channel: (self.layer != 0).then_some(self.channel),
            map_max: if self.layer == 0 {
                1.
            } else {
                self.map_display_max[self.layer - 1]
            },
        };
        if let Some(stats) = preview.map_stats {
            let stats = stats[self.channel];
            ui.label(
                RichText::new(format!(
                    "Full map: {:.3}–{:.3}% · mean {:.3}% · non-zero {:.2}%",
                    stats.minimum * 100.,
                    stats.maximum * 100.,
                    stats.mean * 100.,
                    stats.nonzero_pixels as f64 * 100.
                        / (preview.native_size[0] * preview.native_size[1]).max(1) as f64
                ))
                .small(),
            );
        }
        if self.result_texture.is_none() {
            self.result_texture = Some(ctx.load_texture(
                "result_preview",
                preview.display_with_options(display),
                egui::TextureOptions::LINEAR,
            ));
        }
        if let Some(texture) = &self.result_texture {
            let detail = match self.layer {
                0 => Some(DetailSource::Export(result.paths.master.clone())),
                1 if result.paths.save_coverage => {
                    Some(DetailSource::Export(result.paths.coverage.clone()))
                }
                2 if result.paths.save_high_rejection => {
                    Some(DetailSource::Export(result.paths.high_rejection.clone()))
                }
                _ => None,
            }
            .filter(|_| !self.busy() || self.waiting);
            self.result_view.show(ui, texture, preview, detail, display);
        }
    }
}
