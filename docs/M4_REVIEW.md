# M4: Tiled CPU Integration and PixInsight Master Review

**Historical evidence for the initial cropped candidate.** The owner subsequently requested median/manual references, reference-sized output and diagnostic maps. [Current behavior and new test outputs](M4_REFERENCE_MAPS_REVIEW.md) supersede the defaults and artifact links below; the original outputs and benchmarks remain preserved.

## Status and Remaining Acceptance

M3/G3 was owner-approved on 2026-10-22 and M4 was authorized. M4 is implemented on `m4-tiled-integration`, based on M3. The 32-frame master is ready for product review. **G4 is not signed off:** owner visual approval, full DS-A reference-quality parity and an unconditional absolute analytic CFA-to-RGB flux guarantee remain unverified. The stronger synthetic analytic colored-star comparison has a documented counterexample; the integration-relative flux gate passes. No GPU/GUI work has begun and previous PRs were not merged automatically.

Implementation commit: `c2c2a2e98a238a68bef157a1cbe62955b21dce5a`. Windows MSVC and macOS Apple Silicon CI both passed run `39689889714` on this implementation commit; see draft PR #4. Subsequent report/documentation/native-check helper additions do not alter the Rust engine used for these measured masters.

## Owner Review Files

- **`outputs/M4_DS-B_master.fits`**: ordinary-open linear Float32 RGB, 1324 x 3465 px, 27 accepted of 32 originals, anchor exposure 10 s, accepted acquisition total 270 s. Output origin (469, 226) is in the automatic anchor's original sensor coordinates.
- **`outputs/M4_performance_report.json`**: compact measured-performance/selection/quality summary for both dataset sizes.
- `outputs/M4_DS-B_report.json`: complete input selection, registration, photometry, final weights, tile/memory accounting, pixel rejection, coverage and export limiting counts.
- `outputs/M4_DS-B_comparison.json`: numerical comparison to supplied PI 32-frame reference, including all selected star/sky coordinates.
- `outputs/M4_PI_import_validation.json`: native ordinary PixInsight import/range/orientation check.
- `outputs/M4_DS-A_master.fits` / `M4_DS-A_report.json`: full-night throughput and memory validation. The full JSON includes the catalogs and is approximately 164 MB; use the compact summary first.
- `outputs/M4_validation.json`: independent layout, range, metadata, memory/coverage counters, deterministic output and source-integrity verification.

All images/reports remain local and ignored by Git. [Predeclared quality tests](M4_TEST_PLAN.md) distinguish diagnostic comparison limits from final acceptance.

## Measured Release Performance (2026-10-22)

| Dataset | Input | Accepted / rejected | Inspect + register s | Integrate s | Write s | Complete command s | Peak working set MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| DS-B | 32 | 27 / 5 | 1.069 | 10.214 | 0.096 | 11.441 | 702.8 |
| DS-A | 1,140 | 940 / 200 | 29.237 | 319.499 | 0.099 | 349.663 | 646.6 |

Processing wall time through FITS publication is 11.381 / 349.214 s, or 2.812 / 3.264 input frames per processing second. Outer command timings include JSON publication and startup, excluding compilation. The process peak working set is sampled through FITS export before serializing JSON; it is not a separate post-publication sample. OS cache and background host workload are uncontrolled. The observed full processing time meets the <=18 min numeric target, but no accepted PI speedup/quality-parity claim is made: PI input selection/weighting/rejection differ and exact full-reference input selection is unknown.

Accepted acquisition exposure is 270 / 9,400 s. It is not processing time or equal exposure at every output pixel. EXPTIME remains the anchor's 10 s intensity scale; TEXPTIME is the accepted acquisition sum. NCOMBINE is 27 / 940.

DS-B uses 168 spatial tiles; DS-A uses 125. Default tile geometry is 256 x 128, trimmed at output edges. Conservative planned buffers are 940,036,752 / 1,289,080,528 bytes; actual peak working sets are below 1 GiB and below the configured 12 GiB limit. Only the current output tile's registered samples and bounded source ROIs reside in RAM. Raw ROI counts include repeated halo/support pixels, not unique source pixels.

## Selection, Pixel Rejection and Coverage

Frame selection is unchanged from M2/M3. Rejected frames do not contribute any integration weight. Default winsorized rejection thresholds are low 3.0 / high 2.5 sigma, with at most three median/MAD estimate passes. Original surviving samples are averaged with final M3 weights and compensated f64 sums.

| Measure | DS-B | DS-A |
| --- | ---: | ---: |
| Low rejected channel samples | 1,988,428 | 19,916,651 |
| High rejected channel samples | 8,314,998 | 161,999,159 |
| Valid channel samples before rejection | 365,185,122 | 10,314,664,866 |
| Zero geometric coverage pixels | 0 | 0 |
| Minimum valid frames at a pixel | 20 | 700 |
| Minimum geometric effective-weight fraction | 0.801872 | 0.800172 |
| Minimum surviving fraction after channel rejection | 0.470572 | 0.768145 |
| Final upper sensor-range limited channel samples | 23 | 8 |

Geometric 80% coverage does not guarantee 80% weight after outlier rejection. Channel-sample rejection is distinct from frame rejection, and one frame can be rejected in some pixels only. Final export limiting is separately counted; no lower samples required limiting. Unbounded reconstruction/resampling stays internal until the accepted final ADU/65535 saturation boundary.

## Numerical Comparison with the Supplied DS-B PI Master

The PI reference uses all 32 exposures and ESD rejection. FulgurStack uses 27 exposures and winsorized rejection. Register coordinate systems through the known PI registration template transform; do not compare different anchor origins directly. The comparison reads both masters without modifying either. A single scalar 0.98612255 aligns green stellar flux scales for measurement only; no scalar or stretch is applied to the delivered FITS.

Select 37 isolated, unsaturated catalog stars with source SNR >=100, nearest catalog neighbor >=28 px, usable 15 px edge margin and master peaks <0.9. Compare radius-8 aperture flux with 10..14 px sky annuli and radius-6 positive-flux moments. Select 61 source-free 33 x 33 sky patches on a deterministic grid, outside the galaxy center and catalog-star halos; remove local fitted sky planes for the noise statistic only. Exact coordinates and values are in the comparison JSON. These are catalog-based masks and finite-aperture diagnostics, not an independent full source segmentation or fitted PSF model.

| Diagnostic | Measured | Predeclared limit | Result |
| --- | ---: | ---: | --- |
| Median absolute green flux residual after shared scalar | 0.5572% | <=1% | PASS |
| Median moment-equivalent FWHM ratio to PI | 0.99384 | <=1.05 | PASS |
| Median stellar R/G difference | +0.5416% | absolute <=5% | PASS |
| Median stellar B/G difference | +1.9130% | absolute <=5% | PASS |
| Median robust sky noise ratio to PI | 1.04069 | <=1.10 | PASS |

These diagnostic limits pass; they do not prove equivalent-input quality parity. The measured background noise is about 4% higher with five fewer input frames. Owner visual evaluation remains necessary for star cores, false colors, galaxy details, sharpness and edge coverage. The full DS-A PI XISF master has not been quantitatively aligned/compared here; its exact accepted list/reference anchor remain unconfirmed. Do not label this remaining gate PASS.

## Automatic and Native Gates

- fmt and Clippy with warnings denied: PASS. Release Rust tests: 67/67 (11 CLI, 18 core, 21 pipeline, 17 reconstruction). Legacy adapter tests: 4/4.
- Real ROI reconstruction agrees exactly with full-frame accepted M1 VNG across original sensor edges and internal boundaries. Different tile geometries produce identical synthetic master pixels.
- Synthetic minority trails/outliers are rejected without changing clean core levels. Empty/sparse populations and Neumaier cancellation cases pass.
- Mixed-exposure/subpixel-shift integration preserves accepted M1 reconstructed aperture flux to <=0.657%. Exposure/transparency is applied once; repeated input paths are deduplicated, sources remain unchanged, and no BIAS subtraction occurs.
- The stronger ideal analytic RGB Gaussian comparison initially failed (chosen narrow colored source: green about +3.89%, blue about +10.55% after the pipeline). Independent accepted M1 reconstruction already contains this bias. This is retained as a known limitation, not hidden by a looser threshold. Absolute CFA-to-RGB flux fidelity is not established by the passing integration-relative check.
- Native PixInsight ordinary import: PASS, all 13,762,980 channel samples in 0..1, zero stratified-probe differences, correct 1324 x 3465 RGB dimensions, unmodified loaded image and anchor GPS presence. No loading script, STF or histogram/intensity conversion was applied. This automation checks loaded data, not the owner's visual assessment.
- Independent FITS checks verify Float32 planar layout, finite range, dimensions, crop/exposure/count metadata, exact private anchor GPS cards, absent false WCS/scaling cards and no stale `.tmp` files. All 34 DS-B source/reference hashes, accepted M1 export and all DS-A size/mtime snapshots are unchanged. Pilot and final DS-B master hashes match exactly.

DS-B SHA-256: `33337dd4acaa83a62ab4815d627a37d3ea7f3998d200700a6547eab12cec4fa7`.

DS-A SHA-256: `c092035afc256be6668f4b9eac8db127e0ae3076200ef6a0390672fd149b5070`.

## Owner Actions

In PixInsight use **File > Open** on `outputs/M4_DS-B_master.fits`; apply a display STF as desired. Check bright cores and color satellites, sharpness of isolated stars, M33 detail/background and the chosen crop. GPS is retained in FITS; default reports contain no coordinates. The automatic anchor differs from the supplied PI template, so image origins/rotation and frame inclusion differ.

From the project directory, choose unused output names:

```powershell
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --canvas coverage-optimized --min-coverage 0.80 --memory-budget-mib 12288 --output "outputs/M4_owner_review.fits" --report "outputs/M4_owner_review.json"
```

For the common rectangle use `--canvas common-intersection`. Repeat `--input` for extra folders. Existing destinations are never overwritten. Full DS-A uses the same command with its source directory. Reproduce the development-only numerical comparison (NumPy required, not a Rust runtime dependency):

```powershell
python scripts/tests/compare-master.py --report "outputs/M4_comparison_new.json"
```

The native ordinary-open check is `scripts/tests/pixinsight-master-import.js`; it consumes locally generated expected probes in `outputs/M4_PI_expected.json` and writes only JSON. Run in a separate PixInsight automation instance, preserving the owner's session and preferences. No registered or debayered intermediate image is written.
