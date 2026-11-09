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

/// A warning that has a way to put it right (variant B of the 2026-11-09
/// design): the message on top, and under it a darker strip with the fixing
/// button in red, what it will do, and a quieter second choice.
pub(super) struct Fix<'a> {
    pub message: &'a str,
    pub primary: &'a str,
    pub secondary: Option<&'a str>,
    /// "Keep as is": the owner accepts the situation on purpose.
    pub keep: Option<&'a str>,
    /// A label and chips telling what the fix gives or replaces.
    pub result: Option<(&'a str, Vec<String>)>,
    pub note: Option<&'a str>,
    pub enabled: bool,
    pub hover: Option<&'a str>,
}

impl<'a> Fix<'a> {
    pub(super) fn new(message: &'a str, primary: &'a str) -> Self {
        Self {
            message,
            primary,
            secondary: None,
            keep: None,
            result: None,
            note: None,
            enabled: true,
            hover: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    None,
    Primary,
    Secondary,
    Keep,
}

pub(super) const FIX_RED: Color32 = Color32::from_rgb(214, 69, 69);
const FIX_RED_LINE: Color32 = Color32::from_rgb(255, 107, 96);

/// The red button of a fixing action.
pub(super) fn fix_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(
            RichText::new(text)
                .size(15.0_f32)
                .strong()
                .color(Color32::WHITE),
        )
        .fill(FIX_RED)
        .stroke(Stroke::new(1.0_f32, FIX_RED_LINE))
        .corner_radius(8.0_f32)
        .min_size(vec2(0.0, 34.0)),
    )
}

/// A quieter button beside the fixing one.
pub(super) fn quiet_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(
            RichText::new(text)
                .size(14.0_f32)
                .color(Color32::from_rgb(227, 234, 240)),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(159, 176, 191)))
        .corner_radius(8.0_f32)
        .min_size(vec2(0.0, 32.0)),
    )
}

/// A small rounded label of the result of a fix.
pub(super) fn chip(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(Color32::from_rgb(16, 48, 74))
        .corner_radius(12.0_f32)
        .inner_margin(egui::Margin::symmetric(10, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(13.0_f32)
                    .color(Color32::from_rgb(207, 232, 244)),
            );
        });
}

pub(super) fn fix_card(ui: &mut egui::Ui, fix: &Fix<'_>) -> Choice {
    let mut choice = Choice::None;
    let severity = Severity::Warning;
    let accent = severity.accent();
    egui::Frame::new()
        .fill(severity.fill())
        .stroke(Stroke::new(1.5_f32, accent))
        .corner_radius(8.0_f32)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        icon(ui, severity, 22.0, None);
                        ui.add(
                            egui::Label::new(
                                RichText::new(fix.message)
                                    .size(14.0_f32)
                                    .color(severity.text()),
                            )
                            .wrap(),
                        );
                    });
                });
            let strip = egui::Frame::new()
                .fill(Color32::from_rgb(42, 20, 23))
                .corner_radius(egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 7,
                    se: 7,
                })
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let mut primary = fix_button(ui, fix.primary, fix.enabled);
                        if let Some(hover) = fix.hover {
                            primary = primary.on_hover_text(hover).on_disabled_hover_text(hover);
                        }
                        if primary.clicked() {
                            choice = Choice::Primary;
                        }
                        if let Some(text) = fix.secondary {
                            if quiet_button(ui, text, fix.enabled).clicked() {
                                choice = Choice::Secondary;
                            }
                        }
                        if let Some(text) = fix.keep {
                            if quiet_button(ui, text, fix.enabled)
                                .on_hover_text(
                                    "Accept it as it is. The warning stays hidden until the situation changes.",
                                )
                                .clicked()
                            {
                                choice = Choice::Keep;
                            }
                        }
                        if let Some((label, chips)) = &fix.result {
                            ui.label(RichText::new(*label).size(13.0_f32).color(severity.text()));
                            for text in chips {
                                chip(ui, text);
                            }
                        }
                        if let Some(note) = fix.note {
                            ui.label(
                                RichText::new(note)
                                    .size(12.0_f32)
                                    .color(Color32::from_rgb(159, 176, 191)),
                            );
                        }
                    });
                });
            let rect = strip.response.rect;
            ui.painter()
                .hline(rect.x_range(), rect.top(), Stroke::new(1.5_f32, accent));
        });
    ui.add_space(4.0);
    choice
}

/// What stays of a warning the owner accepted: one quiet line and Undo. True
/// when Undo was pressed.
pub(super) fn accepted_line(ui: &mut egui::Ui, text: &str) -> bool {
    let mut undo = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("✔ {text}"))
                .size(13.0_f32)
                .color(Color32::from_rgb(159, 176, 191)),
        );
        undo = ui.small_button("Undo").clicked();
    });
    undo
}

/// How a status message reads: an error (red), a warning (yellow) or plain
/// information (None, white). Failures are recognised by their prefixes, the
/// rest by words that say something needs the owner's attention.
pub(super) fn message_level(message: &str) -> Option<Severity> {
    if message_severity(message) == Some(Severity::Error) {
        return Some(Severity::Error);
    }
    let text = message.to_lowercase();
    const WARNING_WORDS: [&str; 17] = [
        "⚠",
        "warning",
        "choose ",
        "wait until",
        "no fits",
        "no frames",
        "inputs changed",
        "differ",
        "confirm",
        "cancel",
        "enter ",
        "unreadable",
        "could not",
        "cannot",
        "unable",
        "not found",
        "rejected",
    ];
    WARNING_WORDS
        .iter()
        .any(|word| text.contains(word))
        .then_some(Severity::Warning)
}

/// The colour of a status message in the status bar.
pub(super) fn status_color(level: Option<Severity>) -> Color32 {
    match level {
        None => Color32::from_rgb(227, 234, 240),
        Some(Severity::Warning) => Color32::from_rgb(242, 201, 76),
        Some(Severity::Error) => Color32::from_rgb(255, 138, 128),
        Some(Severity::Note) => Color32::from_rgb(124, 196, 228),
    }
}
