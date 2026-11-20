//! The plate solution of a stack's pixel grid, checked on the finished
//! master and written as FITS WCS (ADR-155). Sky registration already solved
//! the grid's frame; a star registration solves only its reference frame.
//! Before the cards are written, the master's own stars are matched with
//! Gaia through the solution, so a master never carries a WCS it does not
//! fit.

use crate::register::RegistrationReport;
use crate::register::sky_engine::SkyOptions;
use crate::sky::catalogue::{GAIA_EPOCH, GaiaStar};
use crate::sky::solve::{self, BlindCache, Wcs};
use crate::sky::{SkyFrame, detect};
use fulgur_core::output::Astrometry;
use fulgur_core::{Error, Result, RgbImage, check_cancelled};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Brightest master stars compared with Gaia.
const MASTER_STARS: usize = 300;
/// A master star and a Gaia star closer than this are a pair.
const MATCH_PX: f64 = 2.;
/// The solution is written when at least this many stars pair...
const MIN_MATCHES: usize = 20;
/// ...with at most this RMS distance.
const MAX_RMS_PX: f64 = 1.;
/// Catalogue cones, as sky registration reads them.
const DEEP: (f64, f64) = (1.7, 17.5);
const WIDE: (f64, f64) = (6.5, 13.5);

/// The plate solution of the frame whose pixel grid the master uses.
#[derive(Clone, Debug, Serialize)]
pub struct GridSolution {
    /// The frame (or external reference) solved.
    pub source: String,
    /// "sky registration" or "reference plate solve".
    pub origin: &'static str,
    #[serde(skip)]
    pub wcs: Wcs,
    /// Decimal year the Gaia positions were brought to.
    pub epoch: f64,
    pub arcsec_per_px: f64,
    pub held_out_rms_px: f64,
    pub catalogue_matches: usize,
    pub centre_ra_deg: f64,
    pub centre_dec_deg: f64,
    pub north_deg: f64,
    pub mirrored: bool,
}

impl GridSolution {
    /// The solution on the drizzle grid, `scale` times finer (ADR-188).
    pub fn refined(&self, scale: f64) -> Self {
        Self {
            wcs: self.wcs.refined(scale),
            arcsec_per_px: self.arcsec_per_px / scale,
            ..self.clone()
        }
    }
}

impl GridSolution {
    pub fn new(
        source: String,
        origin: &'static str,
        solution: &solve::Solution,
        epoch: f64,
    ) -> Self {
        let (centre_ra_deg, centre_dec_deg) = solution.wcs.centre_sky();
        Self {
            source,
            origin,
            wcs: solution.wcs.clone(),
            epoch,
            arcsec_per_px: solution.arcsec_per_px,
            held_out_rms_px: solution.held_out_rms_px,
            catalogue_matches: solution.matched,
            centre_ra_deg,
            centre_dec_deg,
            north_deg: solution.north_deg,
            mirrored: solution.mirrored,
        }
    }
}

/// What the master's check found, and whether the WCS was written.
#[derive(Clone, Debug, Serialize)]
pub struct MasterAstrometry {
    pub solution: GridSolution,
    pub master_stars: usize,
    pub matched: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rms_px: Option<f64>,
    pub written: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub rule: &'static str,
    #[serde(skip)]
    pub cards: Option<Astrometry>,
}

impl MasterAstrometry {
    /// One line for the console and Results.
    pub fn summary(&self) -> String {
        let check = match self.rms_px {
            Some(rms) => format!(
                "{} of {} master stars on Gaia, RMS {rms:.2} px",
                self.matched, self.master_stars
            ),
            None => format!(
                "{} of {} master stars on Gaia",
                self.matched, self.master_stars
            ),
        };
        if self.written {
            format!(
                "WCS written ({}, {:.3}\u{2033}/px, centre {}; {check})",
                self.solution.origin,
                self.solution.arcsec_per_px,
                crate::sky::catalogue::position_text(
                    self.solution.centre_ra_deg,
                    self.solution.centre_dec_deg
                )
            )
        } else {
            format!(
                "WCS not written: {} ({check})",
                self.reason.as_deref().unwrap_or("check failed")
            )
        }
    }
}

fn catalogue(sky: &SkyOptions, (ra, dec): (f64, f64)) -> Result<Vec<GaiaStar>> {
    let fetch =
        |(radius, g): (f64, f64)| (sky.catalogue)(ra, dec, radius, g).map_err(Error::Message);
    let mut stars = fetch(WIDE)?;
    stars.extend(fetch(DEEP)?);
    // Local catalogue files carry no source ids: a star is known by its
    // position and brightness, as in sky registration.
    let mut seen = std::collections::HashSet::new();
    stars.retain(|s| seen.insert((s.ra.to_bits(), s.dec.to_bits(), s.g.to_bits())));
    stars.sort_by(|a, b| a.g.total_cmp(&b.g).then(a.source_id.cmp(&b.source_id)));
    Ok(stars)
}

/// Plate-solves the frame whose grid a star registration used: its anchor,
/// read and calibrated as inspection read it, or its external reference.
pub fn solve_grid(
    plan: &RegistrationReport,
    root: &Path,
    sets: &[crate::inspect::InputSet],
    sky: &SkyOptions,
    cancel: &AtomicBool,
) -> Result<GridSolution> {
    check_cancelled(cancel)?;
    let (name, width, height, list, epoch) = match &plan.external_reference {
        Some(external) => {
            let plane = fulgur_core::fits::read_reference_plane(Path::new(&external.path), cancel)?;
            let (list, _) = detect::bright_stars(&plane.pixels, plane.width, plane.height, false);
            (
                external.path.clone(),
                plane.width,
                plane.height,
                list,
                GAIA_EPOCH,
            )
        }
        None => {
            let frame = plan
                .inspection
                .frames
                .iter()
                .find(|f| f.file_name == plan.reference_anchor)
                .ok_or(Error::Invalid(
                    "The reference frame is not in the inspection.",
                ))?;
            let set = sets
                .get(frame.light_set)
                .ok_or(Error::Invalid("Light sets differ from the inspection."))?;
            let info = frame
                .frame
                .as_ref()
                .ok_or(Error::Invalid("The reference frame was not read."))?;
            let list = crate::inspect::sky_star_list_of(frame, root, set)?;
            let epoch = crate::register::sky_engine::decimal_year(frame).unwrap_or(GAIA_EPOCH);
            (
                frame.file_name.clone(),
                info.width,
                info.height,
                list,
                epoch,
            )
        }
    };
    let positions: Vec<[f64; 2]> = list.iter().map(|s| [s[0], s[1]]).collect();
    let frame = SkyFrame {
        name: &name,
        width,
        height,
        epoch,
        stars: &positions,
    };
    // The chosen centre first, then the others (ADR-158): a centre left over
    // from another target finds no solution, the frames' own one does.
    let mut failures = vec![];
    for &centre in std::iter::once(&sky.centre).chain(&sky.fallback_centres) {
        check_cancelled(cancel)?;
        let around = format!(
            "around {}",
            crate::sky::catalogue::position_text(centre.0, centre.1)
        );
        let stars = catalogue(sky, centre)?;
        if stars.len() < 50 {
            failures.push(format!("too few Gaia stars {around}"));
            continue;
        }
        let cache = BlindCache::new(Arc::new(stars));
        match solve::solve(&frame, &[centre], &cache, sky.max_solve_rms_px, None) {
            Ok(solution) => {
                return Ok(GridSolution::new(
                    name,
                    "reference plate solve",
                    &solution,
                    epoch,
                ));
            }
            Err(e) => failures.push(format!("{e} {around}")),
        }
    }
    Err(Error::Message(format!(
        "the reference frame could not be plate-solved: {}",
        failures.join("; ")
    )))
}

/// Matches the master's brightest stars with Gaia through `grid` and, when
/// they agree, makes the WCS cards. `offset` is the grid pixel of master
/// pixel (0, 0).
pub fn check_master(
    image: &RgbImage,
    offset: [i32; 2],
    grid: &GridSolution,
    sky: &SkyOptions,
    cancel: &AtomicBool,
) -> Result<MasterAstrometry> {
    check_cancelled(cancel)?;
    let (width, height) = (image.width, image.height);
    let green: Vec<f32> = image.pixels.iter().map(|p| p[1]).collect();
    let (mut found, _) = detect::bright_stars(&green, width, height, false);
    found.truncate(MASTER_STARS);
    let stars = catalogue(sky, (grid.centre_ra_deg, grid.centre_dec_deg))?;
    // Gaia positions in master pixels, on a grid of MATCH_PX cells.
    let mut cells: std::collections::HashMap<(i64, i64), Vec<[f64; 2]>> = Default::default();
    for star in &stars {
        let (ra, dec) = star.at(grid.epoch);
        let Some(p) = grid.wcs.sky_to_pixel(ra, dec) else {
            continue;
        };
        let q = [p[0] - offset[0] as f64, p[1] - offset[1] as f64];
        if q[0] < -MATCH_PX
            || q[1] < -MATCH_PX
            || q[0] > width as f64 + MATCH_PX
            || q[1] > height as f64 + MATCH_PX
        {
            continue;
        }
        cells
            .entry((
                (q[0] / MATCH_PX).floor() as i64,
                (q[1] / MATCH_PX).floor() as i64,
            ))
            .or_default()
            .push(q);
    }
    let mut squares = vec![];
    for s in &found {
        let (cx, cy) = (
            (s[0] / MATCH_PX).floor() as i64,
            (s[1] / MATCH_PX).floor() as i64,
        );
        let nearest = (cx - 1..=cx + 1)
            .flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y)))
            .filter_map(|k| cells.get(&k))
            .flatten()
            .map(|q| (q[0] - s[0]).powi(2) + (q[1] - s[1]).powi(2))
            .fold(f64::INFINITY, f64::min);
        if nearest <= MATCH_PX * MATCH_PX {
            squares.push(nearest);
        }
    }
    let matched = squares.len();
    let rms_px = (matched > 0).then(|| (squares.iter().sum::<f64>() / matched as f64).sqrt());
    let reason = if found.len() < MIN_MATCHES {
        Some("too few stars in the master".to_owned())
    } else if matched < MIN_MATCHES {
        Some("too few master stars match Gaia through the solution".to_owned())
    } else if rms_px.is_some_and(|r| r > MAX_RMS_PX) {
        Some("master stars lie too far from Gaia".to_owned())
    } else {
        None
    };
    let cards = match reason {
        None => crate::sky::fits_wcs::cards(&grid.wcs, [offset[0] as f64, offset[1] as f64]).map(
            |cards| Astrometry {
                cards: cards
                    .into_iter()
                    .map(|(k, v, c)| (k, v, c.to_owned()))
                    .collect(),
                history: history(grid, matched, found.len(), rms_px.unwrap_or(0.)),
            },
        ),
        Some(_) => None,
    };
    let written = cards.is_some();
    Ok(MasterAstrometry {
        solution: grid.clone(),
        master_stars: found.len(),
        matched,
        rms_px,
        written,
        reason: if written {
            None
        } else {
            reason.or(Some("the solution has no usable linear part".to_owned()))
        },
        rule: "the master's 300 brightest stars are paired with Gaia DR3 through the grid's plate solution within 2 px; the WCS is written when at least 20 pair with an RMS distance of at most 1 px",
        cards,
    })
}

/// Replaces a checked WCS by the same solution on the grid `scale` times
/// finer, whose pixel (0, 0) is grid pixel `offset` (drizzle, ADR-189).
pub fn refine_cards(
    checked: &mut MasterAstrometry,
    grid: &GridSolution,
    scale: f64,
    offset: [i32; 2],
) {
    let Some(astrometry) = checked.cards.as_mut() else {
        return;
    };
    match crate::sky::fits_wcs::cards(
        &grid.wcs.refined(scale),
        [f64::from(offset[0]), f64::from(offset[1])],
    ) {
        Some(cards) => {
            astrometry.cards = cards
                .into_iter()
                .map(|(k, v, c)| (k, v, c.to_owned()))
                .collect();
            astrometry.history.push(format!(
                "WCS: checked in reference pixels, written for {scale}x drizzle."
            ));
        }
        None => {
            checked.cards = None;
            checked.written = false;
            checked.reason =
                Some("the drizzle grid's solution has no usable linear part".to_owned());
        }
    }
}

/// History lines of a written WCS, at most 72 characters each.
fn history(grid: &GridSolution, matched: usize, stars: usize, rms: f64) -> Vec<String> {
    let source = Path::new(&grid.source)
        .file_name()
        .map_or(grid.source.clone(), |n| n.to_string_lossy().into_owned());
    let source: String = source.chars().take(46).collect();
    vec![
        format!("WCS: Gaia DR3 solution of {source}"),
        format!(
            "WCS: {}, RMS {:.2} px on {} stars, epoch {:.2}",
            grid.origin, grid.held_out_rms_px, grid.catalogue_matches, grid.epoch
        ),
        format!("WCS: checked on the master: {matched}/{stars} stars, RMS {rms:.2} px"),
        "WCS: TAN with SIP distortion; pixels as stored, row 1 first.".to_owned(),
    ]
}

/// History of a master whose plate solve was tried but gave no WCS
/// (ADR-158); the reason is in the report.
pub fn unsolved() -> Astrometry {
    Astrometry {
        cards: vec![],
        history: vec![
            "No plate solution; crop offsets are not a celestial WCS.".to_owned(),
            "Plate solve tried, no WCS written; reason in the JSON report.".to_owned(),
        ],
    }
}

/// The sky registration's own grid solution, if the plan has one.
pub fn sky_grid(plan: &RegistrationReport) -> Option<GridSolution> {
    plan.sky.as_ref().and_then(|s| s.grid_solution.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sky::solve::Poly;

    /// A 600 x 400 linear solution, 2"/px, rotated by about 10 degrees.
    fn wcs(shift_px: f64) -> Wcs {
        let (s, c) = 0.17_f64.sin_cos();
        Wcs {
            ra0: 23.46,
            dec0: 30.66,
            centre: [299.5 + shift_px, 199.5],
            pixel_norm: 300.,
            sky_norm: 600.,
            forward: Poly {
                degree: 1,
                x: vec![0., -c, s],
                y: vec![0., s, c],
            },
            inverse: Poly {
                degree: 1,
                x: vec![0., -c, s],
                y: vec![0., s, c],
            },
            width: 600,
            height: 400,
        }
    }

    /// Gaia stars at known pixels of `truth`, and a master rendering them.
    fn field(truth: &Wcs) -> (Vec<GaiaStar>, RgbImage) {
        let (width, height) = (600, 400);
        let mut pixels = vec![[1000_f32; 3]; width * height];
        let mut stars = vec![];
        for k in 0..120 {
            let p = [
                20. + (k as f64 * 97.31) % 560.,
                20. + (k as f64 * 53.77) % 360.,
            ];
            let (ra, dec) = truth.pixel_to_sky(p);
            stars.push(GaiaStar {
                source_id: k,
                ra,
                dec,
                pmra: 0.,
                pmdec: 0.,
                g: 9. + (k % 7) as f64,
            });
            let flux = 40000. / (1. + (k % 7) as f64);
            for y in (p[1] as usize - 6)..(p[1] as usize + 7) {
                for x in (p[0] as usize - 6)..(p[0] as usize + 7) {
                    let d2 = (x as f64 - p[0]).powi(2) + (y as f64 - p[1]).powi(2);
                    let v = (flux * (-d2 / (2. * 1.5 * 1.5)).exp()) as f32;
                    for c in &mut pixels[y * width + x] {
                        *c += v;
                    }
                }
            }
        }
        let image = RgbImage {
            width,
            height,
            demosaic_method: fulgur_core::DemosaicMethod::Vng,
            pixels,
        };
        (stars, image)
    }

    fn options(stars: Vec<GaiaStar>) -> SkyOptions {
        SkyOptions::new(
            (23.46, 30.66),
            Arc::new(move |_, _, _, _| Ok(stars.clone())),
        )
    }

    fn grid(wcs: Wcs) -> GridSolution {
        GridSolution {
            source: "reference.fit".into(),
            origin: "reference plate solve",
            wcs,
            epoch: GAIA_EPOCH,
            arcsec_per_px: 2.,
            held_out_rms_px: 0.2,
            catalogue_matches: 100,
            centre_ra_deg: 23.46,
            centre_dec_deg: 30.66,
            north_deg: 10.,
            mirrored: false,
        }
    }

    #[test]
    fn a_solution_that_fits_the_master_is_written_and_a_wrong_one_is_not() {
        let truth = wcs(0.);
        let (stars, image) = field(&truth);
        let sky = options(stars);
        let cancel = AtomicBool::new(false);
        let good = check_master(&image, [0, 0], &grid(truth), &sky, &cancel).unwrap();
        assert!(good.written, "{}", good.summary());
        assert!(good.matched >= 100, "{}", good.summary());
        assert!(good.rms_px.unwrap() < 0.2, "{}", good.summary());
        let cards = good.cards.unwrap();
        assert!(
            cards
                .cards
                .iter()
                .any(|c| c.0 == "CTYPE1" && c.1 == "'RA---TAN'")
        );
        assert!(cards.history.iter().all(|h| h.len() <= 72));
        // A solution 1.5 px off puts every pair beyond the 1 px limit.
        let shifted = check_master(&image, [0, 0], &grid(wcs(1.5)), &sky, &cancel).unwrap();
        assert!(!shifted.written, "{}", shifted.summary());
        assert!(shifted.cards.is_none());
        // A crop: the same solution, the master starting at grid pixel (40, 30).
        let crop = RgbImage {
            width: 500,
            height: 320,
            demosaic_method: image.demosaic_method,
            pixels: (30..350)
                .flat_map(|y| (40..540).map(move |x| (x, y)))
                .map(|(x, y)| image.pixels[y * 600 + x])
                .collect(),
        };
        let cropped = check_master(&crop, [40, 30], &grid(wcs(0.)), &sky, &cancel).unwrap();
        assert!(cropped.written, "{}", cropped.summary());
        let crpix = |a: &MasterAstrometry, key: &str| -> f64 {
            a.cards
                .as_ref()
                .unwrap()
                .cards
                .iter()
                .find(|c| c.0 == key)
                .unwrap()
                .1
                .parse()
                .unwrap()
        };
        let full = check_master(&image, [0, 0], &grid(wcs(0.)), &sky, &cancel).unwrap();
        assert!((crpix(&full, "CRPIX1") - crpix(&cropped, "CRPIX1") - 40.).abs() < 1e-9);
        assert!((crpix(&full, "CRPIX2") - crpix(&cropped, "CRPIX2") - 30.).abs() < 1e-9);
    }
}
