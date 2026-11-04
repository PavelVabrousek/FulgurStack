//! Local check: read a Gaia cone from PixInsight XPSD files and compare it
//! with a Gaia archive CSV. Usage: xpsd_probe DIR RA DEC RADIUS GMAX [CSV]
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize| a[i].parse::<f64>().unwrap();
    let t = Instant::now();
    let cat = fulgur_catalog::xpsd::XpsdCatalogue::open(std::path::Path::new(&a[0])).unwrap();
    let opened = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let stars = cat.cone(num(1), num(2), num(3), num(4)).unwrap();
    println!(
        "{} files (G limit {:.2}), opened in {opened:.2} s; cone: {} stars in {:.2} s",
        cat.files.len(),
        cat.limit(),
        stars.len(),
        t.elapsed().as_secs_f64()
    );
    for s in stars.iter().take(3) {
        println!(
            "  {:.6} {:+.6} G {:.3} pm {:.2} {:.2}",
            s.ra, s.dec, s.g, s.pmra, s.pmdec
        );
    }
    if let Some(csv) = a.get(5) {
        let text = std::fs::read_to_string(csv).unwrap();
        let mut lines = text.lines();
        let head: Vec<_> = lines
            .next()
            .unwrap()
            .split(',')
            .map(|h| h.trim().trim_matches('"').to_string())
            .collect();
        let col = |n: &str| head.iter().position(|h| h == n).unwrap();
        let (cr, cd, cg) = (col("ra"), col("dec"), col("phot_g_mean_mag"));
        let reference: Vec<(f64, f64, f64)> = lines
            .filter_map(|l| {
                let f: Vec<&str> = l.split(',').collect();
                Some((
                    f[cr].parse().ok()?,
                    f[cd].parse().ok()?,
                    f[cg].parse().ok()?,
                ))
            })
            .collect();
        let mut matched = 0;
        let (mut worst_pos, mut worst_mag) = (0f64, 0f64);
        for &(ra, dec, g) in &reference {
            let best = stars
                .iter()
                .map(|s| {
                    let d = ((s.ra - ra) * dec.to_radians().cos()).hypot(s.dec - dec) * 3.6e6;
                    (d, (s.g - g).abs())
                })
                .min_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((d, dg)) = best {
                if d < 50. {
                    matched += 1;
                    worst_pos = worst_pos.max(d);
                    worst_mag = worst_mag.max(dg);
                }
            }
        }
        println!(
            "archive CSV: {} stars, {} matched within 50 mas; worst position {:.3} mas, worst G difference {:.4}",
            reference.len(),
            matched,
            worst_pos,
            worst_mag
        );
    }
}
