//! Points on the sphere, tangent planes and a cell grid for cone queries.
use std::collections::HashMap;

pub(crate) const RAD: f64 = std::f64::consts::PI / 180.0;

pub(crate) type V3 = [f64; 3];

pub(crate) fn unit(ra: f64, dec: f64) -> V3 {
    let (r, d) = (ra * RAD, dec * RAD);
    [d.cos() * r.cos(), d.cos() * r.sin(), d.sin()]
}

pub(crate) fn dot(a: &V3, b: &V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: &V3, b: &V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn normalized(a: &V3) -> V3 {
    let n = dot(a, a).sqrt();
    [a[0] / n, a[1] / n, a[2] / n]
}

/// Angle between two unit vectors, in degrees.
pub(crate) fn separation_deg(a: &V3, b: &V3) -> f64 {
    let c = cross(a, b);
    dot(&c, &c).sqrt().atan2(dot(a, b)) / RAD
}

/// Right ascension in [0, 360) and declination, in degrees.
pub(crate) fn radec(v: &V3) -> (f64, f64) {
    (
        v[1].atan2(v[0]).to_degrees().rem_euclid(360.0),
        v[2].clamp(-1.0, 1.0).asin().to_degrees(),
    )
}

/// A catalogue star kept by the index: a unit vector and its magnitude.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndexStar {
    pub(crate) u: [f32; 3],
    /// Gaia G magnitude.
    pub g: f32,
}

impl IndexStar {
    /// A star at `ra`, `dec` (degrees) of magnitude `g`.
    pub fn new(ra: f64, dec: f64, g: f32) -> Self {
        let v = unit(ra, dec);
        Self {
            u: [v[0] as f32, v[1] as f32, v[2] as f32],
            g,
        }
    }

    pub(crate) fn v(&self) -> V3 {
        [
            f64::from(self.u[0]),
            f64::from(self.u[1]),
            f64::from(self.u[2]),
        ]
    }

    /// Right ascension and declination in degrees.
    pub fn radec(&self) -> (f64, f64) {
        radec(&self.v())
    }
}

/// The tangent plane at a point: east and north unit vectors.
pub(crate) struct Basis {
    a: V3,
    e: V3,
    n: V3,
}

impl Basis {
    pub(crate) fn at(a: V3) -> Self {
        let mut e = cross(&[0.0, 0.0, 1.0], &a);
        if dot(&e, &e) < 1e-18 {
            e = [1.0, 0.0, 0.0];
        }
        let e = normalized(&e);
        let n = cross(&a, &e);
        Self { a, e, n }
    }

    /// Gnomonic coordinates (radians) of a point.
    pub(crate) fn plane(&self, p: &V3) -> [f64; 2] {
        let c = dot(&self.a, p);
        [dot(&self.e, p) / c, dot(&self.n, p) / c]
    }

    /// The point at gnomonic coordinates `q`.
    pub(crate) fn sky(&self, q: [f64; 2]) -> V3 {
        normalized(&[
            self.a[0] + q[0] * self.e[0] + q[1] * self.n[0],
            self.a[1] + q[0] * self.e[1] + q[1] * self.n[1],
            self.a[2] + q[0] * self.e[2] + q[1] * self.n[2],
        ])
    }
}

/// Equal-height rows of about equal-width cells, for cone queries.
pub(crate) struct Grid {
    cs: f64,
    rows: i32,
    cells: HashMap<u64, Vec<u32>>,
}

fn row_cells(row: i32, cs: f64) -> i32 {
    let mid = -90.0 + (f64::from(row) + 0.5) * cs;
    ((360.0 * (mid * RAD).cos() / cs).floor() as i32).max(1)
}

impl Grid {
    pub(crate) fn key(row: i32, j: i32) -> u64 {
        ((row as u64) << 32) | u64::from(j as u32)
    }

    /// The cell of a position for cells of `cs` degrees.
    pub(crate) fn cell_of(ra: f64, dec: f64, cs: f64) -> (i32, i32) {
        let rows = (180.0 / cs).ceil() as i32;
        let row = (((dec + 90.0) / cs).floor() as i32).clamp(0, rows - 1);
        let n = row_cells(row, cs);
        let j = ((ra.rem_euclid(360.0) / (360.0 / f64::from(n))).floor() as i32).rem_euclid(n);
        (row, j)
    }

    pub(crate) fn new(ids: &[u32], stars: &[IndexStar], cs: f64) -> Self {
        let mut cells: HashMap<u64, Vec<u32>> = HashMap::new();
        for &i in ids {
            let (ra, dec) = stars[i as usize].radec();
            let (r, j) = Self::cell_of(ra, dec, cs);
            cells.entry(Self::key(r, j)).or_default().push(i);
        }
        Self {
            cs,
            rows: (180.0 / cs).ceil() as i32,
            cells,
        }
    }

    /// Stars within `r` degrees of the unit vector `c`, into `out`.
    pub(crate) fn near(&self, stars: &[IndexStar], c: &V3, r: f64, out: &mut Vec<u32>) {
        out.clear();
        let (ra, dec) = radec(c);
        let lo = (((dec - r + 90.0) / self.cs).floor() as i32).clamp(0, self.rows - 1);
        let hi = (((dec + r + 90.0) / self.cs).floor() as i32).clamp(0, self.rows - 1);
        let cos_r = (r * RAD).cos();
        for row in lo..=hi {
            let n = row_cells(row, self.cs);
            let (d0, d1) = (
                -90.0 + f64::from(row) * self.cs,
                -90.0 + f64::from(row + 1) * self.cs,
            );
            let cosd = (d0.abs().max(d1.abs()) * RAD).cos().max(1e-6);
            let dra = r / cosd;
            let width = 360.0 / f64::from(n);
            let (j0, j1) = (
                ((ra - dra) / width).floor() as i64,
                ((ra + dra) / width).floor() as i64,
            );
            let all = n == 1 || dra >= 180.0 || j1 - j0 + 1 >= i64::from(n);
            let columns: Vec<i32> = if all {
                (0..n).collect()
            } else {
                (j0..=j1)
                    .map(|j| j.rem_euclid(i64::from(n)) as i32)
                    .collect()
            };
            for j in columns {
                if let Some(ids) = self.cells.get(&Self::key(row, j)) {
                    for &i in ids {
                        if dot(c, &stars[i as usize].v()) >= cos_r {
                            out.push(i);
                        }
                    }
                }
            }
        }
    }
}
