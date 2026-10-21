# M1: FITS Reconstruction and Single-Frame Export

## Status and Current Candidate

The owner approved sensor-range saturation of reconstructed colors on 2026-10-21. Review **`outputs/M1_M33_vng_bounded.fits`** using ordinary PixInsight **File > Open**. No loading script, special STF or manual intensity conversion is required. **M1 / G1: PASS — owner visually accepted this candidate on 2026-10-21.** M2 has not started. See ADR-014 and specification v2.7.

Earlier MHC candidates failed for dark stellar cores and chromatic satellites. Conservative cosmetics and VNG addressed those issues. Unbounded VNG subsequently exposed display wrapping near saturation. ADR-013's STF workaround also failed owner review: its transformed-copy test did not establish actual viewport behavior. The failed workaround is removed. Previous image files and diagnostic reports remain unchanged as historical evidence.

## Owner Sign-off (2026-10-21)

The owner confirmed that visual inspection in PixInsight is satisfactory after reviewing the sensor-bounded export. This closes G1, including the previously reported stellar-core and cyan-pixel defects. Acceptance applies to `outputs/M1_M33_vng_bounded.fits`, SHA-256 `d60369afde3915ebb3a46d1cd159e8e674f4ae351e7b06ed33ce09959118d411`, produced by implementation commit `e1b5788` from `Light_M 33_10.0s_IRCUT_20261006-204534.fit`.

Automatic gates passed: 40 Rust release tests, four legacy importer contracts, independent sample/metadata/source-integrity checks and native PI ordinary-import/direct-render checks. Windows MSVC and macOS Apple Silicon CI passed for the accepted implementation: [run 39676311289](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39676311289). The DS-A baseline/read survey is recorded below. This sign-off closes M1; it does not claim registration/integration quality or begin M2. The existing pull request remains a draft and has not been merged.

## Output Contract

The pure-Rust CPU/Rayon workspace retains immutable signed big-endian CFA FITS input with BZERO, conservative cosmetics, VNG reconstruction, atomic no-overwrite Float32 RGB FITS, privacy-safe telemetry and bounded dataset surveys. No BIAS/dark subtraction or image intermediates are introduced.

Complete reconstruction at export with `value = (source_adu / 65535).clamp(0, 1)`, consistently for VNG and diagnostic MHC. All in-range values retain the same Float32 values; there is no tone stretch, pedestal, per-image extrema normalization or global-preference change. Interpolation remains unbounded internally for diagnostics. The owner accepts this sensor saturation as compatible with the product's linearity/no-data-clipping requirement.

`RGBBOUND='0:1'` and FITS history describe the policy. JSON schema 3 reports lower/upper limited reconstruction channel counts, total `samples_clipped`, final out-of-range counts (zero) and ordinary PI opening. Native source samples, GPS/date/exposure cards and original files are preserved; coordinates remain absent from default logs/reports.

## Automatic Gates (2026-10-21)

| Gate | Result |
| --- | --- |
| Format and warnings-denied Clippy | PASS |
| `cargo test --workspace --release` | PASS: 40/40, including real-input immutability |
| Optimized CLI smoke | PASS: one final RGB FITS and one JSON report |
| Legacy importer contracts | PASS: 4/4; historical files only |
| Independent full-array check | PASS: only 40 above-one samples become 1; all other 24,883,160 values unchanged |
| Native PI ordinary import and direct rendering | PASS: no STF/histogram transformation; all samples match expected bounded values; all seven reported stellar RGB triples and display bytes match PI |

Tests retain signed decoding, fractional signal, CFA phase/orientation, VNG symmetry/native-sample/gain-offset behavior, internal interpolation excursions, star support, hot pixels/noise, cancellation, atomic publication, format rejection and GPS privacy gates. Export regressions verify both saturation bounds, exact counters, fixed interior scaling despite changed image extrema, metadata and unchanged RGB input buffers. Numerical success does not replace owner quality approval.

## Real-Data Evidence

Source: `SeestarS50 Subs/Light_M 33_10.0s_IRCUT_20261006-204534.fit`, 2160 x 3840 GRBG, 10 s. Matched reference: `PixInsight Files/InterimFiles/dLight_M 33_10.0s_IRCUT_20261006-204534.xisf`. The 32-frame integration is not a direct single-frame noise reference.

- Release processing: **0.503 s**, **164.4 MiB** peak working set, 16 threads. Excludes build/startup/report serialization; cache uncontrolled. No stack speedup claim follows from one frame.
- Same 2,178 cosmetic candidates as the preceding VNG candidate; no whole frames rejected.
- New output range **0.027237353846..1.0**; 0 lower and **40 upper** reconstruction limits; 0 final out-of-range values.
- All 8,294,400 native CFA samples match the preceding cleaning; all 42 saturated native samples remain exactly 1. All 24,883,160 previously in-range reconstruction values are bit-identical.
- All 34 manifest source/reference files, matched single-frame reference and previous unbounded export retain their original hashes. GPS/exposure/date cards match the source; no temporary images remain.
- Local reports: `outputs/M1_M33_vng_bounded.json`, `outputs/M1_bounded_validation.json`, `outputs/M1_PI_bounded_validation.json`.

The native regression `scripts/tests/pixinsight-display-regression.js` uses ordinary import (only console verbosity suppressed), confirms identity STF, compares all loaded values against the expected bounded historical baseline, and directly calls `image.render()` on the actual new image. It performs no histogram process or pixel transformation. All seven affected stellar RGB triples match PI within 1e-6 and render with identical bytes. Only JSON is saved and its own windows are closed. This checks native import/direct rendering primitives, not a captured viewport screenshot. Archive the existing native report before rerunning.

## Owner Review

1. Open **`outputs/M1_M33_vng_bounded.fits`** with **File > Open**. Use this new filename, not the older unbounded VNG export. No special loading or STF settings are needed.
2. Compare the matching single-frame XISF above. Inspect **x=1694, y=1409** at the previous 26:1 zoom, both previously reported stars, dark cores, hot-pixel crosses and orientation. Use comparable display settings; optional ordinary STF is display-only.
3. Direct PixelMath subtraction needs no unit conversion. Use actual view IDs and disable result rescaling/truncation to retain signed residuals. Small coefficient, cosmetic and border differences remain; whole-frame bitwise identity is not claimed.
4. Owner visual acceptance and both platform checks are complete: M1/G1 PASS. No M2 work has begun.

Reproduce from Windows PowerShell in the project root with unused names:

```powershell
cargo run --release --bin fulgurstack -- debayer-single --input "SeestarS50 Subs/Light_M 33_10.0s_IRCUT_20261006-204534.fit" --output "outputs/M33_vng_bounded_review.fits" --report "outputs/M33_vng_bounded_review.json"
```

## Historical Evidence and Known Boundaries

Earlier exports and saved PI images are diagnostic history only. Historical DS-A full-file survey: 1,140 frames in 8.762 s, peak 22.6 MiB, effective 2,059.0 MiB/s with uncontrolled cache. This is not a hardware SSD speed claim; the DS-B 32-frame survey was cache assisted.

Registration/integration, GPU, product GUI and XISF application support remain outside M1. Inputs are bounded to 512 MiB and minimum 6 x 6 geometry; unsupported encodings/BLANK/Bayer offsets fail explicitly. Sensor-edge reflection is an M1 policy; later registration requires strict validity masks. Peak RAM telemetry is Windows-only; unsupported platforms report null.

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

Detailed local evidence: `outputs/M1_PI_import_diagnosis.json`; copyable expression: `outputs/M1_PI_fair_difference.txt`. All three comparison inputs were fingerprinted before and after analysis and remained unchanged. At that historical stage G1 was pending; the current bounded output is approved in the sign-off above.

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
