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
    "Registers frames of any camera, optics or orientation through Gaia plate solving, chosen automatically for mixed cameras.",
    "Runs on the CPU, with optional GPU pixel rejection.",
];

const AUTHOR: &str = "Pavel Vabroušek Pavel.Vabrousek@gmail.com & AI";

const CREDITS: &[&str] = &[
    "This work has made use of data from the European Space Agency (ESA) mission Gaia (https://www.cosmos.esa.int/gaia), processed by the Gaia Data Processing and Analysis Consortium (DPAC). Sky registration reads Gaia DR3 from the PixInsight catalogue files or the ESA and Heidelberg (GAVO) archives.",
    "Deep-sky names and positions: OpenNGC by Mattia Verga and contributors (CC BY-SA 4.0); the built-in table derived from it is shared under the same licence.",
    "This research has made use of the SIMBAD database, operated at CDS, Strasbourg, France (LBN, Sharpless and Barnard positions in the built-in table, and online look-up), and of the NASA/IPAC Extragalactic Database (NED).",
];

const NOTES_SCOPE: &str = "Changes since the alpha package 0.12 of 1 November 2026.";

const NOTES: &[(&str, &[&str])] = &[
    (
        "Registration",
        &[
            "Plate solving through Gaia DR3 registers frames of any camera, optics or orientation. It is chosen automatically when the light sets differ in camera, colour layout or sensor size, and the field centre is taken from the reference frame's header. On a five-camera M33 set it stacks 330 frames instead of 213.",
            "Registration accuracy is adjustable: Maximum accuracy (default), Compromise and Maximum coverage.",
            "Several cameras stack together without an external reference, mirrored cameras included, and a wider camera joins a narrower grid on the shared field.",
            "A light set that shares no stars with the reference keeps or links its own brightness instead of failing.",
            "Auto reference weighs plate scale and quality across several nights.",
        ],
    ),
    (
        "Stacking and output",
        &[
            "The master can be written as XISF, with its coverage and high-rejection maps.",
            "The master carries its plate solution as FITS WCS, so PixInsight can annotate it.",
            "The FITS header names the rejection actually applied and the application version.",
            "New calibration type, Hot pixels: a map of the brightest share of pixels (typically 0.01 %) taken from any dark or bias. Those pixels are replaced in every light by their same-colour neighbours, respecting the Bayer mask. Saved maps can be reused with the CLI.",
        ],
    ),
    (
        "Speed",
        &[
            "Spans for distorted frames, a parallel triangle fallback, a parallel blind plate solve and four-column ESD make DS-A stacking faster, with identical masters.",
            "Tile shapes are chosen automatically; 1024 x 1024 tiles are gone.",
        ],
    ),
    (
        "Application",
        &[
            "Settings are in one column with explanations behind an info marker.",
            "A mono and a colour camera never share one light set, and the colour layout is read from the FITS header.",
            "A folder or files of another camera dropped on a light set become a light set of their own.",
            "A dropped folder without frames of its own gives one light set per sub-folder.",
            "A calibration set filled kind by kind is offered, and a raw stack is flagged.",
            "A mixed light set can be split in one click.",
        ],
    ),
    (
        "Fixes",
        &[
            "A light folder gives only the files directly in it.",
            "The master's plate solve falls back to the frames' own field, and a failed solve is recorded.",
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
    section(ui, "Data and credits");
    bullets(ui, CREDITS);
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
