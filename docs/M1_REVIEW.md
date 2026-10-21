# M1: FITS Reconstruction and Single-Frame Export

## Status

Implementation and Windows automatic gates are ready for review. Owner visual acceptance in PixInsight is pending. M2 has not started. Cross-platform CI status must be verified separately before claiming the macOS gate passed.

## Delivered

- Three-crate Rust workspace with `fulgurstack debayer-single` and `fulgurstack survey`.
- Immutable signed big-endian 16-bit CFA FITS decoding with BZERO, positive exposure validation, retained private headers, and no BIAS/dark subtraction.
- Same-color isolated hot-pixel correction with noise/star safeguards and multithreaded 5x5 Malvar-He-Cutler reconstruction.
- Atomic no-overwrite planar RGB float FITS, observing metadata/GPS retention, cleanup on failure/cancellation, and privacy-safe optional JSON reports.
- Real-dataset header and full-file I/O survey; no image intermediates.

## Local Automatic Gates (2026-10-21)

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace --release` | PASS: 30/30 tests, including the enabled real-input test |
| `cargo build --release` | PASS |

Synthetic coverage includes signed/BZERO boundaries, no BIAS subtraction, CFA colors and axis order, MHC impulse coefficients and negative values, fractional ADU preservation, Gaussian-noise false positives, star-core protection, hot-pixel cross removal, planar/byte-order output, privacy, truncation/invalid metadata, very large finite exposure serialization, overwrite/hardlink protection, write-race safety, stale temp files, and cleanup on failure/cancellation.

## Real-Data Evidence

All times below are optimized release processing times on the owner's Windows computer. They exclude compilation, CLI startup, and report serialization. OS cache state was not controlled. No comparison with the 32-frame PI debayer time or complete PI workflow is claimed from a single-frame run.

| Run | Frames | Processing wall time | Peak working set | Effective read throughput |
| --- | ---: | ---: | ---: | ---: |
| Single-frame reconstruction/export | 1 | 0.253 s | 164.5 MiB | Not an I/O-only benchmark |
| DS-B full-file survey | 32 | 0.030 s | 22.9 MiB | 16,958.9 MiB/s; cache assisted |
| DS-A full-file survey | 1,140 | 8.762 s | 22.6 MiB | 2,059.0 MiB/s; cache uncontrolled |

The real single-frame run corrected 5,140 candidate isolated pixels out of 8,294,400 (0.062%). These are correction candidates, not independently proven physical sensor defects. M1 does not reject entire frames. The export retains valid source GPS and produces no intermediate images.

An independent numerical check against the matching PI debayered XISF verifies the source's native R/G/B sample placement and row order. The PI green samples correspond to physical ADU divided by 65,535 with near-unity correlation in the same row order, while vertically flipped correspondence is absent. This does not replace visual evaluation of interpolated star shapes, color, or cosmic artifacts. No claim of pixel identity to an unspecified PI demosaicing method is made.

## Prepared Local Artifacts

- `outputs/M1_M33_rgb.fits`: 2160 x 3840 x 3, `BITPIX=-32`, planar RGB, physical source ADU.
- `outputs/M1_M33_single.json`: timing, peak memory, source metadata, correction count, and output provenance without GPS coordinates.
- `outputs/M1_DS-B_survey.json`: 32-frame read survey.
- `outputs/M1_DS-A_survey.json`: full 1,140-frame metadata baseline and read survey (`B-01`).
- `outputs/M1_independent_validation.json`: independent numerical comparison summary.

Generated images/reports and real reference data remain local and are excluded from Git.

## PixInsight Owner Review

1. Open `outputs/M1_M33_rgb.fits` with File > Open.
2. If **FITS Floating Point Range Options** appears, choose **Rescale**, not **Truncate**. The FITS retains physical ADU and unclipped MHC interpolation values, rather than forcing values into 0..1. Rescaling for PI import is a linear representation change; the on-disk export is unstretched. Keep consistent channel handling when assessing color.
3. Apply a display-only Auto STF. Check the galaxy/star orientation, plausible RGB colors, and the absence of colored hot-pixel crosses. Inspect bright star shapes and edges at 1:1 zoom.
4. Compare with `PixInsight Files/InterimFiles/dLight_M 33_10.0s_IRCUT_20261006-204534.xisf`, using comparable display settings. This is the matching single exposure; `integration.fit` contains 32 exposures and is not a direct single-frame noise reference.
5. Report whether orientation, colors, and cosmetic/star appearance pass. M1 sign-off and M2 work wait for this owner review and the required automatic platform checks.

## Reproduce with New Output Names

Run in Windows PowerShell from the project root. The following review output names differ from the artifacts already generated:

```powershell
cargo run --release --bin fulgurstack -- debayer-single --input "SeestarS50 Subs/Light_M 33_10.0s_IRCUT_20261006-204534.fit" --output "outputs/M33_review.fits" --report "outputs/M33_review.json"
cargo run --release --bin fulgurstack -- survey --input "SeestarS50 Subs" --report "outputs/DS-B_review.json"
```

Use a new output/report name for subsequent runs; FulgurStack never silently overwrites an existing file.

## Known M1 Boundaries

- No registration, multi-frame integration, GPU, GUI, or XISF application support is implemented.
- Full-frame M1 input is capped at 512 MiB raw payload; images must be at least 6 x 6 pixels. Unsupported BLANK handling, Bayer offsets, and FITS encodings are rejected explicitly.
- Sensor-border MHC reflection is an M1 single-frame policy; later registration/integration must use the specified validity masks.
- Peak process working-set telemetry is available on Windows; other platforms currently report null.
