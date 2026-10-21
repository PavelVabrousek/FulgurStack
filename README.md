# FulgurStack

A CPU-first astrophotography stacking project for ZWO Seestar S50 Pro exposures, targeting a linear 32-bit floating-point RGB FITS master for further processing in PixInsight.

## Project Status

M1 implements immutable 16-bit CFA FITS input, isolated hot-pixel correction, multithreaded Malvar-He-Cutler debayering, atomic linear float RGB FITS export, and a dataset I/O survey. Windows release tests and real-data runs are verified. Owner review in PixInsight is required before M1 sign-off and M2 work. See [M1 review instructions and evidence](docs/M1_REVIEW.md).

## M1 Commands

Run from the project root. Output filenames must be new; existing files are never overwritten.

```powershell
cargo run --release --bin fulgurstack -- debayer-single --input "SeestarS50 Subs/Light_M 33_10.0s_IRCUT_20261006-204534.fit" --output "outputs/M33_review.fits" --report "outputs/M33_review.json"
cargo run --release --bin fulgurstack -- survey --input "SeestarS50 Subs" --report "outputs/DS-B_review.json"
```

Float FITS values remain in physical source ADU and are not clipped or stretched. In PixInsight, use rescaling rather than truncation if a floating-point range dialog appears, then use Auto STF for display. Compare against the matching single-frame PI debayer reference, not the integrated 32-frame master. GPS is retained in the FITS, but omitted from default reports and console output.

PI import rescaling changes the numerical gain and zero point. Direct PixelMath subtraction requires a common intensity scale first; see the confirmed import diagnosis and centered difference procedure in [M1 review](docs/M1_REVIEW.md#confirmed-pi-import-diagnosis-2026-10-21). The supplied PI reference uses VNG, while FulgurStack uses MHC; an exactly zero difference is not the acceptance criterion.

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
