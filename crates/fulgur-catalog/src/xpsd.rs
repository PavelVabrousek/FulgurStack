//! Reader of PixInsight point source databases (XPSD 1.0), such as the Gaia
//! DR3 files PixInsight keeps on disk. The layout follows the public PCL
//! headers `StarDatabaseFile.h` and `GaiaDatabaseFile.h`:
//!
//! - an 8-byte signature `XPSD0100`, a little-endian u32 header length, four
//!   reserved bytes, then an XML header (metadata, magnitude range, data
//!   position, compression, index trees);
//! - per tree a quadtree of 48-byte nodes: bounds (x0, y0, x1, y1) in the
//!   tree's projection, then four child indices or a leaf block reference
//!   (63-bit offset from the data position, leaf flag, sizes);
//! - leaf blocks of 32-byte Gaia records, LZ4-compressed after byte shuffling.
//!
//! Files are only read; nothing is cached or written.
use fulgur_pipeline::sky::catalogue::GaiaStar;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Projection {
    Equirectangular,
    TransverseEquirectangular,
    AzimuthalEquidistant,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    /// Child indices nw, ne, sw, se; or leaf offset (with flag) and sizes.
    raw: [u8; 16],
}

impl Node {
    fn word(&self, k: usize) -> u32 {
        u32::from_le_bytes(self.raw[4 * k..4 * k + 4].try_into().unwrap())
    }
    fn is_leaf(&self) -> bool {
        u64::from_le_bytes(self.raw[..8].try_into().unwrap()) & 0x8000_0000_0000_0000 != 0
    }
    fn block_offset(&self) -> u64 {
        u64::from_le_bytes(self.raw[..8].try_into().unwrap()) & 0x7FFF_FFFF_FFFF_FFFF
    }
}

#[derive(Clone, Debug)]
struct Tree {
    projection: Projection,
    center: (f64, f64),
    nodes: Vec<Node>,
}

/// Unit vector of a sky position in degrees.
fn vector(lon: f64, lat: f64) -> [f64; 3] {
    let (sl, cl) = lon.to_radians().sin_cos();
    let (sb, cb) = lat.to_radians().sin_cos();
    [cb * cl, cb * sl, sb]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Angle between two vectors (PCL `Angle3D`).
fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(cross(a, b)).atan2(dot(a, b))
}

/// Signed angle from `a` to `b` about `n` (PCL `Angle3D( v, n )`).
fn signed_angle(a: [f64; 3], b: [f64; 3], n: [f64; 3]) -> f64 {
    let c = cross(a, b);
    let s = norm(c).atan2(dot(a, b));
    if dot(c, n) < 0. { -s } else { s }
}

fn distance(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
    angle(vector(lon1, lat1), vector(lon2, lat2))
}

fn cross_track(lon: f64, lat: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
    if lon == a.0 && lat == a.1 {
        return 0.;
    }
    let p = vector(lon, lat);
    let c = cross(vector(a.0, a.1), vector(b.0, b.1));
    angle(c, p) - std::f64::consts::FRAC_PI_2
}

fn within_extent(lon: f64, lat: f64, a: (f64, f64), b: (f64, f64)) -> bool {
    if a == b {
        return lon == a.0 && lat == a.1;
    }
    let (n0, n1, n2) = (vector(lon, lat), vector(a.0, a.1), vector(b.0, b.1));
    let (d10, d12) = (sub(n0, n1), sub(n2, n1));
    let (d20, d21) = (sub(n0, n2), sub(n1, n2));
    dot(d10, d12) >= 0. && dot(d20, d21) >= 0. && dot(n0, n1) >= 0. && dot(n0, n2) >= 0.
}

fn in_region(lon: f64, lat: f64, c: [(f64, f64); 4]) -> bool {
    let p = vector(lon, lat);
    let v: Vec<[f64; 3]> = c.iter().map(|q| sub(p, vector(q.0, q.1))).collect();
    (signed_angle(v[0], v[1], p)
        + signed_angle(v[1], v[2], p)
        + signed_angle(v[2], v[3], p)
        + signed_angle(v[3], v[0], p))
    .abs()
        > std::f64::consts::PI
}

impl Tree {
    fn unproject(&self, x: f64, y: f64) -> (f64, f64) {
        match self.projection {
            Projection::Equirectangular => (x + self.center.0, y),
            Projection::AzimuthalEquidistant => {
                let (xr, yr) = (x.to_radians(), y.to_radians());
                let mut ra = xr.atan2(yr).to_degrees();
                if ra < 0. {
                    ra += 360.;
                }
                let mut dec = xr.hypot(yr).cos().asin().to_degrees();
                if self.center.1 < 0. {
                    dec = -dec;
                }
                (ra, dec)
            }
            Projection::TransverseEquirectangular => {
                let (sx, cx) = x.to_radians().sin_cos();
                let (sy, cy) = (y + 90.).to_radians().sin_cos();
                let mut ra = sx.atan2(cx * cy).to_degrees();
                if ra < 0. {
                    ra += 360.;
                }
                let mut dec = (sy * cx).asin().to_degrees();
                if self.center.1 < 0. {
                    dec = -dec;
                }
                (ra, dec)
            }
        }
    }

    fn corners(&self, n: &Node) -> [(f64, f64); 4] {
        [
            self.unproject(n.x0, n.y0),
            self.unproject(n.x1, n.y0),
            self.unproject(n.x1, n.y1),
            self.unproject(n.x0, n.y1),
        ]
    }

    /// PCL's `IntersectsNodeRegion`.
    fn intersects(&self, ra: f64, dec: f64, r: f64, n: &Node) -> bool {
        let c = self.corners(n);
        in_region(ra, dec, c)
            || c.iter().any(|q| distance(ra, dec, q.0, q.1) < r)
            || (0..4).any(|k| {
                let (a, b) = (c[k], c[(k + 1) % 4]);
                within_extent(ra, dec, a, b) && cross_track(ra, dec, a, b) < r
            })
    }
}

/// One XPSD file of a point source database.
pub struct XpsdFile {
    path: PathBuf,
    /// Database identifier from the header, e.g. "GaiaDR3".
    pub identifier: String,
    /// Magnitude range of the file: (low, high].
    pub magnitude: (f64, f64),
    /// Database version from the header, e.g. "1.0.0".
    pub version: String,
    /// Header statistics, when present: sources, nodes, leaves.
    statistics: Option<(u64, usize, usize)>,
    file_bytes: u64,
    data_position: u64,
    shuffled: bool,
    item_size: usize,
    trees: Vec<Tree>,
}

fn attribute<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Result<&'a str, String> {
    node.attribute(name)
        .ok_or_else(|| format!("XPSD header lacks attribute {name}"))
}

fn pair(text: &str) -> Result<(f64, f64), String> {
    let mut it = text.split(',').map(|v| v.trim().parse::<f64>());
    match (it.next(), it.next()) {
        (Some(Ok(a)), Some(Ok(b))) => Ok((a, b)),
        _ => Err(format!("XPSD header: bad pair '{text}'")),
    }
}

impl XpsdFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file_bytes = f.metadata().map_err(|e| e.to_string())?.len();
        let mut head = [0u8; 16];
        f.read_exact(&mut head).map_err(|e| e.to_string())?;
        if &head[..8] != b"XPSD0100" {
            return Err(format!("{}: not an XPSD 1.0 file", path.display()));
        }
        let len = u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
        let mut xml = vec![0u8; len];
        f.read_exact(&mut xml).map_err(|e| e.to_string())?;
        let xml = String::from_utf8(xml).map_err(|e| e.to_string())?;
        let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
        let root = doc.root_element();
        let identifier = root
            .descendants()
            .find(|n| n.has_tag_name("DatabaseIdentifier"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string();
        let version = root
            .descendants()
            .find(|n| n.has_tag_name("DatabaseVersion"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string();
        let statistics = root
            .children()
            .find(|n| n.has_tag_name("Statistics"))
            .and_then(|s| {
                Some((
                    s.attribute("totalSources")?.parse().ok()?,
                    s.attribute("totalNodes")?.parse().ok()?,
                    s.attribute("totalLeaves")?.parse().ok()?,
                ))
            });
        let data = root
            .children()
            .find(|n| n.has_tag_name("Data"))
            .ok_or("XPSD header lacks Data")?;
        let magnitude = pair(attribute(data, "magnitudeRange")?)?;
        let data_position: u64 = attribute(data, "position")?
            .parse()
            .map_err(|e| format!("{e}"))?;
        let compression = data.attribute("compression").unwrap_or("");
        let (codec, shuffled) = match compression.split_once('+') {
            Some((c, "sh")) => (c, true),
            _ => (compression, false),
        };
        if !(codec.is_empty() || codec.starts_with("lz4")) {
            return Err(format!("unsupported XPSD compression '{compression}'"));
        }
        let item_size: usize = attribute(data, "itemSize")?
            .parse()
            .map_err(|e| format!("{e}"))?;
        let mut trees = vec![];
        for t in root.children().filter(|n| n.has_tag_name("Tree")) {
            let projection = match attribute(t, "projection")? {
                "Equirectangular" => Projection::Equirectangular,
                "TransverseEquirectangular" => Projection::TransverseEquirectangular,
                "AzimuthalEquidistant" => Projection::AzimuthalEquidistant,
                other => return Err(format!("unknown XPSD projection '{other}'")),
            };
            let center = pair(attribute(t, "center")?)?;
            let position: u64 = attribute(t, "rootPosition")?
                .parse()
                .map_err(|e| format!("{e}"))?;
            let count: usize = attribute(t, "nodeCount")?
                .parse()
                .map_err(|e| format!("{e}"))?;
            f.seek(SeekFrom::Start(position))
                .map_err(|e| e.to_string())?;
            let mut bytes = vec![0u8; count * 48];
            f.read_exact(&mut bytes).map_err(|e| e.to_string())?;
            let nodes = bytes
                .chunks_exact(48)
                .map(|c| {
                    let d = |k: usize| f64::from_le_bytes(c[8 * k..8 * k + 8].try_into().unwrap());
                    Node {
                        x0: d(0),
                        y0: d(1),
                        x1: d(2),
                        y1: d(3),
                        raw: c[32..48].try_into().unwrap(),
                    }
                })
                .collect();
            trees.push(Tree {
                projection,
                center,
                nodes,
            });
        }
        Ok(Self {
            path: path.to_path_buf(),
            identifier,
            magnitude,
            version,
            statistics,
            file_bytes,
            data_position,
            shuffled,
            item_size,
            trees,
        })
    }

    fn block(&self, f: &mut File, node: &Node) -> Result<Vec<u8>, String> {
        let (size, compressed) = (node.word(2) as usize, node.word(3) as usize);
        f.seek(SeekFrom::Start(self.data_position + node.block_offset()))
            .map_err(|e| e.to_string())?;
        let mut raw = vec![0u8; compressed];
        f.read_exact(&mut raw).map_err(|e| e.to_string())?;
        if compressed == size {
            return Ok(raw);
        }
        let packed = lz4_flex::block::decompress(&raw, size)
            .map_err(|e| format!("{}: LZ4 block: {e}", self.path.display()))?;
        if !self.shuffled || self.item_size <= 1 {
            return Ok(packed);
        }
        // Byte shuffling: byte j of every item is stored together.
        let n = size / self.item_size;
        let mut out = packed.clone();
        for i in 0..n {
            for j in 0..self.item_size {
                out[i * self.item_size + j] = packed[j * n + i];
            }
        }
        Ok(out)
    }

    /// Gaia stars within `radius` degrees of (ra, dec) and with G in
    /// [g_low, g_high], positions at the Gaia DR3 epoch.
    pub fn search(
        &self,
        ra: f64,
        dec: f64,
        radius: f64,
        g_low: f64,
        g_high: f64,
    ) -> Result<Vec<GaiaStar>, String> {
        let mut out = vec![];
        if g_high <= self.magnitude.0 || g_low > self.magnitude.1 {
            return Ok(out);
        }
        let r = radius.to_radians();
        let mut f = File::open(&self.path).map_err(|e| e.to_string())?;
        for tree in &self.trees {
            let mut stack = vec![0usize];
            while let Some(i) = stack.pop() {
                let node = &tree.nodes[i];
                if !tree.intersects(ra, dec, r, node) {
                    continue;
                }
                if !node.is_leaf() {
                    for k in 0..4 {
                        let c = node.word(k) as usize;
                        if c != 0 {
                            stack.push(c);
                        }
                    }
                    continue;
                }
                let block = self.block(&mut f, node)?;
                for s in block.chunks_exact(32) {
                    let u32_at = |k: usize| u32::from_le_bytes(s[k..k + 4].try_into().unwrap());
                    let f32_at = |k: usize| f32::from_le_bytes(s[k..k + 4].try_into().unwrap());
                    let u16_at = |k: usize| u16::from_le_bytes(s[k..k + 2].try_into().unwrap());
                    let g = 0.001 * u16_at(20) as f64 - 1.5;
                    if g < g_low || g > g_high {
                        continue;
                    }
                    let x = node.x0 + u32_at(0) as f64 / 3600. / 1000. / 500.;
                    let y = node.y0 + u32_at(4) as f64 / 3600. / 1000. / 500.;
                    let (mut sra, sdec) = tree.unproject(x, y);
                    let dra = i16::from_le_bytes(s[26..28].try_into().unwrap());
                    if dra != 0 {
                        sra = (sra + dra as f64 / 3600. / 1000. / 100.).rem_euclid(360.);
                    }
                    if distance(ra, dec, sra, sdec) >= r {
                        continue;
                    }
                    out.push(GaiaStar {
                        // XPSD keeps no Gaia source identifier.
                        source_id: 0,
                        ra: sra,
                        dec: sdec,
                        pmra: f32_at(12) as f64,
                        pmdec: f32_at(16) as f64,
                        g,
                    });
                }
            }
        }
        Ok(out)
    }
}

/// Integrity of one file, from its index alone and optionally from a sample
/// of decoded blocks.
#[derive(Clone, Debug, serde::Serialize)]
pub struct FileCheck {
    pub file: String,
    pub identifier: String,
    pub version: String,
    pub magnitude: (f64, f64),
    pub sources: u64,
    pub nodes: usize,
    pub leaves: usize,
    pub bytes: u64,
    pub blocks_decoded: usize,
    pub problems: Vec<String>,
}

impl XpsdFile {
    /// Checks that the index agrees with the header statistics, that every
    /// child index and data block lies inside the file, and (with `deep`)
    /// that a sample of blocks decompresses into plausible Gaia records.
    pub fn integrity(&self, deep: bool) -> FileCheck {
        let mut problems = vec![];
        let (mut nodes, mut leaves, mut sources) = (0usize, 0usize, 0u64);
        let mut sample = vec![];
        if self.data_position > self.file_bytes {
            problems.push("data position lies beyond the end of the file".into());
        }
        for (t, tree) in self.trees.iter().enumerate() {
            nodes += tree.nodes.len();
            for (i, n) in tree.nodes.iter().enumerate() {
                if !n.is_leaf() {
                    for k in 0..4 {
                        if n.word(k) as usize >= tree.nodes.len() {
                            problems
                                .push(format!("tree {t} node {i}: child index outside the tree"));
                        }
                    }
                    continue;
                }
                leaves += 1;
                let (size, compressed) = (n.word(2) as u64, n.word(3) as u64);
                if size % self.item_size as u64 != 0 {
                    problems.push(format!(
                        "tree {t} node {i}: block of {size} bytes is no whole number of records"
                    ));
                }
                sources += size / self.item_size as u64;
                if self.data_position + n.block_offset() + compressed > self.file_bytes {
                    problems.push(format!("tree {t} node {i}: data block runs past the end of the file (file truncated?)"));
                }
                sample.push((t, i));
            }
        }
        if let Some((s, n, l)) = self.statistics {
            if s != sources {
                problems.push(format!(
                    "header promises {s} sources, the index holds {sources}"
                ));
            }
            if n != nodes {
                problems.push(format!(
                    "header promises {n} index nodes, the file holds {nodes}"
                ));
            }
            if l != leaves {
                problems.push(format!(
                    "header promises {l} leaves, the index holds {leaves}"
                ));
            }
        }
        problems.truncate(20);
        let mut decoded = 0;
        if deep && problems.is_empty() {
            // About 200 blocks spread over the whole file.
            let step = (sample.len() / 200).max(1);
            match File::open(&self.path) {
                Ok(mut f) => {
                    for &(t, i) in sample.iter().step_by(step) {
                        let node = &self.trees[t].nodes[i];
                        match self.block(&mut f, node) {
                            Ok(block) => {
                                decoded += 1;
                                for s in block.chunks_exact(32) {
                                    let g = 0.001
                                        * u16::from_le_bytes(s[20..22].try_into().unwrap()) as f64
                                        - 1.5;
                                    let y = node.y0
                                        + u32::from_le_bytes(s[4..8].try_into().unwrap()) as f64
                                            / 1.8e9;
                                    if !(self.magnitude.0 - 0.01..=self.magnitude.1 + 0.01)
                                        .contains(&g)
                                        || !y.is_finite()
                                    {
                                        problems.push(format!(
                                            "tree {t} node {i}: decoded record with G {g:.3} outside the file's range"
                                        ));
                                        break;
                                    }
                                }
                            }
                            Err(e) => problems.push(format!("tree {t} node {i}: {e}")),
                        }
                        if problems.len() >= 20 {
                            break;
                        }
                    }
                }
                Err(e) => problems.push(e.to_string()),
            }
        }
        FileCheck {
            file: self
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            identifier: self.identifier.clone(),
            version: self.version.clone(),
            magnitude: self.magnitude,
            sources,
            nodes,
            leaves,
            bytes: self.file_bytes,
            blocks_decoded: decoded,
            problems,
        }
    }
}

/// Result of checking a local Gaia installation.
#[derive(Clone, Debug, serde::Serialize)]
pub struct InstallationCheck {
    pub folder: String,
    /// "complete"; "incomplete" (readable, but gaps in the magnitude or sky
    /// coverage); "damaged" (unreadable or inconsistent files); "missing"
    /// (no Gaia star catalogue files in the folder).
    pub verdict: &'static str,
    pub identifier: Option<String>,
    pub version: Option<String>,
    pub magnitude: Option<(f64, f64)>,
    pub sources: u64,
    pub files: Vec<FileCheck>,
    pub problems: Vec<String>,
    pub notes: Vec<String>,
    /// Sample cones: label, stars, seconds.
    pub probes: Vec<(String, usize, f64)>,
    pub seconds: f64,
}

/// Checks the Gaia XPSD files in `dir`: every file readable and internally
/// consistent, one database and version, and magnitude ranges that join
/// without gaps or overlaps from the brightest stars on. `deep` also
/// decompresses a sample of blocks in every file.
pub fn check_installation(dir: &Path, deep: bool) -> InstallationCheck {
    let start = std::time::Instant::now();
    let mut problems = vec![];
    let mut notes = vec![];
    let mut files = vec![];
    let mut opened = vec![];
    match std::fs::read_dir(dir) {
        Err(e) => problems.push(format!("{}: {e}", dir.display())),
        Ok(entries) => {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("xpsd"))
                })
                .collect();
            paths.sort();
            for p in paths {
                match XpsdFile::open(&p) {
                    Ok(f) if f.identifier.starts_with("Gaia") && !f.identifier.contains("SP") => {
                        opened.push(f);
                    }
                    Ok(f) => notes.push(format!(
                        "{}: database '{}' is not a Gaia point source catalogue; ignored",
                        p.display(),
                        f.identifier
                    )),
                    Err(e) => match identifier_of(&p) {
                        // Another PixInsight database, such as Gaia DR3SP
                        // (spectrophotometry), is no star catalogue.
                        Some(id) if !id.starts_with("Gaia") || id.contains("SP") => notes.push(format!(
                            "{}: database '{id}' is not the Gaia star catalogue (spectra or another catalogue); choose the folder with the Gaia DR3 files",
                            p.display()
                        )),
                        _ => problems.push(format!("{}: {e}", p.display())),
                    },
                }
            }
        }
    }
    if opened.is_empty() && problems.is_empty() {
        problems.push(format!("no Gaia XPSD files in {}", dir.display()));
    }
    opened.sort_by(|a, b| a.magnitude.0.total_cmp(&b.magnitude.0));
    let identifier = opened.first().map(|f| f.identifier.clone());
    let version = opened.first().map(|f| f.version.clone());
    for f in &opened {
        if Some(&f.identifier) != identifier.as_ref() || Some(&f.version) != version.as_ref() {
            problems.push(format!(
                "{}: {} {} mixed with {} {}",
                f.path.display(),
                f.identifier,
                f.version,
                identifier.clone().unwrap_or_default(),
                version.clone().unwrap_or_default()
            ));
        }
    }
    let mut incomplete = false;
    for w in opened.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let gap = b.magnitude.0 - a.magnitude.1;
        if gap > 0.005 {
            incomplete = true;
            problems.push(format!(
                "magnitudes G {:.2}..{:.2} are missing between {} and {}",
                a.magnitude.1,
                b.magnitude.0,
                a.path.file_name().unwrap_or_default().to_string_lossy(),
                b.path.file_name().unwrap_or_default().to_string_lossy()
            ));
        } else if gap < -0.005 {
            problems.push(format!(
                "{} and {} overlap in magnitude (duplicate file?)",
                a.path.file_name().unwrap_or_default().to_string_lossy(),
                b.path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
    }
    if let Some(first) = opened.first() {
        if first.magnitude.0 > -1.0 {
            incomplete = true;
            problems.push(format!(
                "the brightest stars (G below {:.2}) are missing",
                first.magnitude.0
            ));
        }
    }
    let mut damaged = !problems.is_empty() && !incomplete;
    for f in &opened {
        let c = f.integrity(deep);
        if !c.problems.is_empty() {
            damaged = true;
            problems.push(format!(
                "{}: {} problem(s), first: {}",
                c.file,
                c.problems.len(),
                c.problems[0]
            ));
        }
        files.push(c);
    }
    let mut probes = vec![];
    if !damaged && !opened.is_empty() {
        let cat = XpsdCatalogue { files: opened };
        for (label, ra, dec) in [
            ("equator RA 0", 0.0, 0.0),
            ("Milky Way, Cygnus", 305.0, 40.0),
            ("south, RA 180 Dec -30", 180.0, -30.0),
            ("north celestial pole", 0.0, 89.5),
            ("south celestial pole", 0.0, -89.5),
        ] {
            let t = std::time::Instant::now();
            match cat.cone(ra, dec, 1.0, 14.0) {
                Ok(s) => {
                    if s.is_empty() {
                        problems.push(format!(
                            "no stars to G 14 within 1 degree of {label}: the sky is not covered everywhere"
                        ));
                        incomplete = true;
                    }
                    probes.push((label.to_string(), s.len(), t.elapsed().as_secs_f64()));
                }
                Err(e) => {
                    damaged = true;
                    problems.push(format!("{label}: {e}"));
                }
            }
        }
        opened = cat.files;
    }
    let magnitude = match (opened.first(), opened.last()) {
        (Some(a), Some(b)) => Some((a.magnitude.0, b.magnitude.1)),
        _ => None,
    };
    let verdict = if opened.is_empty() {
        "missing"
    } else if damaged {
        "damaged"
    } else if incomplete {
        "incomplete"
    } else {
        "complete"
    };
    InstallationCheck {
        folder: dir.display().to_string(),
        verdict,
        identifier,
        version,
        magnitude,
        sources: files.iter().map(|f| f.sources).sum(),
        files,
        problems,
        notes,
        probes,
        seconds: start.elapsed().as_secs_f64(),
    }
}

/// The database identifier in an XPSD header, without opening its index.
pub fn identifier_of(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let mut head = [0u8; 16];
    f.read_exact(&mut head).ok()?;
    if &head[..8] != b"XPSD0100" {
        return None;
    }
    let len = u32::from_le_bytes(head[8..12].try_into().ok()?) as usize;
    let mut xml = vec![0u8; len.min(1 << 20)];
    f.read_exact(&mut xml).ok()?;
    let xml = String::from_utf8(xml).ok()?;
    let a = xml.find("<DatabaseIdentifier>")? + "<DatabaseIdentifier>".len();
    let b = xml[a..].find('<')? + a;
    Some(xml[a..b].trim().to_string())
}

/// All Gaia DR3 XPSD files of a folder, ordered by magnitude range.
pub struct XpsdCatalogue {
    pub files: Vec<XpsdFile>,
}

impl XpsdCatalogue {
    pub fn open(dir: &Path) -> Result<Self, String> {
        let mut files = vec![];
        for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("xpsd"))
            {
                let f = XpsdFile::open(&p)?;
                if f.identifier.starts_with("Gaia") {
                    files.push(f);
                }
            }
        }
        if files.is_empty() {
            return Err(format!("no Gaia XPSD files in {}", dir.display()));
        }
        files.sort_by(|a, b| a.magnitude.0.total_cmp(&b.magnitude.0));
        Ok(Self { files })
    }

    /// Faintest magnitude the files cover.
    pub fn limit(&self) -> f64 {
        self.files
            .iter()
            .map(|f| f.magnitude.1)
            .fold(f64::MIN, f64::max)
    }

    /// Stars brighter than `g_max` within `radius` degrees, brightest first.
    pub fn cone(
        &self,
        ra: f64,
        dec: f64,
        radius: f64,
        g_max: f64,
    ) -> Result<Vec<GaiaStar>, String> {
        let mut out = vec![];
        for f in &self.files {
            out.extend(f.search(ra, dec, radius, -5., g_max)?);
        }
        out.sort_by(|a, b| a.g.total_cmp(&b.g));
        Ok(out)
    }

    /// Every star brighter than `g_max` all over the sky as (RA, Dec, G),
    /// positions at the Gaia DR3 epoch, for the blind solver's index. The
    /// files are read one after another; `cancel` is looked at between them.
    pub fn all_sky(
        &self,
        g_max: f64,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<(f64, f64, f32)>, String> {
        let mut out = vec![];
        for f in &self.files {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            if f.magnitude.0 >= g_max {
                continue;
            }
            out.extend(
                f.search(0., 0., 180., -5., g_max)?
                    .into_iter()
                    .map(|s| (s.ra, s.dec, s.g as f32)),
            );
        }
        Ok(out)
    }
}
