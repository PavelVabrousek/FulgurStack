//! Virtualized quality table. Row highlighting and scientific Use choices are separate.
use super::*;
use std::{borrow::Cow, collections::BTreeSet};

const ROW_HEIGHT: f32 = 20.;
const USE_WIDTH: f32 = 44.;
const COLUMN_COUNT: usize = 14;
const SET: usize = 0;
const FOLDER: usize = 1;
const FRAME: usize = 2;
const CAPTURED: usize = 4;
const STATUS: usize = 11;
const REASON: usize = 12;
const USE: usize = COLUMN_COUNT - 1;
const HEADERS: [&str; COLUMN_COUNT] = [
    "Set",
    "Folder",
    "Frame",
    "Exp. s",
    "Captured",
    "HFR px",
    "Stars",
    "Ecc.",
    "Sky ADU",
    "Noise ADU/s",
    "Weight",
    "Status",
    "Reason",
    "Use",
];
const WIDTHS: [f32; COLUMN_COUNT] = [
    140., 240., 300., 78., 160., 78., 78., 78., 90., 100., 82., 142., 240., USE_WIDTH,
];

#[derive(Clone, Default, PartialEq)]
struct ColumnFilter {
    text: String,
    minimum: String,
    maximum: String,
    choice: usize,
}

impl ColumnFilter {
    fn active(&self) -> bool {
        self.choice != 0
            || !self.text.trim().is_empty()
            || !self.minimum.trim().is_empty()
            || !self.maximum.trim().is_empty()
    }
    fn bound(text: &str) -> Result<Option<f64>, ()> {
        if text.trim().is_empty() {
            Ok(None)
        } else {
            text.trim()
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map(Some)
                .ok_or(())
        }
    }
    fn range(&self) -> bool {
        match (Self::bound(&self.minimum), Self::bound(&self.maximum)) {
            (Ok(Some(min)), Ok(Some(max))) => min <= max,
            (Ok(_), Ok(_)) => true,
            _ => false,
        }
    }
    fn matches_number(&self, value: Option<f64>) -> bool {
        if self.minimum.trim().is_empty() && self.maximum.trim().is_empty() {
            return true;
        }
        let (Ok(min), Ok(max)) = (Self::bound(&self.minimum), Self::bound(&self.maximum)) else {
            return false;
        };
        value.is_some_and(|v| {
            v.is_finite() && min.is_none_or(|m| v >= m) && max.is_none_or(|m| v <= m)
        }) && self.range()
    }
    fn matches_text(&self, value: &str) -> bool {
        let query = self.text.trim();
        query.is_empty() || value.to_lowercase().contains(&query.to_lowercase())
    }
}

#[derive(Clone, Copy)]
enum Action {
    Include,
    Exclude,
    Toggle,
    Automatic,
    Remove,
}

pub(super) struct QualityTable {
    widths: [f32; COLUMN_COUNT],
    filters: [ColumnFilter; COLUMN_COUNT],
    sort: usize,
    descending: bool,
    scroll_x: f32,
    fitted_names: Option<usize>,
    marked: BTreeSet<PathBuf>,
    anchor: Option<PathBuf>,
    pub(super) removed: BTreeSet<PathBuf>,
    /// Show only one light set (index into the inspected sets).
    pub(super) scope: Option<usize>,
    /// Vertical row scroll requested by keyboard navigation, and the last
    /// observed scroll position/viewport height.
    pending_scroll: Option<f32>,
    view_offset: f32,
    view_height: f32,
}

impl Default for QualityTable {
    fn default() -> Self {
        Self {
            widths: WIDTHS,
            filters: std::array::from_fn(|_| ColumnFilter::default()),
            sort: FRAME,
            descending: false,
            scroll_x: 0.,
            fitted_names: None,
            marked: BTreeSet::new(),
            anchor: None,
            removed: BTreeSet::new(),
            scope: None,
            pending_scroll: None,
            view_offset: 0.,
            view_height: 0.,
        }
    }
}

/// Quick selections offered above the frame list: all usable frames, or the
/// best share of each light set.
pub(super) const QUICK_SELECTIONS: [(&str, f64); 5] = [
    ("All frames", 1.0),
    ("Best 95%", 0.95),
    ("Best 90%", 0.90),
    ("Best 80%", 0.80),
    ("Best 50%", 0.50),
];

/// Use decisions that keep the best `fraction` of the usable listed frames
/// of every light set. Quality is the provisional weight 1 / (noise per
/// second² · HFR²): sharper stars and lower exposure-normalized noise rank
/// higher. Each set is ranked on its own, so filters and exposures that
/// differ between sets never compete; at least one frame per set stays on.
/// Unusable frames are left out (their Use stays off).
pub(super) fn best_selection(
    rows: &[QualityRow],
    listed: &[usize],
    fraction: f64,
) -> Vec<(PathBuf, bool)> {
    let score = |row: &QualityRow| match (row.noise, row.hfr) {
        (Some(noise), Some(hfr)) if noise > 0. && hfr > 0. => 1. / (noise * noise * hfr * hfr),
        _ => 0.,
    };
    let mut sets: BTreeMap<usize, Vec<&QualityRow>> = BTreeMap::new();
    for &i in listed {
        if rows[i].can_include {
            sets.entry(rows[i].set).or_default().push(&rows[i]);
        }
    }
    let mut decisions = vec![];
    for (_, mut members) in sets {
        members.sort_by(|a, b| {
            score(b)
                .total_cmp(&score(a))
                .then_with(|| a.file_name.cmp(&b.file_name))
        });
        let keep = ((members.len() as f64 * fraction).ceil() as usize).clamp(1, members.len());
        for (rank, row) in members.into_iter().enumerate() {
            decisions.push((row.path.clone(), rank < keep));
        }
    }
    decisions
}

fn use_frame(row: &QualityRow, choices: &BTreeMap<PathBuf, bool>) -> bool {
    choices
        .get(&row.path)
        .copied()
        .unwrap_or(row.automatic_accepted)
        && row.can_include
}

fn status(
    row: &QualityRow,
    choices: &BTreeMap<PathBuf, bool>,
    final_weights: bool,
) -> &'static str {
    if !row.can_include {
        "Unusable"
    } else if choices.get(&row.path) == Some(&false) {
        "Excluded · manual"
    } else if final_weights && !row.accepted {
        "Rejected · final"
    } else if choices.get(&row.path) == Some(&true) {
        "Included · manual"
    } else if row.accepted {
        "Accepted"
    } else {
        "Rejected"
    }
}

fn numeric(row: &QualityRow, column: usize) -> Option<f64> {
    match column {
        3 => row.exposure,
        5 => row.hfr,
        6 => Some(row.stars as f64),
        7 => row.eccentricity,
        8 => row.background,
        9 => row.noise,
        10 => Some(row.weight),
        _ => None,
    }
}

fn folder(row: &QualityRow) -> Cow<'_, str> {
    let text = row
        .path
        .parent()
        .map(|p| p.to_string_lossy())
        .unwrap_or_default();
    #[cfg(windows)]
    {
        if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
            return Cow::Owned(format!("\\\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix("\\\\?\\") {
            return Cow::Owned(rest.to_owned());
        }
    }
    text
}

fn frame_name(row: &QualityRow) -> Cow<'_, str> {
    row.path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| Cow::Borrowed(&row.file_name))
}

fn cell_text(
    row: &QualityRow,
    column: usize,
    choices: &BTreeMap<PathBuf, bool>,
    final_weights: bool,
) -> String {
    match column {
        SET => row.set_name.clone(),
        FOLDER => folder(row).into_owned(),
        FRAME => frame_name(row).into_owned(),
        3 => whole_seconds(row.exposure),
        CAPTURED => timestamp_seconds(row.date.as_deref()),
        6 => row.stars.to_string(),
        10 => format!("{:.3}", row.weight),
        STATUS => status(row, choices, final_weights).into(),
        REASON => {
            if row.reasons.is_empty() {
                "—".into()
            } else {
                row.reasons.clone()
            }
        }
        USE => if use_frame(row, choices) { "On" } else { "Off" }.into(),
        _ => number(numeric(row, column)),
    }
}

impl QualityTable {
    pub(super) fn reset_rows(&mut self) {
        self.marked.clear();
        self.anchor = None;
        self.removed.clear();
        self.fitted_names = None;
    }

    fn clear_marks(&mut self) {
        self.marked.clear();
        self.anchor = None;
    }

    fn in_scope(&self, row: &QualityRow) -> bool {
        self.scope.is_none_or(|set| row.set == set)
    }

    fn matches_row(&self, row: &QualityRow, choices: &BTreeMap<PathBuf, bool>) -> bool {
        !self.removed.contains(&row.path)
            && self.in_scope(row)
            && self
                .filters
                .iter()
                .enumerate()
                .all(|(column, filter)| match column {
                    SET => filter.matches_text(&row.set_name),
                    FOLDER => filter.matches_text(&folder(row)),
                    FRAME => filter.matches_text(&frame_name(row)),
                    CAPTURED => filter.matches_text(row.date.as_deref().unwrap_or("")),
                    STATUS => match filter.choice {
                        1 => row.accepted,
                        2 => !row.accepted,
                        3 => choices.contains_key(&row.path),
                        4 => choices.get(&row.path) == Some(&true),
                        5 => choices.get(&row.path) == Some(&false),
                        6 => !row.can_include,
                        _ => true,
                    },
                    REASON => filter.matches_text(&row.reasons),
                    USE => match filter.choice {
                        1 => use_frame(row, choices),
                        2 => !use_frame(row, choices),
                        _ => true,
                    },
                    _ => filter.matches_number(numeric(row, column)),
                })
    }

    pub(super) fn footer_status(
        &self,
        rows: &[QualityRow],
        choices: &BTreeMap<PathBuf, bool>,
        final_weights: bool,
    ) -> (String, String) {
        let mut shown = 0;
        let mut marked = 0;
        let mut used = 0;
        let mut listed = 0;
        let mut exposure = 0.;
        for row in rows {
            if self.matches_row(row, choices) {
                shown += 1;
                marked += usize::from(self.marked.contains(&row.path));
            }
            if !self.removed.contains(&row.path) && self.in_scope(row) {
                listed += 1;
                if use_frame(row, choices) {
                    used += 1;
                    exposure += row.exposure.unwrap_or(0.);
                }
            }
        }
        let summary = format!(
            "Shown {shown}/{listed} · Marked {marked} · Use {used} · {} exposure",
            clock(exposure as u64)
        );
        let active = self.filters.iter().filter(|f| f.active()).count();
        let mut detail = format!(
            "CFA · {} weights · Filters {active}",
            if final_weights {
                "Final"
            } else {
                "Provisional"
            }
        );
        if !self.removed.is_empty() {
            detail.push_str(&format!(" · Removed {}", self.removed.len()));
        }
        let accepted = rows.iter().filter(|r| r.accepted).count();
        if final_weights && accepted != used {
            detail.push_str(&format!(" · Stack eligible {accepted}/{}", rows.len()));
        }
        (summary, detail)
    }

    fn visible(
        &self,
        rows: &[QualityRow],
        choices: &BTreeMap<PathBuf, bool>,
        final_weights: bool,
    ) -> Vec<usize> {
        let mut indices: Vec<_> = (0..rows.len())
            .filter(|&i| self.matches_row(&rows[i], choices))
            .collect();
        indices.sort_by(|&a, &b| {
            let a = &rows[a];
            let b = &rows[b];
            let order = match self.sort {
                SET => a.set.cmp(&b.set),
                FOLDER => a.path.parent().cmp(&b.path.parent()),
                FRAME => frame_name(a).cmp(&frame_name(b)),
                CAPTURED => a.date.cmp(&b.date),
                STATUS => status(a, choices, final_weights).cmp(status(b, choices, final_weights)),
                REASON => a.reasons.cmp(&b.reasons),
                USE => use_frame(a, choices).cmp(&use_frame(b, choices)),
                _ => numeric(a, self.sort)
                    .partial_cmp(&numeric(b, self.sort))
                    .unwrap_or(std::cmp::Ordering::Equal),
            }
            .then_with(|| a.file_name.cmp(&b.file_name))
            .then_with(|| a.path.cmp(&b.path));
            if self.descending {
                order.reverse()
            } else {
                order
            }
        });
        indices
    }

    fn mark(
        &mut self,
        index: usize,
        visible: &[usize],
        rows: &[QualityRow],
        modifiers: egui::Modifiers,
    ) {
        let path = &rows[index].path;
        let additive = modifiers.ctrl || modifiers.command;
        if modifiers.shift {
            let anchor = self
                .anchor
                .as_ref()
                .and_then(|path| visible.iter().position(|&i| &rows[i].path == path));
            let current = visible.iter().position(|&i| i == index).unwrap();
            let anchor = anchor.unwrap_or_else(|| {
                self.anchor = Some(path.clone());
                current
            });
            if !additive {
                self.marked.clear();
            }
            for &i in &visible[anchor.min(current)..=anchor.max(current)] {
                self.marked.insert(rows[i].path.clone());
            }
        } else {
            if !additive {
                self.marked.clear();
            }
            if !additive || !self.marked.remove(path) {
                self.marked.insert(path.clone());
            }
            self.anchor = Some(path.clone());
        }
    }

    fn fit_column(
        &mut self,
        ui: &egui::Ui,
        rows: &[QualityRow],
        column: usize,
        choices: &BTreeMap<PathBuf, bool>,
        final_weights: bool,
    ) {
        let font = egui::FontId::proportional(13.);
        let width = rows
            .iter()
            .filter(|row| !self.removed.contains(&row.path))
            .map(|row| {
                ui.painter()
                    .layout_no_wrap(
                        cell_text(row, column, choices, final_weights),
                        font.clone(),
                        Color32::WHITE,
                    )
                    .size()
                    .x
            })
            .fold(WIDTHS[column], f32::max);
        self.widths[column] = width + 16.;
    }

    fn action_menu(ui: &mut egui::Ui, editable: bool, include_use: bool) -> Option<Action> {
        let mut action = None;
        ui.add_enabled_ui(editable, |ui| {
            for (label, value) in [
                ("Use on", Action::Include),
                ("Use off", Action::Exclude),
                ("Toggle Use", Action::Toggle),
                ("Automatic for marked", Action::Automatic),
                ("Remove from list", Action::Remove),
            ] {
                if !include_use && matches!(value, Action::Include | Action::Exclude | Action::Toggle) { continue; }
                let response = ui.button(label);
                let response = if matches!(value, Action::Remove) {
                    response.on_hover_text("Hide from this quality list and turn Use off. Files stay in place. Restore via More; a fresh inspection resets the list.")
                } else { response };
                if response.clicked() {
                    action = Some(value);
                    ui.close_menu();
                }
            }
        });
        action
    }
}

impl Workspace {
    pub(super) fn quality_table_ui(&mut self, ui: &mut egui::Ui) {
        let editable =
            self.inspection.is_some() && (!self.busy() || self.waiting) && !self.confirm_close;
        let old_filters = self.quality_table.filters.clone();
        let mut action = None;
        let mut quick = None;
        let mut restore = false;
        let mut clear_all = false;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(editable, |ui| {
                ui.menu_button("Select…", |ui| {
                    for (label, fraction) in QUICK_SELECTIONS {
                        if ui.button(label).clicked() {
                            quick = Some((label, fraction));
                            ui.close_menu();
                        }
                    }
                    ui.label(
                        RichText::new("Applies to the listed frames (set and filters). Best = sharpest stars and lowest exposure-normalized noise, ranked within each light set.")
                            .small()
                            .weak(),
                    );
                })
                .response
                .on_hover_text("Use all usable frames, or only the best 95% / 90% / 80% / 50% of each light set");
            });
            ui.separator();
            ui.add_enabled_ui(editable && !self.quality_table.marked.is_empty(), |ui| {
                for (label, value) in [
                    ("Use on", Action::Include),
                    ("Use off", Action::Exclude),
                    ("Toggle Use", Action::Toggle),
                ] {
                    if ui.small_button(label).clicked() {
                        action = Some(value);
                    }
                }
                ui.menu_button("Marked…", |ui| {
                    action = QualityTable::action_menu(ui, true, false).or(action);
                });
            });
            ui.separator();
            if ui.small_button("Clear filters").clicked() {
                self.quality_table.filters = std::array::from_fn(|_| ColumnFilter::default());
            }
            ui.menu_button("More", |ui| {
                if ui
                    .add_enabled(editable, egui::Button::new("Automatic for all listed"))
                    .clicked()
                {
                    clear_all = true;
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        editable && !self.quality_table.removed.is_empty(),
                        egui::Button::new("Restore removed"),
                    )
                    .clicked()
                {
                    restore = true;
                    ui.close_menu();
                }
                if ui.button("Fit filename column").clicked() {
                    self.quality_table.fitted_names = None;
                    ui.close_menu();
                }
                ui.separator();
                ui.checkbox(&mut self.show_quality_chart, "Show HFR plot");
                ui.checkbox(&mut self.show_preview, "Show selected-frame preview");
            });
        });
        if old_filters != self.quality_table.filters {
            self.quality_table.clear_marks();
            ui.ctx().request_repaint();
        }
        if restore {
            for path in &self.quality_table.removed {
                self.manual_selection.insert(path.clone(), false);
            }
            self.quality_table.removed.clear();
            self.quality_table.clear_marks();
            self.quality_table.fitted_names = None;
            self.message = "Removed frames restored to the list. Their Use remains off until changed explicitly.".into();
            ui.ctx().request_repaint();
        }
        if clear_all {
            self.manual_selection
                .retain(|path, _| self.quality_table.removed.contains(path));
            self.refresh_quality_selection();
            ui.ctx().request_repaint();
        }
        let mut visible =
            self.quality_table
                .visible(&self.rows, &self.manual_selection, self.final_weights);
        let visible_paths: BTreeSet<_> =
            visible.iter().map(|&i| self.rows[i].path.clone()).collect();
        self.quality_table
            .marked
            .retain(|path| visible_paths.contains(path));
        if self.rows.is_empty() {
            ui.label("Add inputs, then inspect quality or start stacking.");
            return;
        }
        if self.show_quality_chart {
            self.chart(ui, &visible);
        }
        if self.quality_table.fitted_names != Some(self.rows.len()) {
            self.quality_table.fit_column(
                ui,
                &self.rows,
                FRAME,
                &self.manual_selection,
                self.final_weights,
            );
            self.quality_table.fitted_names = Some(self.rows.len());
        }
        let mut focus = None;
        let mut single_use = None;
        let table_id = ui.id().with("quality_table_focus");
        let keyboard = ui.memory(|m| m.has_focus(table_id)) && !self.confirm_close;
        if keyboard {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
                self.quality_table.marked = visible_paths;
                ui.ctx().request_repaint();
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.quality_table.clear_marks();
                ui.ctx().request_repaint();
            }
            // Arrow keys move the selected row (Shift extends the marked range).
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    table_id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        escape: true,
                        ..Default::default()
                    },
                )
            });
            let mut step = 0_isize;
            let mut modifiers = egui::Modifiers::NONE;
            for (mods, key, delta) in [
                (egui::Modifiers::NONE, egui::Key::ArrowDown, 1),
                (egui::Modifiers::NONE, egui::Key::ArrowUp, -1),
                (egui::Modifiers::SHIFT, egui::Key::ArrowDown, 1),
                (egui::Modifiers::SHIFT, egui::Key::ArrowUp, -1),
            ] {
                if ui.input_mut(|i| i.consume_key(mods, key)) {
                    step = delta;
                    modifiers = mods;
                }
            }
            if step != 0 && !visible.is_empty() {
                let current = self
                    .selected
                    .and_then(|s| visible.iter().position(|&i| i == s));
                let position = match current {
                    Some(p) => (p as isize + step).clamp(0, visible.len() as isize - 1) as usize,
                    None => 0,
                };
                let index = visible[position];
                if !modifiers.shift {
                    self.quality_table.anchor = None;
                }
                self.quality_table
                    .mark(index, &visible, &self.rows, modifiers);
                focus = Some(index);
                // Keep the new row inside the visible part of the list.
                let top = position as f32 * ROW_HEIGHT;
                let table = &mut self.quality_table;
                if top < table.view_offset {
                    table.pending_scroll = Some(top);
                } else if top + ROW_HEIGHT > table.view_offset + table.view_height {
                    table.pending_scroll = Some(top + ROW_HEIGHT - table.view_height);
                }
                ui.ctx().request_repaint();
            }
        }
        let reference = self.registration_reference();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            ui.spacing_mut().interact_size.y = ROW_HEIGHT;
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            let available = ui.available_rect_before_wrap();
            let height = available.height().max(80.);
            let area = egui::Rect::from_min_size(available.min, egui::vec2(available.width(), height));
            ui.allocate_rect(area, egui::Sense::hover());
            ui.interact(area, table_id, egui::Sense::click());
            let bar_width = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
            let use_right = area.right() - bar_width;
            let use_width = self.quality_table.widths[USE];
            let data_right = use_right - use_width;
            let bottom = egui::Rect::from_min_max(egui::pos2(area.left(), area.bottom() - 18.), egui::pos2(data_right, area.bottom()));
            let total: f32 = self.quality_table.widths[..USE].iter().sum();
            ui.scope_builder(egui::UiBuilder::new().max_rect(bottom), |ui| {
                if ui.rect_contains_pointer(area) {
                    let delta = ui.input_mut(|i| {
                        let delta = i.smooth_scroll_delta.x;
                        i.smooth_scroll_delta.x = 0.;
                        delta
                    });
                    self.quality_table.scroll_x = (self.quality_table.scroll_x - delta).max(0.);
                }
                let output = egui::ScrollArea::horizontal().id_salt("quality_horizontal")
                    .horizontal_scroll_offset(self.quality_table.scroll_x)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .auto_shrink([false, false]).max_height(18.)
                    .show(ui, |ui| { ui.allocate_space(egui::vec2(total, 1.)); });
                self.quality_table.scroll_x = output.state.offset.x;
            });
            let header = egui::Rect::from_min_max(area.min, egui::pos2(use_right, area.top() + 46.));
            let filter_before = self.quality_table.filters.clone();
            let sort_before = (self.quality_table.sort, self.quality_table.descending);
            let mut x = area.left() - self.quality_table.scroll_x;
            for (column, label) in HEADERS.iter().enumerate() {
                let (left, width) = if column == USE { (data_right, use_width) } else { (x, self.quality_table.widths[column]) };
                let clip = if column == USE { header } else { egui::Rect::from_min_max(header.min, egui::pos2(data_right, header.bottom())) };
                let rect = egui::Rect::from_min_size(egui::pos2(left, header.top()), egui::vec2(width, 22.));
                if rect.intersects(clip) {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).id_salt(("quality_header", column)), |ui| {
                        ui.set_clip_rect(clip.intersect(ui.clip_rect()));
                        let arrow = if self.quality_table.sort == column { if self.quality_table.descending { " ↓" } else { " ↑" } } else { "" };
                        let filtered = if self.quality_table.filters[column].active() { " *" } else { "" };
                        if ui.put(rect.shrink2(egui::vec2(2., 1.)), egui::Button::new(format!("{label}{filtered}{arrow}"))).clicked() {
                            if self.quality_table.sort == column { self.quality_table.descending = !self.quality_table.descending; }
                            else { self.quality_table.sort = column; self.quality_table.descending = false; }
                        }
                        let filter_rect = egui::Rect::from_min_size(egui::pos2(left + 2., header.top() + 23.), egui::vec2(width - 4., 21.));
                        ui.scope_builder(egui::UiBuilder::new().max_rect(filter_rect).id_salt(("filter", column)), |ui| {
                            ui.set_clip_rect(clip.intersect(ui.clip_rect()));
                            let filter = &mut self.quality_table.filters[column];
                            match column {
                                STATUS | USE => {
                                    let labels: &[&str] = if column == STATUS { &["All states", "Accepted", "Rejected", "Manual", "Included manually", "Excluded manually", "Unusable"] } else { &["All", "On", "Off"] };
                                    egui::ComboBox::from_id_salt(column).width(width - 8.).selected_text(labels[filter.choice]).show_ui(ui, |ui| {
                                        for (index, label) in labels.iter().enumerate() { ui.selectable_value(&mut filter.choice, index, *label); }
                                    });
                                }
                                SET | FOLDER | FRAME | CAPTURED | REASON => { ui.put(filter_rect, egui::TextEdit::singleline(&mut filter.text).hint_text("Contains…")); }
                                _ => {
                                    let half = (filter_rect.width() - 3.) / 2.;
                                    let min = egui::Rect::from_min_size(filter_rect.min, egui::vec2(half, filter_rect.height()));
                                    let max = min.translate(egui::vec2(half + 3., 0.));
                                    let valid = filter.range();
                                    if !valid { ui.visuals_mut().override_text_color = Some(Color32::LIGHT_RED); }
                                    ui.put(min, egui::TextEdit::singleline(&mut filter.minimum).hint_text("Min")).on_hover_text("Inclusive minimum; blank means no limit.");
                                    ui.put(max, egui::TextEdit::singleline(&mut filter.maximum).hint_text("Max")).on_hover_text("Inclusive maximum; blank means no limit. Missing measurements never match an active range.");
                                }
                            }
                        });
                        if column != USE {
                            let handle = egui::Rect::from_min_max(egui::pos2(rect.right() - 3., rect.top()), egui::pos2(rect.right() + 3., header.bottom()));
                            let response = ui.interact(handle, ui.id().with(("resize", column)), egui::Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                            if response.dragged() { self.quality_table.widths[column] = (self.quality_table.widths[column] + ui.input(|i| i.pointer.delta().x)).max(68.); }
                            if response.double_clicked() { self.quality_table.fit_column(ui, &self.rows, column, &self.manual_selection, self.final_weights); }
                        } else {
                            let handle = egui::Rect::from_min_max(egui::pos2(data_right - 3., header.top()), egui::pos2(data_right + 3., header.bottom()));
                            let response = ui.interact(handle, ui.id().with("resize_use"), egui::Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                            if response.dragged() { self.quality_table.widths[USE] = (use_width - ui.input(|i| i.pointer.delta().x)).clamp(USE_WIDTH, 120.); }
                            if response.double_clicked() { self.quality_table.widths[USE] = USE_WIDTH; }
                        }
                    });
                }
                if column != USE { x += width; }
            }
            if filter_before != self.quality_table.filters {
                self.quality_table.clear_marks();
                ui.ctx().request_repaint();
            }
            if filter_before != self.quality_table.filters || sort_before != (self.quality_table.sort, self.quality_table.descending) {
                visible = self.quality_table.visible(&self.rows, &self.manual_selection, self.final_weights);
                ui.ctx().request_repaint();
            }
            let body = egui::Rect::from_min_max(egui::pos2(area.left(), header.bottom() + 2.), egui::pos2(area.right(), bottom.top()));
            ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                let mut rows_area = egui::ScrollArea::vertical().id_salt("quality_rows")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .auto_shrink([false, false]);
                if let Some(offset) = self.quality_table.pending_scroll.take() {
                    rows_area = rows_area.vertical_scroll_offset(offset.max(0.));
                }
                let output = rows_area.show_rows(ui, ROW_HEIGHT, visible.len(), |ui, range| {
                        for position in range {
                            let index = visible[position];
                            let row = &self.rows[index];
                            let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::click());
                            let marked = self.quality_table.marked.contains(&row.path);
                            // Registration reference: wine (automatic) or lighter red (manual).
                            let reference_row = reference.filter(|(i, _, _)| *i == index);
                            let reference_fill = reference_row.map(|(_, manual, _)| if manual { REFERENCE_MANUAL_FILL } else { REFERENCE_AUTO_FILL });
                            let color = if marked { ui.visuals().selection.bg_fill } else if let Some(fill) = reference_fill { fill } else if position % 2 == 1 { Color32::from_rgb(14, 31, 46) } else { Color32::from_rgb(9, 24, 38) };
                            ui.painter().rect_filled(rect, 0., color);
                            if let (true, Some(fill)) = (marked, reference_fill) {
                                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(6., ROW_HEIGHT)), 0., fill);
                            }
                            if self.selected == Some(index) { ui.painter().vline(rect.left(), rect.y_range(), egui::Stroke::new(2_f32, Color32::from_rgb(84, 190, 221))); }
                            let data_clip = rect.intersect(ui.clip_rect()).intersect(egui::Rect::from_min_max(rect.min, egui::pos2(data_right, rect.bottom())));
                            let painter = ui.painter().with_clip_rect(data_clip);
                            let text_color = if marked { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
                            let mut x = area.left() - self.quality_table.scroll_x;
                            for column in 0..USE {
                                let width = self.quality_table.widths[column];
                                let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, ROW_HEIGHT));
                                if cell.intersects(data_clip) {
                                    let numeric = !matches!(column, SET | FOLDER | FRAME | CAPTURED | STATUS | REASON);
                                    let galley = painter.layout_no_wrap(cell_text(row, column, &self.manual_selection, self.final_weights), egui::FontId::proportional(13.), text_color);
                                    let keep_tail = matches!(column, FOLDER | FRAME | CAPTURED) && galley.size().x > cell.width() - 12.;
                                    let point = egui::pos2(if numeric || keep_tail { cell.right() - 6. - galley.size().x } else { cell.left() + 6. }, cell.center().y - galley.size().y * 0.5);
                                    painter.with_clip_rect(cell.shrink2(egui::vec2(6., 0.)).intersect(data_clip)).galley(point, galley, text_color);
                                    painter.vline(cell.right(), rect.y_range(), egui::Stroke::new(0.5_f32, Color32::from_rgb(33, 50, 65)));
                                }
                                x += width;
                            }
                            let use_rect = egui::Rect::from_min_size(egui::pos2(data_right, rect.top()), egui::vec2(use_width, ROW_HEIGHT));
                            ui.painter().rect_filled(use_rect, 0., color);
                            let mut included = use_frame(row, &self.manual_selection);
                            let checkbox = ui.scope_builder(egui::UiBuilder::new().max_rect(use_rect).id_salt(("quality_use", &row.path)), |ui| {
                                ui.add_enabled_ui(editable && (included || row.can_include), |ui| {
                                    let checkbox_rect = egui::Rect::from_center_size(use_rect.center(), egui::vec2(24., ROW_HEIGHT));
                                    ui.put(checkbox_rect, egui::Checkbox::without_text(&mut included))
                                }).inner
                            }).inner;
                            if checkbox.changed() { single_use = Some((row.path.clone(), included)); }
                            if response.clicked() && !checkbox.clicked() {
                                self.quality_table.mark(index, &visible, &self.rows, ui.input(|i| i.modifiers));
                                ui.memory_mut(|m| m.request_focus(table_id));
                                focus = Some(index);
                            }
                            if response.secondary_clicked() {
                                if !self.quality_table.marked.contains(&row.path) {
                                    self.quality_table.clear_marks();
                                    self.quality_table.marked.insert(row.path.clone());
                                    self.quality_table.anchor = Some(row.path.clone());
                                }
                                focus = Some(index);
                                ui.memory_mut(|m| m.request_focus(table_id));
                            }
                            let reference_note = reference_row.map(|(_, _, text)| format!("{text}\n")).unwrap_or_default();
                            response.clone().on_hover_text(format!("{reference_note}{}\nCaptured: {}\n{}\n{}\nCtrl: add/remove · Shift: select range · ↑/↓: move", display_path(&row.path), timestamp_seconds(row.date.as_deref()), status(row, &self.manual_selection, self.final_weights), row.reasons));
                            response.context_menu(|ui| { action = QualityTable::action_menu(ui, editable, true).or(action); });
                        }
                    });
                self.quality_table.view_offset = output.state.offset.y;
                self.quality_table.view_height = output.inner_rect.height();
            });
        });
        if let Some(index) = focus {
            self.select(index);
            ui.ctx().request_repaint();
        }
        if let Some((path, included)) = single_use {
            self.manual_selection.insert(path, included);
            self.refresh_quality_selection();
            ui.ctx().request_repaint();
        }
        if let Some(action) = action {
            self.quality_action(action, &visible);
            ui.ctx().request_repaint();
        }
        if let Some((label, fraction)) = quick {
            self.quick_selection(label, fraction, &visible);
            ui.ctx().request_repaint();
        }
    }

    fn quick_selection(&mut self, label: &str, fraction: f64, visible: &[usize]) {
        if self.inspection.is_none() || (self.busy() && !self.waiting) {
            return;
        }
        let decisions = best_selection(&self.rows, visible, fraction);
        let on = decisions.iter().filter(|(_, include)| *include).count();
        let unusable = visible
            .iter()
            .filter(|&&i| !self.rows[i].can_include)
            .count();
        for (path, include) in decisions {
            self.manual_selection.insert(path, include);
        }
        self.refresh_quality_selection();
        self.message = format!(
            "{label}: Use on for {on} frames, off for {} · {unusable} unusable frames stay off.",
            visible.len() - on - unusable
        );
    }

    fn quality_action(&mut self, action: Action, visible: &[usize]) {
        if self.inspection.is_none() || (self.busy() && !self.waiting) {
            return;
        }
        let mut changed = 0;
        let mut unavailable = 0;
        for &index in visible {
            let row = &self.rows[index];
            if !self.quality_table.marked.contains(&row.path) {
                continue;
            }
            let include = match action {
                Action::Include => true,
                Action::Toggle => !use_frame(row, &self.manual_selection),
                Action::Exclude | Action::Remove => false,
                Action::Automatic => {
                    self.manual_selection.remove(&row.path);
                    changed += 1;
                    continue;
                }
            };
            if include && !row.can_include {
                unavailable += 1;
                continue;
            }
            self.manual_selection.insert(row.path.clone(), include);
            if matches!(action, Action::Remove) {
                self.quality_table.removed.insert(row.path.clone());
            }
            changed += 1;
        }
        if matches!(action, Action::Remove) {
            self.quality_table.clear_marks();
            self.selected = None;
            self.preview_task = None;
            self.preview = None;
            self.preview_texture = None;
            self.preview_view.reset();
            self.quality_table.fitted_names = None;
        }
        self.refresh_quality_selection();
        self.message =
            format!("Updated {changed} marked frames · {unavailable} unusable frames skipped.");
    }
}
