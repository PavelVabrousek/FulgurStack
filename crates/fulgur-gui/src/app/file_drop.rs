//! Folders, FITS and XISF files dragged from the file manager into sets.
//! Files are only added to the workspace, never moved or changed.

use super::compact::WARN;
use super::*;

/// Where dragged files land. Outside every zone they form a new light set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DropTarget {
    Light(u64),
    CalibrationSet(u64),
    CalibrationItem(u64, CalibrationKind),
    NewCalibrationSet,
}

/// The pointer during a file drag, in points. Windows sends the window no
/// mouse moves while another program drags, so it is asked directly.
fn drag_position(ctx: &egui::Context) -> Option<egui::Pos2> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};
        let mut cursor = POINT { x: 0, y: 0 };
        // SAFETY: the pointer refers to a live local POINT for this call only.
        let read = unsafe { GetCursorPos(&mut cursor) } != 0;
        if let (true, Some(window)) = (read, ctx.input(|i| i.viewport().inner_rect)) {
            let scale = ctx.pixels_per_point();
            return Some(
                egui::pos2(cursor.x as f32 / scale, cursor.y as f32 / scale) - window.min.to_vec2(),
            );
        }
    }
    ctx.input(|i| i.pointer.latest_pos())
}

impl Workspace {
    fn drop_hint(&self, target: Option<DropTarget>) -> String {
        let calibration = |id| self.calibration(id).map_or("", |s| s.name.as_str());
        match target {
            None => "New light set".into(),
            Some(DropTarget::Light(id)) => format!(
                "Add to light set “{}”",
                self.light(id).map_or("", |s| s.name.as_str())
            ),
            Some(DropTarget::CalibrationSet(id)) => {
                format!("Sort into “{}” by FITS headers", calibration(id))
            }
            Some(DropTarget::CalibrationItem(id, kind)) => {
                format!("Add to {} of “{}”", kind.title(), calibration(id))
            }
            Some(DropTarget::NewCalibrationSet) => "New calibration set".into(),
        }
    }

    /// Show where a dragged file would land, and take it on the drop.
    pub(super) fn file_drag(&mut self, ctx: &egui::Context) {
        let (hovering, dropped): (bool, Vec<PathBuf>) = ctx.input(|i| {
            (
                !i.raw.hovered_files.is_empty(),
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect(),
            )
        });
        let accepting = !self.busy()
            && !self.confirm_close
            && self.pending_clear.is_none()
            && self.pending_start.is_none()
            && self.pending_calibration_file.is_none();
        if !accepting || (!hovering && dropped.is_empty()) {
            return;
        }
        let zone = drag_position(ctx).and_then(|position| {
            self.drop_zones
                .iter()
                .rev()
                .find(|(rect, _)| rect.contains(position))
                .copied()
        });
        if !dropped.is_empty() {
            self.take_drop(zone.map(|z| z.1), dropped, ctx);
            return;
        }
        let area = zone.map_or(ctx.screen_rect().shrink(2.), |z| z.0);
        let hint = self.drop_hint(zone.map(|z| z.1));
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("file_drag"),
        ));
        painter.rect_stroke(
            area,
            8.,
            egui::Stroke::new(2_f32, WARN),
            egui::StrokeKind::Inside,
        );
        let label = painter.layout_no_wrap(
            hint,
            egui::FontId::proportional(14.),
            Color32::from_rgb(11, 28, 45),
        );
        let at = egui::pos2(area.center().x - label.size().x / 2., area.top() + 10.);
        painter.rect_filled(
            egui::Rect::from_min_size(at, label.size()).expand(6.),
            4.,
            WARN,
        );
        painter.galley(at, label, Color32::WHITE);
        // The window gets no input during a drag; keep following the pointer.
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    fn take_drop(&mut self, target: Option<DropTarget>, paths: Vec<PathBuf>, ctx: &egui::Context) {
        if sets::files_of(&paths).is_empty() {
            self.message = "No FITS or XISF files in what was dropped.".into();
            return;
        }
        match target {
            None => self.add_light_sources(paths, Some(ctx)),
            Some(DropTarget::Light(id)) => self.add_to_light_set(id, paths, ctx),
            Some(DropTarget::CalibrationSet(id)) => {
                self.sort_into_calibration_set(id, paths, ctx);
            }
            Some(DropTarget::NewCalibrationSet) => self.add_calibration_sources(paths, ctx),
            Some(DropTarget::CalibrationItem(id, kind)) => {
                self.selection = Selection::Calibration(id);
                self.view = View::Frames;
                match paths.as_slice() {
                    // A master cannot be told from a frame without the owner.
                    [file] if file.is_file() => {
                        self.pending_calibration_file = Some((id, kind, file.clone()));
                    }
                    _ => self.add_calibration_frames(id, kind, &paths, ctx),
                }
            }
        }
    }

    /// One file dropped on a calibration card: the owner says what it is.
    pub(super) fn calibration_file_modal(&mut self, ctx: &egui::Context) {
        let Some((id, kind, file)) = self.pending_calibration_file.clone() else {
            return;
        };
        let current = self
            .calibration(id)
            .and_then(|s| s.item(kind))
            .map(|i| match &i.source {
                CalibrationSource::Frames(files) => Some(files.len()),
                CalibrationSource::Master(_) => None,
            });
        let mut answer = None;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("calibration_file")).show(ctx, |ui| {
            ui.set_max_width(460.);
            ui.heading("Master or single frame?");
            ui.label(format!(
                "“{}” was dropped on {}.",
                file.file_name().unwrap_or_default().to_string_lossy(),
                kind.title()
            ));
            let effect = match current {
                Some(Some(frames)) => format!(
                    "Master replaces the {frames} frames of this card. Single frame joins them."
                ),
                Some(None) => "Either choice replaces the master file of this card.".into(),
                None => "Master is used as it is. Frames are combined into a master.".into(),
            };
            ui.label(RichText::new(effect).small().weak());
            ui.add_space(6.);
            ui.horizontal(|ui| {
                if ui.button("Master").clicked() {
                    answer = Some(true);
                }
                if ui.button("Single frame").clicked() {
                    answer = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if answer.is_some() || cancel || modal.should_close() {
            self.pending_calibration_file = None;
        }
        match answer {
            Some(true) if self.master_building(id, kind) => {
                self.message = "Wait until the master is stacked, then replace it.".into();
            }
            Some(true) => {
                let source = CalibrationSource::Master(file);
                self.set_calibration_item(id, kind, Some(source), ctx);
                self.message = format!("{}: master file added.", kind.title());
            }
            Some(false) => self.add_calibration_frames(id, kind, &[file], ctx),
            None => {}
        }
    }

    fn add_to_light_set(&mut self, id: u64, paths: Vec<PathBuf>, ctx: &egui::Context) {
        let Some(set) = self.light_mut(id) else {
            return;
        };
        for path in paths {
            if !set.sources.contains(&path) {
                set.sources.push(path);
            }
        }
        set.summary = None;
        let sources = set.sources.clone();
        self.scan_light(id, sources, Some(ctx));
        self.selection = Selection::Light(id);
        self.view = View::Frames;
        self.invalidate_measurements("Light sets changed · measure quality again.");
        self.message = "Files added to the light set · reading file headers.".into();
    }
}
