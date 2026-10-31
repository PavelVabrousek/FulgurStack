//! Scripted window captures for the user guide, taken from a real run on real
//! data. Started with `--ui-guide`; saves nothing to the owner's settings.

use super::*;

const WINDOW: [f32; 2] = [1440., 860.];

pub struct GuideTour {
    pub folder: PathBuf,
    pub prefix: String,
    /// Continue from measuring to a finished stack.
    pub stack: bool,
    /// Calibration folders, sorted into one set by their headers.
    pub calibration: Vec<PathBuf>,
    /// Start from the owner's saved workspace, read only.
    pub saved: bool,
    stage: u32,
    /// Frames to wait before the next step, so images can load.
    wait: u32,
    pending: Option<String>,
}

impl GuideTour {
    pub fn new(
        folder: PathBuf,
        prefix: String,
        stack: bool,
        calibration: Vec<PathBuf>,
        saved: bool,
    ) -> Self {
        Self {
            folder,
            prefix,
            stack,
            calibration,
            saved,
            stage: 0,
            wait: 0,
            pending: None,
        }
    }
}

impl Workspace {
    pub fn enable_guide(&mut self, tour: GuideTour) {
        self.folder = tour.folder.to_string_lossy().into_owned();
        self.filename = format!("{}_master.fits", tour.prefix);
        self.save_coverage = true;
        self.save_high_rejection = true;
        self.geometric_correction = true;
        self.guide = Some(tour);
    }

    fn guide_capture(tour: &mut GuideTour, ctx: &egui::Context, name: &str) {
        tour.pending = Some(format!("{}_{name}.png", tour.prefix));
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }

    pub(super) fn guide_tick(&mut self, ctx: &egui::Context) {
        let Some(mut tour) = self.guide.take() else {
            return;
        };
        ctx.request_repaint();
        let image = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let (Some(image), Some(name)) = (image, tour.pending.as_ref()) {
            let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            if let Err(error) = image::save_buffer(
                tour.folder.join(name),
                &bytes,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            ) {
                eprintln!("Guide capture failed: {error}");
            }
            tour.pending = None;
        }
        if tour.pending.is_some() {
            self.guide = Some(tour);
            return;
        }
        if tour.wait > 0 {
            tour.wait -= 1;
            self.guide = Some(tour);
            return;
        }
        match tour.stage {
            0 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(WINDOW.into()));
                if tour.saved {
                    if let Some(saved) = settings::path()
                        .and_then(|p| settings::load(&p).ok())
                        .flatten()
                    {
                        let (folder, filename) = (self.folder.clone(), self.filename.clone());
                        self.restore_settings(saved);
                        self.rescan_all(ctx);
                        self.folder = folder;
                        self.filename = filename;
                    }
                }
                if !tour.calibration.is_empty() {
                    self.add_calibration_sources(tour.calibration.clone(), ctx);
                }
                tour.stage = 1;
            }
            1 => {
                let lights = self.light_sets.iter().all(|s| s.summary.is_some());
                let calibration = tour.calibration.is_empty()
                    || self
                        .calibration_sets
                        .iter()
                        .any(|c| c.items.iter().flatten().next().is_some());
                if lights && calibration {
                    self.selection = Selection::All;
                    tour.wait = 30;
                    tour.stage = 2;
                }
            }
            2 => {
                Self::guide_capture(&mut tour, ctx, "sets");
                tour.stage = 3;
            }
            3 => match self.calibration_sets.first().map(|c| c.id) {
                Some(id) => {
                    self.selection = Selection::Calibration(id);
                    let flat = self
                        .calibration(id)
                        .is_some_and(|c| c.item(CalibrationKind::Flat).is_some());
                    self.calibration_preview_kind = Some(if flat {
                        CalibrationKind::Flat
                    } else {
                        CalibrationKind::Dark
                    });
                    self.calibration_flat_view = FlatView::Dust;
                    tour.wait = 90;
                    tour.stage = 4;
                }
                None => tour.stage = 5,
            },
            4 => {
                let ready = self
                    .calibration_preview
                    .as_ref()
                    .is_some_and(|p| p.task.is_none() && p.texture.is_some());
                if ready {
                    Self::guide_capture(&mut tour, ctx, "calibration");
                    tour.stage = 5;
                }
            }
            5 => {
                self.confirm_derived_values();
                self.selection = Selection::All;
                self.start(true);
                tour.stage = 6;
            }
            6 if !self.busy() && self.measured() => {
                tour.wait = 60;
                tour.stage = 7;
            }
            7 if self.preview.is_some() => {
                Self::guide_capture(&mut tour, ctx, "frames");
                tour.stage = 8;
            }
            8 if tour.stack => {
                self.start(false);
                tour.stage = 9;
            }
            8 => tour.stage = 12,
            9 => {
                let halfway = self
                    .worker
                    .as_ref()
                    .and_then(|w| w.progress.lock().ok().map(|p| p.clone()))
                    .is_some_and(|p| p.phase == "Integrate" && p.completed * 2 >= p.total.max(1));
                if halfway {
                    self.view = View::Processing;
                    tour.wait = 2;
                    tour.stage = 10;
                } else if self.result.is_some() && !self.busy() {
                    tour.stage = 11;
                }
            }
            10 => {
                Self::guide_capture(&mut tour, ctx, "processing");
                tour.stage = 11;
            }
            11 if self.result.is_some() && !self.busy() => {
                self.view = View::Results;
                self.layer = 0;
                self.result_texture = None;
                tour.wait = 60;
                tour.stage = 13;
            }
            13 => {
                Self::guide_capture(&mut tour, ctx, "results");
                tour.stage = 12;
            }
            12 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                tour.stage = 14;
            }
            _ => {}
        }
        self.guide = Some(tour);
    }
}
