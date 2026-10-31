//! Light and calibration sets: the owner's grouping of input folders.
//! Header-only planning; pixels are read by the pipeline, never here.
use fulgur_core::fits::{FileMetadata, read_light_metadata, read_metadata};
use fulgur_core::{BayerPattern, LayoutChoice};
pub use fulgur_pipeline::calibration::CombineMethod;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Colour layout declared by the headers of a light set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderLayout {
    Bayer(BayerPattern),
    /// No file declares BAYERPAT: processed as mono unless the owner chooses.
    Missing,
    /// Files disagree; the owner must choose one layout for the set.
    Mixed,
}

/// A value reconstructed for missing header cards (ADR-084), with the
/// number of files it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedSummary {
    pub field: &'static str,
    pub value: String,
    pub source: String,
    pub files: usize,
    pub needs_confirmation: bool,
}

impl DerivedSummary {
    pub fn label(&self) -> &'static str {
        match self.field {
            fulgur_core::derive::EXPOSURE => "Exposure",
            fulgur_core::derive::LAYOUT => "Sensor layout",
            "gain" => "Gain",
            "object" => "Object",
            _ => "Value",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetSummary {
    pub files: usize,
    pub unreadable: usize,
    pub width: usize,
    pub height: usize,
    pub sizes_differ: bool,
    pub header_layout: Option<HeaderLayout>,
    pub exposures: Vec<f64>,
    pub filters: Vec<String>,
    pub cameras: Vec<String>,
    pub gains: Vec<f64>,
    pub objects: Vec<String>,
    pub temperature: Option<f64>,
    /// Median camera rotator angle of the lights, when recorded.
    pub rotator: Option<f64>,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub total_exposure: f64,
    /// Values reconstructed for missing header cards (ADR-084).
    pub derived: Vec<DerivedSummary>,
    /// The colour layout came from other cards, the file name or the camera catalogue.
    pub layout_derived: bool,
    /// The camera is colour but its Bayer pattern is not known.
    pub colour_unknown: bool,
    /// Readable files with no exposure in the header or the file name.
    pub missing_exposure: usize,
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

impl SetSummary {
    pub fn from_metadata(items: &[Result<FileMetadata, ()>]) -> Self {
        let mut s = Self {
            files: items.len(),
            ..Self::default()
        };
        let mut layouts = vec![];
        let mut temperatures = vec![];
        let mut rotators = vec![];
        let mut dates = vec![];
        for meta in items.iter().filter_map(|m| m.as_ref().ok()) {
            if s.width == 0 {
                (s.width, s.height) = (meta.width, meta.height);
            } else if (s.width, s.height) != (meta.width, meta.height) {
                s.sizes_differ = true;
            }
            push_unique(&mut layouts, meta.bayer_pattern);
            if let Some(e) = meta.exposure_seconds {
                push_unique(&mut s.exposures, e);
                s.total_exposure += e;
            } else {
                s.missing_exposure += 1;
            }
            s.colour_unknown |= meta.colour_without_pattern;
            for d in &meta.derived {
                s.layout_derived |= d.field == fulgur_core::derive::LAYOUT;
                match s
                    .derived
                    .iter_mut()
                    .find(|g| g.field == d.field && g.value == d.value && g.source == d.source)
                {
                    Some(group) => group.files += 1,
                    None => s.derived.push(DerivedSummary {
                        field: d.field,
                        value: d.value.clone(),
                        source: d.source.clone(),
                        files: 1,
                        needs_confirmation: d.needs_confirmation,
                    }),
                }
            }
            if let Some(f) = &meta.filter {
                push_unique(&mut s.filters, f.trim().to_owned());
            }
            if let Some(c) = meta.camera() {
                push_unique(&mut s.cameras, c);
            }
            if let Some(g) = meta.gain {
                push_unique(&mut s.gains, g);
            }
            if let Some(o) = &meta.object {
                push_unique(&mut s.objects, o.trim().to_owned());
            }
            if let Some(t) = meta.ccd_temperature {
                temperatures.push(t);
            }
            if let Some(r) = meta.rotator_degrees {
                rotators.push(r);
            }
            if let Some(d) = &meta.date_obs {
                dates.push(d.clone());
            }
        }
        s.unreadable = items.iter().filter(|m| m.is_err()).count();
        let number = |d: &DerivedSummary| {
            d.value
                .split(' ')
                .next()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(f64::INFINITY)
        };
        s.derived.sort_by(|a, b| {
            a.field
                .cmp(b.field)
                .then(number(a).total_cmp(&number(b)))
                .then(a.value.cmp(&b.value))
        });
        s.exposures.sort_by(f64::total_cmp);
        s.header_layout = match layouts.as_slice() {
            [] => None,
            [Some(pattern)] => Some(HeaderLayout::Bayer(*pattern)),
            [None] => Some(HeaderLayout::Missing),
            _ => Some(HeaderLayout::Mixed),
        };
        temperatures.sort_by(f64::total_cmp);
        s.temperature = temperatures.get(temperatures.len() / 2).copied();
        rotators.sort_by(f64::total_cmp);
        s.rotator = rotators.get(rotators.len() / 2).copied();
        dates.sort();
        s.first_date = dates.first().cloned();
        s.last_date = dates.last().cloned();
        s
    }

    pub fn is_seestar(&self) -> bool {
        self.cameras
            .iter()
            .any(|c| c.to_ascii_lowercase().contains("seestar"))
    }
    pub fn camera(&self) -> String {
        match self.cameras.as_slice() {
            [] => "unknown camera".into(),
            [one] => one.clone(),
            many => many.join(" + "),
        }
    }
    pub fn filter(&self) -> String {
        match self.filters.as_slice() {
            [] => "no filter card".into(),
            many => many.join(" + "),
        }
    }
    pub fn exposure(&self) -> String {
        match self.exposures.as_slice() {
            [] => "—".into(),
            many => many
                .iter()
                .map(|e| format!("{} s", trim_number(*e)))
                .collect::<Vec<_>>()
                .join(" + "),
        }
    }
    pub fn night(&self) -> Option<String> {
        self.first_date
            .as_deref()
            .map(|d| d.chars().take(10).collect())
    }
}

pub fn trim_number(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub struct LightSet {
    pub id: u64,
    pub name: String,
    pub sources: Vec<PathBuf>,
    pub layout: LayoutChoice,
    /// Exposure entered by the owner for every frame (ADR-084).
    pub exposure: Option<f64>,
    /// The owner confirmed the derived values of the current summary.
    pub derived_confirmed: bool,
    pub include: bool,
    pub calibration: Option<u64>,
    pub summary: Option<SetSummary>,
}

impl LightSet {
    /// True when the owner must declare colour or mono before processing:
    /// when the files disagree or a colour camera's pattern is unknown.
    /// No BAYERPAT at all means mono.
    pub fn needs_layout(&self) -> bool {
        self.layout == LayoutChoice::Auto
            && self
                .summary
                .as_ref()
                .is_some_and(|s| s.header_layout == Some(HeaderLayout::Mixed) || s.colour_unknown)
    }
    /// Derived values still in use, after the owner's layout and exposure entries.
    pub fn derived(&self) -> Vec<&DerivedSummary> {
        self.summary
            .iter()
            .flat_map(|s| s.derived.iter())
            .filter(|d| {
                !(d.field == fulgur_core::derive::LAYOUT && self.layout != LayoutChoice::Auto
                    || d.field == fulgur_core::derive::EXPOSURE && self.exposure.is_some())
            })
            .collect()
    }
    /// Derived exposure or colour layout awaits the owner's confirmation.
    pub fn needs_confirmation(&self) -> bool {
        !self.derived_confirmed && self.derived().iter().any(|d| d.needs_confirmation)
    }
    /// Some frames have no exposure anywhere and none was entered.
    pub fn missing_exposure(&self) -> bool {
        self.exposure.is_none()
            && self
                .summary
                .as_ref()
                .is_some_and(|s| s.missing_exposure > 0)
    }
    pub fn layout_label(&self) -> String {
        match (
            self.layout,
            self.summary.as_ref().and_then(|s| s.header_layout),
        ) {
            (LayoutChoice::Mono, _) => "Mono · no debayering".into(),
            (LayoutChoice::Bayer(p), _) => format!("Colour · {} (chosen)", p.name()),
            (LayoutChoice::Auto, _) if self.needs_layout() => {
                "Colour camera · choose pattern".into()
            }
            (LayoutChoice::Auto, Some(HeaderLayout::Bayer(p)))
                if self.summary.as_ref().is_some_and(|s| s.layout_derived) =>
            {
                format!("Colour · {} (derived)", p.name())
            }
            (LayoutChoice::Auto, Some(HeaderLayout::Bayer(p))) => {
                format!("Colour · {} (FITS header)", p.name())
            }
            (LayoutChoice::Auto, Some(HeaderLayout::Missing)) => {
                "Mono · no Bayer pattern in headers".into()
            }
            (LayoutChoice::Auto, Some(HeaderLayout::Mixed)) => "Headers disagree · choose".into(),
            (LayoutChoice::Auto, None) => "Reading headers…".into(),
        }
    }
    pub fn is_mono(&self) -> bool {
        self.layout == LayoutChoice::Mono
            || (self.layout == LayoutChoice::Auto
                && self.summary.as_ref().is_some_and(|s| {
                    s.header_layout == Some(HeaderLayout::Missing) && !s.colour_unknown
                }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibrationKind {
    Bias,
    Dark,
    Flat,
    DarkFlat,
}

impl CalibrationKind {
    pub const ALL: [Self; 4] = [Self::Bias, Self::Dark, Self::Flat, Self::DarkFlat];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn master_kind(self) -> fulgur_pipeline::calibration::MasterKind {
        use fulgur_pipeline::calibration::MasterKind;
        match self {
            Self::Bias => MasterKind::Bias,
            Self::Dark => MasterKind::Dark,
            Self::Flat => MasterKind::Flat,
            Self::DarkFlat => MasterKind::DarkFlat,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Bias => "Bias",
            Self::Dark => "Darks",
            Self::Flat => "Flats",
            Self::DarkFlat => "Dark flats",
        }
    }
    pub fn role(self) -> &'static str {
        match self {
            Self::Bias => "Read-out offset of the sensor",
            Self::Dark => "Thermal signal and hot pixels",
            Self::Flat => "Vignetting and dust shadows",
            Self::DarkFlat => "Offset for the flats (flat exposure)",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Bias => "bias",
            Self::Dark => "dark",
            Self::Flat => "flat",
            Self::DarkFlat => "dark_flat",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
    /// IMAGETYP conventions of common capture programs.
    pub fn classify(image_type: &str) -> Option<Self> {
        let t = image_type.to_ascii_lowercase().replace(['_', '-'], " ");
        if t.contains("bias") || t.contains("offset") {
            Some(Self::Bias)
        } else if t.contains("dark flat")
            || t.contains("darkflat")
            || t.contains("flat dark")
            || t.contains("flatdark")
        {
            Some(Self::DarkFlat)
        } else if t.contains("dark") {
            Some(Self::Dark)
        } else if t.contains("flat") {
            Some(Self::Flat)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CalibrationSource {
    /// Individual frames; a master would be built in memory for each run.
    Frames(Vec<PathBuf>),
    Master(PathBuf),
}

#[derive(Clone, Debug)]
pub struct CalibrationItem {
    pub source: CalibrationSource,
    pub enabled: bool,
    /// Header of the master or of the first frame, plus readable frame count.
    pub summary: Option<FileMetadata>,
    /// How frames are combined into a master (darks); unused for a master file.
    pub combine: CombineMethod,
    /// Master currently held in RAM after "Stack master".
    pub ram: Option<std::sync::Arc<fulgur_pipeline::calibration::MasterDark>>,
}

impl CalibrationItem {
    pub fn count(&self) -> usize {
        match &self.source {
            CalibrationSource::Frames(files) => files.len(),
            CalibrationSource::Master(_) => 1,
        }
    }
    pub fn first_path(&self) -> Option<&Path> {
        match &self.source {
            CalibrationSource::Frames(files) => files.first().map(PathBuf::as_path),
            CalibrationSource::Master(path) => Some(path),
        }
    }
}

pub struct CalibrationSet {
    pub id: u64,
    pub name: String,
    pub items: [Option<CalibrationItem>; 4],
}

impl CalibrationSet {
    pub fn item(&self, kind: CalibrationKind) -> Option<&CalibrationItem> {
        self.items[kind.index()].as_ref()
    }
    pub fn enabled(&self, kind: CalibrationKind) -> Option<&CalibrationItem> {
        self.item(kind).filter(|i| i.enabled)
    }
    pub fn camera(&self) -> Option<String> {
        self.items
            .iter()
            .flatten()
            .find_map(|i| i.summary.as_ref().and_then(FileMetadata::camera))
    }
    pub fn flat_filter(&self) -> Option<String> {
        self.item(CalibrationKind::Flat)
            .and_then(|i| i.summary.as_ref())
            .and_then(|m| m.filter.clone())
            .map(|f| f.trim().to_owned())
    }
    pub fn is_seestar(&self) -> bool {
        self.camera()
            .is_some_and(|c| c.to_ascii_lowercase().contains("seestar"))
    }
    pub fn summary_text(&self) -> String {
        let parts: Vec<_> = CalibrationKind::ALL
            .iter()
            .filter_map(|&k| {
                self.item(k).map(|i| {
                    format!(
                        "{}{}{}",
                        k.title(),
                        if matches!(i.source, CalibrationSource::Master(_)) {
                            " (M)"
                        } else {
                            ""
                        },
                        if i.enabled { "" } else { " off" }
                    )
                })
            })
            .collect();
        if parts.is_empty() {
            "empty".into()
        } else {
            parts.join(" · ")
        }
    }
    /// Formula of the calibration this set would perform once implemented.
    pub fn formula(&self) -> String {
        let dark = self.enabled(CalibrationKind::Dark).is_some();
        let bias = self.enabled(CalibrationKind::Bias).is_some();
        let flat = self.enabled(CalibrationKind::Flat).is_some();
        let dark_flat = self.enabled(CalibrationKind::DarkFlat).is_some();
        if !(dark || bias || flat) {
            return "Light used as recorded · nothing switched on".into();
        }
        let light = if dark {
            "(Light − Dark)"
        } else if bias {
            "(Light − Bias)"
        } else {
            "Light"
        };
        let flat = if flat {
            format!(
                " ÷ normalize({})",
                if dark_flat {
                    "Flat − Dark flat"
                } else if bias {
                    "Flat − Bias"
                } else {
                    "Flat"
                }
            )
        } else {
            String::new()
        };
        format!("Calibrated light = {light}{flat}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Warn,
    Bad,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub level: Level,
    pub text: String,
}

fn date_days(date: &str) -> Option<i64> {
    let y: i64 = date.get(0..4)?.parse().ok()?;
    let m: i64 = date.get(5..7)?.parse().ok()?;
    let d: i64 = date.get(8..10)?.parse().ok()?;
    // Days from civil (Howard Hinnant), enough for whole-day differences.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    Some(era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy)
}

/// Header-based suitability of a calibration set for a light set. Display
/// only: calibration arithmetic is not implemented in this version.
pub fn compatibility(light: &SetSummary, set: &CalibrationSet) -> (Level, Vec<Check>) {
    let mut checks = vec![];
    let mut add = |level, text: String| checks.push(Check { level, text });
    let seestar = light.is_seestar();
    if !set.items.iter().flatten().any(|i| i.enabled) {
        add(
            Level::Warn,
            "No calibration file is switched on in this set".into(),
        );
    }
    let light_camera = light.cameras.first().cloned();
    match (set.camera(), light_camera) {
        (Some(c), Some(l)) if same_camera(&c, &l) => add(
            Level::Ok,
            format!("Same camera · {} × {}", light.width, light.height),
        ),
        (Some(c), Some(l)) => add(
            Level::Bad,
            format!("Calibration from {c} cannot be used for {l} lights"),
        ),
        _ => add(Level::Warn, "Camera not recorded in the headers".into()),
    }
    for item in set.items.iter().flatten().filter(|i| i.enabled) {
        if let Some(m) = &item.summary {
            if (m.width, m.height) != (light.width, light.height) {
                add(
                    Level::Bad,
                    format!(
                        "Calibration size {} × {} ≠ lights {} × {}",
                        m.width, m.height, light.width, light.height
                    ),
                );
                break;
            }
        }
    }
    if let Some(flat) = set.enabled(CalibrationKind::Flat) {
        match (
            flat.summary.as_ref().and_then(|m| m.filter.clone()),
            light.filters.as_slice(),
        ) {
            (Some(f), [l]) if f.trim() == l => add(Level::Ok, format!("Flat filter {l} matches")),
            (Some(f), [l]) => add(
                Level::Bad,
                format!("Flat filter {} ≠ light filter {l}", f.trim()),
            ),
            _ => add(Level::Warn, "Flat or light filter not recorded".into()),
        }
        if let (Some(f), Some(l)) = (
            flat.summary.as_ref().and_then(|m| m.rotator_degrees),
            light.rotator,
        ) {
            let difference = ((f - l).rem_euclid(360.)).min((l - f).rem_euclid(360.));
            if difference > 5. {
                add(
                    Level::Warn,
                    format!(
                        "Flats taken at rotator {f:.0}° vs lights {l:.0}° · dust and vignetting may not match"
                    ),
                );
            }
        }
        let flat_day = flat
            .summary
            .as_ref()
            .and_then(|m| m.date_obs.as_deref())
            .and_then(date_days);
        let light_day = light.first_date.as_deref().and_then(date_days);
        if let (Some(a), Some(b)) = (flat_day, light_day) {
            let days = (a - b).abs();
            if days <= 1 {
                add(Level::Ok, "Flats from the same night".into());
            } else {
                add(
                    Level::Warn,
                    format!("Flats taken {days} days apart · dust may have moved"),
                );
            }
        }
        if set.enabled(CalibrationKind::DarkFlat).is_none()
            && set.enabled(CalibrationKind::Bias).is_none()
        {
            add(
                Level::Warn,
                "No dark flats or bias · the flat offset is not removed".into(),
            );
        }
    }
    if let Some(dark) = set.enabled(CalibrationKind::Dark) {
        let meta = dark.summary.as_ref();
        match (
            meta.and_then(|m| m.exposure_seconds),
            light.exposures.as_slice(),
        ) {
            (Some(d), [l]) if (d - l).abs() < 1e-6 => add(
                Level::Ok,
                format!("Dark exposure {} s matches", trim_number(d)),
            ),
            (Some(d), lights) => add(
                Level::Warn,
                format!(
                    "Dark exposure {} s ≠ light exposure {} · subtracted without scaling",
                    trim_number(d),
                    lights
                        .iter()
                        .map(|e| format!("{} s", trim_number(*e)))
                        .collect::<Vec<_>>()
                        .join(" + ")
                ),
            ),
            _ => add(Level::Warn, "Dark exposure not recorded".into()),
        }
        if let (Some(g), [l]) = (meta.and_then(|m| m.gain), light.gains.as_slice()) {
            if (g - l).abs() > 1e-6 {
                add(
                    Level::Warn,
                    format!(
                        "Dark gain {} ≠ light gain {}",
                        trim_number(g),
                        trim_number(*l)
                    ),
                );
            }
        }
        if let (Some(t), Some(l)) = (meta.and_then(|m| m.ccd_temperature), light.temperature) {
            if (t - l).abs() > 3. {
                add(
                    Level::Warn,
                    format!("Dark temperature {t:.0} °C vs lights {l:.0} °C"),
                );
            }
        }
        if seestar {
            add(Level::Warn, "Seestar lights are already dark-corrected · subtracting again can clip faint signal".into());
        }
    } else if !seestar && set.camera().is_some() {
        add(
            Level::Warn,
            "No darks in use · hot pixels and amp glow stay in the lights".into(),
        );
    }
    if set.enabled(CalibrationKind::Bias).is_some() && seestar {
        add(
            Level::Warn,
            "Seestar lights are already bias-corrected".into(),
        );
    }
    let level = checks.iter().map(|c| c.level).max().unwrap_or(Level::Ok);
    (level, checks)
}

/// Camera labels written differently by firmware versions, e.g. "DWARFIII"
/// and "DWARF 3": compare letters and digits only, roman numerals as digits.
fn same_camera(a: &str, b: &str) -> bool {
    fn canonical(name: &str) -> String {
        let text: String = name
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_uppercase();
        for (roman, digit) in [("III", "3"), ("II", "2"), ("IV", "4")] {
            if let Some(stem) = text.strip_suffix(roman) {
                if stem.ends_with(|c: char| c.is_ascii_alphabetic()) {
                    return format!("{stem}{digit}");
                }
            }
        }
        text
    }
    canonical(a) == canonical(b)
}

fn is_master(path: &Path, meta: &FileMetadata) -> bool {
    meta.bitpix < 0
        || meta.combined_frames.is_some_and(|n| n > 1)
        || path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().contains("master"))
}

/// Files sorted into calibration kinds by IMAGETYP. A combined master wins
/// over individual frames of the same kind.
pub struct CalibrationScan {
    pub items: [Option<CalibrationItem>; 4],
    pub ignored: usize,
}

pub fn classify_calibration(files: &[(PathBuf, Result<FileMetadata, ()>)]) -> CalibrationScan {
    let mut frames: [Vec<(PathBuf, FileMetadata)>; 4] = Default::default();
    let mut masters: [Vec<(PathBuf, FileMetadata)>; 4] = Default::default();
    let mut ignored = 0;
    for (path, meta) in files {
        let Ok(meta) = meta else {
            ignored += 1;
            continue;
        };
        // IMAGETYP decides; capture programs without it (e.g. DWARF) name
        // the file or its folder instead.
        let name = |p: Option<&std::ffi::OsStr>| p.map(|n| n.to_string_lossy().into_owned());
        let by_name = || {
            name(path.file_name())
                .as_deref()
                .and_then(CalibrationKind::classify)
                .or_else(|| {
                    name(path.parent().and_then(Path::file_name))
                        .as_deref()
                        .and_then(CalibrationKind::classify)
                })
        };
        let Some(kind) = meta
            .image_type
            .as_deref()
            .and_then(CalibrationKind::classify)
            .or_else(|| meta.image_type.is_none().then(by_name)?)
        else {
            ignored += 1;
            continue;
        };
        // Capture programs often write a dark flat as plain IMAGETYP = Dark.
        let kind = if kind == CalibrationKind::Dark && by_name() == Some(CalibrationKind::DarkFlat)
        {
            CalibrationKind::DarkFlat
        } else {
            kind
        };
        let target = if is_master(path, meta) {
            &mut masters
        } else {
            &mut frames
        };
        target[kind.index()].push((path.clone(), meta.clone()));
    }
    // Darks of two exposures, one of them the flats' own: those are dark flats.
    // Mixing them would make a master dark of no real exposure.
    let flat = CalibrationKind::Flat.index();
    let flat_exposures: Vec<f64> = frames[flat]
        .iter()
        .chain(&masters[flat])
        .filter_map(|(_, m)| m.exposure_seconds)
        .collect();
    let matches_flat = |m: &FileMetadata| {
        m.exposure_seconds.is_some_and(|e| {
            flat_exposures
                .iter()
                .any(|f| (e - f).abs() <= f.abs() * 0.01)
        })
    };
    let dark = CalibrationKind::Dark.index();
    if frames[dark].iter().any(|(_, m)| matches_flat(m))
        && frames[dark].iter().any(|(_, m)| !matches_flat(m))
    {
        let (short, long): (Vec<_>, Vec<_>) = std::mem::take(&mut frames[dark])
            .into_iter()
            .partition(|(_, m)| matches_flat(m));
        frames[dark] = long;
        let dark_flats = &mut frames[CalibrationKind::DarkFlat.index()];
        dark_flats.extend(short);
        dark_flats.sort_by(|a, b| a.0.cmp(&b.0));
    }
    let items = std::array::from_fn(|i| {
        if let Some((path, meta)) = masters[i].first() {
            Some(CalibrationItem {
                source: CalibrationSource::Master(path.clone()),
                enabled: true,
                summary: Some(meta.clone()),
                combine: CombineMethod::default(),
                ram: None,
            })
        } else if let Some((_, meta)) = frames[i].first() {
            Some(CalibrationItem {
                source: CalibrationSource::Frames(
                    frames[i].iter().map(|(p, _)| p.clone()).collect(),
                ),
                enabled: true,
                summary: Some(meta.clone()),
                combine: CombineMethod::default(),
                ram: None,
            })
        } else {
            None
        }
    });
    CalibrationScan { items, ignored }
}

/// Add a header scan to a set: new frames join the frames of their kind,
/// anything else takes the kind's place. Returns the kinds that were empty.
pub fn merge_calibration(
    set: &mut CalibrationSet,
    items: [Option<CalibrationItem>; 4],
) -> Vec<CalibrationKind> {
    let mut added = vec![];
    for (kind, found) in CalibrationKind::ALL.into_iter().zip(items) {
        let Some(found) = found else {
            continue;
        };
        let slot = &mut set.items[kind.index()];
        match (slot.as_mut(), &found.source) {
            (Some(old), CalibrationSource::Frames(new)) => {
                if let CalibrationSource::Frames(frames) = &mut old.source {
                    frames.extend(new.iter().cloned());
                    frames.sort();
                    frames.dedup();
                    old.ram = None;
                } else {
                    *slot = Some(found);
                }
            }
            (Some(_), CalibrationSource::Master(_)) => *slot = Some(found),
            (None, _) => {
                *slot = Some(found);
                added.push(kind);
            }
        }
    }
    added
}

/// Header values and pixel statistics of one calibration frame, for the
/// owner's visual check. Reads only; the pixels are copied for the median.
pub fn frame_note(path: &Path, plane: &fulgur_core::fits::ReferencePlane) -> String {
    let meta = read_metadata(path).ok();
    let mut parts = vec![format!("{} × {}", plane.width, plane.height)];
    if let Some(m) = &meta {
        if let Some(e) = m.exposure_seconds {
            parts.push(format!("{} s", trim_number(e)));
        }
        if let Some(g) = m.gain {
            parts.push(format!("gain {}", trim_number(g)));
        }
        if let Some(t) = m.ccd_temperature {
            parts.push(format!("{t:.0} °C"));
        }
    }
    let (low, high) = plane
        .pixels
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), &v| {
            (low.min(v), high.max(v))
        });
    let mut sorted = plane.pixels.clone();
    let middle = sorted.len() / 2;
    let median = sorted
        .get(middle)
        .is_some()
        .then(|| *sorted.select_nth_unstable_by(middle, f32::total_cmp).1);
    let unit = if meta.is_some_and(|m| m.bitpix > 0) {
        " ADU"
    } else {
        ""
    };
    parts.push(format!(
        "median {:.1} · min {low:.1} · max {high:.1}{unit}",
        median.unwrap_or(0.)
    ));
    parts.join(" · ")
}

/// Background header scan; results arrive through `poll`.
pub enum ScanResult {
    Light {
        id: u64,
        summary: SetSummary,
    },
    Calibration {
        id: u64,
        scan: Box<CalibrationScan>,
    },
    Item {
        id: u64,
        kind: CalibrationKind,
        summary: Option<FileMetadata>,
    },
}

pub struct Scanner {
    sender: mpsc::Sender<ScanResult>,
    pub receiver: mpsc::Receiver<ScanResult>,
}

impl Default for Scanner {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }
}

/// Every FITS file of the given files and folders, subfolders included.
pub fn files_of(sources: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = vec![];
    for source in sources {
        let _ = fulgur_pipeline::survey::collect_inputs(source, &mut files);
    }
    files.sort();
    files.dedup();
    files
}

impl Scanner {
    pub fn light(&self, id: u64, sources: Vec<PathBuf>, repaint: impl Fn() + Send + 'static) {
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let metadata: Vec<_> = files_of(&sources)
                .iter()
                .map(|p| read_light_metadata(p).map_err(|_| ()))
                .collect();
            let _ = sender.send(ScanResult::Light {
                id,
                summary: SetSummary::from_metadata(&metadata),
            });
            repaint();
        });
    }
    pub fn calibration(&self, id: u64, sources: Vec<PathBuf>, repaint: impl Fn() + Send + 'static) {
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let files: Vec<_> = files_of(&sources)
                .into_iter()
                .map(|p| {
                    let meta = read_metadata(&p).map_err(|_| ());
                    (p, meta)
                })
                .collect();
            let _ = sender.send(ScanResult::Calibration {
                id,
                scan: Box::new(classify_calibration(&files)),
            });
            repaint();
        });
    }
    pub fn item(
        &self,
        id: u64,
        kind: CalibrationKind,
        path: PathBuf,
        repaint: impl Fn() + Send + 'static,
    ) {
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let _ = sender.send(ScanResult::Item {
                id,
                kind,
                summary: read_metadata(&path).ok(),
            });
            repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darks_without_imagetyp_are_sorted_by_name_and_dwarf_camera_labels_match() {
        let mut dark = meta("", "", 16);
        dark.image_type = None;
        dark.instrument = Some("DWARFIII".into());
        let files = vec![
            (
                PathBuf::from("Dark/Dark_26_raw_60s_0001.fits"),
                Ok(dark.clone()),
            ),
            (
                PathBuf::from("Dark/Dark_26_raw_60s_0002.fits"),
                Ok(dark.clone()),
            ),
            (PathBuf::from("Darks/raw_0003.fits"), Ok(dark.clone())),
            (PathBuf::from("misc/raw_0004.fits"), Ok(dark.clone())),
        ];
        let scan = classify_calibration(&files);
        assert_eq!(scan.ignored, 1, "neither name nor folder says dark");
        assert_eq!(
            scan.items[CalibrationKind::Dark.index()]
                .as_ref()
                .unwrap()
                .count(),
            3
        );
        assert!(same_camera("DWARFIII", "DWARF 3"));
        assert!(same_camera("Seestar S50", "seestar s50"));
        assert!(!same_camera("DWARFII", "DWARF 3"));
        assert!(!same_camera("ASI2600MC", "ASI533MC"));
    }

    #[test]
    fn dark_flats_written_as_darks_are_found_by_name_and_by_flat_exposure() {
        let frame = |kind: &str, seconds: f64| {
            let mut m = meta(kind, "", 16);
            m.exposure_seconds = Some(seconds);
            Ok(m)
        };
        let count = |scan: &CalibrationScan, kind: CalibrationKind| {
            scan.items[kind.index()].as_ref().map_or(0, |i| i.count())
        };
        // The folder name says it, with no flats in this scan.
        let by_name = classify_calibration(&[
            (PathBuf::from("FLATDARK 1s night/a.fit"), frame("Dark", 1.)),
            (PathBuf::from("DARK 300s/b.fit"), frame("Dark", 300.)),
        ]);
        assert_eq!(count(&by_name, CalibrationKind::DarkFlat), 1);
        assert_eq!(count(&by_name, CalibrationKind::Dark), 1);
        // Plain names: the flats' exposure tells the two groups apart.
        let by_exposure = classify_calibration(&[
            (PathBuf::from("c/1.fit"), frame("Flat", 1.)),
            (PathBuf::from("c/2.fit"), frame("Dark", 1.)),
            (PathBuf::from("c/3.fit"), frame("Dark", 1.)),
            (PathBuf::from("c/4.fit"), frame("Dark", 300.)),
        ]);
        assert_eq!(count(&by_exposure, CalibrationKind::DarkFlat), 2);
        assert_eq!(count(&by_exposure, CalibrationKind::Dark), 1);
        // Darks of the flats' exposure alone stay darks: nothing shows otherwise.
        let alone = classify_calibration(&[
            (PathBuf::from("c/1.fit"), frame("Flat", 10.)),
            (PathBuf::from("c/2.fit"), frame("Dark", 10.)),
        ]);
        assert_eq!(count(&alone, CalibrationKind::Dark), 1);
        assert_eq!(count(&alone, CalibrationKind::DarkFlat), 0);
    }

    fn meta(kind: &str, filter: &str, bitpix: i32) -> FileMetadata {
        FileMetadata {
            bitpix,
            width: 2160,
            height: 3840,
            planes: 1,
            image_type: Some(kind.into()),
            filter: Some(filter.into()),
            creator: Some("ZWO Seestar S50 Pro".into()),
            exposure_seconds: Some(10.),
            gain: Some(200.),
            date_obs: Some("2026-10-06T22:00:00".into()),
            ..Default::default()
        }
    }

    #[test]
    fn imagetyp_classification_prefers_dark_flats_and_masters() {
        assert_eq!(
            CalibrationKind::classify("Bias Frame"),
            Some(CalibrationKind::Bias)
        );
        assert_eq!(
            CalibrationKind::classify("Dark Flat"),
            Some(CalibrationKind::DarkFlat)
        );
        assert_eq!(
            CalibrationKind::classify("FLATDARK"),
            Some(CalibrationKind::DarkFlat)
        );
        assert_eq!(
            CalibrationKind::classify("Dark Frame"),
            Some(CalibrationKind::Dark)
        );
        assert_eq!(
            CalibrationKind::classify("Flat Field"),
            Some(CalibrationKind::Flat)
        );
        assert_eq!(CalibrationKind::classify("Light"), None);
        let files = vec![
            (PathBuf::from("f1.fit"), Ok(meta("Flat Field", "IRCUT", 16))),
            (PathBuf::from("f2.fit"), Ok(meta("Flat Field", "IRCUT", 16))),
            (PathBuf::from("df1.fit"), Ok(meta("Dark Flat", "IRCUT", 16))),
            (
                PathBuf::from("MasterDark.fit"),
                Ok(meta("Dark", "IRCUT", -32)),
            ),
            (PathBuf::from("dark1.fit"), Ok(meta("Dark", "IRCUT", 16))),
            (PathBuf::from("light.fit"), Ok(meta("Light", "IRCUT", 16))),
            (PathBuf::from("broken.fit"), Err(())),
        ];
        let scan = classify_calibration(&files);
        assert_eq!(scan.ignored, 2);
        assert!(scan.items[CalibrationKind::Bias.index()].is_none());
        assert_eq!(
            scan.items[CalibrationKind::Flat.index()]
                .as_ref()
                .unwrap()
                .count(),
            2
        );
        assert_eq!(
            scan.items[CalibrationKind::Dark.index()]
                .as_ref()
                .unwrap()
                .source,
            CalibrationSource::Master("MasterDark.fit".into())
        );
    }

    #[test]
    fn compatibility_rejects_other_filters_and_cameras_and_warns_for_seestar_darks() {
        let light = SetSummary::from_metadata(&[Ok(meta("Light", "IRCUT", 16))]);
        assert!(light.is_seestar());
        let item = |m: FileMetadata| {
            Some(CalibrationItem {
                source: CalibrationSource::Frames(vec!["x.fit".into()]),
                enabled: true,
                summary: Some(m),
                combine: CombineMethod::default(),
                ram: None,
            })
        };
        let mut set = CalibrationSet {
            id: 1,
            name: "flats".into(),
            items: [
                None,
                None,
                item(meta("Flat Field", "IRCUT", 16)),
                item(meta("Dark Flat", "IRCUT", 16)),
            ],
        };
        assert_eq!(compatibility(&light, &set).0, Level::Ok);
        set.items[2] = item(meta("Flat Field", "LP", 16));
        assert_eq!(compatibility(&light, &set).0, Level::Bad);
        set.items[2] = item(FileMetadata {
            creator: None,
            instrument: Some("ZWO ASI585MC".into()),
            ..meta("Flat Field", "IRCUT", 16)
        });
        assert_eq!(compatibility(&light, &set).0, Level::Bad);
        set.items[2] = item(meta("Flat Field", "IRCUT", 16));
        set.items[1] = item(meta("Dark", "IRCUT", 16));
        let (level, checks) = compatibility(&light, &set);
        assert_eq!(level, Level::Warn);
        assert!(
            checks
                .iter()
                .any(|c| c.text.contains("already dark-corrected"))
        );
        assert!(set.formula().contains("(Light − Dark)"));
    }

    #[test]
    fn only_mixed_bayer_headers_require_an_owner_choice() {
        let mut mono = meta("Light", "L", 16);
        mono.bayer_pattern = None;
        let mut colour = meta("Light", "L", 16);
        colour.bayer_pattern = Some(BayerPattern::RGGB);
        let mut set = LightSet {
            exposure: None,
            derived_confirmed: false,
            id: 1,
            name: "L".into(),
            sources: vec![],
            layout: LayoutChoice::Auto,
            include: true,
            calibration: None,
            summary: Some(SetSummary::from_metadata(&[Ok(mono.clone())])),
        };
        assert!(!set.needs_layout(), "no BAYERPAT means mono");
        assert!(set.is_mono());
        assert_eq!(set.layout_label(), "Mono · no Bayer pattern in headers");
        set.layout = LayoutChoice::Mono;
        assert!(!set.needs_layout());
        set.layout = LayoutChoice::Auto;
        set.summary = Some(SetSummary::from_metadata(&[Ok(mono), Ok(colour.clone())]));
        assert!(set.needs_layout());
        set.summary = Some(SetSummary::from_metadata(&[Ok(colour)]));
        assert!(!set.needs_layout());
        assert_eq!(set.layout_label(), "Colour · RGGB (FITS header)");
    }

    #[test]
    fn derived_values_need_confirmation_unless_the_owner_corrects_them() {
        use fulgur_core::derive::DerivedValue;
        let derived = |field, value: &str, confirm| DerivedValue {
            field,
            value: value.into(),
            source: "file name".into(),
            needs_confirmation: confirm,
        };
        let mut light = meta("Light", "L", 16);
        light.bayer_pattern = Some(BayerPattern::RGGB);
        light.exposure_seconds = Some(30.);
        light.derived = vec![
            derived("exposure_seconds", "30 s", true),
            derived("sensor_layout", "colour RGGB", true),
            derived("gain", "100", false),
        ];
        let mut set = LightSet {
            exposure: None,
            derived_confirmed: false,
            id: 1,
            name: "L".into(),
            sources: vec![],
            layout: LayoutChoice::Auto,
            include: true,
            calibration: None,
            summary: Some(SetSummary::from_metadata(&[
                Ok(light.clone()),
                Ok(light.clone()),
            ])),
        };
        assert_eq!(set.derived().len(), 3);
        assert_eq!(set.derived()[0].files, 2);
        assert!(set.needs_confirmation());
        assert_eq!(set.layout_label(), "Colour · RGGB (derived)");
        set.derived_confirmed = true;
        assert!(!set.needs_confirmation());
        // Owner corrections remove what they replace; labels need nothing.
        set.derived_confirmed = false;
        set.layout = LayoutChoice::Mono;
        set.exposure = Some(31.);
        assert_eq!(set.derived().len(), 1);
        assert!(!set.needs_confirmation());
        // A colour camera without a catalogued pattern must be declared.
        let mut unknown = meta("Light", "L", 16);
        unknown.colour_without_pattern = true;
        unknown.exposure_seconds = None;
        set.layout = LayoutChoice::Auto;
        set.exposure = None;
        set.summary = Some(SetSummary::from_metadata(&[Ok(unknown)]));
        assert!(set.needs_layout() && !set.is_mono());
        assert!(set.missing_exposure());
    }
}
