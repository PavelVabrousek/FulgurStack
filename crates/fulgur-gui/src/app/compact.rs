//! Compact Desk layout. All actions use the existing worker and immutable results.
use super::*;

impl Workspace {
    pub(super) fn compact_navigation(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("compact_navigation").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("FulgurStack");
                ui.separator();
                ui.label(RichText::new("Compact Desk").small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(if self.gpu { "GPU" } else { "CPU" })
                            .small()
                            .weak(),
                    );
                    ui.label(
                        RichText::new("Local processing · originals preserved")
                            .small()
                            .weak(),
                    );
                });
            });
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add_enabled(!self.busy(), egui::Button::new("Add folder…"))
                        .clicked()
                    {
                        self.add_folder();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(!self.busy(), egui::Button::new("Add FITS files…"))
                        .clicked()
                    {
                        self.add_files();
                        ui.close_menu();
                    }
                    if ui.button("Output settings").clicked() {
                        self.view = View::Setup;
                        self.setup_tab = 0;
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_preview, "Selected-frame preview");
                    ui.checkbox(&mut self.show_quality_chart, "HFR plot");
                });
                ui.separator();
                for (view, label) in [
                    (View::Frames, "Frames"),
                    (View::Quality, "Quality"),
                    (View::Setup, "Stack setup"),
                ] {
                    if ui.selectable_label(self.view == view, label).clicked() {
                        self.view = view;
                    }
                }
                if ui
                    .selectable_label(self.processing_open && !self.processing_saved, "Processing")
                    .clicked()
                {
                    self.processing_saved = false;
                    self.processing_open = true;
                }
                if ui
                    .selectable_label(self.view == View::Results, "Results")
                    .clicked()
                {
                    self.view = View::Results;
                }
            });
        });
    }

    fn compact_stop(&mut self, ui: &mut egui::Ui) {
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
                .fill(Color32::from_rgb(156, 43, 54))
                .min_size(egui::vec2(100., 32.)),
            )
            .on_hover_text(
                "Stop cooperatively and wait for worker cleanup. Published files remain.",
            )
            .clicked()
        {
            self.cancel_run();
            self.processing_saved = false;
        }
    }

    pub(super) fn compact_footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("compact_status")
            .exact_height(50.)
            .show(ctx, |ui| {
                // Keep layout stable, but let the modal own progress and run actions.
                if self.processing_open || self.confirm_close {
                    return;
                }
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 400.).max(160.);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 36.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 1.;
                            let mut detail = String::new();
                            let summary = if self.view == View::Quality {
                                let (summary, quality_detail) = self.quality_table.footer_status(&self.rows, &self.manual_selection, self.final_weights);
                                detail = quality_detail;
                                summary
                            } else if self.rows.is_empty() {
                                format!("{} input locations", self.inputs.len())
                            } else {
                                let selected: Vec<_> = self.rows.iter().filter(|r| r.accepted).collect();
                                let exposure: f64 =
                                    selected.iter().filter_map(|r| r.exposure).sum();
                                format!(
                                    "{} / {} accepted · {} exposure",
                                    selected.len(),
                                    self.rows.len(),
                                    clock(exposure as u64)
                                )
                            };
                            ui.add_sized([width, 18.], egui::Label::new(&summary).truncate()).on_hover_text(&summary);
                            let message = if self.waiting { "Inspection complete · waiting for review" } else { &self.message };
                            let message = self.settings_notice.as_deref().unwrap_or(message);
                            if !detail.is_empty() { detail.push_str(" · "); }
                            detail.push_str(message);
                            ui.add_sized([width, 16.], egui::Label::new(RichText::new(&detail).small().weak()).truncate())
                                .on_hover_text(&detail);
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.busy() && !self.run_complete && self.finished_elapsed.is_none() {
                            self.compact_stop(ui);
                            if self.waiting {
                                if ui.button("Continue stacking").clicked() {
                                    self.continue_run();
                                }
                            } else {
                                ui.label(clock(self.active_elapsed().as_secs()))
                                    .on_hover_text("Open Processing in the top navigation for phase progress and times.");
                            }
                        } else {
                            if ui
                                .add_enabled(
                                    !self.busy() && !self.inputs.is_empty(),
                                    egui::Button::new("Stack images")
                                        .min_size(egui::vec2(130., 32.))
                                        .fill(ui.visuals().selection.bg_fill),
                                )
                                .clicked()
                            {
                                self.start(false);
                            }
                            ui.add_enabled(
                                !self.busy(),
                                egui::Checkbox::new(&mut self.review, "Review before stacking"),
                            );
                        }
                    });
                });
            });
    }

    fn compact_reference(&mut self, ui: &mut egui::Ui) {
        ui.label("Registration reference");
        ui.add_enabled_ui(!self.busy() || self.waiting, |ui| {
            egui::ComboBox::from_id_salt("compact_reference_mode")
                .width(ui.available_width() - 12.)
                .selected_text(
                    [
                        "Automatic · chronological median",
                        "Selected accepted frame",
                        "External FITS · geometry only",
                    ][self.reference_mode],
                )
                .show_ui(ui, |ui| {
                    for (id, label) in [
                        "Automatic · chronological median",
                        "Selected accepted frame",
                        "External FITS · geometry only",
                    ]
                    .iter()
                    .enumerate()
                    {
                        ui.selectable_value(&mut self.reference_mode, id, *label);
                    }
                });
            ui.add_enabled(!self.busy(), egui::Checkbox::new(&mut self.diagnose_excluded, "Also register Use-off frames for diagnostics"))
                .on_hover_text("Off by default. Excluded frames never enter the stack or influence selected-frame registration.");
            if self.reference_mode != 0 {
                ui.add(
                    egui::TextEdit::singleline(&mut self.reference)
                        .desired_width(ui.available_width())
                        .hint_text("Choose a reference in Quality"),
                );
                if self.reference_mode == 2 && ui.button("Choose reference FITS…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("FITS", &["fit", "fits", "fts"])
                        .pick_file()
                    {
                        self.reference = path.to_string_lossy().into_owned();
                    }
                }
            }
        });
        if self.reference_mode == 0 {
            ui.label(
                RichText::new("Accepted frame near the session midpoint.")
                    .small()
                    .weak(),
            );
        }
    }

    fn compact_output(&mut self, ui: &mut egui::Ui) {
        ui.strong("Output");
        ui.add_enabled_ui(!self.busy(), |ui| {
            ui.label("Output folder");
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 85.).max(60.);
                ui.add(egui::TextEdit::singleline(&mut self.folder).desired_width(width));
                if ui.button("Browse…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.folder = path.to_string_lossy().into_owned();
                    }
                }
            });
            ui.label("Master filename");
            ui.add(
                egui::TextEdit::singleline(&mut self.filename).desired_width(ui.available_width()),
            );
            ui.label(
                RichText::new("Master FITS + JSON report. Existing names receive _v2, _v3…")
                    .small()
                    .weak(),
            );
            ui.checkbox(&mut self.save_coverage, "Save coverage map");
            ui.checkbox(&mut self.save_high_rejection, "Save high rejection map");
            ui.label(RichText::new("Both maps are always previewed in memory. These options save extra FITS files.").small().weak());
        });
    }

    pub(super) fn compact_setup(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Stack setup");
            ui.label(
                RichText::new("Last-used settings are remembered locally")
                    .small()
                    .weak(),
            );
        });
        ui.horizontal(|ui| {
            for (id, label) in ["Basic", "Alignment", "Rejection", "Performance"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.setup_tab, id, *label);
            }
        });
        ui.separator();
        ui.columns(2, |cols| {
            if self.setup_tab == 0 {
                let ui = &mut cols[0];
                ui.strong("Processing");
                ui.add_enabled_ui(!self.busy(), |ui| {
                    ui.label("Processing engine");
                    egui::ComboBox::from_id_salt("compact_engine").width(ui.available_width() - 12.)
                        .selected_text(if self.gpu { "GPU" } else { "CPU · default" }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.gpu, false, "CPU · default");
                            ui.selectable_value(&mut self.gpu, true, if cfg!(feature = "gpu") { "GPU" } else { "GPU · requires GPU-enabled build" });
                        });
                });
                self.compact_reference(ui);
                ui.add_space(6.);
                ui.label("Output canvas: Reference size · no crop");
                ui.label(RichText::new("Exact reference dimensions and pixel grid.").small().weak());
                self.compact_output(&mut cols[1]);
            } else if self.setup_tab == 1 {
                self.compact_reference(&mut cols[0]);
                cols[1].strong("Reference canvas");
                cols[1].label("Reference size · no crop");
                cols[1].label("An external FITS supplies geometry only.");
                cols[1].label(RichText::new("Science exposure, photometry and location provenance stay anchored to an input frame.").small().weak());
            } else if self.setup_tab == 2 {
                cols[0].strong("Pixel rejection");
                cols[0].label("Winsorized Sigma Clipping");
                cols[0].label("Low sigma: 3.0 · High sigma: 2.5");
                cols[0].label(RichText::new("Accepted science defaults. This layout does not change rejection arithmetic.").small().weak());
                cols[1].strong("Frame selection");
                cols[1].label(format!("{} selected / {} measured · {} manual choices", self.rows.iter().filter(|r| r.accepted).count(), self.rows.len(), self.manual_selection.len()));
                if cols[1].button("Review quality").clicked() { self.view = View::Quality; }
                cols[1].label(RichText::new("Unreadable or incompatible inputs cannot be forced into a stack.").small().weak());
            } else {
                cols[0].strong("Memory");
                cols[0].add_enabled_ui(!self.busy(), |ui| {
                    ui.label("RAM ceiling (MiB)");
                    ui.add(egui::DragValue::new(&mut self.memory).range(1024..=12288));
                    ui.label("Output tile size");
                    egui::ComboBox::from_id_salt("compact_tiles").selected_text(if self.tile == 0 { "Automatic · available RAM".into() } else { format!("{} × {}", self.tile, self.tile) }).show_ui(ui, |ui| {
                        for size in [0, 128, 256, 512] {
                            ui.selectable_value(&mut self.tile, size, if size == 0 { "Automatic · available RAM".into() } else { format!("{size} × {size}") });
                        }
                    });
                });
                cols[0].label(RichText::new("Configured ceiling, not measured peak RAM.").small().weak());
                cols[1].strong("Storage");
                cols[1].label("Original FITS cache: Off");
                cols[1].label("No calibrated or registered intermediate files.");
            }
        });
    }

    pub(super) fn compact_processing(&mut self, ui: &mut egui::Ui) {
        let saved = self.processing_saved && self.result.is_some();
        let progress = self
            .worker
            .as_ref()
            .and_then(|w| w.progress.lock().ok().map(|p| p.clone()))
            .or_else(|| self.last_progress.clone());
        if saved {
            let result = self.result.as_ref().unwrap();
            ui.label(format!(
                "Processing: {} s · Peak RAM: {}",
                whole_seconds(Some(result.wall_seconds)),
                result
                    .peak_rss_bytes
                    .map(|b| format!("{:.1} MiB", b as f64 / 1048576.))
                    .unwrap_or_else(|| "unavailable".into())
            ));
        } else {
            self.processing_timing(ui, progress.as_ref());
        }
        let times = if saved {
            self.result_phase_times
        } else {
            progress
                .as_ref()
                .map(job::Progress::phase_times)
                .unwrap_or_default()
        };
        let outcomes = if saved {
            self.result.as_ref().unwrap().phase_outcomes
        } else {
            progress
                .as_ref()
                .map(|p| p.phase_outcomes)
                .unwrap_or_default()
        };
        egui::Grid::new("compact_phase_progress")
            .num_columns(4)
            .min_row_height(34.)
            .spacing([14., 8.])
            .show(ui, |ui| {
                ui.strong("Phase");
                ui.strong("Progress");
                ui.strong("Time");
                ui.strong("Status");
                ui.end_row();
                for (index, phase) in job::PHASES.iter().enumerate() {
                    let current = !saved && progress.as_ref().is_some_and(|p| p.phase == *phase);
                    let active =
                        current && self.busy() && !self.waiting && self.finished_elapsed.is_none();
                    let reused = index == 0
                        && if saved {
                            self.result_inspection_reused
                        } else {
                            progress.as_ref().is_some_and(|p| p.inspection_reused)
                        };
                    let fraction = if saved
                        || reused
                        || (times[index].is_some() && !current)
                        || (self.run_complete && !saved)
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
                                .desired_width(150.)
                                .text(format!("{:.0}%", (fraction * 100.).floor())),
                        );
                    } else {
                        ui.label("—");
                    }
                    ui.monospace(times[index].map(phase_clock).unwrap_or_else(|| "—".into()));
                    let status = if let Some(outcome) = outcomes[index] {
                        let mut text = format!("{} {} · {} failed", outcome.succeeded, if index == 1 { "registered" } else { "stacked" }, outcome.failed);
                        if outcome.skipped > 0 { text.push_str(&format!("\n{} excluded", outcome.skipped)); }
                        text
                    } else if reused {
                        "Reused".into()
                    } else if saved {
                        "Complete".into()
                    } else if active {
                        "Running".into()
                    } else if current && self.finished_elapsed.is_some() && !self.run_complete {
                        "Stopped".into()
                    } else if times[index].is_some() {
                        "Complete".into()
                    } else {
                        "Not started".into()
                    };
                    ui.allocate_ui_with_layout(egui::vec2(180., 34.), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(status).wrap()).on_hover_text(if index == 1 {
                        "Successful and failed scheduled registrations; excluded frames are skipped by default. Optional diagnostics never add frames to the stack."
                    } else if index == 2 {
                        "Frames supplied to the completed integration; excluded inputs were not stacked. Pixel rejection is separate. A failed or cancelled integration has no completed frame totals."
                    } else { "Status of this phase." });
                    });
                    ui.end_row();
                }
            });
        ui.add_space(8.);
        if saved {
            ui.label(
                RichText::new(
                    "Completed run · phase wall times are retained separately from new runs.",
                )
                .small()
                .weak(),
            );
        } else {
            ui.add(egui::Label::new(&self.message).truncate())
                .on_hover_text(&self.message);
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
            }
            ui.label(
                RichText::new(
                    "Quality-review wait is excluded. Published files are kept on cancellation.",
                )
                .small()
                .weak(),
            );
        }
    }

    pub(super) fn compact_processing_modal(&mut self, ctx: &egui::Context) {
        if !self.processing_open || self.confirm_close {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("processing_modal")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 48.).clamp(280., 580.));
            ui.spacing_mut().item_spacing.y = 5.;
            ui.horizontal(|ui| {
                ui.heading(if self.processing_saved {
                    "Completed run"
                } else {
                    "Processing"
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let active =
                        self.busy() && !self.run_complete && self.finished_elapsed.is_none();
                    if active {
                        self.compact_stop(ui);
                    }
                    if (self.processing_saved || !active)
                        && ui
                            .add_sized([100., 32.], egui::Button::new("Close"))
                            .clicked()
                    {
                        self.processing_open = false;
                    }
                });
            });
            if self.processing_saved && self.busy() && !self.run_complete {
                ui.label(
                    RichText::new("Saved summary · STOP cancels the current active run")
                        .small()
                        .weak(),
                );
            }
            ui.separator();
            self.processing(ui);
            if !self.processing_saved && self.busy() {
                ui.add_space(6.);
                if ui
                    .button("Back to workspace")
                    .on_hover_text("Keep processing; STOP stays visible in the bottom bar.")
                    .clicked()
                {
                    self.processing_open = false;
                }
            }
        });
        // Dismissal returns to the workspace; only an explicit STOP cancels work.
        if response.should_close() {
            self.processing_open = false;
        }
        if !self.processing_open {
            // Restore workspace actions even when no background heartbeat remains.
            ctx.request_repaint();
        }
    }

    fn compact_result_times(&self, ui: &mut egui::Ui) {
        ui.strong("Phase times");
        for (phase, time) in job::PHASES.iter().zip(self.result_phase_times) {
            ui.horizontal(|ui| {
                ui.label(if *phase == "Inspect" && self.result_inspection_reused {
                    "Inspect (reused)"
                } else {
                    *phase
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.monospace(time.map(phase_clock).unwrap_or_else(|| "—".into()));
                });
            });
        }
    }

    pub(super) fn compact_results(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.heading("Results");
            if self.result.is_some() && ui.button("Phase times").clicked() {
                self.processing_saved = true;
                self.processing_open = true;
            }
        });
        let Some(result) = &self.result else {
            ui.label("Complete a stack to view the master, maps and measured report.");
            return;
        };
        ui.label(
            RichText::new(if self.busy() {
                "Previous completed run · current processing is separate"
            } else {
                "Saved run · changing setup does not recompute this result"
            })
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            for (layer, label) in ["Master", "Coverage", "High rejection", "Report & files"]
                .iter()
                .enumerate()
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
        ui.separator();
        if self.layer == 3 {
            ui.columns(2, |cols| {
                cols[0].strong("Selection and exposure");
                cols[0].label(format!("Inputs: {} · accepted: {} · excluded: {}", result.total, result.accepted, result.total.saturating_sub(result.accepted)));
                cols[0].label(format!("Accepted acquisition exposure: {}", clock(result.exposure as u64)));
                cols[0].label(format!("Output grid: {} × {} · no crop", result.width, result.height));
                cols[0].label(RichText::new("Exposure is acquisition time, separate from processing time. Maps use all original inputs as their denominator.").small().weak());
                cols[1].strong("Published files");
                for (label, path, saved) in [("Master FITS", &result.paths.master, true), ("JSON report", &result.paths.report, true), ("Coverage", &result.paths.coverage, result.paths.save_coverage), ("High rejection", &result.paths.high_rejection, result.paths.save_high_rejection)] {
                    cols[1].horizontal(|ui| {
                        if saved && ui.button(format!("Open {label}")).clicked() { open(path); }
                        if !saved { ui.label(format!("{label}: preview only · not saved")); }
                    });
                }
                if cols[1].button("Open output folder").clicked() { reveal(&result.paths.master); }
            });
            return;
        }
        let sidebar = 260_f32.min(ui.available_width() * 0.34);
        let image_width = (ui.available_width() - sidebar - 18.).max(100.);
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(image_width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.compact_result_image(ui, ctx);
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2(sidebar, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let result = self.result.as_ref().unwrap();
                    ui.spacing_mut().item_spacing.y = 3.;
                    ui.strong("Completed run");
                    ui.label(format!(
                        "Processing: {} s",
                        whole_seconds(Some(result.wall_seconds))
                    ));
                    ui.label(format!(
                        "Peak RAM: {}",
                        result
                            .peak_rss_bytes
                            .map(|b| format!("{:.1} MiB", b as f64 / 1048576.))
                            .unwrap_or_else(|| "unavailable".into())
                    ));
                    ui.separator();
                    self.compact_result_times(ui);
                    ui.separator();
                    ui.label(format!(
                        "{} / {} accepted · {} excluded",
                        result.accepted,
                        result.total,
                        result.total.saturating_sub(result.accepted)
                    ));
                    ui.label(format!(
                        "Accepted exposure: {}",
                        clock(result.exposure as u64)
                    ));
                    if let Some(request) = &self.result_request {
                        ui.label(
                            RichText::new(format!(
                                "{} · reference grid · no crop",
                                if request.gpu { "GPU" } else { "CPU" }
                            ))
                            .small()
                            .weak(),
                        );
                    }
                    ui.horizontal_wrapped(|ui| {
                        let path = match self.layer {
                            0 => Some(&result.paths.master),
                            1 if result.paths.save_coverage => Some(&result.paths.coverage),
                            2 if result.paths.save_high_rejection => {
                                Some(&result.paths.high_rejection)
                            }
                            _ => None,
                        };
                        if ui
                            .add_enabled(
                                path.is_some(),
                                egui::Button::new(if self.layer == 0 {
                                    "Open FITS"
                                } else {
                                    "Open map FITS"
                                }),
                            )
                            .clicked()
                        {
                            if let Some(path) = path {
                                open(path);
                            }
                        }
                        if ui.button("Open folder").clicked() {
                            reveal(&result.paths.master);
                        }
                    });
                    ui.add(egui::Label::new(format!("Reference: {}", result.reference)).truncate())
                        .on_hover_text(&result.reference);
                },
            );
        });
    }

    fn compact_result_image(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let result = self.result.as_ref().unwrap();
        if self.layer == 0 {
            ui.add(
                egui::Label::new(
                    result
                        .paths
                        .master
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                )
                .truncate(),
            );
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
            ui.label(if saved {
                "In-memory preview · FITS saved"
            } else {
                "In-memory preview · FITS not saved · overview only"
            });
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
            ui.label(
                RichText::new(if self.layer == 1 {
                    "Surviving samples / ALL original inputs"
                } else {
                    "Upper rejected samples / ALL original inputs"
                })
                .small()
                .weak(),
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
            if stats.maximum == 0. {
                ui.label("No values above zero in this channel.");
            }
            ui.label(
                RichText::new(if self.layer == 2 {
                    "Peak-preserving overview · display scale only · FITS fractions unchanged"
                } else {
                    "Display scale only · FITS fractions unchanged"
                })
                .small()
                .weak(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_phase_status_shares_the_time_and_progress_vertical_center() {
        let ctx = egui::Context::default();
        let mut app = Workspace {
            last_progress: Some(job::Progress {
                phase: "Write outputs",
                completed: 1,
                total: 2,
                phase_durations: [
                    Some(Duration::from_secs(3)),
                    Some(Duration::from_secs(15)),
                    Some(Duration::from_secs(200)),
                    None,
                ],
                phase_outcomes: [
                    None,
                    Some(job::PhaseOutcome {
                        succeeded: 981,
                        failed: 0,
                        skipped: 159,
                    }),
                    Some(job::PhaseOutcome {
                        succeeded: 981,
                        failed: 0,
                        skipped: 159,
                    }),
                    None,
                ],
                ..Default::default()
            }),
            ..Workspace::default()
        };
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1060., 520.));
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_max_width(580.);
                        app.compact_processing(ui);
                    });
                },
            );
        }
        let text = |needle: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(t) if t.galley.job.text == needle => {
                        Some(t.galley.rect.translate(t.pos.to_vec2()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing visible text: {needle}"))
        };
        let phase = text("Register");
        let time = text("00:00:15");
        let status = text("981 registered · 0 failed\n159 excluded");
        assert!((phase.center().y - time.center().y).abs() <= 1.);
        assert!(
            (status.center().y - time.center().y).abs() <= 1.,
            "Status and Time must share a vertical center"
        );
        assert!(screen.contains_rect(status));
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(t)
            if t.galley.job.text == "100%" && (t.pos.y+t.galley.rect.center().y-time.center().y).abs() <= 1.)));
    }
}
