//! skyreg: universal sky-based registration (standalone experiment beside
//! FulgurStack; FulgurStack itself is unchanged and only its file readers
//! are used).
//!
//! Every frame, whatever camera, optics, filter or colour mosaic, is plate
//! solved on its own against Gaia DR3. A registration source -> target is
//! the composition source pixel -> sky -> target pixel, so any frame of the
//! set or any external image (real or synthetic star field) can be the
//! target. Each registration is verified end to end with the stars both
//! frames detected themselves. Everything happens in memory; only the log
//! and a JSON report are written.
mod catalog;
mod frames;

use fulgur_pipeline::sky::{self, batch, check as verify, solve};
use rayon::prelude::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const USAGE: &str =
    "usage: skyreg --input DIR [--input DIR ...] --out DIR [--object NAME | --ra DEG --dec DEG]
              [--target FILE ...] [--targets-dir DIR ...] [--all-targets] [--negative-control] [--threads N] [--max-solve-rms PX]
  --target   a frame of the inputs or an external FITS/XISF (real or synthetic)
  --targets-dir  every FITS/XISF of a folder as a target
  --all-targets  also verify every solved input frame as a target of all others
  --negative-control  displace every target on purpose; the check must fail";

struct Log {
    file: Mutex<std::io::BufWriter<std::fs::File>>,
}

impl Log {
    fn line(&self, text: impl AsRef<str>) {
        let mut f = self.file.lock().unwrap();
        let _ = writeln!(f, "{}", text.as_ref());
    }
    fn both(&self, text: impl AsRef<str>) {
        eprintln!("{}", text.as_ref());
        self.line(text);
    }
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn opts<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == name)
        .filter_map(|(i, _)| args.get(i + 1).map(String::as_str))
        .collect()
}

fn group(f: &frames::Frame) -> String {
    format!(
        "{} {}x{} {}",
        f.instrument,
        f.width,
        f.height,
        if f.mosaic { "colour" } else { "mono" }
    )
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map_or("-".into(), |v| format!("{v:.2}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("skyreg: {e}");
        std::process::exit(1);
    }
}

struct Entry {
    frame: frames::Frame,
    /// Solver input: detected stars, brightest first.
    stars: Vec<[f64; 2]>,
    /// Point sources for the end-to-end check.
    points: Vec<[f64; 2]>,
    solution: Result<solve::Solution, String>,
    pass: u8,
    solve_s: f64,
}

fn run(args: &[String]) -> Result<(), String> {
    let start = Instant::now();
    let inputs = opts(args, "--input");
    let out = PathBuf::from(opt(args, "--out").ok_or(USAGE)?);
    if inputs.is_empty() {
        return Err(USAGE.into());
    }
    let threads: usize = opt(args, "--threads")
        .map(|t| t.parse().map_err(|e| format!("--threads: {e}")))
        .transpose()?
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(8)
        });
    let max_rms: f64 = opt(args, "--max-solve-rms")
        .map(|t| t.parse().map_err(|e| format!("--max-solve-rms: {e}")))
        .transpose()?
        .unwrap_or(1.0);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let cache = out.join("catalogue_cache");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let log = Log {
        file: Mutex::new(std::io::BufWriter::new(
            std::fs::File::create(out.join("registration.log")).map_err(|e| e.to_string())?,
        )),
    };
    log.both("skyreg 0.1: universal sky-based registration (in memory; nothing but this log and report.json is written)");
    log.line(format!("command: skyreg {}", args.join(" ")));
    log.line(format!(
        "threads: {threads}, solve limit: held-out RMS <= {max_rms} px"
    ));

    // ------------------------------------------------------------ inputs
    let mut paths = vec![];
    for dir in &inputs {
        let dir = Path::new(dir);
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                log.line(format!("input: skipped sub-folder {}", p.display()));
            } else if frames::is_image(&p)
                && opt(args, "--only").is_none_or(|s| p.to_string_lossy().contains(s))
            {
                paths.push(p);
            } else {
                log.line(format!("input: skipped non-image {}", p.display()));
            }
        }
    }
    log.both(format!("input: {} image files", paths.len()));

    // ------------------------------------------------------------ catalogue
    let hint = match (opt(args, "--ra"), opt(args, "--dec")) {
        (Some(r), Some(d)) => (
            r.parse::<f64>().map_err(|e| e.to_string())?,
            d.parse::<f64>().map_err(|e| e.to_string())?,
        ),
        _ => catalog::resolve(opt(args, "--object").ok_or("give --object or --ra/--dec")?)?,
    };
    log.both(format!("hint: RA {:.4} Dec {:+.4}", hint.0, hint.1));
    let say = |s: String| log.both(s);
    let t = Instant::now();
    // A deep cone around the target and a mosaic of bright cones out to 4.8
    // degrees for wide-field lenses; each query stays small enough for a
    // synchronous TAP answer.
    let deep = catalog::cone(hint.0, hint.1, 1.7, 17.5, &cache, &say)?;
    let mut wide = vec![];
    for i in -2i32..=2 {
        for j in -2i32..=2 {
            let (dx, dy) = (i as f64 * 2.4, j as f64 * 2.4);
            if dx.hypot(dy) > 4.8 {
                continue;
            }
            let (ra, dec) = catalog::deproject([dx * 3600., dy * 3600.], hint.0, hint.1);
            wide.extend(catalog::cone(ra, dec, 1.75, 13.5, &cache, &say)?);
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut cat: Vec<catalog::CatStar> = wide
        .into_iter()
        .chain(deep)
        .filter(|s| seen.insert(s.source_id))
        .collect();
    cat.sort_by(|a, b| a.g.total_cmp(&b.g));
    let cat: Vec<sky::catalogue::GaiaStar> = cat
        .into_iter()
        .map(|c| sky::catalogue::GaiaStar {
            source_id: c.source_id,
            ra: c.ra,
            dec: c.dec,
            pmra: c.pmra,
            pmdec: c.pmdec,
            g: c.g,
        })
        .collect();
    log.both(format!(
        "catalogue: {} Gaia DR3 stars (G {:.1}..{:.1}), {:.1} s",
        cat.len(),
        cat.first().map_or(0., |s| s.g),
        cat.last().map_or(0., |s| s.g),
        t.elapsed().as_secs_f64()
    ));
    let blind = solve::BlindCache::new(Arc::new(cat));

    // ------------------------------------------------------------ frames
    let t = Instant::now();
    let done = AtomicUsize::new(0);
    let total = paths.len();
    let loaded: Vec<Result<frames::Frame, String>> = paths
        .par_iter()
        .map(|p| {
            let r = frames::load(p);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 25 == 0 || n == total {
                eprintln!("  read and detected {n}/{total}");
            }
            r
        })
        .collect();
    let mut entries = vec![];
    log.line("");
    log.line("== FRAMES: read, colour-mosaic decision and star detection ==");
    for (p, r) in paths.iter().zip(loaded) {
        match r {
            Ok(frame) => {
                log.line(format!(
                    "frame {:<70} {} ({}) | {} | epoch {:.3} ({}) | mosaic: {} ({}) | {} stars, FWHM {:.2} px | read {:.2} s, detect {:.2} s",
                    frame.name,
                    frame.format,
                    frame.reader,
                    group(&frame),
                    frame.epoch,
                    frame.epoch_source,
                    frame.mosaic,
                    frame.mosaic_source,
                    frame.detected,
                    frame.fwhm_px,
                    frame.read_s,
                    frame.detect_s
                ));
                let (stars, points) = star_lists(&frame);
                entries.push(Entry {
                    frame,
                    stars,
                    points,
                    solution: Err("not solved".into()),
                    pass: 0,
                    solve_s: 0.,
                });
            }
            Err(e) => log.both(format!("frame {} UNREADABLE: {e}", p.display())),
        }
    }
    log.both(format!(
        "frames: {} read in {:.1} s",
        entries.len(),
        t.elapsed().as_secs_f64()
    ));

    // ------------------------------------------------------------ spike S2
    // FulgurStack's own inspection star lists (250 per frame) instead of
    // skyreg's detection: what the integrated engine would receive.
    // --fs-sky-stars: the bright-star list inspection keeps for sky
    // registration (plan phase 3), the list the engine will receive.
    let fs_sky = args.iter().any(|a| a == "--fs-sky-stars");
    if fs_sky || args.iter().any(|a| a == "--fs-stars") {
        use fulgur_pipeline::inspect::{InputSet, InspectConfig};
        let t = Instant::now();
        let sets: Vec<InputSet> = inputs
            .iter()
            .map(|d| InputSet {
                label: d.to_string(),
                inputs: vec![PathBuf::from(d)],
                layout: fulgur_core::LayoutChoice::Auto,
                // Star lists do not depend on it; it only keeps frames
                // without an exposure card from being refused.
                exposure: Some(300.),
                accept_derived: true,
                calibration: None,
            })
            .collect();
        let (report, _) = fulgur_pipeline::inspect::inspect_sets(
            &sets,
            InspectConfig {
                sky_stars: fs_sky,
                ..InspectConfig::default()
            },
            &std::sync::atomic::AtomicBool::new(false),
            None,
        )
        .map_err(|e| e.to_string())?;
        log.both(format!(
            "FulgurStack inspection ({} sky star list): {} frames in {:.1} s",
            if fs_sky { "with" } else { "without" },
            report.frames.len(),
            t.elapsed().as_secs_f64()
        ));
        let by_name: BTreeMap<String, &fulgur_pipeline::inspect::QualityFrame> = report
            .frames
            .iter()
            .map(|q| (q.file_name.replace('\\', "/"), q))
            .collect();
        let mut counts = vec![];
        for e in &mut entries {
            let Some(q) = by_name.get(&e.frame.name) else {
                continue;
            };
            // All 250 stars, ordered by flux (FulgurStack's catalogue leaves
            // saturated stars out).
            let mut order: Vec<usize> = (0..q.stars.len()).collect();
            order.sort_by(|&a, &b| q.stars[b].flux_adu.total_cmp(&q.stars[a].flux_adu));
            let stars: Vec<[f64; 4]> = if fs_sky {
                q.sky_stars.iter().map(|s| [s[0], s[1], s[2], s[3]]).collect()
            } else {
                order
                    .iter()
                    .map(|&i| &q.stars[i])
                    .map(|s| [s.x_px, s.y_px, s.moment_fwhm_px, s.eccentricity])
                    .collect()
            };
            // How FulgurStack's positions sit against skyreg's own detections.
            let own = solve::Grid::new(e.stars.clone(), 8.);
            let offsets: Vec<[f64; 2]> = stars
                .iter()
                .filter_map(|s| {
                    own.nearest([s[0], s[1]], 3.).map(|(n, _)| {
                        let o = own.points[n];
                        [s[0] - o[0], s[1] - o[1]]
                    })
                })
                .collect();
            let med = |k: usize| {
                let mut v: Vec<f64> = offsets.iter().map(|o| o[k]).collect();
                v.sort_by(f64::total_cmp);
                v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
            };
            log.line(format!(
                "spike S2 {}: {} FulgurStack stars, {} within 3 px of skyreg's, median offset {:.2}/{:.2} px; first {:?}",
                e.frame.name,
                stars.len(),
                offsets.len(),
                med(0),
                med(1),
                stars.iter().take(3).map(|s| [s[0].round(), s[1].round()]).collect::<Vec<_>>()
            ));
            e.stars = stars.iter().map(|s| [s[0], s[1]]).collect();
            e.points = sky::point_sources(&stars);
            counts.push(e.stars.len());
        }
        counts.sort_unstable();
        log.both(format!(
            "spike S2: FulgurStack inspection stars for {}/{} frames (median {} stars per frame), inspection {:.1} s",
            counts.len(),
            entries.len(),
            counts.get(counts.len() / 2).copied().unwrap_or(0),
            t.elapsed().as_secs_f64()
        ));
    }

    // ------------------------------------------------------------ solving
    // fulgur_pipeline::sky::batch: one blind search per camera group, the
    // rest seeded from a solved sibling (ADR-131, plan phase 1).
    let t = Instant::now();
    let solver_frames: Vec<sky::SkyFrame> = entries
        .iter()
        .map(|e| sky::SkyFrame {
            name: &e.frame.name,
            width: e.frame.width,
            height: e.frame.height,
            epoch: e.frame.epoch,
            stars: &e.stars,
        })
        .collect();
    let groups: Vec<String> = entries.iter().map(|e| group(&e.frame)).collect();
    let header_hints: Vec<Option<(f64, f64)>> =
        entries.iter().map(|e| e.frame.header_hint).collect();
    let (outcomes, rounds) = batch::solve_all(
        &solver_frames,
        &groups,
        &header_hints,
        hint,
        &blind,
        max_rms,
    );
    drop(solver_frames);
    for (k, r) in rounds.iter().enumerate() {
        log.both(format!(
            "solve round {}: {} blind, {} seeded; {}/{} solved ({:.1} s)",
            k + 1,
            r.blind,
            r.seeded,
            r.solved,
            entries.len(),
            r.seconds
        ));
    }
    let mut priors: BTreeMap<String, Vec<solve::Prior>> = BTreeMap::new();
    for (e, o) in entries.iter_mut().zip(outcomes) {
        if let (Ok(s), 1) = (&o.solution, o.pass) {
            priors
                .entry(group(&e.frame))
                .or_default()
                .push(solve::Prior::of(s));
        }
        e.solution = o.solution;
        e.pass = o.pass;
        e.solve_s = o.seconds;
    }
    let mut centres: Vec<(f64, f64)> = vec![];
    for e in &entries {
        if let Ok(s) = &e.solution {
            if centres
                .iter()
                .all(|c| sky::catalogue::separation(c.0, c.1, s.centre_ra, s.centre_dec) > 0.3)
                && sky::catalogue::separation(hint.0, hint.1, s.centre_ra, s.centre_dec) > 0.3
            {
                centres.push((s.centre_ra, s.centre_dec));
            }
        }
    }
    centres.truncate(8);
    log.line("");
    log.line("== PLATE SOLUTIONS (each frame on its own against Gaia DR3) ==");
    let mut by_group: BTreeMap<String, (usize, usize, Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for e in &entries {
        let g = by_group.entry(group(&e.frame)).or_default();
        g.1 += 1;
        match &e.solution {
            Ok(s) => {
                g.0 += 1;
                g.2.push(s.held_out_rms_px);
                g.3.push(s.arcsec_per_px);
                log.line(format!(
                    "SOLVED {:<70} pass {} | {:.4}\"/px, north {:7.2} deg, {} | centre RA {:.4} Dec {:+.4} | degree {} on {} stars, held-out {} stars RMS {:.3} px ({:.2}\") [by degree 1/2/3: {}/{}/{}], fit RMS {:.3}, max {:.2}, quadrants {}/{}/{}/{} px | blind: {} votes (runner-up {}), half field {:.2} deg, {} | {:.2} s",
                    e.frame.name,
                    e.pass,
                    s.arcsec_per_px,
                    s.north_deg,
                    if s.mirrored { "mirrored" } else { "direct" },
                    s.centre_ra,
                    s.centre_dec,
                    s.degree,
                    s.matched,
                    s.held_out,
                    s.held_out_rms_px,
                    s.held_out_rms_px * s.arcsec_per_px,
                    fmt_opt(s.held_out_rms_by_degree[0]),
                    fmt_opt(s.held_out_rms_by_degree[1]),
                    fmt_opt(s.held_out_rms_by_degree[2]),
                    s.fit_rms_px,
                    s.max_px,
                    fmt_opt(s.quadrant_rms_px[0]),
                    fmt_opt(s.quadrant_rms_px[1]),
                    fmt_opt(s.quadrant_rms_px[2]),
                    fmt_opt(s.quadrant_rms_px[3]),
                    s.blind.votes,
                    s.blind.runner_up,
                    s.blind.half_field_deg,
                    s.blind.parity,
                    e.solve_s
                ));
            }
            Err(reason) => log.line(format!(
                "FAILED {:<70} {} stars, FWHM {:.2} px: {reason} | {:.2} s",
                e.frame.name, e.frame.detected, e.frame.fwhm_px, e.solve_s
            )),
        }
    }
    let solved = entries.iter().filter(|e| e.solution.is_ok()).count();
    log.both(format!(
        "solve: {solved}/{} frames solved in {:.1} s",
        entries.len(),
        t.elapsed().as_secs_f64()
    ));
    // Where the time goes, summed over frames (thread seconds).
    let sum = |f: &dyn Fn(&Entry) -> f64| entries.iter().map(f).sum::<f64>();
    let solved_only = |f: &dyn Fn(&solve::Solution) -> f64| {
        entries
            .iter()
            .filter_map(|e| e.solution.as_ref().ok())
            .map(f)
            .sum::<f64>()
    };
    let configs: usize = entries
        .iter()
        .filter_map(|e| e.solution.as_ref().ok())
        .map(|s| s.blind.configs)
        .sum();
    let evaluated: usize = entries
        .iter()
        .filter_map(|e| e.solution.as_ref().ok())
        .map(|s| s.blind.evaluated)
        .sum();
    log.both(format!(
        "timing (thread seconds): read {:.1}, detect {:.1}, solve {:.1} = blind {:.1} ({configs} configurations, {evaluated} poses voted) + refine {:.1}",
        sum(&|e| e.frame.read_s),
        sum(&|e| e.frame.detect_s),
        sum(&|e| e.solve_s),
        solved_only(&|s| s.blind.seconds),
        solved_only(&|s| s.refine_s),
    ));
    log.line("");
    log.line("== SOLUTIONS PER CAMERA GROUP ==");
    for (g, (ok, n, rms, scale)) in &by_group {
        let med = |v: &Vec<f64>| {
            let mut v = v.clone();
            v.sort_by(f64::total_cmp);
            v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
        };
        log.both(format!(
            "group {g:<45} solved {ok:>3}/{n:<3} | scale {:.3}\"/px | held-out RMS median {:.3} px, worst {:.3} px",
            med(scale),
            med(rms),
            rms.iter().copied().fold(0., f64::max)
        ));
    }

    // ------------------------------------------------------------ targets
    let canonical = |p: &Path| std::fs::canonicalize(p).ok();
    // A target is an input frame or an external image.
    let mut targets: Vec<(String, Result<usize, usize>)> = vec![];
    let mut externals: Vec<Entry> = vec![];
    let mut target_paths: Vec<PathBuf> = opts(args, "--target")
        .into_iter()
        .map(PathBuf::from)
        .collect();
    for dir in opts(args, "--targets-dir") {
        let mut found: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{dir}: {e}"))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && frames::is_image(p))
            .collect();
        found.sort();
        target_paths.extend(found);
    }
    for path in target_paths {
        let t = path.display().to_string();
        let c = canonical(&path);
        if let Some(i) = entries
            .iter()
            .position(|e| c.is_some() && canonical(&e.frame.path) == c)
        {
            targets.push((format!("input frame {}", entries[i].frame.name), Ok(i)));
            continue;
        }
        let frame = frames::load(&path)?;
        let (stars, points) = star_lists(&frame);
        let sky_frame = sky::SkyFrame {
            name: &frame.name,
            width: frame.width,
            height: frame.height,
            epoch: frame.epoch,
            stars: &stars,
        };
        let mut hints = vec![];
        if let Some(h) = frame.header_hint {
            hints.push(h);
        }
        hints.push(hint);
        hints.extend(centres.iter().copied());
        let s = Instant::now();
        // A target of a known camera starts from its group's solutions.
        let mut solution = Err("no sibling solution fits".to_string());
        for p in priors.get(&group(&frame)).into_iter().flatten() {
            solution = solve::solve(&sky_frame, &[p.centre, hint], &blind, max_rms, Some(p));
            if solution.is_ok() {
                break;
            }
        }
        if solution.is_err() {
            solution = solve::solve(&sky_frame, &hints, &blind, max_rms, None);
        }
        log.line("");
        match &solution {
            Ok(s) => log.both(format!(
                "external target {}: SOLVED {:.4}\"/px, {} stars, held-out RMS {:.3} px, degree {}, {}",
                frame.name,
                s.arcsec_per_px,
                s.matched,
                s.held_out_rms_px,
                s.degree,
                if s.mirrored { "mirrored" } else { "direct" }
            )),
            Err(e) => log.both(format!("external target {}: FAILED {e}", frame.name)),
        }
        externals.push(Entry {
            frame,
            stars,
            points,
            solution,
            pass: 1,
            solve_s: s.elapsed().as_secs_f64(),
        });
        targets.push((format!("external {t}"), Err(externals.len() - 1)));
    }

    let mut report_targets = vec![];
    for (label, which) in &targets {
        let te = match which {
            Ok(i) => &entries[*i],
            Err(k) => &externals[*k],
        };
        let skip = which.ok();
        log.line("");
        log.both(format!("== REGISTRATION ONTO {label} =="));
        let Ok(ts) = &te.solution else {
            log.both("  target not solved: no registration possible");
            continue;
        };
        let check_start = Instant::now();
        let target = verify::Target::new(ts, &te.points);
        let checks: Vec<(usize, Option<verify::Check>)> = (0..entries.len())
            .into_par_iter()
            .filter(|&i| Some(i) != skip)
            .map(|i| {
                let e = &entries[i];
                (
                    i,
                    e.solution
                        .as_ref()
                        .ok()
                        .map(|s| verify::check(i, s, &e.points, &target)),
                )
            })
            .collect();
        let mut counts: BTreeMap<String, BTreeMap<&str, usize>> = BTreeMap::new();
        for (i, c) in &checks {
            let e = &entries[*i];
            let status = c.as_ref().map_or("NOT_SOLVED", |c| c.status);
            *counts
                .entry(group(&e.frame))
                .or_default()
                .entry(status)
                .or_default() += 1;
            match c {
                Some(c) => log.line(format!(
                    "  {:<10} {:<70} {:>4}/{:<4} pairs, {:.0} % within 1 coarse px | offset: frame {:.3} px (limit {:.3}), worst of {} cells {:.3} px (limit {:.3}) | cells {} | scatter {:.3} px = {:.2}\" (all pairs {:.3})",
                    c.status,
                    e.frame.name,
                    c.matched,
                    c.inside,
                    c.kept_fraction * 100.,
                    c.mean_offset_target_px,
                    verify::MAX_MEAN_COARSE_PX * c.coarse_target_px,
                    c.judged_cells,
                    c.worst_cell_offset_target_px,
                    verify::MAX_CELL_COARSE_PX * c.coarse_target_px,
                    c.cell_offsets_target_px
                        .iter()
                        .map(|v| fmt_opt(*v))
                        .collect::<Vec<_>>()
                        .join("/"),
                    c.rms_target_px,
                    c.rms_arcsec,
                    c.raw_rms_target_px,
                )),
                None => log.line(format!(
                    "  NOT_SOLVED {:<70} {}",
                    e.frame.name,
                    e.solution.as_ref().err().unwrap()
                )),
            }
        }
        let mut sum: BTreeMap<&str, usize> = BTreeMap::new();
        for (g, c) in &counts {
            log.both(format!("  summary {g:<45} {c:?}"));
            for (k, v) in c {
                *sum.entry(k).or_default() += v;
            }
        }
        log.both(format!(
            "  TOTAL {sum:?} (target solve {:.2} s, mapping and checks {:.2} s)",
            te.solve_s,
            check_start.elapsed().as_secs_f64()
        ));
        // Spike S1: the composed map as FulgurStack's cubic carrier.
        let carriers: Vec<Option<sky::warp::Carrier>> = (0..entries.len())
            .into_par_iter()
            .filter(|&i| Some(i) != skip)
            .map(|i| {
                entries[i]
                    .solution
                    .as_ref()
                    .ok()
                    .and_then(|s| sky::warp::carry(s, ts))
            })
            .collect();
        let sources: Vec<usize> = (0..entries.len()).filter(|&i| Some(i) != skip).collect();
        let mut by_camera: BTreeMap<String, f64> = BTreeMap::new();
        for (&i, c) in sources.iter().zip(&carriers) {
            if let Some(c) = c {
                let w = by_camera.entry(group(&entries[i].frame)).or_insert(0.);
                *w = w.max(c.max_error_px);
            }
        }
        for (g, w) in &by_camera {
            log.line(format!("  carrier worst error from {g}: {w:.4} px"));
        }
        let mut worst: Vec<f64> = carriers.iter().flatten().map(|c| c.max_error_px).collect();
        worst.sort_by(f64::total_cmp);
        let refused = carriers.iter().filter(|c| c.is_none()).count();
        let mirrored = carriers
            .iter()
            .flatten()
            .filter(|c| c.mirrored_rows)
            .count();
        let pick = |q: f64| {
            worst
                .get(((worst.len() as f64 - 1.) * q).round() as usize)
                .copied()
                .unwrap_or(f64::NAN)
        };
        log.both(format!(
            "  carrier (cubic Warp vs exact composition, reference px): max error median {:.4}, 99 % {:.4}, worst {:.4}; mirrored rows {mirrored}; no valid carrier {refused}",
            pick(0.5),
            pick(0.99),
            pick(1.)
        ));
        report_targets.push(json!({
            "carrier": {
                "max_error_median_px": pick(0.5),
                "max_error_p99_px": pick(0.99),
                "max_error_worst_px": pick(1.),
                "mirrored_rows": mirrored,
                "no_valid_carrier": refused,
            },
            "target": label,
            "summary": sum,
            "per_group": counts,
            "checks": checks.iter().zip(&carriers).map(|((i, c), carrier)| json!({
                "source": entries[*i].frame.name,
                "check": c,
                "carrier_max_error_px": carrier.as_ref().map(|c| c.max_error_px),
                "carrier_mirrored_rows": carrier.as_ref().map(|c| c.mirrored_rows),
            })).collect::<Vec<_>>(),
        }));
    }

    // ------------------------------------------------------------ negative control
    // The check must catch a wrong registration: every target is displaced
    // on purpose by a fraction of a pixel and all sources checked again.
    let mut control = vec![];
    if args.iter().any(|a| a == "--negative-control") {
        log.line("");
        log.both("== NEGATIVE CONTROL: targets displaced on purpose ==");
        for (label, which) in &targets {
            let te = match which {
                Ok(i) => &entries[*i],
                Err(k) => &externals[*k],
            };
            let Ok(ts) = &te.solution else { continue };
            for shift in [0.05, 0.1, 0.2, 0.5] {
                let mut moved = ts.clone();
                moved.wcs.centre[0] += shift;
                let target = verify::Target::new(&moved, &te.points);
                let mut sum: BTreeMap<&str, usize> = BTreeMap::new();
                for (i, e) in entries.iter().enumerate() {
                    if which.ok() == Some(i) {
                        continue;
                    }
                    if let Ok(s) = &e.solution {
                        *sum.entry(verify::check(i, s, &e.points, &target).status)
                            .or_default() += 1;
                    }
                }
                log.both(format!("  {label}: shifted {shift} px -> {sum:?}"));
                control.push(json!({"target": label, "shift_px": shift, "summary": sum}));
            }
        }
    }

    // ------------------------------------------------------------ all targets
    let mut matrix = vec![];
    if args.iter().any(|a| a == "--all-targets") {
        let t = Instant::now();
        log.line("");
        log.both("== EVERY SOLVED INPUT FRAME AS TARGET ==");
        let solved: Vec<usize> = (0..entries.len())
            .filter(|&i| entries[i].solution.is_ok())
            .collect();
        let rows: Vec<(usize, usize, usize, usize, f64)> = solved
            .par_iter()
            .map(|&ti| {
                let te = &entries[ti];
                let target = verify::Target::new(te.solution.as_ref().unwrap(), &te.points);
                let (mut pass, mut fail, mut unv) = (0, 0, 0);
                let mut rms = vec![];
                for &si in &solved {
                    if si == ti {
                        continue;
                    }
                    let s = &entries[si];
                    let c = verify::check(si, s.solution.as_ref().unwrap(), &s.points, &target);
                    match c.status {
                        "PASS" => {
                            pass += 1;
                            rms.push(c.rms_arcsec);
                        }
                        "FAIL" => fail += 1,
                        _ => unv += 1,
                    }
                }
                rms.sort_by(f64::total_cmp);
                (
                    ti,
                    pass,
                    fail,
                    unv,
                    rms.get(rms.len() / 2).copied().unwrap_or(f64::NAN),
                )
            })
            .collect();
        let others = solved.len().saturating_sub(1);
        let mut pass_all = 0;
        for &(ti, pass, fail, unv, med) in &rows {
            if pass == others {
                pass_all += 1;
            }
            log.line(format!(
                "  target {:<70} PASS {pass:>3} FAIL {fail:>3} UNVERIFIED {unv:>3} of {others} | median RMS {med:.2}\"",
                entries[ti].frame.name
            ));
            matrix.push(json!({"target": entries[ti].frame.name, "pass": pass, "fail": fail, "unverified": unv, "median_rms_arcsec": med}));
        }
        let fails: usize = rows.iter().map(|r| r.2).sum();
        let unv: usize = rows.iter().map(|r| r.3).sum();
        let pairs = rows.len() * others;
        log.both(format!(
            "  {} targets x {} sources = {} registrations: PASS {}, FAIL {fails}, UNVERIFIED {unv}; targets with every source PASS: {pass_all} ({:.1} s)",
            rows.len(),
            others,
            pairs,
            pairs - fails - unv,
            t.elapsed().as_secs_f64()
        ));
    }

    // ------------------------------------------------------------ report
    let report = json!({
        "tool": "skyreg 0.1",
        "hint": hint,
        "limits": {
            "solve_held_out_rms_px": max_rms,
            "pair_min_stars": verify::MIN_PAIRS,
            "pair_clip_coarse_px": verify::CLIP_COARSE_PX,
            "pair_min_kept_fraction": verify::MIN_KEPT,
            "pair_mean_offset_coarse_px": verify::MAX_MEAN_COARSE_PX,
            "pair_cell_offset_coarse_px": verify::MAX_CELL_COARSE_PX,
            "pair_cell_min_pairs": verify::CELL_MIN_PAIRS,
        },
        "frames": entries.iter().chain(externals.iter()).map(|e| json!({
            "frame": e.frame,
            "solution": e.solution.as_ref().ok(),
            "failure": e.solution.as_ref().err(),
            "pass": e.pass,
            "solve_s": e.solve_s,
        })).collect::<Vec<_>>(),
        "targets": report_targets,
        "all_targets": matrix,
        "negative_control": control,
        "wall_s": start.elapsed().as_secs_f64(),
        "peak_rss_bytes": fulgur_core::telemetry::peak_rss_bytes(),
    });
    std::fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    log.both(format!(
        "done in {:.1} s, peak RAM {:.2} GB; log {}",
        start.elapsed().as_secs_f64(),
        fulgur_core::telemetry::peak_rss_bytes().unwrap_or(0) as f64 / 1e9,
        out.join("registration.log").display()
    ));
    Ok(())
}

/// Solver stars and check points of a frame, brightest first.
fn star_lists(frame: &frames::Frame) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let stars = frame.stars.iter().map(|s| [s.x, s.y]).collect();
    let described: Vec<[f64; 4]> = frame
        .stars
        .iter()
        .map(|s| [s.x, s.y, s.fwhm, s.ecc])
        .collect();
    (stars, sky::point_sources(&described))
}
