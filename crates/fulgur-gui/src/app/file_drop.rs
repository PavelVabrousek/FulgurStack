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
    /// What dropping `paths` outside every zone gives, and whether it adds
    /// anything (ADR-159). Folders are read once per drag.
    fn new_light_preview(&mut self, paths: &[PathBuf]) -> (String, bool) {
        if let Some((cached, text, useful)) = &self.drag_preview {
            if cached == paths {
                return (text.clone(), *useful);
            }
        }
        let mut sets = 0;
        let mut calibration = 0;
        let mut loose = false;
        for path in paths {
            if path.is_dir() {
                let found = sets::light_folders(path);
                sets += found.sets.len();
                calibration += found.calibration.len();
            } else if fulgur_core::fits::is_input(path) {
                loose = true;
            }
        }
        sets += usize::from(loose);
        let mut text = match sets {
            0 => "No FITS or XISF files here or in its sub-folders".to_owned(),
            1 => "New light set".to_owned(),
            n => format!("{n} new light sets, one per folder"),
        };
        if calibration > 0 {
            text.push_str(&format!(
                " · {calibration} calibration folder{} left out",
                if calibration == 1 { "" } else { "s" }
            ));
        }
        self.drag_preview = Some((paths.to_vec(), text.clone(), sets > 0));
        (text, sets > 0)
    }

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
        let (hovered, dropped): (Vec<PathBuf>, Vec<PathBuf>) = ctx.input(|i| {
            (
                i.raw
                    .hovered_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect(),
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect(),
            )
        });
        let hovering = !hovered.is_empty();
        if !hovering {
            self.drag_preview = None;
        }
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
        // Outside every zone, say what the folders really give.
        let (hint, useful) = match zone {
            None => self.new_light_preview(&hovered),
            Some((_, target)) => (self.drop_hint(Some(target)), true),
        };
        let colour = if useful {
            WARN
        } else {
            Color32::from_rgb(156, 43, 54)
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("file_drag"),
        ));
        painter.rect_stroke(
            area,
            8.,
            egui::Stroke::new(2_f32, colour),
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
            colour,
        );
        painter.galley(at, label, Color32::WHITE);
        // The window gets no input during a drag; keep following the pointer.
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    pub(super) fn take_drop(
        &mut self,
        target: Option<DropTarget>,
        paths: Vec<PathBuf>,
        ctx: &egui::Context,
    ) {
        self.drag_preview = None;
        // New light sets search sub-folders and explain what they found.
        if target.is_none() {
            self.add_light_sources(paths, Some(ctx));
            return;
        }
        let lights = matches!(target, Some(DropTarget::Light(_)));
        let found = if lights {
            sets::light_files_of(&paths)
        } else {
            sets::files_of(&paths)
        };
        if found.is_empty() {
            self.message = if lights {
                "No FITS or XISF files directly in what was dropped (sub-folders are not read: drop them as their own light sets).".into()
            } else {
                "No FITS or XISF files in what was dropped.".into()
            };
            return;
        }
        match target {
            None => {}
            Some(DropTarget::Light(id)) => {
                // Another camera or colour layout in the headers never extends
                // the set: it becomes a light set of its own.
                let own = self.light(id).and_then(|s| light_identity(&s.sources));
                let (same, other): (Vec<PathBuf>, Vec<PathBuf>) =
                    paths.into_iter().partition(|path| {
                        match (&own, light_identity(std::slice::from_ref(path))) {
                            (Some(a), Some(b)) => same_camera(a, &b),
                            _ => true,
                        }
                    });
                if !same.is_empty() {
                    self.add_to_light_set(id, same, ctx);
                }
                if !other.is_empty() {
                    self.add_light_sources(other, Some(ctx));
                    self.message = format!(
                        "Another camera or colour layout in the FITS headers: a light set of its own, not added to the set dropped on. {}",
                        self.message
                    );
                }
            }
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

/// Camera name and Bayer pattern from the headers of the first light frame
/// found in `paths`.
type LightIdentity = (Option<String>, Option<fulgur_core::BayerPattern>);

fn light_identity(paths: &[PathBuf]) -> Option<LightIdentity> {
    let file = sets::light_files_of(paths).into_iter().next()?;
    let meta = fulgur_core::fits::read_light_metadata(&file).ok()?;
    Some((meta.camera(), meta.bayer_pattern))
}

/// Same camera model (when both name one) and the same header colour layout.
fn same_camera(a: &LightIdentity, b: &LightIdentity) -> bool {
    let camera = match (&a.0, &b.0) {
        (Some(x), Some(y)) => x.trim().eq_ignore_ascii_case(y.trim()),
        _ => true,
    };
    camera && a.1 == b.1
}
