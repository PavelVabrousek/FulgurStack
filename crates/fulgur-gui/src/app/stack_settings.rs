//! The stacking and output settings the owner chooses and the application
//! saves between sessions (G-01): one group instead of loose `Workspace`
//! fields, saved and restored in one place.
use super::{AUTOMATIC_TILE, Accuracy, TILE_SHAPES};
use crate::job;
use crate::settings::{self, AppSettings};
use fulgur_pipeline::integrate::{Drizzle, Rejection, TrailMask, TrailMode};
use fulgur_pipeline::master_groups::MasterSplit;

pub(crate) struct StackSettings {
    /// Output folder and master file name.
    pub(crate) folder: String,
    pub(crate) filename: String,
    /// Reference choice (0..=3) and the reference file it may name.
    pub(crate) reference_mode: usize,
    pub(crate) reference: String,
    pub(crate) save_coverage: bool,
    pub(crate) save_high_rejection: bool,
    pub(crate) geometric_correction: bool,
    pub(crate) partial_frames: bool,
    pub(crate) accuracy: Accuracy,
    pub(crate) diagnose_excluded: bool,
    pub(crate) rejection: Rejection,
    /// Trail mask around ESD's upper rejections (ADR-187).
    pub(crate) trail: TrailMask,
    /// Drizzle 2x, off by default (ADR-188).
    pub(crate) drizzle: Option<Drizzle>,
    pub(crate) master_split: MasterSplit,
    pub(crate) gpu: bool,
    /// Output tile width and height.
    pub(crate) tile: (usize, usize),
    pub(crate) cpu_threads: usize,
    pub(crate) memory: usize,
    /// Share of pixels a hot pixel map repairs, in percent.
    pub(crate) hot_percent: f64,
}

impl Default for StackSettings {
    fn default() -> Self {
        Self {
            folder: std::env::current_dir()
                .unwrap_or_default()
                .join("outputs")
                .to_string_lossy()
                .into_owned(),
            filename: "master.fits".into(),
            reference_mode: 0,
            reference: String::new(),
            save_coverage: false,
            save_high_rejection: false,
            geometric_correction: true,
            partial_frames: false,
            accuracy: Accuracy::default(),
            diagnose_excluded: false,
            rejection: Rejection::Auto,
            trail: TrailMask::default(),
            drizzle: None,
            master_split: MasterSplit::Combined,
            gpu: false,
            tile: AUTOMATIC_TILE,
            cpu_threads: 0,
            memory: 12288,
            hot_percent: fulgur_pipeline::calibration::HOT_PIXEL_DEFAULT_PERCENT,
        }
    }
}

impl StackSettings {
    /// Writes these settings into the saved settings, with absolute paths.
    pub(crate) fn save(&self, settings: &mut AppSettings) {
        settings.reference_path = if !self.reference.trim().is_empty()
            && std::path::Path::new(&self.reference).exists()
        {
            settings::absolute(std::path::Path::new(&self.reference))
                .to_string_lossy()
                .into_owned()
        } else {
            self.reference.clone()
        };
        settings.output_folder = if self.folder.trim().is_empty() {
            self.folder.clone()
        } else {
            settings::absolute(std::path::Path::new(&self.folder))
                .to_string_lossy()
                .into_owned()
        };
        settings.master_filename = self.filename.clone();
        settings.reference_mode = self.reference_mode;
        settings.save_coverage = self.save_coverage;
        settings.save_high_rejection = self.save_high_rejection;
        settings.geometric_correction = self.geometric_correction;
        settings.partial_frames = self.partial_frames;
        settings.registration_rms_px = self.accuracy.rms_px;
        settings.registration_star_percent = self.accuracy.star_percent;
        settings.registration_min_stars = self.accuracy.min_stars;
        settings.diagnose_excluded = self.diagnose_excluded;
        settings.gpu = self.gpu;
        settings.tile_size = self.tile.0;
        settings.tile_height = self.tile.1;
        settings.cpu_threads = self.cpu_threads;
        settings.memory_budget_mib = self.memory;
        settings.hot_pixel_percent = self.hot_percent;
        settings.rejection = self.rejection.key().into();
        settings.trail_mask = self.trail.mode.key().into();
        settings.trail_radius_px = self.trail.radius_px;
        settings.trail_sigma = self.trail.sigma;
        settings.drizzle = self.drizzle.is_some();
        if let Some(d) = self.drizzle {
            settings.drizzle_pixfrac = d.pixfrac;
            settings.drizzle_debayer = !d.bayer;
        }
        settings.master_split = self.master_split.key().into();
    }

    /// Takes the saved settings, keeping the current value wherever a saved
    /// one is missing or outside what the application offers.
    pub(crate) fn restore(&mut self, saved: &AppSettings) {
        if !saved.output_folder.trim().is_empty() {
            self.folder = saved.output_folder.clone();
        }
        if job::valid_filename(&saved.master_filename) {
            self.filename = saved.master_filename.clone();
        }
        self.reference_mode = if saved.reference_mode <= 3 {
            saved.reference_mode
        } else {
            0
        };
        self.reference = saved.reference_path.clone();
        self.master_split = MasterSplit::parse(&saved.master_split).unwrap_or_default();
        self.save_coverage = saved.save_coverage;
        self.save_high_rejection = saved.save_high_rejection;
        self.diagnose_excluded = saved.diagnose_excluded;
        self.geometric_correction = saved.geometric_correction;
        self.partial_frames = saved.partial_frames;
        self.accuracy = Accuracy::restored(
            saved.registration_rms_px,
            saved.registration_star_percent,
            saved.registration_min_stars,
        );
        self.rejection = Rejection::parse(&saved.rejection).unwrap_or(Rejection::Auto);
        self.drizzle = saved.drizzle.then(|| Drizzle {
            pixfrac: if (0.3..=1.).contains(&saved.drizzle_pixfrac) {
                saved.drizzle_pixfrac
            } else {
                Drizzle::default().pixfrac
            },
            bayer: !saved.drizzle_debayer,
            ..Drizzle::default()
        });
        let default = TrailMask::default();
        self.trail = TrailMask {
            mode: TrailMode::parse(&saved.trail_mask).unwrap_or_default(),
            radius_px: if (1.0..=16.).contains(&saved.trail_radius_px) {
                saved.trail_radius_px
            } else {
                default.radius_px
            },
            sigma: if (0.5..=5.).contains(&saved.trail_sigma) {
                saved.trail_sigma
            } else {
                default.sigma
            },
        };
        self.gpu = saved.gpu;
        // A saved tile without a height is a square from an earlier version.
        let height = match saved.tile_height {
            0 => saved.tile_size,
            height => height,
        };
        let saved_tile = (saved.tile_size, height);
        self.tile = if TILE_SHAPES.contains(&saved_tile) {
            saved_tile
        } else {
            AUTOMATIC_TILE
        };
        self.memory = saved.memory_budget_mib.clamp(1024, 12288);
        self.hot_percent = if saved.hot_pixel_percent.is_finite() && saved.hot_pixel_percent > 0. {
            saved
                .hot_pixel_percent
                .min(fulgur_pipeline::calibration::HOT_PIXEL_MAX_PERCENT)
        } else {
            fulgur_pipeline::calibration::HOT_PIXEL_DEFAULT_PERCENT
        };
        self.cpu_threads = if [0, 12, 16, 24].contains(&saved.cpu_threads) {
            saved.cpu_threads
        } else {
            0
        };
    }
}
