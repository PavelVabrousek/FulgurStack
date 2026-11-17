//! Local release-only profiling of a frozen accepted registration plan.
//! Writes only final master/maps and JSON, never source or intermediate images.
use fulgur_core::{
    DemosaicMethod,
    fits::read_header,
    output::{MasterMetadata, write_fraction_map_atomic, write_master_atomic},
};
use fulgur_pipeline::{
    integrate::{
        CpuIntegrationEngine, IntegrationConfig, IntegrationEngine, IntegrationSource, TileMode,
    },
    register::{Canvas, Transform},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs::File, io::BufReader, path::PathBuf, sync::atomic::AtomicBool};

#[derive(Deserialize)]
struct Stored {
    registration: Plan,
}
#[derive(Deserialize)]
struct Plan {
    reference_anchor: String,
    frame_count: usize,
    accepted_count: usize,
    accepted_exposure_seconds: f64,
    reference_width: usize,
    reference_height: usize,
    frames: Vec<Row>,
}
#[derive(Deserialize)]
struct Correction {
    model: Option<Model>,
}
#[derive(Deserialize)]
struct Model {
    base: Mapping,
    center: [f64; 2],
    normalization: [f64; 2],
    coefficients: [[f64; 10]; 2],
}
#[derive(Deserialize)]
struct Row {
    file_name: String,
    accepted: bool,
    transform: Option<Mapping>,
    #[serde(default)]
    geometric_correction: Option<Correction>,
    photometric_scale: Option<f64>,
    background_offset_adu: Option<f64>,
    channel_offsets_adu: Option<[f64; 3]>,
    final_weight: f64,
}
#[derive(Deserialize)]
struct Mapping {
    scale_factor: f64,
    theta_rad: f64,
    tx_px: f64,
    ty_px: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Err("Profiling requires --release".into());
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 5 {
        return Err("Usage: profile_integration <stored-stack-report> <source-root> <new-output-dir> <threads> <center|full>".into());
    }
    let root = PathBuf::from(&args[1]).canonicalize()?;
    let dest = PathBuf::from(&args[2]);
    if dest.exists() {
        return Err("Profile output directory must be new".into());
    }
    let threads = args[3]
        .to_str()
        .ok_or("Invalid threads")?
        .parse::<usize>()?;
    if ![12, 16, 24].contains(&threads) {
        return Err("Choose 12, 16 or 24 threads".into());
    }
    let full = match args[4].to_str() {
        Some("center") => false,
        Some("full") => true,
        _ => return Err("Choose center or full".into()),
    };
    let open = || -> std::io::Result<_> {
        Ok(BufReader::with_capacity(1024 * 1024, File::open(&args[0])?))
    };
    // Accept a complete stack report, or a stored `register` report holding
    // the same plan at top level. Both are streamed; unknown fields are skipped.
    let plan = match serde_json::from_reader::<_, Stored>(open()?) {
        Ok(stored) => stored.registration,
        Err(_) => serde_json::from_reader::<_, Plan>(open()?)?,
    };
    let anchor = read_header(&root.join(&plan.reference_anchor))?;
    let mut sources = vec![];
    let mut identities = vec![];
    for row in plan.frames.into_iter().filter(|f| f.accepted) {
        let path = root.join(&row.file_name).canonicalize()?;
        if !path.starts_with(&root) {
            return Err("Source escaped root".into());
        }
        let header = read_header(&path)?;
        let meta = std::fs::metadata(&path)?;
        identities.push((path.clone(), meta.len(), meta.modified()?));
        let t = row.transform.ok_or("Missing selected transform")?;
        // Replays each frame's validated cubic exactly as the stack uses it.
        let distortion = row.geometric_correction.and_then(|c| c.model).map(|m| {
            fulgur_pipeline::distortion::Warp {
                base: Transform {
                    scale_factor: m.base.scale_factor,
                    theta_rad: m.base.theta_rad,
                    tx_px: m.base.tx_px,
                    ty_px: m.base.ty_px,
                },
                center: m.center,
                normalization: m.normalization,
                coefficients: m.coefficients,
            }
        });
        sources.push(IntegrationSource {
            calibration: None,
            distortion,
            partial: None,
            path,
            header,
            transform: Transform {
                scale_factor: t.scale_factor,
                theta_rad: t.theta_rad,
                tx_px: t.tx_px,
                ty_px: t.ty_px,
            },
            scale: row.photometric_scale.ok_or("Missing scale")?,
            offset: match row.channel_offsets_adu {
                Some(offsets) => offsets,
                None => [row.background_offset_adu.ok_or("Missing offset")?; 3],
            },
            weight: row.final_weight,
        });
    }
    if sources.len() != plan.accepted_count {
        return Err("Frozen selected count mismatch".into());
    }
    let canvas = if full {
        Canvas {
            origin_x_px: 0,
            origin_y_px: 0,
            width: plan.reference_width,
            height: plan.reference_height,
            minimum_effective_weight_fraction: 0.,
            coordinate_rule: "frozen complete reference grid; no crop",
        }
    } else {
        if plan.reference_width < 1536 || plan.reference_height < 2048 {
            return Err("Center profile requires a large reference grid".into());
        }
        Canvas {
            origin_x_px: 512,
            origin_y_px: 1024,
            width: 1024,
            height: 1024,
            minimum_effective_weight_fraction: 0.,
            coordinate_rule: "profiling-only four complete 512-square reference tiles; not a full-stack benchmark",
        }
    };
    // The stack's own tile choice and Auto rejection, as `stack` uses them,
    // unless PROFILE_TILES=WxH asks for a tile size.
    let rejection = fulgur_pipeline::integrate::Rejection::Auto;
    let config = match std::env::var("PROFILE_TILES").ok().and_then(|v| {
        let (w, h) = v.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    }) {
        Some((tile_width, tile_height)) => IntegrationConfig {
            rejection,
            tile_mode: TileMode::Manual,
            tile_width,
            tile_height,
            ..Default::default()
        },
        None => IntegrationConfig {
            rejection,
            tile_mode: TileMode::Auto,
            ..Default::default()
        },
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()?;
    let cancel = AtomicBool::new(false);
    let result = pool.install(|| {
        CpuIntegrationEngine.integrate_with_maps(
            &sources,
            &canvas,
            &config,
            plan.frame_count,
            &cancel,
        )
    })?;
    let mut hashes = vec![];
    for image in [&result.image, &result.coverage, &result.high_rejection] {
        let mut digest = Sha256::new();
        for p in &image.pixels {
            for v in p {
                digest.update(v.to_bits().to_le_bytes());
            }
        }
        hashes.push(format!("{:x}", digest.finalize()));
    }
    for (p, size, modified) in identities {
        let m = std::fs::metadata(p)?;
        if m.len() != size || m.modified()? != modified {
            return Err("Source identity changed".into());
        }
    }
    std::fs::create_dir(&dest)?;
    let export = write_master_atomic(
        &dest.join("master.fits"),
        &result.image,
        &anchor,
        &MasterMetadata {
            rejection: result.stats.rejection,
            geometric_correction_frames: sources.iter().filter(|s| s.distortion.is_some()).count(),
            accepted_frames: sources.len(),
            total_exposure_seconds: plan.accepted_exposure_seconds,
            origin_x_px: canvas.origin_x_px,
            origin_y_px: canvas.origin_y_px,
            external_geometry_scale: None,
            monochrome_frames: 0,
            calibrated_frames: 0,
            calibration_formula: None,
            astrometry: None,
            median_date_obs: None,
        },
        &cancel,
    )?;
    write_fraction_map_atomic(
        &dest.join("coverage.fits"),
        &result.coverage,
        "COVERAGE",
        plan.frame_count,
        None,
        &cancel,
    )?;
    write_fraction_map_atomic(
        &dest.join("high_rejection.fits"),
        &result.high_rejection,
        "HIGHREJ",
        plan.frame_count,
        None,
        &cancel,
    )?;
    assert_eq!(result.image.demosaic_method, DemosaicMethod::Vng);
    let summary = serde_json::json!({"scope":if full {"complete frozen reference integration"} else {"four central reference tiles; all frozen selected sources"},"threads":threads,"original_frames":plan.frame_count,"accepted_frames":sources.len(),"width":canvas.width,"height":canvas.height,"origin":[canvas.origin_x_px,canvas.origin_y_px],"config":config,"integration":result.stats,"export":export,"raw_float_sha256":hashes,"source_metadata_unchanged":true});
    serde_json::to_writer_pretty(File::create_new(dest.join("report.json"))?, &summary)?;
    println!(
        "{}",
        serde_json::json!({"threads":threads,"integration_seconds":result.stats.wall_seconds,"reconstruction_seconds":result.stats.reconstruction_wall_seconds,"reduction_seconds":result.stats.reduction_wall_seconds,"cpu_profile":result.stats.cpu_profile,"peak_rss_bytes":result.stats.peak_rss_bytes,"raw_float_sha256":hashes})
    );
    Ok(())
}
