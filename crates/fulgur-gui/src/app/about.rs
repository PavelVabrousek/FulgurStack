//! About step: what FulgurStack is, and what changed since the last package.
use super::compact::card_pair;
use eframe::egui::{self, RichText};

const MOTIVATION: &str = "FulgurStack was started to stack very large sets of short exposures, \
such as thousands of Seestar frames, quickly on an ordinary computer, without giving up \
scientific accuracy. It turns raw frames into a linear master ready for further work in \
PixInsight, and it never needs disk space for intermediate images.";

const FEATURES: &[&str] = &[
    "Reads FITS and XISF frames and never changes them.",
    "Writes a linear 32-bit FITS master, with coverage and high-rejection maps on request.",
    "Writes no intermediate images. Frames are processed in tiles, so memory use stays bounded.",
    "Calibrates with bias, darks and flats.",
    "Measures frame quality and shows it in a table and a plot.",
    "Registers frames with validated geometric correction.",
    "Rejects outliers with ESD, winsorized clipping, linear fit or median, chosen automatically by default.",
    "Makes one master, or one master per filter or light set, on a shared pixel grid.",
    "Combines several cameras on an external reference.",
    "Runs on the CPU, with optional GPU pixel rejection.",
];

const AUTHOR: &str = "Pavel Vabroušek Pavel.Vabrousek@gmail.com & AI";

const NOTES_SCOPE: &str = "Changes since the alpha package 0.1.0 of 31 October 2026.";

const NOTES: &[(&str, &[&str])] = &[
    (
        "Speed",
        &[
            "Large stacks take a third to a half less time, with identical masters. 941 of 1140 Seestar frames: 242 s before, 115–122 s now.",
            "ESD and median rejection, frame measurement and reconstruction are all faster.",
        ],
    ),
    (
        "Registration",
        &[
            "Dense star fields keep far more frames: 160 of 166 on a tester's ASI6200 set, where the alpha kept 64 to 128.",
            "Geometric correction is on by default in the application.",
            "Auto reference warns about frames it cannot map and continues.",
            "Experimental: partial frames join the tiles where their stars stay accurate. Off by default, in Settings.",
        ],
    ),
    (
        "Stacking",
        &[
            "One run can make one master per filter or per light set, all on one pixel grid.",
            "One stack can combine several cameras on an external reference.",
            "Sky offset is now set per colour channel, which evens out rejection between channels.",
        ],
    ),
    (
        "Input",
        &[
            "XISF files work as lights, calibration frames, masters and external references.",
            "FITS frames with bottom-up row order, as Siril writes them, are read correctly. Earlier versions misread them silently.",
            "Sensors below 16 bits are recognized, so star measurement and calibration use their true saturation level.",
        ],
    ),
    (
        "Application",
        &[
            "The version number is in the window title.",
            "This About page is new.",
        ],
    ),
];

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong().size(15.));
    ui.add_space(6.);
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(10.);
    ui.label(RichText::new(text).strong());
    ui.add_space(2.);
}

fn bullets(ui: &mut egui::Ui, items: &[&str]) {
    for item in items {
        ui.label(format!("•  {item}"));
    }
}

fn about_text(ui: &mut egui::Ui) {
    heading(ui, &format!("FulgurStack {}", crate::VERSION));
    ui.label(MOTIVATION);
    section(ui, "What it does");
    bullets(ui, FEATURES);
    section(ui, "Author");
    ui.label(AUTHOR);
}

fn release_notes(ui: &mut egui::Ui) {
    heading(ui, "Release notes");
    ui.label(RichText::new(NOTES_SCOPE).weak());
    for (title, items) in NOTES {
        section(ui, title);
        bullets(ui, items);
    }
}

fn panel(ui: &mut egui::Ui, id: &str, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .max_height(height)
        .show(ui, add);
}

pub(super) fn view(ui: &mut egui::Ui) {
    let height = (ui.available_height() - 30.).max(240.);
    egui::ScrollArea::vertical()
        .id_salt("about_page")
        .show(ui, |ui| {
            card_pair(
                ui,
                760.,
                |ui| panel(ui, "about_text", height, about_text),
                |ui| panel(ui, "release_notes", height, release_notes),
            );
        });
}
