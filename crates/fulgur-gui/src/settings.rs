//! Local UI preferences only; no images, catalogs, results or running jobs.
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct SavedLightSet {
    pub name: String,
    pub sources: Vec<PathBuf>,
    /// "auto", "mono" or a Bayer pattern.
    pub layout: String,
    pub include: bool,
    /// Index into `calibration_sets`.
    pub calibration: Option<usize>,
    /// Exposure entered by the owner for every frame (ADR-084).
    pub exposure: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct SavedCalibrationItem {
    /// "bias", "dark", "flat" or "dark_flat".
    pub kind: String,
    pub master: Option<PathBuf>,
    pub frames: Vec<PathBuf>,
    pub enabled: bool,
    /// Combine method key for frames ("average", "median", "sigma_kappa_2").
    pub combine: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct SavedCalibrationSet {
    pub name: String,
    pub items: Vec<SavedCalibrationItem>,
}

/// Normal size and place of the main window, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SavedWindow {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl SavedWindow {
    /// Plausible values within the window's own size limits.
    pub fn usable(&self) -> bool {
        [self.x, self.y].iter().all(|v| v.abs() < 100_000.)
            && (800. ..=16_384.).contains(&self.width)
            && (460. ..=16_384.).contains(&self.height)
    }
    /// The title bar lands on a monitor, so an unplugged screen cannot strand it.
    pub fn on_screen(&self) -> bool {
        #[cfg(windows)]
        {
            use windows_sys::Win32::{
                Foundation::POINT,
                Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint},
            };
            let point = POINT {
                x: self.x as i32 + 80,
                y: self.y as i32 + 16,
            };
            // SAFETY: the call takes the point by value and keeps no reference.
            !unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) }.is_null()
        }
        #[cfg(not(windows))]
        true
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppSettings {
    pub schema_version: u32,
    /// Earlier flat input list; restored as one light set per entry.
    pub inputs: Vec<PathBuf>,
    pub light_sets: Vec<SavedLightSet>,
    pub calibration_sets: Vec<SavedCalibrationSet>,
    pub output_folder: String,
    pub master_filename: String,
    pub reference_mode: usize,
    pub reference_path: String,
    pub save_coverage: bool,
    pub save_high_rejection: bool,
    pub review_quality: bool,
    pub geometric_correction: bool,
    /// Experimental partial frames (ADR-109).
    pub partial_frames: bool,
    /// Registration accuracy chosen in Settings: RMS limit in pixels, the
    /// share of the weaker frame's stars that must match and the fewest
    /// matched stars (ADR-121).
    pub registration_rms_px: u8,
    pub registration_star_percent: u8,
    pub registration_min_stars: u8,
    pub diagnose_excluded: bool,
    pub gpu: bool,
    /// Output tile width; 0 is Automatic.
    pub tile_size: usize,
    /// Output tile height; 0 means a square of `tile_size`.
    pub tile_height: usize,
    pub cpu_threads: usize,
    pub memory_budget_mib: usize,
    /// Pixel rejection key ("auto", "winsorized", ...; ADR-087).
    pub rejection: String,
    pub show_quality_chart: bool,
    pub window: Option<SavedWindow>,
    /// Masters of a run: "combined", "per-filter" or "per-set" (ADR-101).
    pub master_split: String,
    /// Sky registration through Gaia plate solutions (ADR-138).
    pub sky_registration: bool,
    /// Folder of the PixInsight Gaia DR3 files; never a fixed path (ADR-135).
    pub gaia_folder: String,
    pub gaia_download: bool,
    /// 0: from the target name, 1: coordinates.
    pub sky_field_mode: usize,
    pub sky_object: String,
    pub sky_ra: String,
    pub sky_dec: String,
    pub sky_online_lookup: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 2,
            inputs: vec![],
            light_sets: vec![],
            calibration_sets: vec![],
            output_folder: String::new(),
            master_filename: "master.fits".into(),
            reference_mode: 0,
            reference_path: String::new(),
            save_coverage: false,
            save_high_rejection: false,
            review_quality: false,
            geometric_correction: true,
            partial_frames: false,
            registration_rms_px: 1,
            registration_star_percent: 80,
            registration_min_stars: 50,
            diagnose_excluded: false,
            gpu: false,
            tile_size: 0,
            tile_height: 0,
            cpu_threads: 0,
            memory_budget_mib: 12288,
            rejection: "auto".into(),
            show_quality_chart: true,
            window: None,
            master_split: "combined".into(),
            sky_registration: false,
            gaia_folder: String::new(),
            gaia_download: false,
            sky_field_mode: 0,
            sky_object: String::new(),
            sky_ra: String::new(),
            sky_dec: String::new(),
            sky_online_lookup: false,
        }
    }
}

pub(crate) fn path() -> Option<PathBuf> {
    #[cfg(windows)]
    let folder = std::env::var_os("LOCALAPPDATA")
        .filter(|s| !s.is_empty())
        .map(|p| PathBuf::from(p).join("FulgurStack"));
    #[cfg(target_os = "macos")]
    let folder = std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(|p| PathBuf::from(p).join("Library/Application Support/FulgurStack"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let folder = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|s| !s.is_empty())
                .map(|p| PathBuf::from(p).join(".config"))
        })
        .map(|p| p.join("fulgurstack"));
    folder
        .filter(|p| p.is_absolute())
        .map(|p| p.join("settings.json"))
}

pub(crate) fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn load(path: &Path) -> io::Result<Option<AppSettings>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    const LIMIT: u64 = 4 * 1024 * 1024;
    let mut bytes = vec![];
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Settings file is too large.",
        ));
    }
    let settings: AppSettings = serde_json::from_slice(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    upgraded(settings).map(Some)
}

/// Version 1 stored the old default for geometric correction, not a choice;
/// the new default reaches saved workspaces once (ADR-103).
fn upgraded(mut settings: AppSettings) -> io::Result<AppSettings> {
    match settings.schema_version {
        1 => {
            settings.schema_version = 2;
            settings.geometric_correction = true;
            Ok(settings)
        }
        2 => Ok(settings),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsupported settings version.",
        )),
    }
}

pub(crate) fn save(path: &Path, settings: &AppSettings) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Settings require an application data folder.",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    // Replacing this private preferences file is intentional. Scientific FITS
    // publication continues to use its separate atomic no-clobber writers.
    let mut temporary = tempfile::Builder::new()
        .prefix(".settings-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), settings).map_err(io::Error::other)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod distortion_tests {
    use super::*;
    #[test]
    fn distortion_correction_defaults_on_and_a_later_choice_to_turn_it_off_stays() {
        assert!(AppSettings::default().geometric_correction);
        let old: AppSettings =
            serde_json::from_str("{\"schema_version\":1,\"geometric_correction\":false}").unwrap();
        let moved = upgraded(old).unwrap();
        assert!(moved.geometric_correction, "version 1 held the old default");
        assert_eq!(moved.schema_version, 2);
        let chosen = AppSettings {
            geometric_correction: false,
            ..moved
        };
        let text = serde_json::to_string(&chosen).unwrap();
        let restored = upgraded(serde_json::from_str(&text).unwrap()).unwrap();
        assert!(!restored.geometric_correction);
        let future = AppSettings {
            schema_version: 3,
            ..restored
        };
        assert!(upgraded(future).is_err());
    }
}
