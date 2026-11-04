//! Local phase 2 gate run (network and the PixInsight files needed).
//! Usage: phase2_gates PIXINSIGHT_GAIA_DIR CACHE_DIR
use fulgur_catalog::{GaiaSources, download, dso, gaia_field_with, lookup, xpsd};
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let pix = PathBuf::from(&a[0]);
    let cache = PathBuf::from(&a[1]);
    let cat = xpsd::XpsdCatalogue::open(&pix).unwrap();

    println!("== G1: a 2 degree cone from the PixInsight files");
    for (name, ra, dec) in [("M33", 23.4621, 30.66), ("NGC 7000", 314.82, 44.53)] {
        let t = Instant::now();
        let s = cat.cone(ra, dec, 2.0, 17.5).unwrap();
        println!(
            "  {name}: {} stars to G 17.5 in {:.2} s",
            s.len(),
            t.elapsed().as_secs_f64()
        );
    }

    println!("== G2: PixInsight files against the Gaia archive (position, G)");
    for (name, ra, dec, r, g) in [
        ("M33", 23.4621, 30.66, 0.8, 17.0),
        ("NGC 7000 (Milky Way)", 314.82, 44.53, 0.4, 15.5),
        ("IC 1396", 324.74, 57.49, 0.5, 16.0),
    ] {
        let archive = download::cone(&cache, ra, dec, r, g, &download::http_fetch).unwrap();
        let local = cat.cone(ra, dec, r, g).unwrap();
        let mut matched = 0;
        let (mut worst_pos, mut worst_g) = (0f64, 0f64);
        for s in &archive.stars {
            let best = local
                .iter()
                .map(|l| {
                    let d = ((l.ra - s.ra) * s.dec.to_radians().cos()).hypot(l.dec - s.dec) * 3.6e6;
                    (
                        d,
                        (l.g - s.g).abs(),
                        (l.pmra - s.pmra).abs().max((l.pmdec - s.pmdec).abs()),
                    )
                })
                .min_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((d, dg, _)) = best.filter(|b| b.0 < 50.) {
                matched += 1;
                worst_pos = worst_pos.max(d);
                worst_g = worst_g.max(dg);
            }
        }
        println!(
            "  {name}: archive {} ({}), local {}, matched {matched}; worst position {worst_pos:.3} mas, worst G {worst_g:.4}",
            archive.stars.len(),
            archive.sources.join("+"),
            local.len()
        );
    }

    println!("== G3: no network when the PixInsight files cover the request");
    let refuse = |_: &str, _: &str| -> Result<String, String> { panic!("network used") };
    let sources = GaiaSources {
        pixinsight: Some(pix.clone()),
        cache: cache.clone(),
        download: true,
    };
    let f = gaia_field_with(&sources, 83.82, -5.39, 1.5, 17.5, &refuse).unwrap();
    println!(
        "  M42 1.5 degree cone: {} stars from {}",
        f.stars.len(),
        f.source
    );

    println!("== G4: built-in deep-sky table against NED");
    let mut notes = vec![];
    for name in [
        "M33", "NGC 7000", "IC 1396", "M42", "M27", "NGC 869", "NGC 884",
    ] {
        let local = dso::find(name);
        notes.clear();
        let online = lookup::find(name, &lookup::http_get, &mut notes);
        match (local, online) {
            (Some(l), Some((ra, dec, service))) => {
                let d = ((l.ra - ra) * dec.to_radians().cos()).hypot(l.dec - dec) * 60.;
                println!("  {name}: table {} vs {service}: {d:.3} arcmin", l.name);
            }
            (l, o) => println!(
                "  {name}: table {:?}, online {:?} {notes:?}",
                l.map(|l| l.name),
                o
            ),
        }
    }

    println!("== G5: online lookup order for a name the table lacks");
    notes.clear();
    let found = lookup::find("LBN 437", &lookup::http_get, &mut notes);
    println!("  LBN 437: {found:?}; failures before: {notes:?}");
    notes.clear();
    let none = lookup::find("NoSuchObjectXYZ123", &lookup::http_get, &mut notes);
    println!("  unknown name: {none:?}; failures: {notes:?}");
}
