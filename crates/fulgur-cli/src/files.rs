//! Report and review-table destinations shared by the commands.
use crate::CANCEL;
use fulgur_core::output::{atomic_write, ensure_new_destination};
use fulgur_core::{Error, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// A JSON report destination must be a new `.json` file.
pub(crate) fn validate_report(report: Option<&Path>) -> Result<()> {
    if let Some(path) = report {
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            return Err(Error::Invalid(
                "Report filename must have a .json extension.",
            ));
        }
        ensure_new_destination(path)?;
    }
    Ok(())
}

pub(crate) fn save_report(report: Option<&Path>, value: &impl Serialize) -> Result<()> {
    if let Some(path) = report {
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|_| Error::Invalid("JSON serialization failed."))?;
        atomic_write(path, &CANCEL, |writer| {
            writer.write_all(&bytes)?;
            writer.write_all(b"\n")?;
            Ok(())
        })?;
    }
    Ok(())
}

/// A review table destination must be a new `.csv` file.
pub(crate) fn validate_csv(csv: Option<&Path>) -> Result<()> {
    if let Some(path) = csv {
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
        {
            return Err(Error::Invalid("Review table must have a .csv extension."));
        }
        ensure_new_destination(path)?;
    }
    Ok(())
}

pub(crate) fn save_csv(csv: Option<&Path>, table: impl FnOnce() -> String) -> Result<()> {
    if let Some(path) = csv {
        let table = table();
        atomic_write(path, &CANCEL, |writer| {
            writer.write_all(table.as_bytes())?;
            Ok(())
        })?;
    }
    Ok(())
}

pub(crate) fn filename(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// `path` with `suffix` replacing its extension.
pub(crate) fn companion(path: &Path, suffix: &str) -> PathBuf {
    let mut stem = path.file_stem().unwrap_or_default().to_os_string();
    stem.push(suffix);
    path.with_file_name(stem)
}

/// `path` with the group label before its extension.
pub(crate) fn labelled(path: &Path, label: &str) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{stem}_{label}.{extension}"))
}

/// Peak memory in MiB for the console, or "unavailable".
pub(crate) fn peak_mib(bytes: Option<u64>) -> String {
    bytes
        .map(|b| format!("{:.1}", b as f64 / 1048576.))
        .unwrap_or_else(|| "unavailable".to_owned())
}

/// The absolute, normalized form of a destination, so two spellings of one
/// file are recognised as the same destination.
pub(crate) fn destination_identity(path: &Path) -> Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for c in absolute.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(c.as_os_str()),
        }
    }
    let mut base = normalized.clone();
    let mut suffix = vec![];
    while !base.exists() {
        suffix.push(
            base.file_name()
                .ok_or(Error::Invalid("Invalid destination path."))?
                .to_os_string(),
        );
        base.pop();
    }
    let mut resolved = base.canonicalize()?;
    for c in suffix.into_iter().rev() {
        resolved.push(c);
    }
    let text = resolved.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        Ok(text.to_lowercase())
    }
    #[cfg(not(windows))]
    {
        Ok(text)
    }
}
