//! The quad index: bright, evenly spread catalogue stars grouped into quads
//! of several size bands, hashed by their codes.
use crate::quad::{self, BUCKETS, P2, canonical, dequantize, quantize};
use crate::sphere::{Basis, Grid, IndexStar, V3, normalized, separation_deg};
use rayon::prelude::*;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const MAGIC: &[u8; 8] = b"FSOLVIDX";
const VERSION: u32 = 1;

/// One band of quad sizes (the distance A to B, in degrees).
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    pub dmin: f64,
    pub dmax: f64,
    /// Width of the cells that thin the stars, in degrees, and how many of
    /// the brightest stars each keeps.
    pub cell: f64,
    pub per_cell: usize,
    /// Partner stars B tried for every A, and (C, D) pairs for every AB.
    pub partners: usize,
    pub pairs: usize,
}

/// Edges of the quad size bands, in degrees. A band is two neighbours.
const LADDER: [f64; 12] = [
    0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0,
];

/// Quads a band of each lower edge holds for stars to G 13 (measured on the
/// whole Gaia DR3 sky, 2026-11-09; the two smallest are estimates).
const BAND_QUADS: [(f64, f64); 10] = [
    (0.025, 12.0e6),
    (0.05, 12.0e6),
    (0.1, 8.9e6),
    (0.25, 15.5e6),
    (0.5, 13.05e6),
    (1.0, 3.29e6),
    (2.0, 0.82e6),
    (4.0, 0.205e6),
    (8.0, 0.051e6),
    (16.0, 0.013e6),
];

/// Stars to G 13 in the whole sky (Gaia DR3).
const STARS_G13: u64 = 7_372_672;

/// Smallest and largest field (long side, degrees) an index may cover.
pub const FIELD_LIMITS_DEG: (f64, f64) = (0.25, 40.0);

/// The standard coverage: fields of 0.5 to 12 degrees.
pub const STANDARD_FIELDS_DEG: (f64, f64) = (0.5, 12.0);

/// A band's quads span 0.2 to 1.33 fields, so a coverage of fields gives the
/// sizes of the quads.
const QUAD_PER_FIELD: (f64, f64) = (0.2, 1.33);

impl Band {
    /// The bands for fields of `min_deg` to `max_deg` degrees across (long
    /// side). The standard 0.5 to 12 degrees gives the seven bands measured
    /// in the 2026-11-09 feasibility study (quads of 0.1 to 16 degrees).
    pub fn for_fields(min_deg: f64, max_deg: f64) -> Vec<Band> {
        let (lo, hi) = (min_deg * QUAD_PER_FIELD.0, max_deg * QUAD_PER_FIELD.1);
        LADDER
            .windows(2)
            .filter(|w| w[1] > lo && w[0] < hi)
            .map(|w| {
                let (dmin, dmax) = (w[0], w[1]);
                let (cell, per_cell, partners, pairs) = if dmin < 0.1 {
                    (0.1, 6, 2, 1)
                } else if dmin < 0.2 {
                    (0.15, 6, 2, 1)
                } else if dmin < 0.4 {
                    (0.25, 8, 2, 2)
                } else {
                    (dmin, 10, 4, 2)
                };
                Band {
                    dmin,
                    dmax,
                    cell,
                    per_cell,
                    partners,
                    pairs,
                }
            })
            .collect()
    }

    /// The bands of the standard coverage, 0.5 to 12 degrees.
    pub fn standard() -> Vec<Band> {
        Self::for_fields(STANDARD_FIELDS_DEG.0, STANDARD_FIELDS_DEG.1)
    }

    /// Quads this band holds, for stars to G 13 (an estimate).
    fn estimated_quads(&self) -> f64 {
        BAND_QUADS
            .iter()
            .find(|(edge, _)| (edge - self.dmin).abs() < 1e-9)
            .map_or(0.0, |(_, quads)| *quads)
    }
}

/// What an index of these bands will take, estimated from the measured
/// whole-sky counts for stars to G 13.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeEstimate {
    pub quads: u64,
    /// The index file, and the memory a solving program holds it in.
    pub file_bytes: u64,
    /// Memory while the index is being built (several times the file).
    pub build_bytes: u64,
}

impl SizeEstimate {
    pub fn of(bands: &[Band]) -> Self {
        let quads = bands.iter().map(Band::estimated_quads).sum::<f64>() as u64;
        let file_bytes = STARS_G13 * 16 + quads * 24 + 64;
        Self {
            quads,
            file_bytes,
            build_bytes: file_bytes * 5 / 2,
        }
    }

    /// For fields of `min_deg` to `max_deg` degrees across.
    pub fn for_fields(min_deg: f64, max_deg: f64) -> Self {
        Self::of(&Band::for_fields(min_deg, max_deg))
    }
}

/// What one band contributed to the index.
#[derive(Clone, Debug, PartialEq)]
pub struct BandSummary {
    pub dmin: f64,
    pub dmax: f64,
    pub stars: u64,
    pub quads: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Entry {
    pub(crate) code: [u16; 4],
    pub(crate) stars: [u32; 4],
}

/// The index: the catalogue stars, and every quad hashed by its code.
pub struct Index {
    pub(crate) stars: Vec<IndexStar>,
    pub(crate) all: Grid,
    entries: Vec<Entry>,
    offsets: Vec<u32>,
    gmax: f32,
    bands: Vec<BandSummary>,
}

fn entry_bucket(e: &Entry) -> usize {
    quad::bucket(&dequantize(&e.code))
}

/// The brightest `band.per_cell` stars of every cell, and the quads they
/// make: for each star A the brightest partners B at a distance of the band,
/// and for each AB the brightest stars inside the circle on AB as C and D.
fn band_quads(stars: &[IndexStar], band: &Band) -> (usize, Vec<[u32; 4]>) {
    let mut keyed: Vec<(u64, f32, u32)> = stars
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (ra, dec) = s.radec();
            let (r, j) = Grid::cell_of(ra, dec, band.cell);
            (Grid::key(r, j), s.g, i as u32)
        })
        .collect();
    keyed.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut ids: Vec<u32> = vec![];
    let mut run = (u64::MAX, 0usize);
    for (key, _, id) in keyed {
        if key != run.0 {
            run = (key, 0);
        }
        if run.1 < band.per_cell {
            ids.push(id);
        }
        run.1 += 1;
    }
    let grid = Grid::new(&ids, stars, band.dmax);
    let quads: Vec<[u32; 4]> = ids
        .par_iter()
        .flat_map_iter(|&a| quads_of(stars, &grid, band, a))
        .collect();
    (ids.len(), quads)
}

fn quads_of(stars: &[IndexStar], grid: &Grid, band: &Band, a: u32) -> Vec<[u32; 4]> {
    let mut out: Vec<[u32; 4]> = vec![];
    let (ua, mut near, mut inner) = (stars[a as usize].v(), vec![], vec![]);
    grid.near(stars, &ua, band.dmax, &mut near);
    let mut partners: Vec<(f32, u32)> = near
        .iter()
        .filter(|&&b| b != a)
        .filter(|&&b| {
            let d = separation_deg(&ua, &stars[b as usize].v());
            d >= band.dmin && d <= band.dmax
        })
        .map(|&b| (stars[b as usize].g, b))
        .collect();
    partners.sort_by(|x, y| x.0.total_cmp(&y.0));
    let basis = Basis::at(ua);
    for &(_, b) in partners.iter().take(band.partners) {
        let ub = stars[b as usize].v();
        let middle = normalized(&[ua[0] + ub[0], ua[1] + ub[1], ua[2] + ub[2]]);
        grid.near(stars, &middle, separation_deg(&ua, &ub) / 2.0, &mut inner);
        let mut within: Vec<(f32, u32)> = inner
            .iter()
            .filter(|&&c| c != a && c != b)
            .map(|&c| (stars[c as usize].g, c))
            .collect();
        within.sort_by(|x, y| x.0.total_cmp(&y.0));
        let within: Vec<u32> = within.iter().take(3).map(|x| x.1).collect();
        if within.len() < 2 {
            continue;
        }
        for (first, second) in [(0, 1), (0, 2)].into_iter().take(band.pairs) {
            let (Some(&c), Some(&d)) = (within.get(first), within.get(second)) else {
                continue;
            };
            let ids = [a, b, c, d];
            let points: [P2; 4] = ids.map(|i| basis.plane(&stars[i as usize].v()));
            if let Some((_, order)) = canonical(points) {
                out.push(order.map(|o| ids[o]));
            }
        }
    }
    out
}

impl Index {
    /// Builds the index of `stars` (every star brighter than the catalogue
    /// limit `gmax`, all over the sky) for the given bands. `say` is told
    /// what is happening.
    pub fn build(
        stars: Vec<IndexStar>,
        gmax: f32,
        bands: &[Band],
        say: &dyn Fn(String),
        cancel: &AtomicBool,
    ) -> Option<Self> {
        let mut quads: Vec<[u32; 4]> = vec![];
        let mut summary = vec![];
        for band in bands {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let started = std::time::Instant::now();
            let (used, found) = band_quads(&stars, band);
            say(format!(
                "band {} to {} degrees: {used} stars, {} quads, {:.1} s",
                band.dmin,
                band.dmax,
                found.len(),
                started.elapsed().as_secs_f64()
            ));
            summary.push(BandSummary {
                dmin: band.dmin,
                dmax: band.dmax,
                stars: used as u64,
                quads: found.len() as u64,
            });
            quads.extend(found);
        }
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        say("hashing the quads".to_owned());
        let mut entries: Vec<Entry> = quads
            .par_iter()
            .filter_map(|q| {
                let basis = Basis::at(stars[q[0] as usize].v());
                let points: [P2; 4] = q.map(|i| basis.plane(&stars[i as usize].v()));
                canonical(points).map(|(code, _)| Entry {
                    code: quantize(&code),
                    stars: *q,
                })
            })
            .collect();
        drop(quads);
        entries.par_sort_unstable_by_key(entry_bucket);
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        Some(Self::finish(stars, entries, gmax, summary))
    }

    /// The fields this index covers, long side in degrees: from the smallest
    /// quad it holds to the largest (a quad spans 0.2 to 1.33 fields).
    pub fn coverage_deg(&self) -> Option<(f64, f64)> {
        let first = self.bands.first()?;
        let last = self.bands.last()?;
        let round = |x: f64| (x * 100.0).round() / 100.0;
        Some((
            round(first.dmin / QUAD_PER_FIELD.0),
            round(last.dmax / QUAD_PER_FIELD.1),
        ))
    }

    /// The hash offsets and the cone grid, from the stars and the sorted entries.
    fn finish(
        stars: Vec<IndexStar>,
        entries: Vec<Entry>,
        gmax: f32,
        bands: Vec<BandSummary>,
    ) -> Self {
        let mut offsets = vec![0u32; BUCKETS.pow(4) + 1];
        for e in &entries {
            offsets[entry_bucket(e) + 1] += 1;
        }
        for i in 1..offsets.len() {
            offsets[i] += offsets[i - 1];
        }
        let ids: Vec<u32> = (0..stars.len() as u32).collect();
        let all = Grid::new(&ids, &stars, 1.0);
        Self {
            stars,
            all,
            entries,
            offsets,
            gmax,
            bands,
        }
    }

    pub fn star_count(&self) -> usize {
        self.stars.len()
    }

    pub fn quad_count(&self) -> usize {
        self.entries.len()
    }

    /// The faintest catalogue magnitude of the stars.
    pub fn gmax(&self) -> f32 {
        self.gmax
    }

    pub fn bands(&self) -> &[BandSummary] {
        &self.bands
    }

    /// Bytes the index holds in memory.
    pub fn memory_bytes(&self) -> usize {
        self.stars.len() * std::mem::size_of::<IndexStar>()
            + self.entries.len() * std::mem::size_of::<Entry>()
            + self.offsets.len() * 4
    }

    /// The stars of every quad whose code is within `tol` of `code`.
    pub(crate) fn lookup(&self, code: &[f64; 4], tol: f64, out: &mut Vec<[u32; 4]>) {
        out.clear();
        let mut options: [Vec<usize>; 4] = Default::default();
        for (k, choices) in options.iter_mut().enumerate() {
            let x = code[k] * BUCKETS as f64;
            let b = quad::cell(code[k]);
            choices.push(b);
            let frac = x - b as f64;
            let t = tol * BUCKETS as f64;
            if frac < t && b > 0 {
                choices.push(b - 1);
            }
            if frac > 1.0 - t && b + 1 < BUCKETS {
                choices.push(b + 1);
            }
        }
        for &b0 in &options[0] {
            for &b1 in &options[1] {
                for &b2 in &options[2] {
                    for &b3 in &options[3] {
                        let b = ((b0 * BUCKETS + b1) * BUCKETS + b2) * BUCKETS + b3;
                        let range = self.offsets[b] as usize..self.offsets[b + 1] as usize;
                        for e in &self.entries[range] {
                            let c = dequantize(&e.code);
                            if (0..4).all(|k| (c[k] - code[k]).abs() <= tol) {
                                out.push(e.stars);
                            }
                        }
                    }
                }
            }
        }
    }

    /// The unit vector of a star of the index.
    pub(crate) fn star(&self, id: u32) -> V3 {
        self.stars[id as usize].v()
    }
}

// ------------------------------------------------------------------- files

fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

/// Reads `count` records of `size` bytes in chunks, parsing each with `f`.
fn read_records<R: Read, T>(
    reader: &mut R,
    count: usize,
    size: usize,
    mut f: impl FnMut(&[u8]) -> T,
) -> io::Result<Vec<T>> {
    let mut out = Vec::with_capacity(count);
    let per_chunk = (4usize << 20) / size;
    let mut buffer = vec![0u8; per_chunk * size];
    let mut left = count;
    while left > 0 {
        let n = left.min(per_chunk);
        reader.read_exact(&mut buffer[..n * size])?;
        out.extend(buffer[..n * size].chunks_exact(size).map(&mut f));
        left -= n;
    }
    Ok(out)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

/// Bytes of the fixed header: magic, version, G limit, band, star and entry counts.
const HEADER_BYTES: u64 = 8 + 4 + 4 + 4 + 8 + 8;

/// File length an index with these counts must have, or `None` on overflow.
fn expected_length(n_bands: u64, n_stars: u64, n_entries: u64) -> Option<u64> {
    HEADER_BYTES
        .checked_add(n_bands.checked_mul(32)?)?
        .checked_add(n_stars.checked_mul(16)?)?
        .checked_add(n_entries.checked_mul(24)?)
}

impl Index {
    /// The partial file a save writes before it is renamed to `path`: the
    /// full file name plus `.tmp`, so two outputs never share one.
    pub fn temporary_path(path: &Path) -> std::path::PathBuf {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".tmp");
        path.with_file_name(name)
    }

    /// Writes the index to `path`, atomically: a fresh partial file is
    /// renamed into place, and removed again if writing fails.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let temporary = Self::temporary_path(path);
        // A partial file left by an interrupted earlier save is replaced.
        let _ = std::fs::remove_file(&temporary);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = self
            .write_to(file)
            .and_then(|()| std::fs::rename(&temporary, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    fn write_to(&self, file: File) -> io::Result<()> {
        let mut w = BufWriter::with_capacity(8 << 20, file);
        w.write_all(MAGIC)?;
        w.write_all(&VERSION.to_le_bytes())?;
        w.write_all(&self.gmax.to_le_bytes())?;
        w.write_all(&(self.bands.len() as u32).to_le_bytes())?;
        w.write_all(&(self.stars.len() as u64).to_le_bytes())?;
        w.write_all(&(self.entries.len() as u64).to_le_bytes())?;
        for b in &self.bands {
            w.write_all(&b.dmin.to_le_bytes())?;
            w.write_all(&b.dmax.to_le_bytes())?;
            w.write_all(&b.stars.to_le_bytes())?;
            w.write_all(&b.quads.to_le_bytes())?;
        }
        for s in &self.stars {
            for v in s.u {
                w.write_all(&v.to_le_bytes())?;
            }
            w.write_all(&s.g.to_le_bytes())?;
        }
        for e in &self.entries {
            for c in e.code {
                w.write_all(&c.to_le_bytes())?;
            }
            for s in e.stars {
                w.write_all(&s.to_le_bytes())?;
            }
        }
        w.flush()
    }

    /// Reads an index written by [`Index::save`].
    pub fn load(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut r = BufReader::with_capacity(8 << 20, file);
        let mut head = [0u8; HEADER_BYTES as usize];
        r.read_exact(&mut head)?;
        if &head[..8] != MAGIC {
            return Err(bad("not a fulgur-solve index"));
        }
        if u32_at(&head, 8) != VERSION {
            return Err(bad("index version not supported; build it again"));
        }
        let gmax = f32_at(&head, 12);
        let n_bands = u32_at(&head, 16);
        let n_stars = u64::from_le_bytes(head[20..28].try_into().expect("8 bytes"));
        let n_entries = u64::from_le_bytes(head[28..36].try_into().expect("8 bytes"));
        if n_bands > 64 || n_stars > u64::from(u32::MAX) || n_entries > u64::from(u32::MAX) * 2 {
            return Err(bad("index header is not valid"));
        }
        // Checked before any record is allocated: a damaged or half-written
        // file is refused instead of asking for memory its counts claim.
        if expected_length(u64::from(n_bands), n_stars, n_entries) != Some(length) {
            return Err(bad("index file is incomplete or damaged; build it again"));
        }
        let (n_bands, n_stars, n_entries) =
            (n_bands as usize, n_stars as usize, n_entries as usize);
        let bands = read_records(&mut r, n_bands, 32, |b| BandSummary {
            dmin: f64::from_le_bytes(b[0..8].try_into().expect("8 bytes")),
            dmax: f64::from_le_bytes(b[8..16].try_into().expect("8 bytes")),
            stars: u64::from_le_bytes(b[16..24].try_into().expect("8 bytes")),
            quads: u64::from_le_bytes(b[24..32].try_into().expect("8 bytes")),
        })?;
        let stars = read_records(&mut r, n_stars, 16, |b| IndexStar {
            u: [f32_at(b, 0), f32_at(b, 4), f32_at(b, 8)],
            g: f32_at(b, 12),
        })?;
        let entries = read_records(&mut r, n_entries, 24, |b| Entry {
            code: std::array::from_fn(|k| {
                u16::from_le_bytes(b[2 * k..2 * k + 2].try_into().expect("2 bytes"))
            }),
            stars: std::array::from_fn(|k| u32_at(b, 8 + 4 * k)),
        })?;
        let mut extra = [0u8; 1];
        if r.read(&mut extra)? != 0 {
            return Err(bad("index file has trailing data"));
        }
        if entries
            .iter()
            .any(|e| e.stars.iter().any(|&s| s as usize >= n_stars))
        {
            return Err(bad("index refers to a star it does not hold"));
        }
        if entries
            .windows(2)
            .any(|w| entry_bucket(&w[0]) > entry_bucket(&w[1]))
        {
            return Err(bad("index entries are not in order"));
        }
        Ok(Self::finish(stars, entries, gmax, bands))
    }
}
