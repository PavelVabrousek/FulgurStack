# M4 Revision: Reference Geometry and Diagnostic Maps

**Review update (2026-10-22):** The owner approved these DS-B products visually. The subsequent full 1,140-frame run with the same reference/canvas policy and diagnostic maps passed automated verification in 561.284 s complete-command time, using 773.6 MiB peak working set. See [full-run products, profiling and optimization proposals](M4_FULL_RUN_PROFILE.md). The measurements below remain the original DS-B evidence; full G4 quality sign-off is still pending.

The subsequent [scaled external-reference revision](M4_SCALED_REFERENCE_REVIEW.md) adds 33..300% geometry templates to the input-reference behavior documented below.

## Contract

The owner requested an accepted chronological-median reference, manual reference override, no default crop and final coverage/high-rejection FITS maps. These changes remain within M4 and do not close pending G4 image-quality gates. Previous cropped products remain preserved.

Automatic selection uses original FITS DATE-OBS, including subsecond time and timezone offsets, among quality-accepted frames. An even count selects the earlier central frame. Equal times use source-name order. An absent/invalid accepted timestamp requires --reference; filenames and quality weights do not substitute for acquisition time. A manual path or unique filename must identify a quality-accepted input.

The default output has the exact reference dimensions, origin (0,0) and pixel grid. Coverage thresholds do not crop it. Existing explicit --canvas coverage-optimized/common-intersection options remain available. Incomplete interpolation footprints are excluded; uncovered master pixels are zero.

Every stack writes a final scientific master and two final RGB Float32 maps on the same grid. Coverage = surviving samples / ALL original deduplicated frames after whole-frame, geometry and channel-dependent low/high rejection. High rejection = upper rejected samples / the same original count. These are counts, not weights/exposure; high rejection excludes invalid borders, low rejection, quality rejection and final sensor saturation. Fractions 0..1 mean 0..100%; NINPUT and PCTFACT=100 document the denominator/units. A 27/32 accepted stack cannot exceed 84.375% coverage. Maps are not divided by 65535 or min/max stretched.

Default map paths are derived from the master stem with _coverage.fits and _high_rejection.fits. --coverage-output and --high-rejection-output override them. All four destinations (master, two maps, report) must be distinct and new. Each file is atomically published; there is no whole-bundle transaction. No registered/debayered intermediate image is written. Memory planning includes all three final RGB buffers.

## Reproduction

Run from the repository directory with unused destinations:

```powershell
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --output "outputs/M4_median_review.fits" --report "outputs/M4_median_review.json"
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference "Light_M 33_10.0s_IRCUT_20261006-232807.fit" --output "outputs/M4_manual_review.fits" --report "outputs/M4_manual_review.json"
```

Open all three matching FITS files normally in PixInsight. Use STF as desired. Coverage and high rejection are independent per RGB channel; a value of 0.5 means 50%. In the master inspect the full sensor frame; use coverage to locate weak or absent edge data and high rejection to locate upper outliers. Do not interpret zero high rejection as proof of any coverage.

## Verification

Local fmt/Clippy and release gates pass: **71/71 Rust tests** (12 CLI, 19 FITS, 23 pipeline, 17 reconstruction) and four legacy import tests. Both real 32-frame runs completed. Tests cover accepted chronological medians, subsecond ordering, even-count selection, manual overrides and failures, original dimensions, exact per-channel count maps, clipping/border semantics, tile invariance, atomic no-overwrite publication and unchanged inputs. Final measured evidence follows below.


## Real DS-B Products and Telemetry

| Mode | Reference filename suffix | Master | Complete command s | Processing s | Peak working set MiB |
| --- | --- | --- | ---: | ---: | ---: |
| Automatic accepted median | 20261006-233727 | `outputs/M4_DS-B_reference_median.fits` | 18.626 | 18.530 | 709.2 |
| Manual PI reference | 20261006-232807 | `outputs/M4_DS-B_reference_PI.fits` | 21.388 | 21.287 | 711.6 |

Both runs keep 27 of 32 originals, accepted acquisition total 270 s, anchor EXPTIME 10 s, all products 2160 x 3840, output origin (0,0). These measured runs use release mode; compilation is excluded. Complete command includes JSON publication; process peak is sampled before JSON serialization. OS cache/background load are uncontrolled. An earlier cropped benchmark is not directly comparable to these larger three-image outputs. This revision was benchmarked on the owner-requested 32-frame set, not rerun on full DS-A.

The automatic reference DATE-OBS is `2026-10-06T21:37:05.722764`; the manually selected PI reference is `2026-10-06T21:27:45.078362`. Acquisition-header times determine selection; filename timestamps are not substituted.

Automatic products:

- `outputs/M4_DS-B_reference_median.fits` — final scientific master.
- `outputs/M4_DS-B_reference_median_coverage.fits` — used original-data fractions per channel.
- `outputs/M4_DS-B_reference_median_high_rejection.fits` — upper rejected original-data fractions per channel.
- `outputs/M4_DS-B_reference_median.json` — complete report, map definitions, selected reference and processing accounting.

Manual-reference products use the `M4_DS-B_reference_PI` prefix with the same suffixes. All previous masters/reports are preserved.

Coverage reaches exactly **84.375% = 27/32** in both stacks. Mean coverage over the complete sensor/RGB planes is 72.6407% / 72.6739% (median/manual); edges and rejection lower actual used counts. Maximum upper rejection is 31.25% of all original inputs in either case. The map count identities were independently verified across every output sample:

- Median run: 578,410,483 used + 3,536,399 low-rejected + 13,995,282 high-rejected = 595,942,164 geometrically valid channel samples.
- Manual run: 578,675,303 used + 3,530,115 low-rejected + 13,978,807 high-rejected = 596,184,225 geometrically valid channel samples.

Every map value times 32 is an exact integer in these runs. Channel maps differ where clipping differs; they are not weight maps or pre-rejection coverage estimates. Uncovered samples produce zero scientific output. Map headers use fraction units and NINPUT=32, with no ADU divisor or automatic FITS scaling. Master dimensions/crop/exposure metadata and exact private reference GPS cards pass independent checks. All 34 source/reference SHA-256 hashes remain unchanged; no temporary image files remain.

`outputs/M4_reference_maps_validation.json` records selected frames, count identities, dimensions, ranges and six product SHA-256 fingerprints. `outputs/M4_reference_maps_timing.json` records complete command times. The reproducible development checker is `scripts/tests/verify-reference-maps.py` (NumPy required, not a product runtime dependency). It produces local expected probes for `scripts/tests/pixinsight-reference-maps.js`, which verifies ordinary PI loading without intensity conversion and writes JSON only.

This revision changes reference/canvas selection and diagnostics, not the accepted VNG reconstruction or clipping equations. Previously documented absolute analytic photometry and full-reference quality limitations remain open. G4 is not closed by these successful functional tests.
