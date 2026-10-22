# FulgurStack

A CPU-first astrophotography stacking project for ZWO Seestar S50 Pro exposures, targeting a linear 32-bit floating-point RGB FITS master for further processing in PixInsight.

## Project Status

M1 implements immutable 16-bit CFA FITS input, conservative isolated hot-pixel correction, multithreaded VNG debayering, atomic linear-light float RGB FITS export, and a dataset I/O survey. Earlier MHC outputs failed owner review because of stellar artifacts; Sensor-bounded VNG passed owner visual inspection in PixInsight on 2026-10-21: **M1 / G1 PASS**. MHC remains available for diagnostics. M2/G2, M3/G3 and M4/G4 were owner-accepted on 2026-10-22. M5 starts with optional GPU cosmetics/VNG acceleration; the approved CPU engine remains the default. See [M1 review instructions and evidence](docs/M1_REVIEW.md).

## M2 Quality Inspection

M2 adds CPU inspection of original CFA lights with sky/noise estimates, detected star counts, HFR, moment eccentricity, explicit rejection reasons and provisional weights. M1 and M2/G2 are owner-approved. See [M2 validation and manual-review table](docs/M2_REVIEW.md).

```powershell
cargo run --release --bin fulgurstack -- inspect --input "SeestarS50 Subs" --report "outputs/M2_review_new.json" --csv "outputs/M2_review_new.csv"
```

Choose unused report names. The CSV uses UTF-8 with BOM, semicolons and decimal points; the JSON includes up to 250 measured stars per frame with zero-based source coordinates. CSV star counts are uncapped detections, distinct from measured stars. Weights are provisional until M3 matches stars and measures transparency. Inspection preserves every input and writes no intermediate images.

## M3 Registration Plan

M3 directly registers original-CFA star catalogs to an automatic chronological-median anchor, including arbitrary field rotation and session-anchor composition. It measures matched-star photometry, final weights and safe common/80%-weight canvas rectangles. All 32 DS-B frames registered; M2 retains 27 and excludes five. See [M3 gates and telemetry](docs/M3_REVIEW.md) and [32-frame manual table](docs/M3_DS-B_REVIEW.md).

```powershell
cargo run --release --bin fulgurstack -- register --input "SeestarS50 Subs" --report "outputs/M3_owner_review.json" --csv "outputs/M3_owner_review.csv"
```

Reports use zero-based original sensor coordinates and source-to-master rigid transforms. Rejected measurable frames are registered for diagnostics only and retain zero final weight. No image intermediates are written; resampling and the integrated FITS master belong to M4 after owner G3 approval.

## M4 Tiled Integration

M4 processes raw CFA ROIs in bounded spatial tiles and integrates registered, normalized, pixel-rejected samples into one atomic linear 32-bit RGB FITS. It preserves accepted M1 VNG reconstruction, M2 selection and M3 registration/photometry. See [M4 review and performance evidence](docs/M4_REVIEW.md) and [predeclared quality gates](docs/M4_TEST_PLAN.md).

```powershell
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --output "outputs/M4_reference_review.fits" --report "outputs/M4_reference_review.json"
```

Open the FITS normally in PixInsight and apply a display STF as desired. No loading script or manual intensity conversion is required. The JSON distinguishes frame rejection, channel-sample rejection, geometric coverage and surviving weight. The default canvas preserves the reference FITS dimensions at origin (0,0), without cropping. The reference is the chronological median of quality-accepted inputs; `--reference` selects an accepted source manually. Both `register` and `stack` support it. Every stack also writes RGB coverage and high-rejection FITS maps beside the master (custom paths: `--coverage-output`, `--high-rejection-output`). Map values 0..1 mean 0..100% of all original input frames; coverage counts actual surviving samples after all rejection. See [current reference/map review](docs/M4_REFERENCE_MAPS_REVIEW.md). An existing external FITS path is also accepted as `--reference`, including cropped templates at 33..300% scale. It defines geometry only; native source photometry, exposure and GPS provenance remain unchanged. See [scaled reference tests and commands](docs/M4_SCALED_REFERENCE_REVIEW.md). `--canvas coverage-optimized` or `--canvas common-intersection` explicitly requests a crop. Repeat `--input` for additional folders. Existing outputs are never overwritten. The owner accepted G4 after the full-set visual review; documented analytic-photometry and unmatched PI-workload limitations remain explicit. GPU acceleration starts in M5 below.

## M5 GPU Reconstruction

Build with `--features gpu` and select `stack --engine gpu` to reconstruct bounded CFA ROI batches on a hardware GPU. Inspection, registration, geometric resampling, rejection and f64 accumulation retain the CPU implementation. GPU selection fails clearly if unsupported; it never silently substitutes a CPU benchmark. The report identifies the adapter/backend, initialization, transferred bytes, reconstruction batch time and reserved GPU/readback buffers. Readback time includes CPU copies and waiting; it is not a GPU-only kernel timer. GUI and GPU warp/reduction are later work.

```powershell
cargo run --release --features gpu --bin fulgurstack -- stack --engine gpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --output "outputs/M5_owner_review.fits" --report "outputs/M5_owner_review.json"
```

Use unused output names. Open the master and its automatically generated coverage/high-rejection maps normally in PixInsight. Compare with the accepted CPU image on the same reference grid. Hardware numeric parity is tested locally with `FULGURSTACK_TEST_GPU=1`; CI validates shaders and compiles/tests GPU features on Windows and macOS without claiming a hardware parity pass. See [M5 GPU validation and performance](docs/M5_GPU_REVIEW.md).

## M1 Commands

Run from the project root. Output filenames must be new; existing files are never overwritten.

```powershell
cargo run --release --bin fulgurstack -- debayer-single --input "SeestarS50 Subs/Light_M 33_10.0s_IRCUT_20261006-204534.fit" --output "outputs/M33_vng_review.fits" --report "outputs/M33_vng_review.json" --demosaic vng
cargo run --release --bin fulgurstack -- survey --input "SeestarS50 Subs" --report "outputs/DS-B_review.json"
```

Float FITS samples use a fixed linear scale: source ADU / 65,535, with zero offset and the same divisor for every image and channel. Reconstruction is completed at export by limiting interpolation excursions to the normalized sensor range 0..1, as approved by the owner. In-range values remain unchanged; reports count lower/upper limits. GPS is retained in the FITS and excluded from default reports and console output.

In PixInsight, open the new FITS normally with **File > Open**. No loading script, special STF or manual intensity conversion is required. Exports retain the supplied single-frame reference's intensity units and saturation bounds. Different diagonal coefficients, borders and cosmetics mean bitwise equality is not promised. Historical unbounded exports and the legacy loading script are not the current review candidate. Use `--demosaic mhc` only to reproduce the earlier reconstruction baseline (its final export uses the same 0..1 bounds). See [M1 review](docs/M1_REVIEW.md).

## First-Release Goals

- Process hundreds to thousands of exposures of one target and filter, across one or more sessions, including different exposure durations.
- Preserve source images and avoid writing intermediate calibrated, debayered, or registered images.
- Handle field rotation and keep memory bounded during integration.
- Preserve linear photometry and retain reference-anchor observing coordinates in the final master while redacting them from default logs and reports.
- Validate output quality against supplied PixInsight references before accepting speed improvements.

## Documentation

- [Full 1,140-frame 200% reference run](docs/M4_FULL_SCALED_RUN_REVIEW.md): 4000 x 4000 master/maps, measured performance and verification.
- [Full 1,140-frame native-reference run and performance profile](docs/M4_FULL_RUN_PROFILE.md): no-crop master/maps, measured bottlenecks and CPU/GPU optimization proposals.
- [Project specification](PROJECT_SPEC.md): authoritative scope and milestone acceptance gates.
- [Engineering instructions](AGENTS.md): implementation and verification rules.
- [Architecture decisions](docs/ARCHITECTURE_DECISIONS.md): recorded engineering choices.
- [Benchmark references](docs/BENCHMARKS.md): datasets, PixInsight settings, and owner-reported timings.
- [Development setup](docs/DEVELOPMENT.md): repository setup and Windows prerequisites.

## Local Data

Real FITS/XISF images, the `SeestarS50 Subs/` and `PixInsight Files/` directories, build artifacts, and generated outputs are excluded from Git. Keep these files locally; GitHub is not a backup of the observing data. The DS-B JSON manifest records relative filenames, sizes, and hashes without image pixels or GPS values.

## Language and Platforms

Project artifacts are English; communication with the project owner is Czech. Primary validation targets Windows 11 x86_64, with macOS Apple Silicon build and test support required by the project specification.
