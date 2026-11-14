//! `solve` and `solve-index`: blind plate solving of frames with no position
//! given (ADR-168), part of `fulgurstack` since ADR-181.
//!
//! * `solve-index build --gaia <folder> --output <file>` reads the PixInsight
//!   Gaia DR3 files and writes the quad index (about 1.4 GB for stars to
//!   G 13), once.
//! * `solve-index info <file>` describes an index.
//! * `solve --index <file> <frames or folders>` places each frame on the sky;
//!   with `--refine --gaia <folder>` FulgurStack's own solver then fits the
//!   full plate solution around the answer.
//!
//! Input frames are only read. No observing site coordinates are read,
//! printed or written.
use crate::CANCEL;
use crate::files::{save_report, validate_report};
use fulgur_catalog::xpsd::XpsdCatalogue;
use fulgur_core::{Error, Result};
use fulgur_pipeline::sky::SkyFrame;
use fulgur_pipeline::sky::catalogue::{GAIA_EPOCH, decimal_year, position_text};
use fulgur_pipeline::sky::detect::bright_stars;
use fulgur_pipeline::sky::solve::{BlindCache, Solution, solve as refine_solve};
use fulgur_solve::{Band, Hypothesis, Index, IndexStar, SolveOptions};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(clap::Args)]
pub(crate) struct SolveArgs {
    /// The index written by `solve-index build`.
    #[arg(long)]
    index: PathBuf,
    /// Frames (FITS or XISF) or folders of them (the files directly in the folder).
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// Fit the full plate solution around each answer with FulgurStack's
    /// own solver (needs --gaia).
    #[arg(long, requires = "gaia")]
    refine: bool,
    /// Folder of the PixInsight Gaia DR3 files, for --refine.
    #[arg(long)]
    gaia: Option<PathBuf>,
    /// How many of the brightest stars make quads.
    #[arg(long, default_value_t = 30)]
    top: usize,
    /// A new JSON report of every frame.
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(clap::Args)]
pub(crate) struct IndexArgs {
    #[command(subcommand)]
    action: IndexAction,
}

#[derive(clap::Subcommand)]
enum IndexAction {
    /// Read the Gaia DR3 files and write the index.
    Build {
        /// Folder of the PixInsight Gaia DR3 files (gdr3-...xpsd).
        #[arg(long)]
        gaia: PathBuf,
        /// Index every star brighter than this Gaia G magnitude.
        #[arg(long, default_value_t = 13.0)]
        gmax: f32,
        /// The index file to write.
        #[arg(long)]
        output: PathBuf,
        /// Replace an index that exists.
        #[arg(long)]
        force: bool,
    },
    /// Describe an index file.
    Info { file: PathBuf },
}

pub(crate) fn run_index(args: IndexArgs) -> Result<()> {
    match args.action {
        IndexAction::Build {
            gaia,
            gmax,
            output,
            force,
        } => build(&gaia, gmax, &output, force),
        IndexAction::Info { file } => info(&file),
    }
}

/// The catalogue's text failures as errors.
fn message(text: String) -> Error {
    Error::Message(text)
}

fn load_index(file: &Path) -> Result<Index> {
    Index::load(file).map_err(Error::file(file))
}

fn build(gaia: &Path, gmax: f32, output: &Path, force: bool) -> Result<()> {
    if output.exists() && !force {
        return Err(Error::Message(format!(
            "{} exists; add --force to replace it.",
            output.display()
        )));
    }
    let started = Instant::now();
    println!("Reading the Gaia stars brighter than G {gmax} ...");
    let catalogue = XpsdCatalogue::open(gaia).map_err(message)?;
    let stars: Vec<IndexStar> = catalogue
        .all_sky(f64::from(gmax), &CANCEL)
        .map_err(message)?
        .into_iter()
        .map(|(ra, dec, g)| IndexStar::new(ra, dec, g))
        .collect();
    println!(
        "  {} stars in {:.1} s",
        stars.len(),
        started.elapsed().as_secs_f64()
    );
    let built = Instant::now();
    let index = Index::build(
        stars,
        gmax,
        &Band::standard(),
        &|line| println!("  {line}"),
        &CANCEL,
    )
    .ok_or(Error::Cancelled)?;
    println!(
        "Index: {} quads, {:.0} MB in memory, built in {:.1} s",
        index.quad_count(),
        index.memory_bytes() as f64 / 1e6,
        built.elapsed().as_secs_f64()
    );
    index.save(output).map_err(Error::file(output))?;
    let bytes = std::fs::metadata(output).map_or(0, |m| m.len());
    println!(
        "PASS: wrote {} ({:.0} MB) in {:.1} s overall.",
        output.display(),
        bytes as f64 / 1e6,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn info(file: &Path) -> Result<()> {
    let started = Instant::now();
    let index = load_index(file)?;
    println!(
        "{}: stars to G {}, {} stars, {} quads, {:.0} MB in memory, read in {:.1} s",
        file.display(),
        index.gmax(),
        index.star_count(),
        index.quad_count(),
        index.memory_bytes() as f64 / 1e6,
        started.elapsed().as_secs_f64()
    );
    for b in index.bands() {
        println!(
            "  quads of {} to {} degrees: {} stars, {} quads",
            b.dmin, b.dmax, b.stars, b.quads
        );
    }
    Ok(())
}

#[derive(Serialize)]
struct Refined {
    centre_ra_deg: f64,
    centre_dec_deg: f64,
    arcsec_per_px: f64,
    held_out_rms_px: f64,
    polynomial_degree: usize,
    mirrored: bool,
}

#[derive(Serialize)]
struct FrameReport {
    file: String,
    width: usize,
    height: usize,
    detections: usize,
    solved: bool,
    ra_deg: Option<f64>,
    dec_deg: Option<f64>,
    arcsec_per_px: Option<f64>,
    field_deg: Option<f64>,
    mirrored: Option<bool>,
    matched: Option<usize>,
    log10_chance: Option<f64>,
    seconds: f64,
    refined: Option<Refined>,
    note: Option<String>,
}

impl FrameReport {
    fn new(file: String) -> Self {
        Self {
            file,
            width: 0,
            height: 0,
            detections: 0,
            solved: false,
            ra_deg: None,
            dec_deg: None,
            arcsec_per_px: None,
            field_deg: None,
            mirrored: None,
            matched: None,
            log10_chance: None,
            seconds: 0.0,
            refined: None,
            note: None,
        }
    }
}

/// FulgurStack's solver around a hypothesis: the refinement, the plate
/// solution and its held-out check.
fn refine(
    catalogue: &XpsdCatalogue,
    name: &str,
    size: (usize, usize),
    epoch: f64,
    stars: &[[f64; 2]],
    hypothesis: &Hypothesis,
) -> std::result::Result<Solution, String> {
    let hint = (hypothesis.ra, hypothesis.dec);
    let wide = (hypothesis.field_deg * 0.75).clamp(6.5, 15.0);
    let mut catalogue_stars = catalogue.cone(hint.0, hint.1, wide, 13.5)?;
    catalogue_stars.extend(catalogue.cone(hint.0, hint.1, 1.7, 17.5)?);
    let mut seen = HashSet::new();
    catalogue_stars.retain(|s| seen.insert((s.ra.to_bits(), s.dec.to_bits(), s.g.to_bits())));
    catalogue_stars.sort_by(|a, b| a.g.total_cmp(&b.g));
    let cache = BlindCache::new(Arc::new(catalogue_stars));
    let frame = SkyFrame {
        name,
        width: size.0,
        height: size.1,
        epoch,
        stars,
    };
    refine_solve(&frame, &[hint], &cache, 1.0, None)
}

pub(crate) fn run(args: SolveArgs) -> Result<()> {
    let SolveArgs {
        index,
        inputs,
        refine: refine_requested,
        gaia,
        top,
        report,
    } = args;
    validate_report(report.as_deref())?;
    let started = Instant::now();
    let index = load_index(&index)?;
    println!(
        "Index read in {:.1} s ({} quads).",
        started.elapsed().as_secs_f64(),
        index.quad_count()
    );
    let catalogue = refine_requested
        .then_some(gaia)
        .flatten()
        .as_deref()
        .map(XpsdCatalogue::open)
        .transpose()
        .map_err(message)?;
    let mut files: Vec<PathBuf> = vec![];
    for input in &inputs {
        fulgur_pipeline::survey::collect_folder_inputs(input, &mut files)?;
    }
    files.sort();
    if files.is_empty() {
        return Err(Error::Invalid("No FITS or XISF frames in the inputs."));
    }
    let options = SolveOptions {
        top,
        ..SolveOptions::default()
    };
    let mut reports = vec![];
    let (mut solved, mut refined_count) = (0usize, 0usize);
    for path in &files {
        fulgur_core::check_cancelled(&CANCEL)?;
        let name = crate::files::filename(path);
        let begun = Instant::now();
        let mut line = FrameReport::new(name.clone());
        match fulgur_core::fits::read_raw(path) {
            Err(e) => line.note = Some(format!("unreadable: {e}")),
            Ok(raw) => {
                let info = &raw.header.info;
                let mosaic = info.mosaic == fulgur_core::Mosaic::Bayer;
                let (mut found, _) = bright_stars(&raw.pixels, info.width, info.height, mosaic);
                found.sort_by(|a, b| b[4].total_cmp(&a[4]));
                let stars: Vec<[f64; 2]> = found.iter().map(|s| [s[0], s[1]]).collect();
                line.width = info.width;
                line.height = info.height;
                line.detections = stars.len();
                let hypothesis =
                    fulgur_solve::solve(&index, info.width, info.height, &stars, &options, &CANCEL);
                match hypothesis {
                    None => line.note = Some("no convincing hypothesis".into()),
                    Some(h) => {
                        solved += 1;
                        line.solved = true;
                        line.ra_deg = Some(h.ra);
                        line.dec_deg = Some(h.dec);
                        line.arcsec_per_px = Some(h.arcsec_per_px);
                        line.field_deg = Some(h.field_deg);
                        line.mirrored = Some(h.mirrored);
                        line.matched = Some(h.matched);
                        line.log10_chance = Some(h.log10_p);
                        if let Some(catalogue) = &catalogue {
                            // Without a readable capture date no proper motion is applied (B-20).
                            let epoch = info
                                .date_obs
                                .as_deref()
                                .and_then(decimal_year)
                                .unwrap_or(GAIA_EPOCH);
                            match refine(
                                catalogue,
                                &name,
                                (info.width, info.height),
                                epoch,
                                &stars,
                                &h,
                            ) {
                                Ok(s) => {
                                    refined_count += 1;
                                    line.refined = Some(Refined {
                                        centre_ra_deg: s.centre_ra,
                                        centre_dec_deg: s.centre_dec,
                                        arcsec_per_px: s.arcsec_per_px,
                                        held_out_rms_px: s.held_out_rms_px,
                                        polynomial_degree: s.degree,
                                        mirrored: s.mirrored,
                                    });
                                }
                                Err(e) => line.note = Some(format!("refinement failed: {e}")),
                            }
                        }
                    }
                }
            }
        }
        line.seconds = begun.elapsed().as_secs_f64();
        println!("{}", describe(&line));
        reports.push(line);
    }
    save_report(report.as_deref(), &reports)?;
    println!(
        "PASS: {} of {} frames placed{}; {:.1} s in all.",
        solved,
        files.len(),
        if catalogue.is_some() {
            format!(", {refined_count} refined")
        } else {
            String::new()
        },
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn describe(r: &FrameReport) -> String {
    let place = match (r.ra_deg, r.dec_deg, r.arcsec_per_px, r.field_deg) {
        (Some(ra), Some(dec), Some(scale), Some(field)) => format!(
            "{}, {scale:.2} arcsec/px, field {field:.2} deg, {} stars matched",
            position_text(ra, dec),
            r.matched.unwrap_or(0)
        ),
        _ => r.note.clone().unwrap_or_else(|| "not placed".into()),
    };
    let refined = r.refined.as_ref().map_or(String::new(), |s| {
        format!(
            "; refined {}, held-out {:.2} px",
            position_text(s.centre_ra_deg, s.centre_dec_deg),
            s.held_out_rms_px
        )
    });
    let note = match (&r.note, r.solved) {
        (Some(n), true) => format!(" ({n})"),
        _ => String::new(),
    };
    format!("{}: {place}{refined}{note} [{:.2} s]", r.file, r.seconds)
}
