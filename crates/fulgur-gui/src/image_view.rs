//! Display-only pan/zoom with a bounded, cancellable native-resolution viewport.
use crate::preview::{DisplayOptions, Thumbnail, display_pixel};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextureHandle, Vec2};
use fulgur_core::{DemosaicMethod, Error, Result, check_cancelled};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

// Eight million visible pixels, never a full reconstructed dataset. The halo
// and buffers fit the existing GUI/runtime reserve and are released before work.
const DETAIL_PIXELS: usize = 8_388_608;
const DETAIL_EDGE: usize = 8192;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum DetailSource {
    /// A source frame with its light set's sensor layout.
    Cfa(PathBuf, fulgur_core::LayoutChoice),
    Export(PathBuf),
}

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
                .num_threads(1)
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
        let detail_wanted = !self.fit
            && self.scale > (thumbnail.width as f32 / native.x)
            && bounds[2] > 0
            && bounds[3] > 0
            && bounds[2] <= texture_edge
            && bounds[3] <= texture_edge
            && bounds[2].saturating_mul(bounds[3]) <= DETAIL_PIXELS;
        let overview_is_native = [thumbnail.width, thumbnail.height] == thumbnail.native_size;
        let no_native_source = source.is_none();
        let overview_only =
            !self.fit && !overview_is_native && (!detail_wanted || no_native_source);
        let desired = source.filter(|_| detail_wanted).map(|source| DetailKey {
            source,
            size: thumbnail.native_size,
            bounds,
            display,
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
                        ui.ctx().load_texture(
                            "native_viewport",
                            image,
                            egui::TextureOptions::NEAREST,
                        ),
                    ))
                }
                Err(error) => self.error = Some(format!("Detail unavailable: {error}")),
            }
        }
        if let Some(key) = &self.desired {
            let ready = self.ready.as_ref().is_some_and(|(loaded, _)| loaded == key);
            if !ready && self.task.is_none() && self.error.is_none() {
                if self
                    .changed
                    .is_some_and(|at| at.elapsed() >= Duration::from_millis(120))
                {
                    self.task = Some(DetailTask::start(
                        key.clone(),
                        thumbnail.stretch_parameters(),
                    ));
                } else {
                    ui.ctx().request_repaint_after(Duration::from_millis(125));
                }
            }
            if !ready {
                let message = self.error.as_deref().unwrap_or("Loading native detail…");
                painter.text(
                    viewport.left_bottom() + egui::vec2(8., -8.),
                    egui::Align2::LEFT_BOTTOM,
                    message,
                    egui::FontId::proportional(12.),
                    Color32::WHITE,
                );
            }
        }
        if let Some((key, texture)) = &self.ready {
            if self.desired.as_ref() == Some(key) {
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
        }
        if overview_only {
            painter.text(
                viewport.left_bottom() + egui::vec2(8., -8.),
                egui::Align2::LEFT_BOTTOM,
                if no_native_source {
                    "Overview preview only"
                } else {
                    "Overview preview · zoom further for native detail"
                },
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
            let raw = fulgur_core::fits::read_roi(
                path,
                &header,
                [left, top, right - left, bottom - top],
                cancel,
            )?;
            let (rgb, _) = fulgur_pipeline::debayer::debayer(&raw, cancel, DemosaicMethod::Vng)?;
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
        DetailSource::Export(path) => {
            let mut file = File::open(path)?;
            let before = file.metadata()?;
            let offset = export_offset(&mut file, key.size)?;
            let plane = key.size[0]
                .checked_mul(key.size[1])
                .ok_or(Error::Invalid("Display dimensions overflow."))?
                as u64;
            if before.len() < offset + plane * 12 {
                return Err(Error::Invalid("Truncated display FITS."));
            }
            let mut row = vec![[0_f32; 3]; width];
            let mut stored = vec![0_u8; width * 4];
            for line in y..y + height {
                check_cancelled(cancel)?;
                for channel in 0..3 {
                    file.seek(SeekFrom::Start(
                        offset + (channel as u64 * plane + (line * key.size[0] + x) as u64) * 4,
                    ))?;
                    file.read_exact(&mut stored)?;
                    for (value, word) in row.iter_mut().zip(stored.chunks_exact(4)) {
                        value[channel] = f32::from_be_bytes(word.try_into().unwrap());
                        if !value[channel].is_finite() {
                            return Err(Error::Invalid("Nonfinite display FITS sample."));
                        }
                    }
                }
                for &value in &row {
                    bytes.extend(display_pixel(value, params, key.display));
                }
            }
            let after = file.metadata()?;
            if before.len() != after.len() || before.modified()? != after.modified()? {
                return Err(Error::Invalid("Display FITS changed while reading."));
            }
        }
    }
    check_cancelled(cancel)?;
    Ok(egui::ColorImage::from_rgb([width, height], &bytes))
}

/// Only read FulgurStack's already published top-down float RGB products.
/// Header cards remain private; no GPS fields enter errors, labels or logs.
fn export_offset(file: &mut File, size: [usize; 2]) -> Result<u64> {
    let mut cards = BTreeMap::new();
    for block in 0..128 {
        let mut bytes = [0_u8; 2880];
        file.read_exact(&mut bytes)?;
        for card in bytes.chunks_exact(80) {
            let name = std::str::from_utf8(&card[..8]).unwrap_or("").trim();
            if name == "END" {
                let valid = cards.get("SIMPLE").is_some_and(|v: &String| v == "T")
                    && cards.get("BITPIX").is_some_and(|v| v == "-32")
                    && cards.get("NAXIS").is_some_and(|v| v == "3")
                    && cards.get("NAXIS3").is_some_and(|v| v == "3")
                    && cards.get("NAXIS1") == Some(&size[0].to_string())
                    && cards.get("NAXIS2") == Some(&size[1].to_string())
                    && cards.get("ROWORDER").is_some_and(|v| v == "'TOP-DOWN'")
                    && !cards.contains_key("BZERO")
                    && !cards.contains_key("BSCALE");
                return if valid {
                    Ok((block + 1) * 2880)
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
