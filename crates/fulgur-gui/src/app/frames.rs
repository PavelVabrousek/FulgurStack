//! Frames step: light sets, calibration sets and the measured frame list.
use super::compact::{GOOD, WARN, card};
use super::*;

const MUTED: Color32 = Color32::from_rgb(159, 176, 191);
const LINK: Color32 = Color32::from_rgb(124, 196, 228);
const SELECTED: Color32 = Color32::from_rgb(18, 50, 74);
const SELECTED_LINE: Color32 = Color32::from_rgb(43, 110, 145);
const BAD: Color32 = Color32::from_rgb(255, 138, 128);
const OK_TEXT: Color32 = Color32::from_rgb(159, 216, 204);

fn level_color(level: Level) -> Color32 {
    match level {
        Level::Ok => OK_TEXT,
        Level::Warn => WARN,
        Level::Bad => BAD,
    }
}

fn level_mark(level: Level) -> &'static str {
    match level {
        Level::Ok => "✔",
        Level::Warn => "⚠",
        Level::Bad => "✖",
    }
}

/// Selectable rail entry with a title and up to two detail lines.
fn rail_item(
    ui: &mut egui::Ui,
    on: bool,
    title: &str,
    lines: &[(String, Color32)],
) -> egui::Response {
    let frame = egui::Frame::new()
        .fill(if on { SELECTED } else { Color32::TRANSPARENT })
        .stroke(egui::Stroke::new(
            1_f32,
            if on {
                SELECTED_LINE
            } else {
                Color32::TRANSPARENT
            },
        ))
        .corner_radius(6.)
        .inner_margin(egui::Margin::symmetric(8, 6));
    let inner = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(title).strong()).truncate());
        for (text, color) in lines {
            ui.add(egui::Label::new(RichText::new(text).small().color(*color)).truncate());
        }
    });
    ui.interact(
        inner.response.rect,
        ui.id().with(("rail", title, lines.len())),
        egui::Sense::click(),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl Workspace {
    pub(super) fn sets_rail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sets_rail")
            .default_width(240.)
            .min_width(200.)
            .max_width(340.)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let editable = !self.busy();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("SETS").small().color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_enabled_ui(editable, |ui| {
                                ui.menu_button("Clear…", |ui| {
                                    for (kind, label, enabled) in [
                                        (ClearKind::Lights, "Clear light sets", !self.light_sets.is_empty()),
                                        (ClearKind::Calibration, "Clear calibration sets", !self.calibration_sets.is_empty()),
                                        (ClearKind::All, "Clear both", !self.light_sets.is_empty() || !self.calibration_sets.is_empty()),
                                    ] {
                                        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                            self.pending_clear = Some(kind);
                                            ui.close_menu();
                                        }
                                    }
                                    ui.label(RichText::new("Files on disk are never touched.").small().color(MUTED));
                                })
                                .response
                                .on_hover_text("Remove light and/or calibration sets from the workspace");
                            });
                        });
                    });
                    let frames: usize = self
                        .light_sets
                        .iter()
                        .filter_map(|s| s.summary.as_ref().map(|m| m.files))
                        .sum();
                    let mut meta = format!("{frames} frames · {} light sets", self.light_sets.len());
                    if self.measured() {
                        meta.push_str(&format!(
                            " · Use {}",
                            self.rows.iter().filter(|r| r.accepted).count()
                        ));
                    }
                    if rail_item(ui, self.selection == Selection::All, "All light frames", &[(meta, MUTED)])
                        .clicked()
                    {
                        self.selection = Selection::All;
                    }
                    ui.add_space(8.);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("LIGHT SETS").small().color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(editable, egui::Button::new("+ Files").small())
                                .clicked()
                            {
                                self.add_light_files(ctx);
                            }
                            if ui
                                .add_enabled(editable, egui::Button::new("+ Folder").small())
                                .on_hover_text("Each added folder becomes a new light set")
                                .clicked()
                            {
                                self.add_light_folder(ctx);
                            }
                        });
                    });
                    let filters = self.included_filters();
                    let main_filter = self.main_filter();
                    let mut clicked = None;
                    for set in &self.light_sets {
                        let mut meta = match &set.summary {
                            Some(s) => format!("{} · {} · {}", s.files, s.exposure(), s.filter()),
                            None => "Reading headers…".into(),
                        };
                        if set.is_mono() {
                            meta.push_str(" · mono");
                        }
                        let meta_color = if filters.len() > 1
                            && set.include
                            && set.summary.as_ref().is_some_and(|s| s.filters.first() != main_filter.as_ref())
                        {
                            WARN
                        } else {
                            MUTED
                        };
                        let status = if !set.include {
                            ("Not in this stack".into(), MUTED)
                        } else if set.needs_layout() {
                            ("Choose colour or mono".into(), WARN)
                        } else if self.size_differs(set) {
                            ("Different sensor size · not stackable yet".into(), WARN)
                        } else if let Some(c) = set.calibration.and_then(|id| self.calibration(id)) {
                            let level = set
                                .summary
                                .as_ref()
                                .map_or(Level::Warn, |s| sets::compatibility(s, c).0);
                            (format!("Cal: {}", c.name), level_color(level))
                        } else {
                            ("No calibration".into(), MUTED)
                        };
                        if rail_item(
                            ui,
                            self.selection == Selection::Light(set.id),
                            &set.name,
                            &[(meta, meta_color), status],
                        )
                        .clicked()
                        {
                            clicked = Some(Selection::Light(set.id));
                        }
                    }
                    ui.add_space(10.);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("CALIBRATION SETS").small().color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(editable, egui::Button::new("+ Empty").small())
                                .clicked()
                            {
                                self.add_empty_calibration();
                            }
                            if ui
                                .add_enabled(editable, egui::Button::new("+ Folder").small())
                                .on_hover_text("Files are sorted into Bias, Darks, Flats and Dark flats by IMAGETYP")
                                .clicked()
                            {
                                self.add_calibration_folder(ctx);
                            }
                        });
                    });
                    if self.calibration_sets.is_empty() {
                        ui.label(
                            RichText::new("None yet. Seestar lights can be stacked without calibration.")
                                .small()
                                .color(MUTED),
                        );
                    }
                    for set in &self.calibration_sets {
                        let users: Vec<_> = self
                            .light_sets
                            .iter()
                            .filter(|l| l.calibration == Some(set.id))
                            .map(|l| l.name.clone())
                            .collect();
                        let used = if users.is_empty() {
                            ("Not assigned".into(), WARN)
                        } else {
                            (format!("Used by {}", users.join(", ")), MUTED)
                        };
                        if rail_item(
                            ui,
                            self.selection == Selection::Calibration(set.id),
                            &set.name,
                            &[(set.summary_text(), MUTED), used],
                        )
                        .clicked()
                        {
                            clicked = Some(Selection::Calibration(set.id));
                        }
                    }
                    if let Some(selection) = clicked {
                        self.selection = selection;
                    }
                });
            });
    }

    /// Filter with the most frames among included sets.
    fn main_filter(&self) -> Option<String> {
        let mut counts: Vec<(String, usize)> = vec![];
        for set in self.included_sets() {
            if let Some(s) = &set.summary {
                if let Some(f) = s.filters.first() {
                    match counts.iter_mut().find(|(name, _)| name == f) {
                        Some((_, n)) => *n += s.files,
                        None => counts.push((f.clone(), s.files)),
                    }
                }
            }
        }
        counts.into_iter().max_by_key(|(_, n)| *n).map(|(f, _)| f)
    }

    pub(super) fn frames_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let calibration_selected =
            matches!(self.selection, Selection::Calibration(id) if self.calibration(id).is_some());
        if self.light_sets.is_empty() && !calibration_selected {
            self.empty_state(ui, ctx);
            return;
        }
        if let Selection::Light(id) = self.selection {
            if self.light(id).is_none() {
                self.selection = Selection::All;
            }
        }
        if let Selection::Calibration(id) = self.selection {
            if self.calibration(id).is_none() {
                self.selection = Selection::All;
            }
        }
        match self.selection {
            Selection::Calibration(id) => {
                egui::ScrollArea::vertical().show(ui, |ui| self.calibration_view(ui, ctx, id));
                return;
            }
            Selection::Light(id) => self.light_header(ui, id),
            Selection::All => self.overview(ui),
        }
        ui.add_space(6.);
        // Inspected set index of the selection, for scoping the list.
        let scope = match self.selection {
            Selection::Light(id) => Some(self.inspected_ids.iter().position(|&i| i == id)),
            _ => None,
        };
        let measuring = self.busy() && self.run_kind == Some(RunKind::Measure) && !self.measured();
        if measuring {
            card(ui, |ui| {
                ui.heading("Measuring quality…");
                let progress = self.last_progress.as_ref();
                let fraction = progress
                    .filter(|p| p.total > 0)
                    .map(|p| p.completed as f32 / p.total as f32)
                    .unwrap_or(0.);
                ui.add(egui::ProgressBar::new(fraction).show_percentage());
                ui.label(
                    RichText::new(format!(
                        "Elapsed {} · {} remaining · the frame list appears here when done.",
                        clock(self.active_elapsed().as_secs()),
                        self.remaining_text(progress)
                    ))
                    .color(MUTED),
                );
            });
            return;
        }
        if !self.measured() {
            if self.busy() {
                card(ui, |ui| {
                    ui.label("Stacking in progress. The frame list appears after inspection; details are on the Processing screen.");
                });
                return;
            }
            card(ui, |ui| {
                let frames: usize = self
                    .included_sets()
                    .filter_map(|s| s.summary.as_ref().map(|m| m.files))
                    .sum();
                ui.heading(format!(
                    "{frames} frames in {} light set{} ready",
                    self.included_sets().count(),
                    if self.included_sets().count() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ));
                ui.label(RichText::new("Measure quality to see star size, star count and elongation for every frame and decide what goes into the stack. Or stack directly — frames are then measured and selected automatically. Both are at the top right.").color(MUTED));
            });
            return;
        }
        match scope {
            Some(None) => {
                card(ui, |ui| {
                    ui.label("This light set was not part of the last quality measurement. Include it and measure quality again to see its frames.");
                });
            }
            scope => {
                self.quality_table.scope = scope.flatten();
                self.quality_table_ui(ui);
            }
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(20.));
            ui.set_max_width(520.);
            card(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Add light frames");
                    ui.label(
                        RichText::new("Each folder you add becomes its own light set — for example one night, one exposure time, one filter or one camera. Calibration sets are added next to them and assigned per light set.")
                            .color(MUTED),
                    );
                    ui.add_space(6.);
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 250.).max(0.) / 2.);
                        if ui
                            .add(egui::Button::new(RichText::new("Add light folder…").strong().color(Color32::WHITE)).fill(Color32::from_rgb(14, 106, 147)))
                            .clicked()
                        {
                            self.add_light_folder(ctx);
                        }
                        if ui.button("Add files…").clicked() {
                            self.add_light_files(ctx);
                        }
                    });
                    ui.label(RichText::new("You can also drop folders or FITS files onto the window. Originals are only read, never changed.").small().color(MUTED));
                });
            });
        });
    }

    fn overview(&mut self, ui: &mut egui::Ui) {
        let mixed = self.mixed_filters();
        let mixed_layouts = self.mixed_layouts();
        let main_filter = self.main_filter();
        let mut clicked = None;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Set overview");
                ui.label(
                    RichText::new(format!(
                        "{} light sets · {} calibration sets · {} in this stack",
                        self.light_sets.len(),
                        self.calibration_sets.len(),
                        self.included_sets().count()
                    ))
                    .color(MUTED),
                );
            });
            if mixed {
                ui.label(RichText::new(format!(
                    "⚠ This stack mixes filters: {}. Allowed, but background level, colour balance and star profiles differ between filters. Check the master carefully or exclude sets to stack one filter only.",
                    self.included_filters().join(" + ")
                )).color(WARN));
            }
            if mixed_layouts {
                ui.label(RichText::new("⚠ Mono and colour sets in one stack: monochrome frames contribute equally to R, G and B.").color(WARN));
            }
            egui::Grid::new("set_overview")
                .num_columns(6)
                .striped(true)
                .spacing([14., 4.])
                .show(ui, |ui| {
                    for h in [
                        "Light set",
                        "Frames",
                        "Exposure",
                        "Filter",
                        "Calibration set",
                        "Status",
                    ] {
                        ui.label(RichText::new(h).small().color(MUTED));
                    }
                    ui.end_row();
                    for set in &self.light_sets {
                        if ui.link(&set.name).clicked() {
                            clicked = Some(set.id);
                        }
                        let summary = set.summary.as_ref();
                        ui.label(summary.map_or("…".into(), |s| s.files.to_string()));
                        ui.label(summary.map_or("…".into(), |s| s.exposure()));
                        let other_filter = mixed
                            && set.include
                            && summary.is_some_and(|s| s.filters.first() != main_filter.as_ref());
                        ui.label(
                            RichText::new(summary.map_or("…".into(), |s| s.filter())).color(
                                if other_filter {
                                    WARN
                                } else {
                                    Color32::from_rgb(227, 234, 240)
                                },
                            ),
                        );
                        let calibration = set.calibration.and_then(|id| self.calibration(id));
                        ui.label(calibration.map_or("—".into(), |c| c.name.clone()));
                        let (status, color) = if !set.include {
                            ("Excluded by you".to_owned(), MUTED)
                        } else if set.needs_layout() {
                            ("Choose colour or mono".to_owned(), WARN)
                        } else if self.size_differs(set) {
                            ("Different sensor size · rejected".to_owned(), WARN)
                        } else if let (Some(c), Some(s)) = (calibration, summary) {
                            let level = sets::compatibility(s, c).0;
                            (
                                match level {
                                    Level::Ok => "Ready · calibration assigned",
                                    Level::Warn => "Ready · calibration warnings",
                                    Level::Bad => "Calibration incompatible",
                                }
                                .to_owned(),
                                level_color(level),
                            )
                        } else {
                            ("Ready · uncalibrated".to_owned(), GOOD)
                        };
                        let status = if other_filter {
                            format!("{status} · other filter")
                        } else {
                            status
                        };
                        ui.label(RichText::new(status).color(color));
                        ui.end_row();
                    }
                });
            ui.label(
                RichText::new("The assigned calibration set is applied before measuring and stacking: bias, darks (scaled by exposure when a bias is present) and flats.")
                    .small()
                    .color(MUTED),
            );
        });
        if let Some(id) = clicked {
            self.selection = Selection::Light(id);
        }
    }

    fn light_header(&mut self, ui: &mut egui::Ui, id: u64) {
        let editable = !self.busy();
        let mixed = self.mixed_filters();
        let filters = self.included_filters();
        let size_differs = self.light(id).is_some_and(|s| self.size_differs(s));
        let calibration_options: Vec<(u64, String, Level, Vec<sets::Check>)> = {
            let set = self.light(id).unwrap();
            self.calibration_sets
                .iter()
                .map(|c| {
                    let (level, checks) = set
                        .summary
                        .as_ref()
                        .map_or((Level::Warn, vec![]), |s| sets::compatibility(s, c));
                    (c.id, c.name.clone(), level, checks)
                })
                .collect()
        };
        let mut include_change = None;
        let mut layout_change = None;
        let mut calibration_change = None;
        let mut remove = false;
        let set = self.light_mut(id).unwrap();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_enabled(
                    editable,
                    egui::TextEdit::singleline(&mut set.name)
                        .font(egui::TextStyle::Heading)
                        .desired_width(260.),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(editable, egui::Button::new("Remove set"))
                        .clicked()
                    {
                        remove = true;
                    }
                    let mut include = set.include;
                    if ui
                        .add_enabled(
                            editable,
                            egui::Checkbox::new(&mut include, "Include in stack"),
                        )
                        .on_hover_text("Exclude the whole set from this stack without removing it")
                        .changed()
                    {
                        include_change = Some(include);
                    }
                });
            });
            let sources: Vec<String> = set.sources.iter().map(|p| display_path(p)).collect();
            ui.add(
                egui::Label::new(RichText::new(sources.join(" · ")).small().color(MUTED))
                    .truncate(),
            )
            .on_hover_text(sources.join("\n"));
            if let Some(s) = &set.summary {
                ui.horizontal_wrapped(|ui| {
                    let mut prop = |k: &str, v: String| {
                        ui.label(RichText::new(k).color(MUTED));
                        ui.label(v);
                        ui.add_space(8.);
                    };
                    prop("Frames", s.files.to_string());
                    prop("Exposure", s.exposure());
                    prop("Filter", s.filter());
                    prop("Camera", s.camera());
                    prop("Sensor", format!("{} × {}", s.width, s.height));
                    if !s.gains.is_empty() {
                        prop(
                            "Gain",
                            s.gains
                                .iter()
                                .map(|g| sets::trim_number(*g))
                                .collect::<Vec<_>>()
                                .join(" + "),
                        );
                    }
                    if let Some(t) = s.temperature {
                        prop("Temp.", format!("{t:.0} °C"));
                    }
                    if let Some(n) = s.night() {
                        prop("Night", n);
                    }
                });
                if s.unreadable > 0 {
                    ui.label(
                        RichText::new(format!(
                            "⚠ {} files have unreadable FITS headers and will be rejected.",
                            s.unreadable
                        ))
                        .color(WARN),
                    );
                }
                if s.sizes_differ {
                    ui.label(RichText::new("⚠ Frames in this set have different sizes; frames that differ from the majority will be rejected.").color(WARN));
                }
            } else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading FITS headers…");
                });
            }
            if size_differs && set.include {
                ui.label(RichText::new("⚠ Different sensor size than the main light set. Cross-camera registration is not available yet, so these frames will be rejected.").color(WARN));
            }
            if mixed && set.include {
                ui.label(RichText::new(format!(
                    "⚠ Mixed filters in this stack: {}. Allowed, but check the master carefully, or exclude this set to stack one filter only.",
                    filters.join(" + ")
                )).color(WARN));
            }
            ui.separator();
            ui.label(RichText::new("Sensor layout").strong());
            let header = set.summary.as_ref().and_then(|s| s.header_layout);
            let header_text = match header {
                Some(sets::HeaderLayout::Bayer(p)) => {
                    format!("From FITS header · colour {}", p.name())
                }
                Some(sets::HeaderLayout::Missing) => {
                    "From FITS header · no Bayer pattern → mono".into()
                }
                Some(sets::HeaderLayout::Mixed) => "From FITS header · files disagree".into(),
                None => "From FITS header".into(),
            };
            if set.needs_layout() {
                ui.label(RichText::new("Some files record a Bayer colour mask and others do not. Choose colour (with its pattern) or mono before measuring or stacking.").color(WARN));
            }
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .radio(set.layout == LayoutChoice::Auto, header_text)
                        .clicked()
                    {
                        layout_change = Some(LayoutChoice::Auto);
                    }
                    let colour = matches!(set.layout, LayoutChoice::Bayer(_));
                    if ui.radio(colour, "Colour (Bayer mask)").clicked() && !colour {
                        let pattern = match header {
                            Some(sets::HeaderLayout::Bayer(p)) => p,
                            _ => fulgur_core::BayerPattern::RGGB,
                        };
                        layout_change = Some(LayoutChoice::Bayer(pattern));
                    }
                    if let LayoutChoice::Bayer(current) = set.layout {
                        egui::ComboBox::from_id_salt(("bayer", id))
                            .selected_text(current.name())
                            .width(70.)
                            .show_ui(ui, |ui| {
                                for p in [
                                    fulgur_core::BayerPattern::RGGB,
                                    fulgur_core::BayerPattern::GRBG,
                                    fulgur_core::BayerPattern::GBRG,
                                    fulgur_core::BayerPattern::BGGR,
                                ] {
                                    if ui.selectable_label(current == p, p.name()).clicked() {
                                        layout_change = Some(LayoutChoice::Bayer(p));
                                    }
                                }
                            });
                    }
                    if ui
                        .radio(
                            set.layout == LayoutChoice::Mono,
                            "Mono (no Bayer mask) · skip debayering",
                        )
                        .clicked()
                    {
                        layout_change = Some(LayoutChoice::Mono);
                    }
                });
            });
            ui.separator();
            ui.label(RichText::new("Calibration set for these lights").strong());
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let seestar = set.summary.as_ref().is_some_and(|s| s.is_seestar());
                    let none_hint = if seestar {
                        "Seestar default"
                    } else {
                        "Not recommended"
                    };
                    if ui
                        .selectable_label(set.calibration.is_none(), format!("None · {none_hint}"))
                        .clicked()
                    {
                        calibration_change = Some(None);
                    }
                    for (cid, name, level, _) in &calibration_options {
                        let label = format!(
                            "{name} · {}",
                            match level {
                                Level::Ok => "Matches",
                                Level::Warn => "Check warnings",
                                Level::Bad => "Incompatible",
                            }
                        );
                        let response = ui.add_enabled(
                            *level != Level::Bad,
                            egui::SelectableLabel::new(
                                set.calibration == Some(*cid),
                                RichText::new(label).color(level_color(*level)),
                            ),
                        );
                        if response.clicked() {
                            calibration_change = Some(Some(*cid));
                        }
                    }
                });
            });
            let checks = set
                .calibration
                .and_then(|cid| calibration_options.iter().find(|o| o.0 == cid))
                .map(|o| o.3.clone());
            match checks {
                Some(checks) => {
                    for check in checks {
                        ui.label(
                            RichText::new(format!("{} {}", level_mark(check.level), check.text))
                                .color(level_color(check.level)),
                        );
                    }
                }
                None => {
                    let seestar = set.summary.as_ref().is_some_and(|s| s.is_seestar());
                    ui.label(RichText::new(if seestar {
                        "No calibration · Seestar lights are already bias- and dark-corrected in the telescope."
                    } else {
                        "No calibration · these lights are not pre-calibrated; darks and flats are recommended."
                    }).color(if seestar { MUTED } else { WARN }));
                }
            }
            ui.label(
                RichText::new(
                    "Calibration (bias, darks, flats) is applied with masters built in RAM.",
                )
                .small()
                .color(MUTED),
            );
        });
        if let Some(include) = include_change {
            self.set_include(id, include);
        }
        if let Some(layout) = layout_change {
            self.set_layout(id, layout);
        }
        if let Some(calibration) = calibration_change {
            if let Some(set) = self.light_mut(id) {
                set.calibration = calibration;
            }
        }
        if remove {
            self.remove_light_set(id);
        }
    }

    fn calibration_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, id: u64) {
        let editable = !self.busy();
        let users: Vec<(u64, String)> = self
            .light_sets
            .iter()
            .filter(|l| l.calibration == Some(id))
            .map(|l| (l.id, l.name.clone()))
            .collect();
        let mut remove = false;
        let mut goto = None;
        let mut item_change: Option<(CalibrationKind, Option<CalibrationSource>)> = None;
        let mut toggle: Option<CalibrationKind> = None;
        let mut combine_change: Option<(CalibrationKind, sets::CombineMethod)> = None;
        let mut stack: Option<CalibrationKind> = None;
        let mut save: Option<CalibrationKind> = None;
        let building = self
            .master_task
            .as_ref()
            .filter(|t| t.set == id)
            .map(|t| t.kind);
        let building_fraction = self
            .master_task
            .as_ref()
            .map_or(0., |t| t.progress.load(Ordering::Relaxed) as f32 / 1000.);
        self.update_calibration_preview(id, ctx);
        let mut preview = self.calibration_preview.take();
        let shown = preview.as_ref().map(|p| p.key.1);
        let mut pick: Option<CalibrationKind> = None;
        // Wide enough for the Combine / Stack / Save row; rows wrap below it.
        let left_width = (ui.available_width() * 0.5).clamp(480., 640.);
        let set = self.calibration_mut(id).unwrap();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_enabled(
                    editable,
                    egui::TextEdit::singleline(&mut set.name)
                        .font(egui::TextStyle::Heading)
                        .desired_width(280.),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(editable, egui::Button::new("Remove set"))
                        .clicked()
                    {
                        remove = true;
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Camera").color(MUTED));
                ui.label(set.camera().unwrap_or_else(|| "—".into()));
                ui.add_space(8.);
                ui.label(RichText::new("Flat filter").color(MUTED));
                ui.label(set.flat_filter().unwrap_or_else(|| "—".into()));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Used by").color(MUTED));
                if users.is_empty() {
                    ui.label(
                        RichText::new("no light set yet · assign it in a light set").color(WARN),
                    );
                }
                for (lid, name) in &users {
                    if ui.link(name).clicked() {
                        goto = Some(*lid);
                    }
                }
            });
            ui.label(RichText::new("Masters are built in memory when processing starts and reused while the files are unchanged; they reach disk only with Save master…. Flats use the dark flats (else the bias) as their offset.").small().color(MUTED));
        });
        if set.is_seestar() {
            ui.add_space(6.);
            card(ui, |ui| {
                ui.label(RichText::new("Seestar lights are already bias- and dark-corrected in the telescope, so Bias and Darks start switched off. Flats with dark flats can still remove dust shadows and vignetting.").color(LINK));
            });
        }
        ui.add_space(6.);
        ui.horizontal_top(|ui| {
          ui.vertical(|ui| {
            ui.set_width(left_width);
            for kind in CalibrationKind::ALL {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(kind.title()).strong().size(15.));
                            ui.label(RichText::new(kind.role()).small().color(MUTED));
                        });
                        if building == Some(kind) {
                            ui.add_space(12.);
                            ui.add(
                                egui::ProgressBar::new(building_fraction)
                                    .desired_width(180.)
                                    .show_percentage(),
                            )
                            .on_hover_text("Stacking the master in memory");
                        }
                        if let Some(item) = set.item(kind) {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                let mut enabled = item.enabled;
                                if ui.add_enabled(editable, egui::Checkbox::new(&mut enabled, "Use")).changed() {
                                    toggle = Some(kind);
                                }
                                if Workspace::previewable(item).is_some()
                                    && ui
                                        .selectable_label(shown == Some(kind), "Preview")
                                        .on_hover_text("Show this master in the preview")
                                        .clicked()
                                {
                                    pick = Some(kind);
                                }
                            });
                        }
                    });
                    match set.item(kind) {
                        None => {
                            ui.label(RichText::new(if set.is_seestar() && matches!(kind, CalibrationKind::Bias | CalibrationKind::Dark) {
                                "Not added · usually not needed for Seestar lights."
                            } else {
                                "Not added."
                            }).color(MUTED));
                            ui.add_enabled_ui(editable, |ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("Add frames…").clicked() {
                                        if let Some(files) = rfd::FileDialog::new()
                                            .add_filter("FITS", &["fit", "fits", "fts"])
                                            .pick_files()
                                        {
                                            item_change = Some((kind, Some(CalibrationSource::Frames(files))));
                                        }
                                    }
                                    if ui.button("Add master…").clicked() {
                                        if let Some(file) = rfd::FileDialog::new()
                                            .add_filter("FITS", &["fit", "fits", "fts"])
                                            .pick_file()
                                        {
                                            item_change = Some((kind, Some(CalibrationSource::Master(file))));
                                        }
                                    }
                                });
                            });
                        }
                        Some(item) => {
                            let label = match &item.source {
                                CalibrationSource::Frames(f) => format!("{} frames → master in RAM", f.len()),
                                CalibrationSource::Master(_) => "Master file".into(),
                            };
                            let path = item.first_path().map(display_path).unwrap_or_default();
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(label).small().color(LINK));
                                ui.add(egui::Label::new(
                                    RichText::new(
                                        item.first_path()
                                            .and_then(|p| p.file_name())
                                            .unwrap_or_default()
                                            .to_string_lossy(),
                                    )
                                    .strong(),
                                ).truncate())
                                .on_hover_text(path);
                            });
                            if let Some(m) = &item.summary {
                                ui.horizontal_wrapped(|ui| {
                                    let mut prop = |k: &str, v: String| {
                                        ui.label(RichText::new(k).small().color(MUTED));
                                        ui.label(RichText::new(v).small());
                                        ui.add_space(6.);
                                    };
                                    if let Some(e) = m.exposure_seconds {
                                        prop("Exposure", format!("{} s", sets::trim_number(e)));
                                    }
                                    if let Some(f) = &m.filter {
                                        prop("Filter", f.trim().to_owned());
                                    }
                                    if let Some(g) = m.gain {
                                        prop("Gain", sets::trim_number(g));
                                    }
                                    if let Some(t) = m.ccd_temperature {
                                        prop("Temp.", format!("{t:.0} °C"));
                                    }
                                    if let Some(n) = m.combined_frames {
                                        prop("Combined", format!("{n} frames"));
                                    }
                                    prop("Size", format!("{} × {}", m.width, m.height));
                                });
                            } else {
                                ui.label(RichText::new("Reading header…").small().color(MUTED));
                            }
                            if set.is_seestar() && item.enabled && matches!(kind, CalibrationKind::Bias | CalibrationKind::Dark) {
                                ui.label(RichText::new("⚠ Seestar lights are already corrected · subtracting again can clip faint signal").color(WARN));
                            }
                            {
                                let what = kind.master_kind().title();
                                ui.add_space(2.);
                                ui.add_enabled_ui(editable, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        if matches!(item.source, CalibrationSource::Frames(_)) {
                                            ui.label(RichText::new("Combine").small().color(MUTED));
                                            egui::ComboBox::from_id_salt(("combine", id, kind.index()))
                                                .selected_text(item.combine.label())
                                                .show_ui(ui, |ui| {
                                                    for m in sets::CombineMethod::ALL {
                                                        if ui.selectable_label(item.combine == m, m.label()).clicked() && item.combine != m {
                                                            combine_change = Some((kind, m));
                                                        }
                                                    }
                                                })
                                                .response
                                                .on_hover_text(if kind == CalibrationKind::Flat {
                                                    "Each flat is first scaled to the common median level. Average: plain mean. Median: robust, slightly noisier. Average, sigma-kappa 2: mean after rejecting samples beyond 2 standard deviations; it needs at least 6 frames to reject a single outlier."
                                                } else {
                                                    "Average: plain mean. Median: robust, slightly noisier. Average, sigma-kappa 2: mean after rejecting samples beyond 2 standard deviations; it needs at least 6 frames to reject a single outlier."
                                                });
                                            if ui
                                                .add_enabled(building.is_none(), egui::Button::new("Stack master"))
                                                .on_hover_text(format!("Combine these frames into a master {what} in memory now"))
                                                .clicked()
                                            {
                                                stack = Some(kind);
                                            }
                                        }
                                        if ui
                                            .add_enabled(item.ram.is_some(), egui::Button::new("Save master…"))
                                            .on_hover_text("Write the master in memory to a 32-bit FITS file (ADU) and use that file from now on")
                                            .on_disabled_hover_text("Stack the master first")
                                            .clicked()
                                        {
                                            save = Some(kind);
                                        }
                                    });
                                });
                                let status = if building == Some(kind) {
                                    (format!("Stacking master {what} in memory…"), LINK)
                                } else if let Some(m) = &item.ram {
                                    let s = &m.summary;
                                    (
                                        format!(
                                            "Master in memory · {}{} · median {:.1} ADU",
                                            if s.frames > 0 { format!("{} frames · ", s.frames) } else { String::new() },
                                            s.combine.map_or("loaded file", |c| c.label()),
                                            s.median_adu
                                        ),
                                        LINK,
                                    )
                                } else if matches!(item.source, CalibrationSource::Frames(_)) {
                                    (
                                        "Not stacked yet · it is stacked automatically when processing starts".to_owned(),
                                        MUTED,
                                    )
                                } else {
                                    (
                                        "Master file · loaded when processing starts (ADU or normalized 0..1)".to_owned(),
                                        MUTED,
                                    )
                                };
                                ui.label(RichText::new(status.0).small().color(status.1));
                            }
                            if kind == CalibrationKind::DarkFlat && set.item(CalibrationKind::Flat).is_none() {
                                ui.label(RichText::new("⚠ Dark flats are only used together with flats").color(WARN));
                            }
                            ui.add_enabled_ui(editable, |ui| {
                                ui.horizontal(|ui| {
                                    let swap = match item.source {
                                        CalibrationSource::Frames(_) => "Use a master instead…",
                                        CalibrationSource::Master(_) => "Use frames instead…",
                                    };
                                    if ui.button(swap).clicked() {
                                        let dialog = rfd::FileDialog::new().add_filter("FITS", &["fit", "fits", "fts"]);
                                        item_change = match item.source {
                                            CalibrationSource::Frames(_) => dialog
                                                .pick_file()
                                                .map(|f| (kind, Some(CalibrationSource::Master(f)))),
                                            CalibrationSource::Master(_) => dialog
                                                .pick_files()
                                                .map(|f| (kind, Some(CalibrationSource::Frames(f)))),
                                        };
                                    }
                                    if ui.button("Remove").clicked() {
                                        item_change = Some((kind, None));
                                    }
                                });
                            });
                        }
                    }
                });
                ui.add_space(6.);
            }
            card(ui, |ui| {
                ui.label(
                    RichText::new("HOW LIGHTS USING THIS SET WILL BE CALIBRATED")
                        .small()
                        .color(MUTED),
                );
                ui.monospace(set.formula());
                ui.label(RichText::new("Compatibility with each light set (camera, filter, exposure, gain, temperature, night) is checked where the set is assigned.").small().color(MUTED));
            });
          });
          ui.add_space(8.);
          ui.vertical(|ui| {
            card(ui, |ui| {
                ui.label(RichText::new("MASTER PREVIEW").small().color(MUTED));
                match &mut preview {
                    None => {
                        ui.add_space(8.);
                        ui.label(RichText::new("No master to show yet. Stack a master from frames, or add a master file.").color(MUTED));
                    }
                    Some(p) => {
                        let kind = p.key.1;
                        let item = set.item(kind);
                        let name = match item.map(|i| &i.source) {
                            Some(CalibrationSource::Master(path)) => path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                            _ => "Master in memory".into(),
                        };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(kind.title()).strong());
                            ui.label(RichText::new(name).small().color(MUTED));
                        });
                        if let Some(master) = item.and_then(|i| i.ram.as_ref()) {
                            let s = &master.summary;
                            ui.label(
                                RichText::new(format!(
                                    "{} × {} · {} s · median {:.1} ADU · pedestal {:.0} ADU{}",
                                    s.width,
                                    s.height,
                                    sets::trim_number(s.exposure_seconds),
                                    s.median_adu,
                                    s.pedestal_adu,
                                    if s.frames > 0 { format!(" · {} frames", s.frames) } else { String::new() }
                                ))
                                .small()
                                .color(MUTED),
                            );
                        }
                        ui.label(RichText::new("Auto stretch · display only · the master is unchanged").small().color(MUTED));
                        match (&p.texture, &p.thumbnail, &p.error) {
                            (Some(texture), Some(thumbnail), _) => {
                                p.view.show(
                                    ui,
                                    texture,
                                    thumbnail,
                                    p.plane.clone().map(DetailSource::Plane),
                                    DisplayOptions {
                                        stretch: true,
                                        ..DisplayOptions::default()
                                    },
                                );
                            }
                            (_, _, Some(error)) => {
                                ui.label(RichText::new(format!("Preview unavailable: {error}")).color(WARN));
                            }
                            _ => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(RichText::new("Loading master…").color(MUTED));
                                });
                            }
                        }
                    }
                }
            });
          });
        });
        if let Some(kind) = pick {
            self.calibration_preview_kind = Some(kind);
        }
        self.calibration_preview = preview;
        if let Some((kind, method)) = combine_change {
            if let Some(item) = self
                .calibration_mut(id)
                .and_then(|s| s.items[kind.index()].as_mut())
            {
                item.combine = method;
                item.ram = None;
            }
        }
        if let Some(kind) = stack {
            self.stack_master(id, kind, ctx);
        }
        if let Some(kind) = save {
            self.save_master(id, kind, ctx);
        }
        if let Some(kind) = toggle {
            if let Some(item) = self
                .calibration_mut(id)
                .and_then(|s| s.items[kind.index()].as_mut())
            {
                item.enabled = !item.enabled;
            }
        }
        if let Some((kind, source)) = item_change {
            self.set_calibration_item(id, kind, source, ctx);
        }
        if let Some(lid) = goto {
            self.selection = Selection::Light(lid);
        }
        if remove {
            self.remove_calibration_set(id);
        }
    }
}
