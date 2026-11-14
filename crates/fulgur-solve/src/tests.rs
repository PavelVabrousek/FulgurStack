//! Tests on a synthetic sky: random stars all over the sphere, an index of
//! three bands, frames cut out of that sky at random places.
use super::*;
use crate::sphere::{Basis, RAD, radec, unit};
use std::sync::atomic::AtomicBool;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as f64 / u64::MAX as f64
    }

    fn normal(&mut self) -> f64 {
        let (a, b) = (self.next().max(1e-12), self.next());
        (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
    }
}

fn sky(n: usize, rng: &mut Rng) -> Vec<IndexStar> {
    (0..n)
        .map(|_| {
            let dec = (2.0 * rng.next() - 1.0).asin().to_degrees();
            let ra = rng.next() * 360.0;
            IndexStar::new(ra, dec, (5.0 + 8.0 * rng.next().sqrt()) as f32)
        })
        .collect()
}

fn bands() -> Vec<Band> {
    let band = |dmin, dmax, cell| Band {
        dmin,
        dmax,
        cell,
        per_cell: 10,
        partners: 4,
        pairs: 2,
    };
    vec![
        band(0.5, 1.0, 0.5),
        band(1.0, 2.0, 1.0),
        band(2.0, 4.0, 2.0),
    ]
}

fn index(rng: &mut Rng) -> Index {
    Index::build(
        sky(300_000, rng),
        13.0,
        &bands(),
        &|_| {},
        &AtomicBool::new(false),
    )
    .unwrap()
}

/// A frame of 3840 x 2160 pixels at a random place, rotation and mirroring:
/// the `n` brightest stars with a noisy order, a fifth missed, centroids off
/// by a few tenths of a pixel. Returns the detections and the truth.
fn frame(index: &Index, rng: &mut Rng, n: usize) -> (Vec<[f64; 2]>, (f64, f64, f64)) {
    let (w, h) = (3840.0, 2160.0);
    let ra = rng.next() * 360.0;
    let dec = (2.0 * rng.next() - 1.0).asin().to_degrees();
    let long_side = 2.0 + 2.0 * rng.next();
    let scale = long_side * RAD / w;
    let (theta, mirrored) = (rng.next() * std::f64::consts::TAU, rng.next() < 0.5);
    let centre = unit(ra, dec);
    let basis = Basis::at(centre);
    let mut near = vec![];
    index.all.near(
        &index.stars,
        &centre,
        w.hypot(h) / 2.0 * scale / RAD * 1.05,
        &mut near,
    );
    let mut seen: Vec<(f64, [f64; 2])> = vec![];
    for &i in &near {
        let q = basis.plane(&index.stars[i as usize].v());
        let (c, s) = (theta.cos(), theta.sin());
        let (mut x, y) = ((c * q[0] - s * q[1]) / scale, (s * q[0] + c * q[1]) / scale);
        if mirrored {
            x = -x;
        }
        let (x, y) = (x + w / 2.0, y + h / 2.0);
        if x < 0.0 || y < 0.0 || x >= w || y >= h || rng.next() < 0.2 {
            continue;
        }
        let key = f64::from(index.stars[i as usize].g) + 0.6 * rng.normal();
        seen.push((key, [x + 0.3 * rng.normal(), y + 0.3 * rng.normal()]));
    }
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    (
        seen.iter().take(n).map(|s| s.1).collect(),
        (ra, dec, scale / RAD * 3600.0),
    )
}

fn separation(a: (f64, f64), b: (f64, f64)) -> f64 {
    crate::sphere::separation_deg(&unit(a.0, a.1), &unit(b.0, b.1))
}

#[test]
fn frames_cut_from_the_sky_are_placed_at_random_places() {
    let mut rng = Rng(7);
    let index = index(&mut rng);
    assert!(index.quad_count() > 100_000, "{} quads", index.quad_count());
    let cancel = AtomicBool::new(false);
    let mut placed = 0;
    for _ in 0..12 {
        let (stars, truth) = frame(&index, &mut rng, 60);
        let found = solve(
            &index,
            3840,
            2160,
            &stars,
            &SolveOptions::default(),
            &cancel,
        );
        if let Some(h) = found {
            assert!(
                separation((h.ra, h.dec), (truth.0, truth.1)) < 0.1,
                "wrong place: {h:?} for {truth:?}"
            );
            assert!(
                (h.arcsec_per_px / truth.2 - 1.0).abs() < 0.02,
                "{h:?} {truth:?}"
            );
            assert!(h.matched >= 12 && h.log10_p <= -12.0);
            placed += 1;
        }
    }
    assert!(placed >= 10, "only {placed} of 12 frames placed");
}

#[test]
fn random_star_lists_are_never_placed() {
    let mut rng = Rng(11);
    let index = index(&mut rng);
    let cancel = AtomicBool::new(false);
    for _ in 0..15 {
        let stars: Vec<[f64; 2]> = (0..100)
            .map(|_| [rng.next() * 3840.0, rng.next() * 2160.0])
            .collect();
        assert!(
            solve(
                &index,
                3840,
                2160,
                &stars,
                &SolveOptions::default(),
                &cancel
            )
            .is_none()
        );
    }
}

#[test]
fn a_cancelled_search_gives_nothing() {
    let mut rng = Rng(5);
    let index = index(&mut rng);
    let (stars, _) = frame(&index, &mut rng, 60);
    let cancelled = AtomicBool::new(true);
    assert!(
        solve(
            &index,
            3840,
            2160,
            &stars,
            &SolveOptions::default(),
            &cancelled
        )
        .is_none()
    );
}

#[test]
fn too_few_stars_give_nothing() {
    let mut rng = Rng(5);
    let index = index(&mut rng);
    let cancel = AtomicBool::new(false);
    let stars = [[100.0, 100.0], [500.0, 900.0], [2000.0, 300.0]];
    assert!(
        solve(
            &index,
            3840,
            2160,
            &stars,
            &SolveOptions::default(),
            &cancel
        )
        .is_none()
    );
}

#[test]
fn a_saved_index_reads_back_the_same() {
    let mut rng = Rng(3);
    let index = index(&mut rng);
    let directory = std::env::temp_dir().join(format!("fulgur-solve-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sky.fsidx");
    // A partial file left by an interrupted save does not block the next one.
    let partial = Index::temporary_path(&path);
    assert_eq!(partial.file_name().unwrap(), "sky.fsidx.tmp");
    std::fs::write(&partial, b"left over").unwrap();
    index.save(&path).unwrap();
    assert!(!partial.exists());
    let again = Index::load(&path).unwrap();
    assert_eq!(again.star_count(), index.star_count());
    assert_eq!(again.quad_count(), index.quad_count());
    assert_eq!(again.bands(), index.bands());
    assert_eq!(again.gmax(), 13.0);
    // The same frame is placed the same way by both.
    let (stars, _) = frame(&index, &mut rng, 60);
    let cancel = AtomicBool::new(false);
    let options = SolveOptions::default();
    let (a, b) = (
        solve(&index, 3840, 2160, &stars, &options, &cancel),
        solve(&again, 3840, 2160, &stars, &options, &cancel),
    );
    assert_eq!(a.map(|h| (h.ra, h.dec)), b.map(|h| (h.ra, h.dec)));
    // Damaged files are refused, not trusted.
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() - 5]).unwrap();
    assert!(Index::load(&path).is_err());
    let mut wrong = bytes.clone();
    wrong[0] = b'X';
    std::fs::write(&path, wrong).unwrap();
    assert!(Index::load(&path).is_err());
    // A header that claims billions of entries is refused before any
    // memory is asked for it.
    let mut inflated = bytes.clone();
    inflated[28..36].copy_from_slice(&(u64::from(u32::MAX) * 2).to_le_bytes());
    std::fs::write(&path, inflated).unwrap();
    let error = Index::load(&path).err().unwrap();
    assert!(
        error.to_string().contains("incomplete or damaged"),
        "{error}"
    );
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_saturated_star_and_a_crowd_are_cleaned_away() {
    // One star broken into four detections, and eight knots of a nebula.
    let mut stars = vec![
        [1800.0, 3062.0],
        [1800.0, 3056.0],
        [1801.0, 3059.0],
        [1795.0, 3062.0],
    ];
    for k in 0..8 {
        stars.push([2150.0 + 8.0 * f64::from(k), 1650.0 + 5.0 * f64::from(k % 3)]);
    }
    stars.push([500.0, 500.0]);
    stars.push([3000.0, 1000.0]);
    let cleaned = clean_detections(&stars, 4656, 3520);
    // The saturated star counts once; the nebula's knots are all gone.
    assert!(cleaned.contains(&[1800.0, 3062.0]));
    assert!(!cleaned.contains(&[1800.0, 3056.0]));
    assert!(cleaned.contains(&[500.0, 500.0]) && cleaned.contains(&[3000.0, 1000.0]));
    assert!(
        cleaned.iter().all(|s| !(s[0] > 2100.0 && s[0] < 2300.0)),
        "{cleaned:?}"
    );
    assert_eq!(cleaned.len(), 3);
}

#[test]
fn the_sky_position_of_a_star_survives_the_unit_vector() {
    let star = IndexStar::new(123.456, -45.678, 9.0);
    let (ra, dec) = star.radec();
    assert!((ra - 123.456).abs() < 1e-5 && (dec + 45.678).abs() < 1e-5);
    let (ra, dec) = radec(&unit(359.999, 89.9));
    assert!((ra - 359.999).abs() < 1e-9 && (dec - 89.9).abs() < 1e-9);
}

#[test]
fn the_standard_coverage_is_the_seven_measured_bands() {
    let standard = Band::standard();
    let edges: Vec<(f64, f64)> = standard.iter().map(|b| (b.dmin, b.dmax)).collect();
    assert_eq!(
        edges,
        [
            (0.1, 0.25),
            (0.25, 0.5),
            (0.5, 1.0),
            (1.0, 2.0),
            (2.0, 4.0),
            (4.0, 8.0),
            (8.0, 16.0)
        ]
    );
    assert_eq!(standard[0].per_cell, 6);
    assert_eq!((standard[2].cell, standard[2].partners), (0.5, 4));
}

#[test]
fn a_wider_range_needs_more_bands_and_more_room() {
    let standard = SizeEstimate::for_fields(0.5, 12.0);
    let wider = SizeEstimate::for_fields(0.25, 12.0);
    let narrower = SizeEstimate::for_fields(2.0, 6.0);
    assert!(wider.file_bytes > standard.file_bytes);
    assert!(narrower.file_bytes < standard.file_bytes);
    // The measured standard index is 1121 MB.
    let mb = standard.file_bytes as f64 / 1e6;
    assert!((mb - 1121.0).abs() < 30.0, "{mb} MB");
    assert!(standard.build_bytes > 2 * standard.file_bytes);
}

#[test]
fn an_index_reports_the_fields_it_covers() {
    let mut rng = Rng(9);
    let index = index(&mut rng);
    // Quads of 0.5 to 4 degrees: fields of 2.5 degrees to about 3.
    let (low, high) = index.coverage_deg().unwrap();
    assert!(
        (low - 2.5).abs() < 1e-9 && (high - 3.01).abs() < 0.02,
        "{low} {high}"
    );
}

#[test]
fn a_cancelled_build_gives_no_index() {
    let mut rng = Rng(2);
    let stars = sky(20_000, &mut rng);
    assert!(Index::build(stars, 13.0, &bands(), &|_| {}, &AtomicBool::new(true)).is_none());
}
