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

const NOTES_SCOPE: &str = "Changes since the alpha package 0.13 of 8 November 2026.";

const NOTES: &[(&str, &[&str])] = &[
    (
        "Registration",
        &[
            "Blind plate solving: a new module places a frame on the sky with no coordinates and no target name, for fields of 0.5 to 12 degrees, from an index built once from the Gaia DR3 files. Auto-solve in the Field centre row finds the field of the first frames by itself.",
            "A second camera that shares only saturated stars with the reference now registers through their positions (Phi Tauri: 41 of 46 frames instead of 22).",
            "When the headers name no target, one is suggested from the object name, the file names and the folder names; you confirm it or enter your own.",
            "A plate solve that finds nothing says where it searched and why, warns about a field centre saved from an earlier session, and offers the star registration.",
        ],
    ),
    (
        "Stacking and output",
        &[
            "Exposure written in milliseconds (the Vaonis Stellina) is read correctly: the unit comes from the card's comment or the camera catalogue, and a doubtful value asks you.",
            "The Vaonis Stellina is in the camera catalogue.",
            "Positions are shown as hours, minutes and seconds of right ascension and degrees, minutes and seconds of declination, with the degrees in brackets.",
        ],
    ),
    (
        "Speed",
        &[
            "Calibration masters stay in memory between measuring quality and stacking, so they are built once and the measurement is reused.",
        ],
    ),
    (
        "Application",
        &[
            "A warning that can be put right has a red fix button that says what it does: split a mixed set, one master per filter, use a fitting calibration set, turn off a bias or dark for Seestar lights, clear a saved centre, use the header position, test the catalogue.",
            "Where leaving things as they are is a fair choice, Keep as is accepts the warning on purpose; it stays hidden until the situation changes, and Undo brings it back.",
            "A stopped run shows a red banner with the cause and a way back to the star registration; the status message sits at the right of the status bar in 12 points, white, yellow or red.",
            "Settings: a Blind-solving index under the Gaia DR3 folder (folder, test, build with a chosen range of fields and a size estimate, delete); method, engine and accuracy are drop-down lists.",
            "The smallest text is one unit larger.",
        ],
    ),
    (
        "Command line",
        &[
            "New program fulgur-solve: build the index from the Gaia files, inspect it, and place frames on the sky.",
        ],
    ),
    (
        "Fixes",
        &[
            "Calibration masters were built again in every phase when a run needed more than four of them.",
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
