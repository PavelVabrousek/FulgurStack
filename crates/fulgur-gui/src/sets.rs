//! Light and calibration sets: the owner's grouping of input folders.
//! Header-only planning; pixels are read by the pipeline, never here.
use fulgur_core::fits::{FileMetadata, read_metadata};
use fulgur_core::{BayerPattern, LayoutChoice};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Colour layout declared by the headers of a light set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderLayout {
    Bayer(BayerPattern),
    /// No file declares BAYERPAT: colour or mono must be chosen by the owner.
    Missing,
    /// Files disagree; the owner must choose one layout for the set.
    Mixed,
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
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub total_exposure: f64,
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
            if let Some(d) = &meta.date_obs {
                dates.push(d.clone());
            }
        }
        s.unreadable = items.iter().filter(|m| m.is_err()).count();
        s.exposures.sort_by(f64::total_cmp);
        s.header_layout = match layouts.as_slice() {
            [] => None,
            [Some(pattern)] => Some(HeaderLayout::Bayer(*pattern)),
            [None] => Some(HeaderLayout::Missing),
            _ => Some(HeaderLayout::Mixed),
        };
        temperatures.sort_by(f64::total_cmp);
        s.temperature = temperatures.get(temperatures.len() / 2).copied();
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
    pub include: bool,
    pub calibration: Option<u64>,
    pub summary: Option<SetSummary>,
}

impl LightSet {
    /// True when the owner must declare colour or mono before processing.
    pub fn needs_layout(&self) -> bool {
        self.layout == LayoutChoice::Auto
            && self.summary.as_ref().is_some_and(|s| {
                matches!(
                    s.header_layout,
                    Some(HeaderLayout::Missing | HeaderLayout::Mixed)
                )
            })
    }
    pub fn layout_label(&self) -> String {
        match (
            self.layout,
            self.summary.as_ref().and_then(|s| s.header_layout),
        ) {
            (LayoutChoice::Mono, _) => "Mono · no debayering".into(),
            (LayoutChoice::Bayer(p), _) => format!("Colour · {} (chosen)", p.name()),
            (LayoutChoice::Auto, Some(HeaderLayout::Bayer(p))) => {
                format!("Colour · {} (FITS header)", p.name())
            }
            (LayoutChoice::Auto, Some(HeaderLayout::Missing)) => {
                "No Bayer pattern in headers · choose".into()
            }
            (LayoutChoice::Auto, Some(HeaderLayout::Mixed)) => "Headers disagree · choose".into(),
            (LayoutChoice::Auto, None) => "Reading headers…".into(),
        }
    }
    pub fn is_mono(&self) -> bool {
        self.layout == LayoutChoice::Mono
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
        (Some(c), Some(l)) if c != l => add(
            Level::Bad,
            format!("Calibration from {c} cannot be used for {l} lights"),
        ),
        (Some(_), Some(_)) => add(
            Level::Ok,
            format!("Same camera · {} × {}", light.width, light.height),
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
                Level::Bad,
                format!(
                    "Dark exposure {} s ≠ light exposure {}",
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
                    Level::Bad,
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
        let Some(kind) = meta
            .image_type
            .as_deref()
            .and_then(CalibrationKind::classify)
        else {
            ignored += 1;
            continue;
        };
        let target = if is_master(path, meta) {
            &mut masters
        } else {
            &mut frames
        };
        target[kind.index()].push((path.clone(), meta.clone()));
    }
    let items = std::array::from_fn(|i| {
        if let Some((path, meta)) = masters[i].first() {
            Some(CalibrationItem {
                source: CalibrationSource::Master(path.clone()),
                enabled: true,
                summary: Some(meta.clone()),
            })
        } else if let Some((_, meta)) = frames[i].first() {
            Some(CalibrationItem {
                source: CalibrationSource::Frames(
                    frames[i].iter().map(|(p, _)| p.clone()).collect(),
                ),
                enabled: true,
                summary: Some(meta.clone()),
            })
        } else {
            None
        }
    });
    CalibrationScan { items, ignored }
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

fn files_of(sources: &[PathBuf]) -> Vec<PathBuf> {
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
                .map(|p| read_metadata(p).map_err(|_| ()))
                .collect();
            let _ = sender.send(ScanResult::Light {
                id,
                summary: SetSummary::from_metadata(&metadata),
            });
            repaint();
        });
    }
    pub fn calibration(&self, id: u64, folder: PathBuf, repaint: impl Fn() + Send + 'static) {
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let files: Vec<_> = files_of(&[folder])
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
    fn missing_or_mixed_bayer_headers_require_an_owner_choice() {
        let mut mono = meta("Light", "L", 16);
        mono.bayer_pattern = None;
        let mut colour = meta("Light", "L", 16);
        colour.bayer_pattern = Some(BayerPattern::RGGB);
        let mut set = LightSet {
            id: 1,
            name: "L".into(),
            sources: vec![],
            layout: LayoutChoice::Auto,
            include: true,
            calibration: None,
            summary: Some(SetSummary::from_metadata(&[Ok(mono.clone())])),
        };
        assert!(set.needs_layout());
        set.layout = LayoutChoice::Mono;
        assert!(!set.needs_layout());
        set.layout = LayoutChoice::Auto;
        set.summary = Some(SetSummary::from_metadata(&[Ok(mono), Ok(colour.clone())]));
        assert!(set.needs_layout());
        set.summary = Some(SetSummary::from_metadata(&[Ok(colour)]));
        assert!(!set.needs_layout());
        assert_eq!(set.layout_label(), "Colour · RGGB (FITS header)");
    }
}
