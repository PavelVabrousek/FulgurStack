//! Messages the owner must not miss (variant C of the 2026-11-09 design):
//! an error is a solid banner under the workflow bar, a warning is a callout
//! at the setting it concerns, and the status bar keeps only a counter. The
//! three severities differ in colour, icon and text size, never in colour
//! alone.
use super::*;
use egui::{Align2, FontId, Pos2, Sense, Shape, Stroke, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Severity {
    /// The run stopped or an action failed.
    Error,
    /// The result may be wrong or the run may fail.
    Warning,
    /// For the owner's information.
    Note,
}

impl Severity {
    /// Icon, border and text colour on a dark surface.
    pub(super) fn accent(self) -> Color32 {
        match self {
            Self::Error => Color32::from_rgb(255, 107, 96),
            Self::Warning => Color32::from_rgb(232, 162, 106),
            Self::Note => Color32::from_rgb(124, 196, 228),
        }
    }

    fn fill(self) -> Color32 {
        match self {
            Self::Error => Color32::from_rgb(58, 26, 31),
            Self::Warning => Color32::from_rgb(58, 42, 22),
            Self::Note => Color32::from_rgb(16, 48, 74),
        }
    }

    fn text(self) -> Color32 {
        match self {
            Self::Error => Color32::from_rgb(255, 214, 210),
            Self::Warning => Color32::from_rgb(241, 211, 184),
            Self::Note => Color32::from_rgb(207, 232, 244),
        }
    }

    pub(super) fn word(self, count: usize) -> &'static str {
        match (self, count == 1) {
            (Self::Error, true) => "error",
            (Self::Error, false) => "errors",
            (Self::Warning, true) => "warning",
            (Self::Warning, false) => "warnings",
            (Self::Note, true) => "note",
            (Self::Note, false) => "notes",
        }
    }
}

/// The severity of a status message that reports a failure, if it does.
pub(super) fn message_severity(message: &str) -> Option<Severity> {
    const FAILURES: [&str; 6] = [
        "Unable to finish",
        "Master failed",
        "Master not saved",
        "UI capture failed",
        "Preview:",
        "Reference preview:",
    ];
    FAILURES
        .iter()
        .any(|prefix| message.starts_with(prefix))
        .then_some(Severity::Error)
}

/// Whether a stopped run's message says that plate solving, the field centre
/// or the Gaia catalogue was the cause (not a disk or memory problem).
pub(super) fn sky_failure(message: &str) -> bool {
    let text = message.to_ascii_lowercase();
    ["plate-solv", "sky registration", "field centre", "gaia"]
        .iter()
        .any(|key| text.contains(key))
}

/// A round (error, note) or triangular (warning) badge with its sign. On a
/// solid banner the badge is white and the sign takes the banner colour.
pub(super) fn icon(ui: &mut egui::Ui, severity: Severity, size: f32, on_solid: Option<Color32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let painter = ui.painter();
    let (badge, sign) = match on_solid {
        Some(solid) => (Color32::WHITE, solid),
        None => (severity.accent(), Color32::from_rgb(20, 12, 12)),
    };
    let centre = rect.center();
    let glyph = match severity {
        Severity::Error => "×",
        Severity::Warning => "!",
        Severity::Note => "i",
    };
    let mut offset = 0.;
    if severity == Severity::Warning {
        let half = size / 2.;
        painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(centre.x, rect.top() + 1.),
                Pos2::new(rect.right() - 0.5, rect.bottom() - 1.5),
                Pos2::new(rect.left() + 0.5, rect.bottom() - 1.5),
            ],
            badge,
            Stroke::NONE,
        ));
        offset = half * 0.22;
    } else {
        painter.circle_filled(centre, size / 2., badge);
    }
    painter.text(
        Pos2::new(centre.x, centre.y + offset),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(size * 0.72),
        sign,
    );
}

/// A bordered message at the place it concerns: 14 pt text, the severity icon
/// and, when the owner can fix it there, a button. True when the button was
/// pressed.
pub(super) fn callout(
    ui: &mut egui::Ui,
    severity: Severity,
    text: &str,
    action: Option<&str>,
) -> bool {
    let mut pressed = false;
    egui::Frame::new()
        .fill(severity.fill())
        .stroke(Stroke::new(1.5_f32, severity.accent()))
        .corner_radius(8.)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                icon(ui, severity, 22., None);
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new(text).size(14.).color(severity.text()))
                            .wrap(),
                    );
                    if let Some(action) = action {
                        ui.add_space(2.);
                        pressed = ui
                            .add(egui::Button::new(
                                RichText::new(action).size(13.).color(severity.accent()),
                            ))
                            .clicked();
                    }
                });
            });
        });
    ui.add_space(4.);
    pressed
}
