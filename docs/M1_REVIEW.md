# M1: FITS Reconstruction and Single-Frame Export

## Status

The owner rejected the normalized MHC export for dark cores and then the core-preserving MHC export for colored satellites. The first issue was cosmetic filtering; the second is reproduced by MHC reconstruction on untouched raw data. Default reconstruction is now unclipped VNG. The new VNG candidate is ready for renewed review; G1 is not approved and M2 has not started. Cross-platform CI status must be verified for each code revision.

## Delivered

- Three-crate Rust workspace with `fulgurstack debayer-single` and `fulgurstack survey`.
- Immutable signed big-endian 16-bit CFA FITS decoding with BZERO, positive exposure validation, retained private headers, and no BIAS/dark subtraction.
- Same-color isolated hot-pixel correction with noise/star safeguards and multithreaded 5x5 VNG reconstruction; MHC remains available through explicit `--demosaic mhc` for diagnostics.
- Atomic no-overwrite planar RGB float FITS, observing metadata/GPS retention, cleanup on failure/cancellation, and privacy-safe optional JSON reports.
- Real-dataset header and full-file I/O survey; no image intermediates.

## Local Automatic Gates (2026-10-21)

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace --release` | PASS: 40/40 tests, including the enabled real-input test |
| `cargo build --release` | PASS |
| PI import adapter contract (`node --test scripts/tests/open-fulgurstack.test.cjs`) | PASS: 4/4; host API doubles, not native PI execution |

Synthetic coverage includes signed/BZERO boundaries, no BIAS subtraction, CFA colors and axis order, MHC impulse coefficients and negative values, fractional ADU preservation, Gaussian-noise false positives, star-core protection, hot-pixel cross removal, planar/byte-order output, privacy, truncation/invalid metadata, very large finite exposure serialization, overwrite/hardlink protection, write-race safety, stale temp files, and cleanup on failure/cancellation.

VNG regressions cover native samples, all CFA phases, flat fractional colors, common gain/offset behavior, quarter-turn symmetry, a failing chromatic-satellite neighborhood, common spatial gradients and retained interpolation overshoot from bounded source values. The selected method is included in FITS and JSON provenance; unknown CLI methods fail before writing outputs.

## Real-Data Evidence

All times below are optimized release processing times on the owner's Windows computer. They exclude compilation, CLI startup, and report serialization. OS cache state was not controlled. No comparison with the 32-frame PI debayer time or complete PI workflow is claimed from a single-frame run.

| Run | Frames | Processing wall time | Peak working set | Effective read throughput |
| --- | ---: | ---: | ---: | ---: |
| Current VNG reconstruction/export | 1 | 0.463 s | 164.5 MiB | Not an I/O-only benchmark |
| Stellar-support fix: single-frame reconstruction/export | 1 | 0.346 s | 164.3 MiB | Not an I/O-only benchmark |
| Fixed-scale single-frame reconstruction/export | 1 | 0.357 s | 164.4 MiB | Not an I/O-only benchmark |
| Single-frame reconstruction/export | 1 | 0.253 s | 164.5 MiB | Not an I/O-only benchmark |
| DS-B full-file survey | 32 | 0.030 s | 22.9 MiB | 16,958.9 MiB/s; cache assisted |
| DS-A full-file survey | 1,140 | 8.762 s | 22.6 MiB | 2,059.0 MiB/s; cache uncontrolled |

The current real single-frame run corrects 2,178 candidates out of 8,294,400 (0.026%). This cleaning is unchanged from the core-protection fix; the reconstruction method has changed to VNG. The earlier 5,140-candidate result included destructive stellar-core changes and failed owner visual review. These are correction candidates, not independently proven physical sensor defects. M1 does not reject entire frames. The export retains valid source GPS and produces no intermediate images.

An independent numerical check against the matching PI debayered XISF verifies the source's native R/G/B sample placement and row order. The PI green samples correspond to physical ADU divided by 65,535 with near-unity correlation in the same row order, while vertically flipped correspondence is absent. The supplied PI history identifies VNG; FulgurStack now also uses VNG, with documented clipping, coefficient, cosmetic and border differences. This does not replace visual quality approval.

## Prepared Local Artifacts

- **Current:** `outputs/M1_M33_vng.fits`: fixed-scale RGB Float32 VNG FITS, unchanged conservative cleaning, no clipping and retained GPS.
- `outputs/M1_M33_vng.json`: current release timing, method and export telemetry.
- `outputs/M1_vng_validation.json`: independent native/prototype checks, reported-star comparisons and integrity evidence.
- **Rejected historical version:** `outputs/M1_M33_star_safe.fits`: core-protected MHC, rejected for chromatic satellites.
- `outputs/M1_M33_star_safe.json`: historical MHC core-fix timing and correction/export telemetry.
- `outputs/M1_star_safe_validation.json`: independent bright-core verification and immutable-file checks.
- **Rejected historical version:** `outputs/M1_M33_rgb_normalized.fits`: correct fixed scale, but destructive cosmetic changes in bright stellar cores; retained for diagnosis.
- `outputs/M1_M33_normalized.json`: schema-2 telemetry, fixed divisor, output range and preserved out-of-range counts.
- `outputs/M1_normalized_validation.json`: full-array independent verification, source/reference integrity and common-scale residual statistics.
- **Historical:** `outputs/M1_M33_rgb.fits`: original physical-ADU export retained for diagnosis; use the VNG export for current review.
- `outputs/M1_M33_single.json`: timing, peak memory, source metadata, correction count, and output provenance without GPS coordinates.
- `outputs/M1_DS-B_survey.json`: 32-frame read survey.
- `outputs/M1_DS-A_survey.json`: full 1,140-frame metadata baseline and read survey (`B-01`).
- `outputs/M1_independent_validation.json`: independent numerical comparison summary.

Generated images/reports and real reference data remain local and are excluded from Git.

## PixInsight Owner Review

1. In PixInsight choose **Script > Execute Script**, select project file `scripts/OpenFulgurStack.js`, then explicitly select `outputs/M1_M33_vng.fits`. The picker starts in the outputs directory without preselecting an obsolete export. The view ID includes the filename, normally `FulgurStack_M1_M33_vng` (PI may append a suffix if it is already open). No pixel arithmetic or global preference change is performed. Old physical-ADU exports are rejected by the adapter.
2. Use this adapter instead of ordinary File > Open for exact comparisons: the current FITS contains 40 above-one channel samples, and ordinary opening can invoke PI's configured whole-image rescaling or truncation. Their preservation is intentional; there are no negative samples in this particular VNG result, not because of clipping. FITS headers alone cannot override that PI setting. The fixed scale matches the reference without a pedestal or per-image normalization.
3. Apply a display-only Auto STF. Recheck both stars that showed green/blue satellites, at 1:1 and at the previous 10:1 zoom, and the previously damaged cores. Also check orientation, RGB color, star sharpness and hot-pixel crosses. Use consistent display settings. PI clamps reconstructed channels, while this export retains excursions; exact RGB equality is not expected in saturated samples.
4. Compare with `PixInsight Files/InterimFiles/dLight_M 33_10.0s_IRCUT_20261006-204534.xisf`, using comparable display settings. This is the matching single exposure; `integration.fit` contains 32 exposures and is not a direct single-frame noise reference.
5. Report whether orientation, colors, and cosmetic/star appearance pass. M1 sign-off and M2 work wait for this owner review and the required automatic platform checks.

Direct PixelMath subtraction is `Image06 - FulgurStack_M1_M33_vng` if `Image06` is the matching reference (use actual view IDs). Create a new RGB floating-point image and disable **Rescale result** and **Truncate result** to retain signed differences. No unit-conversion coefficients are needed. A display-only STF does not modify samples.

Current VNG output retains all native samples from the prior conservative cleaning, including all 42 saturated source samples; the 71 formerly damaged bright samples remain protected. An independent Float64 prototype matches 1,024 sampled positions within 0.000227 ADU. The known satellite now has RGB approximately 4,858 / 5,167.68 / 5,079.43 ADU, matching PI within Float32 rounding rather than the MHC 4,858 / -338.5 / -3,162.625. At the reported red-star region, RMS difference to PI falls from 1,194 to 11.8 ADU. Its green aperture flux differs by 0.024% and HFR by 0.039%. These are local diagnostics, not a whole-field quality gate or an estimate of added noise.

The export stays at fixed source ADU / 65,535, zero offset and zero clipped samples, with range 0.02723735..1.17160451. All 34 manifest files, prior exports, saved loaded PI image and matched reference remain unchanged. GPS and exposure/date cards are preserved verbatim; no temporary image files remain. See ADR-012 for the quality-driven default change and compatibility limits.

Historical normalization-only verification confirmed every prior output sample matched the original reconstruction divided by 65,535 at Float32 precision. That proved the export scaling but did not validate cosmetic changes in stars. The owner's later screenshot exposed that separate quality failure. The new synthetic regressions include the actual failing intensity pattern and 60 colored/saturated PSF cases. Visual approval of the repaired image remains required.

The adapter's API signature was checked against installed PI documentation and its range hints against official module source. Its contract tests run without PixInsight using API doubles. The owner's screenshot shows the prior normalized image opened in PI and establishes the stellar-core quality failure. The newly repaired candidate still requires owner visual review; no automated native PI round-trip is claimed.

### Confirmed PI Import Diagnosis (2026-10-21)

**Historical evidence for the original export only.** The conversion below is unnecessary for the new normalized export opened through the adapter. Do not apply it to the new image.

The owner saved the loaded M1 image as `outputs/M1_loaded_in_PI.xisf`. Full-array comparison confirms a common linear min/max conversion of the FITS into PI's 0..1 representation: the original -5222.25..86902.59375 ADU range was mapped to 0..1. The same gain and offset apply to all RGB channels. The maximum discrepancy from that mapping is 2.98e-8, consistent with Float32 rounding; no additional clipping, geometric change, or channel permutation was detected. This establishes the transformation between the two files, without proving the exact UI action or preference that invoked it.

The reference PI debayer image's embedded processing history identifies **VNG**, while FulgurStack uses the specified **MHC** reconstruction and isolated-pixel correction. Even after matching intensity units, their interpolated pixels can differ. This is independent of the import conversion and requires quality evaluation rather than an identity test.

The loaded M1 green-channel median is 0.0817505, versus 0.0352483 for the PI reference. Approximately 99.99% of loaded M1 samples exceed the reference, explaining the strongly one-sided subtraction in the screenshot when negative values are suppressed by display or processing. Exact PixelMath truncation/rescale settings cannot be inferred from these files. After restoring common units, median signed differences are effectively zero; residual RMS is approximately 87.3, 71.1, and 87.4 ADU in R/G/B. These RMS values describe differences between methods, not measured added noise or a quality sign-off.

The earlier Rescale recommendation is valid for viewing but does not establish a common photometric scale for direct subtraction. For the screenshot's original view identifiers (`Image06` = PI reference; `Image05` = the saved loaded M1 image), use this centered signed diagnostic in PixelMath:

```text
0.5 + 0.25*(Image06 - (1.4057350080109865*Image05 - 0.079686427100022883))
```

Create a new RGB floating-point image; disable both **Rescale result** and **Truncate result**. Equality is neutral 0.5 rather than black, and both signs are retained. The computed output range for this pair is 0.41849..0.70650, so no samples require truncation. STF may be applied for display. These coefficients reverse this particular saved image's import conversion and must not be reused as universal normalization constants.

Detailed local evidence: `outputs/M1_PI_import_diagnosis.json`; copyable expression: `outputs/M1_PI_fair_difference.txt`. All three comparison inputs were fingerprinted before and after analysis and remained unchanged. G1 is still pending owner visual/quality approval.

## Reproduce with New Output Names

Run in Windows PowerShell from the project root. The following review output names differ from the artifacts already generated:

```powershell
cargo run --release --bin fulgurstack -- debayer-single --input "SeestarS50 Subs/Light_M 33_10.0s_IRCUT_20261006-204534.fit" --output "outputs/M33_vng_review.fits" --report "outputs/M33_vng_review.json" --demosaic vng
cargo run --release --bin fulgurstack -- survey --input "SeestarS50 Subs" --report "outputs/DS-B_review.json"
```

Use a new output/report name for subsequent runs; FulgurStack never silently overwrites an existing file.

## Known M1 Boundaries

- No registration, multi-frame integration, GPU, GUI, or XISF application support is implemented.
- Full-frame M1 input is capped at 512 MiB raw payload; images must be at least 6 x 6 pixels. Unsupported BLANK handling, Bayer offsets, and FITS encodings are rejected explicitly.
- Sensor-border reflection is an M1 single-frame policy; later registration/integration must use the specified validity masks.
- Peak process working-set telemetry is available on Windows; other platforms currently report null.
