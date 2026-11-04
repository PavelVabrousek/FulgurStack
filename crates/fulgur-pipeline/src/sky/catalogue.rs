//! Gaia DR3 stars and the gnomonic projection. Reading or downloading the
//! catalogue is the caller's business (plan phase 2).

/// Reference epoch of Gaia DR3 positions.
pub const GAIA_EPOCH: f64 = 2016.0;
const ARCSEC: f64 = 180. / std::f64::consts::PI * 3600.;

#[derive(Clone, Debug)]
pub struct GaiaStar {
    pub source_id: u64,
    pub ra: f64,
    pub dec: f64,
    /// mu_alpha* (mas/yr, already multiplied by cos dec) and mu_delta.
    pub pmra: f64,
    pub pmdec: f64,
    pub g: f64,
}

impl GaiaStar {
    /// ICRS position at a decimal-year epoch.
    pub fn at(&self, epoch: f64) -> (f64, f64) {
        let dt = epoch - GAIA_EPOCH;
        let dec = self.dec + self.pmdec * dt / 3.6e6;
        let ra = self.ra + self.pmra * dt / 3.6e6 / self.dec.to_radians().cos();
        (ra, dec)
    }
}

/// Gnomonic projection about (ra0, dec0) in arcseconds; xi east, eta north.
pub fn project(ra: f64, dec: f64, ra0: f64, dec0: f64) -> Option<[f64; 2]> {
    let (a, d, a0, d0) = (
        ra.to_radians(),
        dec.to_radians(),
        ra0.to_radians(),
        dec0.to_radians(),
    );
    let cos_c = d0.sin() * d.sin() + d0.cos() * d.cos() * (a - a0).cos();
    if cos_c <= 0.2 {
        return None;
    }
    let xi = d.cos() * (a - a0).sin() / cos_c;
    let eta = (d0.cos() * d.sin() - d0.sin() * d.cos() * (a - a0).cos()) / cos_c;
    Some([xi * ARCSEC, eta * ARCSEC])
}

pub fn deproject(p: [f64; 2], ra0: f64, dec0: f64) -> (f64, f64) {
    let (xi, eta) = (p[0] / ARCSEC, p[1] / ARCSEC);
    let (a0, d0) = (ra0.to_radians(), dec0.to_radians());
    let rho = xi.hypot(eta);
    if rho == 0. {
        return (ra0, dec0);
    }
    let c = rho.atan();
    let dec = (c.cos() * d0.sin() + eta * c.sin() * d0.cos() / rho).asin();
    let ra = a0 + (xi * c.sin()).atan2(rho * d0.cos() * c.cos() - eta * d0.sin() * c.sin());
    (ra.to_degrees().rem_euclid(360.), dec.to_degrees())
}

/// Angular distance in degrees.
pub fn separation(ra1: f64, dec1: f64, ra2: f64, dec2: f64) -> f64 {
    let (a1, d1, a2, d2) = (
        ra1.to_radians(),
        dec1.to_radians(),
        ra2.to_radians(),
        dec2.to_radians(),
    );
    let h = ((d2 - d1) / 2.).sin().powi(2) + d1.cos() * d2.cos() * ((a2 - a1) / 2.).sin().powi(2);
    (2. * h.sqrt().min(1.).asin()).to_degrees()
}
