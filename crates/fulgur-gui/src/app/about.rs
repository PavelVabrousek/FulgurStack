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

const NOTES_SCOPE: &str = "Changes since the alpha package 0.14 of 9 November 2026.";

const NOTES: &[(&str, &[&str])] = &[
    (
        "Stacking and output",
        &[
            "The coverage and high-rejection maps carry the same plate solution (WCS) as the master, in FITS and XISF, so they line up with the master in PixInsight.",
            "The master and both maps carry the median capture time of the stacked frames as DATE-OBS and DATE-AVG, instead of the start time of the reference frame.",
            "Stars clipped by a sensor below 16 bits (12-bit Dwarf3, 14-bit DSLRs) no longer enter the brightness scale; masters from 16-bit cameras are unchanged.",
            "The application and the command line make masters through the same code after registration.",
        ],
    ),
    (
        "Results",
        &[
            "Zooming into an XISF master, coverage map or high-rejection map shows sharp native pixels instead of the enlarged overview.",
            "When the full-resolution detail cannot be read, a red banner says so.",
        ],
    ),
    (
        "Application",
        &[
            "A light set set to None calibration stays uncalibrated after rescans, new calibration sets and restarts.",
            "Opening files with the program no longer replaces the saved light sets.",
            "Closing the application warns about an index build or master in progress.",
            "Background tasks (Gaia check, target look-up, index test and build, Auto-solve) no longer stay running after an internal error.",
            "A file that cannot be opened is named in the error.",
        ],
    ),
    (
        "Command line",
        &[
            "Blind plate solving moved into the main program: fulgurstack solve-index build|info and fulgurstack solve; the separate fulgur-solve program is gone.",
            "With --input-sets, --exposure takes one value per light set.",
        ],
    ),
    (
        "Fixes",
        &[
            "A damaged blind-solving index or XISF file is refused instead of ending the program.",
            "Calibration masters from many frames stay within about 2 GB of memory.",
            "A cancel during geometric correction stops sooner.",
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
