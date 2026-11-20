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

const NOTES_SCOPE: &str = "Changes since the alpha package 0.15 of 17 November 2026.";

const NOTES: &[(&str, &[&str])] = &[
    (
        "Drizzle",
        &[
            "Drizzle 2× in Settings › Stacking: the frames are integrated onto a grid with half-size pixels, each input pixel shrunk to a square drop (0.86 px by default) and weighted by its overlap.",
            "Colour cameras use Bayer drizzle: the raw sensor samples drop into their own colour without demosaicing. On M 45 (Seestar S30 Pro, 617 frames) the master matched PixInsight's DrizzleIntegration in noise, with slightly smaller stars, in 3 min 46 s instead of 28 min 43 s.",
            "A drizzle master and its maps are written tile by tile as they are made, so the sensor size is not limited by memory (a 104 Mpx drizzle master from an ASI2600MC needed 3.4 GB).",
            "Drizzle helps most when stars are under about 2 pixels across (2 × HFR); it runs on the CPU.",
        ],
    ),
    (
        "Stacking",
        &[
            "Trail mask for generalized ESD (Settings › Stacking, off by default): around a pair of rejected samples, the faint edges of asteroid and satellite trails in the same frame are rejected too. Grow removes the traces; Hysteresis is gentler.",
        ],
    ),
    (
        "Command line",
        &[
            "--drizzle, --drizzle-pixfrac and --drizzle-debayer; --trail-mask off|hysteresis|grow with --trail-radius and --trail-sigma.",
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
