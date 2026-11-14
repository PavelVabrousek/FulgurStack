//! Gaia DR3 stars and the gnomonic projection. Reading or downloading the
//! catalogue is the caller's business (plan phase 2).

/// Reference epoch of Gaia DR3 positions.
pub const GAIA_EPOCH: f64 = 2016.0;
const ARCSEC: f64 = 180. / std::f64::consts::PI * 3600.;

/// A DATE-OBS value as a decimal year, the epoch of the frame's star
/// positions; `None` when the value cannot be read. Callers use
/// [`GAIA_EPOCH`] then, so no proper motion is applied.
pub fn decimal_year(date_obs: &str) -> Option<f64> {
    crate::register::date_micros(date_obs).map(|t| 1970. + t as f64 / (365.25 * 86400. * 1e6))
}

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
/// Right ascension as hours, minutes and seconds: `01h 33m 50.9s`.
pub fn format_ra(ra_deg: f64) -> String {
    // Tenths of a second of time; 24 hours are 864 000 of them.
    let tenths = ((ra_deg.rem_euclid(360.0) / 15.0 * 36_000.0).round() as u64) % 864_000;
    let (hours, rest) = (tenths / 36_000, tenths % 36_000);
    let (minutes, tenths) = (rest / 600, rest % 600);
    format!(
        "{hours:02}h {minutes:02}m {:02}.{}s",
        tenths / 10,
        tenths % 10
    )
}

/// Declination as degrees, arcminutes and arcseconds: `+30° 39′ 36″`.
pub fn format_dec(dec_deg: f64) -> String {
    let arcsec = (dec_deg.abs() * 3600.0).round() as u64;
    let sign = if dec_deg < 0.0 && arcsec > 0 {
        '-'
    } else {
        '+'
    };
    format!(
        "{sign}{:02}° {:02}′ {:02}″",
        arcsec / 3600,
        arcsec % 3600 / 60,
        arcsec % 60
    )
}

/// A position for the owner to read: hours, minutes and seconds of right
/// ascension, degrees, arcminutes and arcseconds of declination, and the
/// degrees in brackets to copy: `RA 01h 33m 50.9s · Dec +30° 39′ 36″
/// (23.4621° · +30.6600°)`.
pub fn position_text(ra_deg: f64, dec_deg: f64) -> String {
    format!(
        "RA {} · Dec {} ({ra_deg:.4}° · {dec_deg:+.4}°)",
        format_ra(ra_deg),
        format_dec(dec_deg)
    )
}

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

#[cfg(test)]
mod position_text_tests {
    use super::{format_dec, format_ra, position_text};

    #[test]
    fn right_ascension_is_hours_minutes_seconds() {
        assert_eq!(format_ra(23.4621), "01h 33m 50.9s");
        assert_eq!(format_ra(0.0), "00h 00m 00.0s");
        assert_eq!(format_ra(180.0), "12h 00m 00.0s");
        // Rounding up to 24 hours wraps to 0 hours.
        assert_eq!(format_ra(359.999_999), "00h 00m 00.0s");
        assert_eq!(format_ra(-15.0), "23h 00m 00.0s");
    }

    #[test]
    fn declination_is_degrees_arcminutes_arcseconds() {
        assert_eq!(format_dec(30.66), "+30° 39′ 36″");
        assert_eq!(format_dec(-5.3911), "-05° 23′ 28″");
        assert_eq!(format_dec(0.0), "+00° 00′ 00″");
        assert_eq!(format_dec(-0.000_01), "+00° 00′ 00″");
        assert_eq!(format_dec(-90.0), "-90° 00′ 00″");
    }

    #[test]
    fn the_text_keeps_the_degrees_to_copy() {
        assert_eq!(
            position_text(23.4621, 30.66),
            "RA 01h 33m 50.9s · Dec +30° 39′ 36″ (23.4621° · +30.6600°)"
        );
    }
}
