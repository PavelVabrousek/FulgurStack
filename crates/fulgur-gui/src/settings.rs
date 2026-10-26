//! Local UI preferences only; no images, catalogs, results or running jobs.
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppSettings {
    pub schema_version: u32,
    pub inputs: Vec<PathBuf>,
    pub output_folder: String,
    pub master_filename: String,
    pub reference_mode: usize,
    pub reference_path: String,
    pub save_coverage: bool,
    pub save_high_rejection: bool,
    pub review_quality: bool,
    pub diagnose_excluded: bool,
    pub gpu: bool,
    pub tile_size: usize,
    pub memory_budget_mib: usize,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            inputs: vec![],
            output_folder: String::new(),
            master_filename: "master.fits".into(),
            reference_mode: 0,
            reference_path: String::new(),
            save_coverage: false,
            save_high_rejection: false,
            review_quality: false,
            diagnose_excluded: false,
            gpu: false,
            tile_size: 0,
            memory_budget_mib: 12288,
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
    if settings.schema_version != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsupported settings version.",
        ));
    }
    Ok(Some(settings))
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
