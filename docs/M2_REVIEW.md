# M2: CPU Subframe Inspector and Quality Filtering

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

## Status

M1/G1 is owner-approved. M2 was explicitly requested on 2026-10-21 and is implemented on `m2-subframe-inspector`, based on the accepted M1 branch. Local automatic G2 gates pass; exact-commit cross-platform CI is tracked in the draft PR. The owner approved the manual table and closed M2/G2 on 2026-10-22. Exact implementation commit `97c0569314f98da3ac2845e1d40b8dfa66cb320c` passed Windows MSVC and macOS Apple Silicon CI run `39682032218`. M3 was authorized in the same owner message. The M1 PR is not merged automatically.

## Manual Review Artifacts

- **[All 32 frame measurements](M2_DS-B_REVIEW.md)**: compact English table and the five flagged frames with representative star coordinates.
- `outputs/M2_DS-B_report.csv`: all 32 rows, full filenames, 22 columns, UTF-8 BOM, semicolon delimiter, decimal point. Columns distinguish detected peaks, confirmed stars and measured photometry stars. It includes reasons, actual thresholds and a representative star's source x/y/HFR.
- `outputs/M2_DS-B_report.json`: complete precision, configuration, phase timings and up to 250 measured stars per frame.
- `outputs/M2_DS-A_report.json` / `.csv`: required full-dataset G2 run.
- `outputs/M2_report_timing.json`: complete command timings, including report serialization/publication.
- `outputs/M2_validation.json`: independent CSV/JSON/counter/median/input-integrity validation.

No input was moved, modified or deleted. Rejected frames are selection recommendations and remain available for manual review. Raw FITS/XISF images and generated reports remain excluded from Git. The 32-row Markdown table contains privacy-safe measurements, not image data or GPS coordinates.

## Automatic Gates

| Gate | Result |
| --- | --- |
| Format and warnings-denied Clippy | PASS |
| Release Rust tests | PASS: 52/52 (9 CLI, 16 FITS, 10 quality/photometry, 17 reconstruction) |
| Legacy PI adapter contracts | PASS: 4/4; M2 has no PI dependency |
| Gaussian HFR | PASS: 192 cases across four CFA patterns, three widths and 16 subpixel phases; maximum relative error 2.937751%, below 3% |
| Quality classification | PASS: synthetic blur/cloud/tracking, exposure groups, finite provisional weights, majority compatibility and insufficient-population behavior |
| Immutable I/O / safety | PASS: both decoder modes agree with all 8,294,400 independently decoded real samples; source SHA-256 unchanged; no intermediate images |
| Real DS-B table | PASS: all 32 CSV rows match JSON; catalog medians/coordinates and weights independently checked |
| Full DS-A speed | PASS: 1,140 frames, complete command 14.342 s < 15 s |
| Owner M2/G2 review | PENDING |

All 34 DS-B manifest files and the accepted M1 FITS still match their original hashes. Optimizations retain the same frame selections, counts and metrics as the preceding full/32-frame runs (within 1e-9 for floating-point metrics). Reports contain no GPS keywords, weights are finite in 0..1 and accepted weights normalize to one. Temporary image outputs are absent. DS-A size/write-time snapshots also remained unchanged during benchmark validation.

## Measured Performance and Selection

Release processing excludes CLI startup and report serialization; complete-command time includes both. Compilation is excluded from both measures. OS cache was uncontrolled and repeated runs may benefit from cache: these are observed machine timings, not cold-storage guarantees or a PixInsight speedup comparison.

| Dataset | Frames | Processing s | Complete command s | Peak working set MiB | Accepted | Rejected |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| DS-B selected set | 32 | 0.401 | 0.447 | 552.3 | 27 | 5 |
| DS-A full set | 1,140 | 14.059 | 14.342 | 713.7 | 940 | 200 |

DS-B input/accepted exposure: 320 / 270 s. DS-A: 11,400 / 9,400 s. These are acquisition totals, not processing time or final registered-stack exposure. This is M2 preselection; M3 registration failures can alter later acceptance. Earlier complete full-set runs were about 16.9..17.1 s and missed G2. Forward-only chunked reads and per-handle Windows sequential-scan hints resolve the observed I/O bottleneck in the final run. Cache was uncontrolled, so no isolated attribution of the entire timing improvement is claimed.

## Measurement and Selection Conventions

- Decode immutable signed big-endian 16-bit source pixels plus BZERO. Never subtract BIAS or darks. Average the two native greens per CFA cell for detection (1080 x 1920 on the reference sensor); do not run RGB demosaicing or write intermediate images.
- Estimate sky/noise on a 16 x 16 spatial grid, with at most 256 stratified pixel samples per cell. One 3-sigma star/outlier clipping pass precedes recomputed medians and MAD. Global noise is the median pixel-noise estimate, not scatter of cell medians. Values use source ADU on the two-green average.
- Detect local 6-sigma peaks with local sky thresholds and a global floor. Reject isolated unsupported peaks and saturation above 62,000 ADU in any central CFA color. Count supported, unsaturated detections without a 250-star cap. Choose up to 250 photometry stars through deterministic 4 x 4 round-robin buckets, with 12 px separation and a source-edge margin. Counts are heuristic detections, not a verified astronomical object list.
- Native-green centroiding and a 7..10 px sky annulus precede a 6 px circular photometry aperture. HFR is cumulative half flux, with Lanczos-3 interpolation of the rotated native-green lattice and fractional pixel-area contributions. No VNG color values or clipped scientific output are used for M2 measurements. Moment FWHM is a Gaussian-equivalent second-moment diagnostic, not a fitted PI FWHM. Broad/faint/blended stars can be biased by the finite aperture and positive-flux rule; the Gaussian gate does not establish 3% accuracy for every real star.
- Defaults: HFR > population median + 2.5 robust scatter (0.05 px scatter floor), star count < 0.45 of its exposure-group median, eccentricity > 0.60, fewer than 10 measurable stars, unusable noise or incompatible sensor/target/filter metadata. Population rules require at least five comparable frames; count groups use exposure tolerance 1e-6. HFR is evaluated across compatible exposures. Relative criteria use the initially measurable population and report their actual limits.
- **Weights are provisional:** use `1 / ((noise_adu / EXPTIME)^2 * HFR^2)` and normalize accepted frames only. Atmospheric transparency has not been measured. M3 must match unsaturated stars, establish the actual photometric normalization/transparency and finalize weights exactly once. Registration-failure rejection is deferred to M3.
- Inspect inputs in batches of at most 16 frames, independently of total dataset size. Within a frame worker, decoding is sequential to avoid nested scheduling. Header parsing leaves the cursor at the pixel payload; retain a forward-only stream and bounded 1 MiB reads. Windows sequential-scan hints affect only a read-only handle; no global cache/preferences changes or C/C++ image dependencies are introduced. macOS uses the portable path; unavailable RAM telemetry remains null.

## Owner Actions and Reproduction

Review the five flagged rows in the 32-frame table against nearby accepted frames in PixInsight. In particular, `20261006-210159` has eccentricity about 0.6017, just over 0.60; review whether this borderline case should be kept. The remaining marked frames show stronger elongation; three also exceed the HFR limit. Inputs remain untouched regardless of classification.

Run from the project directory in Windows PowerShell with unused report names:

```powershell
cargo run --release --bin fulgurstack -- inspect --input "SeestarS50 Subs" --report "outputs/M2_owner_review.json" --csv "outputs/M2_owner_review.csv"
cargo run --release --bin fulgurstack -- inspect --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --report "outputs/M2_full_owner_review.json" --csv "outputs/M2_full_owner_review.csv"
```

Thresholds may be changed with `--hfr-sigma`, `--min-star-fraction` and `--max-eccentricity`. Keep defaults for the first comparison so decisions remain reproducible. M2 is a quality inspector; registration, matched-star transparency, stacking, GPU and GUI remain later milestones.
