# FulgurStack

A CPU-first astrophotography stacking project for ZWO Seestar S50 Pro exposures, targeting a linear 32-bit floating-point RGB FITS master for further processing in PixInsight.

## Project Status

M1 implements immutable 16-bit CFA FITS input, conservative isolated hot-pixel correction, multithreaded VNG debayering, atomic linear-light float RGB FITS export, and a dataset I/O survey. Earlier MHC outputs failed owner review because of stellar artifacts; Sensor-bounded VNG passed owner visual inspection in PixInsight on 2026-10-21: **M1 / G1 PASS**. MHC remains available for diagnostics. M2 has not started. See [M1 review instructions and evidence](docs/M1_REVIEW.md).

## M2 Quality Inspection

M2 adds CPU inspection of original CFA lights with sky/noise estimates, detected star counts, HFR, moment eccentricity, explicit rejection reasons and provisional weights. M1 is approved; M2/G2 requires owner review before M3. See [M2 validation and manual-review table](docs/M2_REVIEW.md).

```powershell
cargo run --release --bin fulgurstack -- inspect --input "SeestarS50 Subs" --report "outputs/M2_review_new.json" --csv "outputs/M2_review_new.csv"
```

Choose unused report names. The CSV uses UTF-8 with BOM, semicolons and decimal points; the JSON includes up to 250 measured stars per frame with zero-based source coordinates. CSV star counts are uncapped detections, distinct from measured stars. Weights are provisional until M3 matches stars and measures transparency. Inspection preserves every input and writes no intermediate images.

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

- [Project specification](PROJECT_SPEC.md): authoritative scope and milestone acceptance gates.
- [Engineering instructions](AGENTS.md): implementation and verification rules.
- [Architecture decisions](docs/ARCHITECTURE_DECISIONS.md): recorded engineering choices.
- [Benchmark references](docs/BENCHMARKS.md): datasets, PixInsight settings, and owner-reported timings.
- [Development setup](docs/DEVELOPMENT.md): repository setup and Windows prerequisites.

## Local Data

Real FITS/XISF images, the `SeestarS50 Subs/` and `PixInsight Files/` directories, build artifacts, and generated outputs are excluded from Git. Keep these files locally; GitHub is not a backup of the observing data. The DS-B JSON manifest records relative filenames, sizes, and hashes without image pixels or GPS values.

## Language and Platforms

Project artifacts are English; communication with the project owner is Czech. Primary validation targets Windows 11 x86_64, with macOS Apple Silicon build and test support required by the project specification.
