//! Display-only pan/zoom with a bounded, cancellable native-resolution viewport.
use crate::preview::{DisplayOptions, Thumbnail, display_pixel};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextureHandle, Vec2};
use fulgur_core::{DemosaicMethod, Error, Result, check_cancelled};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

// Sixteen million visible pixels, never a full reconstructed dataset. The halo
// and buffers fit the existing GUI/runtime reserve and are released before work.
const DETAIL_PIXELS: usize = 16_777_216;
const DETAIL_EDGE: usize = 8192;
/// Native detail is read this much wider than the view, so panning stays sharp.
const DETAIL_MARGIN: f64 = 1.4;
/// Wait after a change of view before reading native detail.
const DETAIL_DELAY: Duration = Duration::from_millis(50);
/// Pixels of one plane read from an XISF product at a time.
const XISF_STRIP_PIXELS: usize = 1 << 20;
/// Overviews shrink through mipmaps and enlarge smoothly.
pub(crate) const OVERVIEW_TEXTURE: egui::TextureOptions =
    egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
/// Native detail shows exact pixels when enlarged and averages them when shrunk.
const DETAIL_TEXTURE: egui::TextureOptions = egui::TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    ..OVERVIEW_TEXTURE
};

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum DetailSource {
    /// A source frame with its light set's sensor layout.
    Cfa(PathBuf, fulgur_core::LayoutChoice),
    Export(PathBuf),
    /// A calibration master plane already in memory.
    Plane(PlaneRef),
}

/// Pixels of one calibration plane: a RAM master or a loaded master file.
pub(crate) enum PlanePixels {
    Master(Arc<fulgur_pipeline::calibration::MasterDark>),
    Owned(Vec<f32>),
}

pub(crate) struct PlaneData {
    pub width: usize,
    pub height: usize,
    pixels: PlanePixels,
    /// 65535 for ADU values, 1 for planes normalized to 0..1.
    divisor: f32,
}

impl PlaneData {
    pub fn new(width: usize, height: usize, pixels: PlanePixels) -> Self {
        let mut plane = Self {
            width,
            height,
            pixels,
            divisor: 1.,
        };
        let maximum = plane.values().iter().copied().fold(0_f32, f32::max);
        plane.divisor = if maximum > 1.5 { 65535. } else { 1. };
        plane
    }
    pub fn values(&self) -> &[f32] {
        match &self.pixels {
            PlanePixels::Master(m) => &m.pixels,
            PlanePixels::Owned(v) => v,
        }
    }
}

/// Shared plane compared by identity, so detail caching never copies pixels.
#[derive(Clone)]
pub(crate) struct PlaneRef(pub Arc<PlaneData>);
impl PartialEq for PlaneRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for PlaneRef {}

#[derive(Clone, PartialEq)]
struct DetailKey {
    source: DetailSource,
    size: [usize; 2],
    bounds: [usize; 4],
    display: DisplayOptions,
}

struct DetailTask {
    key: DetailKey,
    result: mpsc::Receiver<Result<egui::ColorImage>>,
    cancel: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for DetailTask {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl DetailTask {
    fn start(key: DetailKey, params: [(f32, f32); 3]) -> Self {
        let (tx, result) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let request = key.clone();
        let handle = std::thread::spawn(move || {
            let result = rayon::ThreadPoolBuilder::new()
                .num_threads(crate::preview::preview_threads())
                .build()
                .map_err(|_| Error::Invalid("Could not start detail preview worker."))
                .and_then(|pool| pool.install(|| load_detail(&request, params, &worker_cancel)));
            let _ = tx.send(result);
        });
        Self {
            key,
            result,
            cancel,
            handle: Some(handle),
        }
    }
}

#[derive(Default)]
pub(crate) struct ImageView {
    fit: bool,
    initialized: bool,
    scale: f32,
    center: Pos2,
    task: Option<DetailTask>,
    desired: Option<DetailKey>,
    changed: Option<Instant>,
    ready: Option<(DetailKey, TextureHandle)>,
    error: Option<String>,
    /// Native size of the shown image; zoom/pan carry over between images of
    /// the same size and reset for another size.
    native: Option<[usize; 2]>,
}

impl ImageView {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Follow another view's zoom and pan (blink partners share geometry).
    pub fn copy_geometry(&mut self, other: &ImageView) {
        self.fit = other.fit;
        self.initialized = other.initialized;
        self.scale = other.scale;
        self.center = other.center;
        self.native = other.native;
    }

    /// The overview changed: detail that failed against the old one may retry.
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    pub fn stop_loading(&mut self) {
        self.task = None;
        self.ready = None;
        self.desired = None;
        self.error = None;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        overview: &TextureHandle,
        thumbnail: &Thumbnail,
        source: Option<DetailSource>,
        display: DisplayOptions,
    ) {
        let previous_scale = self.scale;
        let native = Vec2::new(
            thumbnail.native_size[0] as f32,
            thumbnail.native_size[1] as f32,
        );
        if self.native != Some(thumbnail.native_size) {
            self.initialized = false;
            self.native = Some(thumbnail.native_size);
        }
        if !self.initialized {
            self.center = (native * 0.5).to_pos2();
            self.fit = true;
            self.initialized = true;
        }
        let ppp = ui.ctx().pixels_per_point();
        ui.horizontal(|ui| {
            if ui.selectable_label(self.fit, "Fit").clicked() {
                self.fit = true;
                self.center = (native * 0.5).to_pos2();
            }
            if ui
                .add_enabled(source.is_some(), egui::Button::new("1:1"))
                .on_hover_text("One image pixel per physical screen pixel")
                .clicked()
            {
                self.fit = false;
                self.scale = 1.;
            }
            ui.label(format!("{:.1}%", self.scale * 100.));
        });
        ui.label(
            egui::RichText::new("Wheel: zoom · Drag: pan · Double-click: fit")
                .small()
                .weak(),
        );
        let available = ui.available_size().max(Vec2::splat(1.));
        let (viewport, response) = ui.allocate_exact_size(available, Sense::click_and_drag());
        let fit_scale = (viewport.width() / native.x).min(viewport.height() / native.y) * ppp;
        if self.fit {
            self.scale = fit_scale;
            self.center = (native * 0.5).to_pos2();
        }
        if response.double_clicked() {
            self.fit = true;
            self.scale = fit_scale;
            self.center = (native * 0.5).to_pos2();
        }
        if response.hovered() {
            // Consume scrolling only over the image, so its containing panel
            // cannot scroll at the same time as the image zooms.
            let (scroll, pinch, pointer) = ui.input_mut(|input| {
                let scroll = input.smooth_scroll_delta.y;
                input.smooth_scroll_delta = Vec2::ZERO;
                input.raw_scroll_delta = Vec2::ZERO;
                (scroll, input.zoom_delta(), input.pointer.hover_pos())
            });
            let factor = (scroll * 0.003).exp() * pinch;
            if (factor - 1.).abs() > 0.00001 {
                let pointer = pointer.unwrap_or(viewport.center());
                let old = self.scale / ppp;
                let new_scale = (self.scale * factor).clamp(fit_scale.min(0.01), 32.);
                self.center += (pointer - viewport.center()) * (1. / old - ppp / new_scale);
                self.scale = new_scale;
                self.fit = false;
            }
        }
        let points_per_pixel = self.scale / ppp;
        if response.dragged() {
            self.fit = false;
            // Apply only this frame's motion in image coordinates.
            self.center -= ui.input(|i| i.pointer.delta()) / points_per_pixel;
        }
        for axis in 0..2 {
            let half = viewport.size()[axis] / points_per_pixel * 0.5;
            self.center[axis] = if native[axis] > 2. * half {
                self.center[axis].clamp(half, native[axis] - half)
            } else {
                native[axis] * 0.5
            };
        }
        if response.dragged() {
            response.on_hover_cursor(egui::CursorIcon::Grabbing);
        } else {
            response.on_hover_cursor(egui::CursorIcon::Grab);
        }
        let image_origin = viewport.center() - self.center.to_vec2() * points_per_pixel;
        let image_rect = Rect::from_min_size(image_origin, native * points_per_pixel);
        let painter = ui.painter().with_clip_rect(viewport);
        painter.rect_filled(viewport, 0., Color32::from_rgb(3, 9, 16));
        painter.image(
            overview.id(),
            image_rect,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );

        let visible = image_rect.intersect(viewport);
        let start = ((visible.min - image_origin) / points_per_pixel)
            .floor()
            .max(Vec2::ZERO);
        let end = ((visible.max - image_origin) / points_per_pixel)
            .ceil()
            .min(native);
        let bounds = [
            start.x as usize,
            start.y as usize,
            (end.x - start.x).max(0.) as usize,
            (end.y - start.y).max(0.) as usize,
        ];
        let texture_edge = ui.input(|input| input.max_texture_side).min(DETAIL_EDGE);
        let detail_wanted = self.scale > (thumbnail.width as f32 / native.x)
            && bounds[2] > 0
            && bounds[3] > 0
            && bounds[2] <= texture_edge
            && bounds[3] <= texture_edge
            && bounds[2].saturating_mul(bounds[3]) <= DETAIL_PIXELS;
        let overview_is_native = [thumbnail.width, thumbnail.height] == thumbnail.native_size;
        let overview_only = !self.fit && !overview_is_native && source.is_none();
        // Detail already read or being read that covers the view needs no new read.
        let desired = source.filter(|_| detail_wanted).map(|source| {
            let covers = |key: &&DetailKey| {
                key.source == source
                    && key.size == thumbnail.native_size
                    && key.display == display
                    && contains(key.bounds, bounds)
            };
            let known = self.ready.as_ref().map(|(key, _)| key).filter(covers);
            match known.or(self.desired.as_ref().filter(covers)) {
                Some(key) => key.clone(),
                None => DetailKey {
                    bounds: widened(bounds, thumbnail.native_size, texture_edge),
                    source,
                    size: thumbnail.native_size,
                    display,
                },
            }
        });
        if self.desired != desired {
            self.task = None;
            self.desired = desired;
            self.changed = Some(Instant::now());
            self.error = None;
        }
        let completed = self
            .task
            .as_ref()
            .and_then(|task| match task.result.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(Error::Invalid("Detail preview worker stopped.")))
                }
            });
        if let Some(result) = completed {
            let task = self.task.take().unwrap();
            match result {
                Ok(image) => {
                    self.ready = Some((
                        task.key.clone(),
                        ui.ctx()
                            .load_texture("native_viewport", image, DETAIL_TEXTURE),
                    ))
                }
                Err(error) => self.error = Some(format!("Detail unavailable: {error}")),
            }
        }
        if let Some(key) = &self.desired {
            let ready = self.ready.as_ref().is_some_and(|(loaded, _)| loaded == key);
            if !ready && self.task.is_none() && self.error.is_none() {
                if self.changed.is_some_and(|at| at.elapsed() >= DETAIL_DELAY) {
                    let params = if key.display.stretch {
                        thumbnail.stretch_parameters()
                    } else {
                        [(0., 0.5); 3]
                    };
                    self.task = Some(DetailTask::start(key.clone(), params));
                } else {
                    ui.ctx().request_repaint_after(DETAIL_DELAY);
                }
            }
        }
        // The last detail stays until the next one is in, even of another frame:
        // moving or changing frames never falls back to a softer image.
        let shown = self.ready.as_ref().filter(|(key, _)| {
            self.desired.is_some() && key.size == thumbnail.native_size && key.display == display
        });
        if let Some((key, texture)) = shown {
            let [x, y, width, height] = key.bounds;
            let rect = Rect::from_min_size(
                image_origin + egui::vec2(x as f32, y as f32) * points_per_pixel,
                egui::vec2(width as f32, height as f32) * points_per_pixel,
            );
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        if let Some(message) = self.error.as_deref().filter(|_| self.desired.is_some()) {
            // A failed full-resolution read must not pass for a sharp image:
            // the softer overview stays visible under a clear red banner.
            let text = format!(
                "{message}
Showing the softer overview instead."
            );
            let galley = painter.layout(
                text,
                egui::FontId::proportional(15.),
                Color32::WHITE,
                (viewport.width() - 48.).max(120.),
            );
            let size = galley.size() + egui::vec2(24., 16.);
            let banner = Rect::from_center_size(
                egui::pos2(viewport.center().x, viewport.top() + 12. + size.y / 2.),
                size,
            );
            painter.rect_filled(banner, 6., Color32::from_rgb(170, 30, 30));
            painter.galley(banner.min + egui::vec2(12., 8.), galley, Color32::WHITE);
        }
        if overview_only {
            painter.text(
                viewport.left_bottom() + egui::vec2(8., -8.),
                egui::Align2::LEFT_BOTTOM,
                "Overview preview only",
                egui::FontId::proportional(12.),
                Color32::WHITE,
            );
        }
        if self.scale != previous_scale {
            // Refresh the percentage once after a fit/gesture changes scale.
            ui.ctx().request_repaint();
        }
        if self.task.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }
}

fn contains(outer: [usize; 4], inner: [usize; 4]) -> bool {
    inner[0] >= outer[0]
        && inner[1] >= outer[1]
        && inner[0] + inner[2] <= outer[0] + outer[2]
        && inner[1] + inner[3] <= outer[1] + outer[3]
}

/// `bounds` with a margin on every side, within the image and the read limits.
fn widened(bounds: [usize; 4], size: [usize; 2], edge: usize) -> [usize; 4] {
    let area = (bounds[2] * bounds[3]).max(1) as f64;
    let grow = (DETAIL_PIXELS as f64 / area)
        .sqrt()
        .clamp(1., DETAIL_MARGIN);
    let side = |start: usize, length: usize, limit: usize| {
        let wide = ((length as f64 * grow) as usize)
            .min(edge.max(length))
            .min(limit)
            .max(length);
        (
            start.saturating_sub((wide - length) / 2).min(limit - wide),
            wide,
        )
    };
    let (x, width) = side(bounds[0], bounds[2], size[0]);
    let (y, height) = side(bounds[1], bounds[3], size[1]);
    [x, y, width, height]
}

fn load_detail(
    key: &DetailKey,
    params: [(f32, f32); 3],
    cancel: &AtomicBool,
) -> Result<egui::ColorImage> {
    let [x, y, width, height] = key.bounds;
    if width == 0
        || height == 0
        || width > DETAIL_EDGE
        || height > DETAIL_EDGE
        || width.checked_mul(height).is_none_or(|n| n > DETAIL_PIXELS)
        || x.checked_add(width).is_none_or(|end| end > key.size[0])
        || y.checked_add(height).is_none_or(|end| end > key.size[1])
    {
        return Err(Error::Invalid("Invalid bounded display viewport."));
    }
    let mut bytes = Vec::with_capacity(width * height * 3);
    match &key.source {
        DetailSource::Cfa(path, layout) => {
            let before = std::fs::metadata(path)?;
            let mut header = fulgur_core::fits::read_header(path)?;
            header.info.apply_layout(*layout);
            if [header.info.width, header.info.height] != key.size {
                return Err(Error::Invalid("Source dimensions changed; reload preview."));
            }
            // Keep original CFA phase and enough halo for cosmetics plus VNG.
            let left = x.saturating_sub(8) & !1;
            let top = y.saturating_sub(8) & !1;
            let right = (x + width + 8).min(key.size[0]);
            let bottom = (y + height + 8).min(key.size[1]);
            let roi = [left, top, right - left, bottom - top];
            let rgb = if header.info.mosaic == fulgur_core::Mosaic::Rgb {
                fulgur_core::fits::read_rgb_roi(&header, roi, cancel, vec![])?
            } else {
                let raw = fulgur_core::fits::read_roi(path, &header, roi, cancel)?;
                fulgur_pipeline::debayer::debayer(&raw, cancel, DemosaicMethod::Vng)?.0
            };
            for row in 0..height {
                check_cancelled(cancel)?;
                for col in 0..width {
                    let value = rgb.pixels[(y + row - top) * rgb.width + x + col - left]
                        .map(|v| (v / 65535.).clamp(0., 1.));
                    bytes.extend(display_pixel(value, params, key.display));
                }
            }
            let after = std::fs::metadata(path)?;
            if before.len() != after.len() || before.modified()? != after.modified()? {
                return Err(Error::Invalid(
                    "Source changed while reading detail; reload preview.",
                ));
            }
        }
        DetailSource::Plane(plane) => {
            let p = &plane.0;
            if [p.width, p.height] != key.size {
                return Err(Error::Invalid("Master dimensions changed; reload preview."));
            }
            let values = p.values();
            for row in y..y + height {
                check_cancelled(cancel)?;
                for &value in &values[row * p.width + x..row * p.width + x + width] {
                    let grey = (value / p.divisor).clamp(0., 1.);
                    bytes.extend(display_pixel([grey; 3], params, key.display));
                }
            }
        }
        DetailSource::Export(path) => {
            read_export_rows(path, key.size, key.bounds, cancel, |row| {
                for &value in row {
                    bytes.extend(display_pixel(value, params, key.display));
                }
                Ok(())
            })?;
        }
    }
    check_cancelled(cancel)?;
    Ok(egui::ColorImage::from_rgb([width, height], &bytes))
}

/// Rows of a published product (master or map, FITS or XISF) inside `bounds`,
/// in the stored 0..1 output scale that the overview stretch is fitted to.
/// A monochrome product is one plane repeated as grey.
fn read_export_rows(
    path: &Path,
    size: [usize; 2],
    bounds: [usize; 4],
    cancel: &AtomicBool,
    emit: impl FnMut(&[[f32; 3]]) -> Result<()>,
) -> Result<()> {
    if fulgur_core::xisf::is_xisf(path) {
        read_xisf_rows(path, size, bounds, XISF_STRIP_PIXELS, cancel, emit)
    } else {
        read_fits_rows(path, size, bounds, cancel, emit)
    }
}

fn read_fits_rows(
    path: &Path,
    size: [usize; 2],
    bounds: [usize; 4],
    cancel: &AtomicBool,
    mut emit: impl FnMut(&[[f32; 3]]) -> Result<()>,
) -> Result<()> {
    let [x, y, width, height] = bounds;
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    let (offset, planes) = export_offset(&mut file, size)?;
    let plane = size[0]
        .checked_mul(size[1])
        .ok_or(Error::Invalid("Display dimensions overflow."))? as u64;
    if before.len() < offset + plane * 4 * planes as u64 {
        return Err(Error::Invalid("Truncated display FITS."));
    }
    let mut row = vec![[0_f32; 3]; width];
    let mut stored = vec![0_u8; width * 4];
    for line in y..y + height {
        check_cancelled(cancel)?;
        for channel in 0..planes {
            file.seek(SeekFrom::Start(
                offset + (channel as u64 * plane + (line * size[0] + x) as u64) * 4,
            ))?;
            file.read_exact(&mut stored)?;
            for (value, word) in row.iter_mut().zip(stored.chunks_exact(4)) {
                value[channel] = f32::from_be_bytes(word.try_into().unwrap());
                if !value[channel].is_finite() {
                    return Err(Error::Invalid("Nonfinite display FITS sample."));
                }
            }
        }
        if planes == 1 {
            for value in &mut row {
                *value = [value[0]; 3];
            }
        }
        emit(&row)?;
    }
    let after = file.metadata()?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(Error::Invalid("Display FITS changed while reading."));
    }
    Ok(())
}

/// XISF products are read through the core reader in strips, so a compressed
/// file is decoded once per band and memory stays bounded. The reader returns
/// ADU (`stored * 65535` for FulgurStack's `bounds="0:1"` Float32), so dividing
/// by the output divisor gives the same scale as the FITS samples.
fn read_xisf_rows(
    path: &Path,
    size: [usize; 2],
    bounds: [usize; 4],
    strip_pixels: usize,
    cancel: &AtomicBool,
    mut emit: impl FnMut(&[[f32; 3]]) -> Result<()>,
) -> Result<()> {
    let [x, y, width, height] = bounds;
    let before = std::fs::metadata(path)?;
    let image = fulgur_core::xisf::open_image(path)?;
    if [image.width, image.height] != size || !matches!(image.channels, 1 | 3) {
        return Err(Error::Invalid("Unsupported or changed display XISF image."));
    }
    let planes = image.channels;
    let strip = (strip_pixels / width.max(1)).clamp(1, height.max(1));
    let mut buffers = vec![Vec::new(); planes];
    let mut row = vec![[0_f32; 3]; width];
    let mut first = y;
    while first < y + height {
        let rows = strip.min(y + height - first);
        for (channel, buffer) in buffers.iter_mut().enumerate() {
            buffer.resize(width * rows, 0.);
            image.read_region(channel, [x, first, width, rows], buffer, cancel)?;
        }
        for line in 0..rows {
            check_cancelled(cancel)?;
            for (col, value) in row.iter_mut().enumerate() {
                for (channel, sample) in value.iter_mut().enumerate() {
                    *sample = buffers[channel.min(planes - 1)][line * width + col]
                        / fulgur_core::output::OUTPUT_ADU_DIVISOR;
                    if !sample.is_finite() {
                        return Err(Error::Invalid("Nonfinite display XISF sample."));
                    }
                }
            }
            emit(&row)?;
        }
        first += rows;
    }
    let after = std::fs::metadata(path)?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(Error::Invalid("Display XISF changed while reading."));
    }
    Ok(())
}

/// Only read FulgurStack's already published top-down float products: planar
/// RGB, or one plane for monochrome masters and maps. Returns the data offset
/// and plane count. Header cards remain private; no GPS fields enter errors,
/// labels or logs.
fn export_offset(file: &mut File, size: [usize; 2]) -> Result<(u64, usize)> {
    let mut cards = BTreeMap::new();
    for block in 0..128 {
        let mut bytes = [0_u8; 2880];
        file.read_exact(&mut bytes)?;
        for card in bytes.chunks_exact(80) {
            let name = std::str::from_utf8(&card[..8]).unwrap_or("").trim();
            if name == "END" {
                let planes = match (
                    cards.get("NAXIS").map(String::as_str),
                    cards.get("NAXIS3").map(String::as_str),
                ) {
                    (Some("3"), Some("3")) => 3,
                    (Some("2"), None) => 1,
                    _ => 0,
                };
                let valid = cards.get("SIMPLE").is_some_and(|v: &String| v == "T")
                    && cards.get("BITPIX").is_some_and(|v| v == "-32")
                    && planes > 0
                    && cards.get("NAXIS1") == Some(&size[0].to_string())
                    && cards.get("NAXIS2") == Some(&size[1].to_string())
                    && cards.get("ROWORDER").is_some_and(|v| v == "'TOP-DOWN'")
                    && !cards.contains_key("BZERO")
                    && !cards.contains_key("BSCALE");
                return if valid {
                    Ok(((block + 1) as u64 * 2880, planes))
                } else {
                    Err(Error::Invalid(
                        "Unsupported or changed display FITS header.",
                    ))
                };
            }
            if matches!(
                name,
                "SIMPLE"
                    | "BITPIX"
                    | "NAXIS"
                    | "NAXIS1"
                    | "NAXIS2"
                    | "NAXIS3"
                    | "ROWORDER"
                    | "BZERO"
                    | "BSCALE"
            ) {
                let value = std::str::from_utf8(&card[10..])
                    .unwrap_or("")
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .trim();
                if cards.insert(name.to_owned(), value.to_owned()).is_some() {
                    return Err(Error::Invalid("Duplicate display FITS keyword."));
                }
            }
        }
    }
    Err(Error::Invalid(
        "Display FITS header exceeds its bounded size.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fulgur_core::output::write_rgb_atomic;
    use fulgur_core::{RgbImage, fits::ImageHeader};
    use std::io::Cursor;

    const SIZE: [usize; 2] = [37, 23];

    fn source() -> ImageHeader {
        let mut bytes = Vec::new();
        for (key, value) in [
            ("SIMPLE", "T"),
            ("BITPIX", "16"),
            ("NAXIS", "2"),
            ("NAXIS1", "37"),
            ("NAXIS2", "23"),
            ("BZERO", "32768"),
            ("BSCALE", "1"),
            ("BAYERPAT", "'GRBG'"),
            ("EXPTIME", "10."),
        ] {
            bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
        }
        bytes.extend(format!("{:<80}", "END").as_bytes());
        bytes.resize(bytes.len().div_ceil(2880) * 2880, b' ');
        bytes.resize(
            bytes.len() + (SIZE[0] * SIZE[1] * 2).div_ceil(2880) * 2880,
            0,
        );
        ImageHeader::read(&mut Cursor::new(&bytes), bytes.len() as u64).unwrap()
    }

    /// Distinct, irregular values per pixel and channel, some outside 0..1.
    /// A monochrome product is written from the middle channel.
    fn master(planes: usize) -> RgbImage {
        RgbImage {
            width: SIZE[0],
            height: SIZE[1],
            demosaic_method: if planes == 1 {
                DemosaicMethod::None
            } else {
                DemosaicMethod::Mhc
            },
            pixels: (0..SIZE[0] * SIZE[1])
                .map(|i| {
                    let base = (i * 7919 % 65_000) as f32 + 0.37;
                    [base, base * 0.61 + 11.5, 66_000. - base * 1.03]
                })
                .collect(),
        }
    }

    fn crop(
        read: impl FnOnce(&mut dyn FnMut(&[[f32; 3]]) -> Result<()>) -> Result<()>,
    ) -> Vec<[f32; 3]> {
        let mut out = Vec::new();
        read(&mut |row| {
            out.extend_from_slice(row);
            Ok(())
        })
        .unwrap();
        out
    }

    #[test]
    fn xisf_and_fits_master_crops_match_file_pixels() {
        let directory = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let bounds = [5, 3, 20, 11];
        for planes in [1, 3] {
            let image = master(planes);
            let fits = directory.path().join(format!("master{planes}.fits"));
            let xisf = directory.path().join(format!("master{planes}.xisf"));
            write_rgb_atomic(&fits, &image, &source(), &cancel).unwrap();
            write_rgb_atomic(&xisf, &image, &source(), &cancel).unwrap();
            let from_fits = crop(|emit| read_export_rows(&fits, SIZE, bounds, &cancel, emit));
            let from_xisf = crop(|emit| read_export_rows(&xisf, SIZE, bounds, &cancel, emit));
            // Several strips, including a short last one, give the same pixels.
            let strips = crop(|emit| read_xisf_rows(&xisf, SIZE, bounds, 60, &cancel, emit));
            assert_eq!(from_fits.len(), bounds[2] * bounds[3]);
            assert_eq!(strips, from_xisf);
            for (index, (fits_value, xisf_value)) in from_fits.iter().zip(&from_xisf).enumerate() {
                let (row, col) = (index / bounds[2], index % bounds[2]);
                let pixel = image.pixels[(bounds[1] + row) * SIZE[0] + bounds[0] + col];
                for channel in 0..3 {
                    let stored =
                        (pixel[if planes == 1 { 1 } else { channel }] / 65535.).clamp(0., 1.);
                    assert_eq!(fits_value[channel], stored);
                    assert!((xisf_value[channel] - stored).abs() <= 1e-6);
                }
            }
        }
        // The whole frame reads too, and a wrong size is refused for both formats.
        let xisf = directory.path().join("master3.xisf");
        assert_eq!(
            crop(|emit| read_export_rows(&xisf, SIZE, [0, 0, SIZE[0], SIZE[1]], &cancel, emit))
                .len(),
            SIZE[0] * SIZE[1]
        );
        for name in ["master3.xisf", "master3.fits"] {
            let path = directory.path().join(name);
            assert!(read_export_rows(&path, [36, 23], [0, 0, 4, 4], &cancel, |_| Ok(())).is_err());
        }
    }
}
