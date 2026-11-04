//! Header inventory plus an actual full-file read. No image intermediates.

use fulgur_core::fits::{FitsHeader, FrameInfo};
use fulgur_core::{Error, Result, check_cancelled};
use rayon::prelude::*;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct SurveyFrame {
    pub file_name: String,
    #[serde(flatten)]
    pub info: FrameInfo,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct SurveyReport {
    pub schema_version: u32,
    pub command: &'static str,
    pub frame_count: usize,
    pub total_bytes_read: u64,
    pub total_exposure_seconds: f64,
    pub wall_seconds: f64,
    pub read_mib_per_second: f64,
    pub cache_state: &'static str,
    pub peak_rss_bytes: Option<u64>,
    pub frames: Vec<SurveyFrame>,
}

/// Light frames of a file or a folder: a folder gives only the files directly
/// in it, never those of its sub-folders (owner decision 2026-11-04,
/// ADR-140). A sub-folder is often another night, camera or a set of
/// registered or template frames, and becomes a light set of its own.
pub fn collect_folder_inputs(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        if fulgur_core::fits::is_input(path) {
            files.push(path.to_owned());
        }
    } else if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_file() && fulgur_core::fits::is_input(&entry.path()) {
                files.push(entry.path());
            }
        }
    } else {
        return Err(Error::Invalid(
            "Survey input does not exist or is not a file/directory.",
        ));
    }
    Ok(())
}

/// Calibration frames of a file or a folder, sub-folders included: a
/// calibration folder often keeps its bias, darks and flats in sub-folders.
pub fn collect_inputs(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        if fulgur_core::fits::is_input(path) {
            files.push(path.to_owned());
        }
    } else if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if !entry.file_type()?.is_symlink() {
                collect_inputs(&entry.path(), files)?;
            }
        }
    } else {
        return Err(Error::Invalid(
            "Survey input does not exist or is not a file/directory.",
        ));
    }
    Ok(())
}

pub fn survey(input: &Path, cancel: &AtomicBool) -> Result<SurveyReport> {
    let start = Instant::now();
    let mut files = Vec::new();
    collect_folder_inputs(input, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err(Error::Invalid("No FITS or XISF input files found."));
    }
    let frames = files
        .par_iter()
        .map(|path| {
            check_cancelled(cancel)?;
            let file = File::open(path)?;
            let bytes = file.metadata()?.len();
            let mut reader = BufReader::new(file);
            let header = if fulgur_core::xisf::is_xisf(path) {
                fulgur_core::fits::read_calibration_header(path)?
            } else {
                FitsHeader::read(&mut reader, bytes)?
            };
            let mut buffer = vec![0; 1024 * 1024];
            let mut total = header.data_offset;
            loop {
                check_cancelled(cancel)?;
                let count = reader.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                total += count as u64;
                std::hint::black_box(&buffer[..count]);
            }
            if total != bytes {
                return Err(Error::Invalid("Input length changed during survey."));
            }
            Ok(SurveyFrame {
                file_name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                info: header.info,
                bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let wall_seconds = start.elapsed().as_secs_f64();
    let total_bytes_read = frames.iter().map(|frame| frame.bytes).sum::<u64>();
    Ok(SurveyReport {
        schema_version: 1,
        command: "survey",
        frame_count: frames.len(),
        total_bytes_read,
        total_exposure_seconds: frames.iter().map(|frame| frame.info.exposure_seconds).sum(),
        wall_seconds,
        read_mib_per_second: total_bytes_read as f64 / wall_seconds / 1048576.0,
        cache_state: "uncontrolled; effective OS/filesystem throughput, not an isolated SSD benchmark",
        peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
        frames,
    })
}

#[cfg(test)]
mod folder_tests {
    use super::*;

    #[test]
    fn a_light_folder_gives_its_own_files_and_a_calibration_folder_its_sub_folders_too() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Registration templates");
        fs::create_dir(&sub).unwrap();
        for path in [
            dir.path().join("light_1.fit"),
            dir.path().join("light_2.xisf"),
            dir.path().join("notes.txt"),
            sub.join("template.fit"),
        ] {
            fs::write(path, b"x").unwrap();
        }
        let mut lights = vec![];
        collect_folder_inputs(dir.path(), &mut lights).unwrap();
        lights.sort();
        assert_eq!(
            lights,
            [
                dir.path().join("light_1.fit"),
                dir.path().join("light_2.xisf")
            ]
        );
        let mut all = vec![];
        collect_inputs(dir.path(), &mut all).unwrap();
        assert_eq!(
            all.len(),
            3,
            "calibration folders still include sub-folders"
        );
        let mut one = vec![];
        collect_folder_inputs(&sub.join("template.fit"), &mut one).unwrap();
        assert_eq!(one.len(), 1, "a file given by name is always read");
    }
}
