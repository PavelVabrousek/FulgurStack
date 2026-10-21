# M3: Direct-to-Anchor Rigid Registration and Canvas Planning

## Status

M2/G2 was approved and closed by the owner on 2026-10-22. M3 was authorized in the same message. M3 local automatic gates pass; owner G3 review is pending. Cross-platform verification of the exact implementation commit is tracked in the M3 draft PR. M4 has not started. Earlier PRs are not merged automatically.

## Review Artifacts

- [32-frame registration table and coordinate examples](M3_DS-B_REVIEW.md).
- `outputs/M3_DS-B_report.csv`: 32 rows, 18 columns, full source names, rotations/translations, unique inliers, residual RMSE, measured transparency, normalization, final weights and retained rejection reasons.
- `outputs/M3_DS-B_report.json`: complete inspection catalogs, direct/session transforms, local/bridge correspondences, photometric correspondences, exact common polygon and both proposed rectangular canvases.
- `outputs/M3_DS-A_report.json` / `.csv`: full-night required registration gate.
- `outputs/M3_report_timing.json`: complete prebuilt release command timings, including report publication, excluding compilation.
- `outputs/M3_validation.json`: independent correspondence/photometry/weight/CSV/canvas and source-integrity checks, with report SHA-256 fingerprints.

## Automatic Gates

| Verification | Result |
| --- | --- |
| Format and Clippy, warnings denied | PASS |
| Release Rust tests | PASS: 60/60 (10 CLI, 16 FITS, 17 pipeline, 17 reconstruction) |
| Legacy import adapter tests | PASS: 4/4 |
| Rotation recovery 0, 15, 90, 179.5, -179.5 degrees with centroid noise, missing stars and false detections | PASS: maximum transform recovery RMSE 0.002873 px, below 0.15 px |
| Real DS-A CFA subset rotated 180 degrees and shifted in memory | PASS: three frames; 244/246/243 inliers, transform error below 0.000001 px |
| Two-session anchor composition, mixed exposures and retained quality rejection | PASS |
| Saturated photometric-star exclusion, invalid coverage and missing anchor handling | PASS |
| Exact polygon intersection, weighted coverage and safe canvas | PASS |
| CLI source integrity, GPS redaction, no-clobber reports, no intermediate images | PASS |
| Independent real-data recalculation of every matched residual, photometric scale/transparency, final weight and CSV row | PASS |
| Independent exact weighted-coverage verification for every row of both selected canvases, DS-B and DS-A | PASS |
| All 34 DS-B source/reference SHA-256 fingerprints and accepted M1 FITS | PASS: unchanged |
| All DS-A source sizes and modification timestamps over the run | PASS: unchanged |

The optional real-CFA test requires `FULGURSTACK_TEST_FITS_DIR`; it ran locally against DS-A. Rotation and shift change only an in-memory copy, including CFA phase. No pseudo-night image is written. Synthetic gates run in CI; CI has no private observing data. A recovery error against known synthetic geometry is different from the residual of a real matched-star fit; neither alone proves the final integrated image quality.

## Measured Release Telemetry (2026-10-22)

| Dataset | Input / registered | Accepted / rejected | Inspection s | Registration + photometry + canvas s | Complete command s | Peak process working set MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| DS-B selected set | 32 / 32 | 27 / 5 | 0.238 | 1.090 | 1.396 | 679.6 |
| DS-A full night | 1,140 / 1,140 | 940 / 200 | 18.108 | 14.811 | 33.362 | 684.1 |

Processing wall time before report serialization: DS-B 1.328 s, DS-A 32.919 s. The process records peak RAM before serializing reports; this is not a separately sampled post-publication memory peak. OS cache is uncontrolled. The current full-night M3 inspection phase was slower than the previously accepted M2 G2 run (14.059 s); M2 inspection code is unchanged except visibility of its internal row constructor. Do not claim a new <15 s inspection result from this M3 run. No direct speedup claim against PI is made: reference selection and implementation differ, and M3 produces catalogs/plans rather than registered RGB disk images.

Accepted acquisition exposure totals are 270 s (DS-B) and 9,400 s (DS-A). All geometric registrations succeeded, including the quality-rejected diagnostic rows; no rejected frame was restored to the integration set.

- DS-B automatic anchor: `Light_M 33_10.0s_IRCUT_20261007-001131.fit`. Accepted-frame inliers: 157..250, maximum residual RMSE 0.539074 px (median over all rows 0.274769 px). Accepted rotations: -48.440379 to +15.904519 degrees. Methods: one identity anchor, one sequential-prediction match, 30 triangle fallbacks. The selected set is sparse in time, making global matching useful.
- DS-A automatic anchor: `Light_M 33_10.0s_IRCUT_20261007-002758.fit`. Accepted-frame inliers: 119..250, maximum residual RMSE 0.522040 px (median over all rows 0.271193 px). Accepted rotations: -46.123724 to +20.641192 degrees. Methods: one identity anchor, 770 sequential-prediction matches, 369 triangle fallbacks. Every frame fits directly to the same anchor within this session; prediction does not chain fitted transforms.
- DS-B common rectangle: origin (479, 1250), 1451 x 1579 px. >=80% effective-weight rectangle: origin (469, 226), 1324 x 3465 px.
- DS-A common rectangle: origin (413, 870), 1117 x 1295 px. >=80% effective-weight rectangle: origin (490, 303), 1156 x 3183 px.

All originals remain unchanged. GPS is excluded from reports and console output. Accepted M1 export SHA-256 remains `d60369afde3915ebb3a46d1cd159e8e674f4ae351e7b06ed33ce09959118d411`.

## Measurement Conventions and Limits

Registration uses original native-green CFA catalogs before RGB reconstruction. A proper rigid transform has scale exactly one: `q = R(theta) p + [tx, ty]`. Coordinates are zero-based original sensor pixel centers, top-down. Rotation in this convention is not celestial position angle. Origin translation can be large for a rotation about the field center. Local pair indices reference each row's named session-anchor catalog; session bridge pairs reference the master. The reference identity row uses self-matches and zero residual.

Header timestamps determine order and >6 h session gaps. If any timestamp is missing or invalid, report the single-session sorted-path fallback. Anchors maximize provisional quality within the middle half of accepted temporal ranks. No filename timestamp or hardcoded PI template is substituted. Both real datasets are one night; actual multi-night observing data remains unverified. Synthetic two-session and real-CFA pseudo-night gates cover composition and 180-degree flips.

Sequential predictions extrapolate angular and transformed-center velocities using header times. They seed nearest-pair searches only. Rotation-invariant local triangles, rigid-length checks and transform voting provide fallback. Deterministic two-pair consensus and weighted Procrustes SVD reject reflections and fix scale to one. Three 1.5 px inlier-refinement passes follow; fewer than ten unique pairs fails registration. Reported real RMSE is conditional on these inliers, not ground-truth accuracy or a residual of every detected star. Optical distortion, scale drift or a changed optical configuration is outside the rigid model and may cause failures.

Photometric scales are medians of unsaturated matched reference/source flux ratios; total flux ratios are separate medians of source/reference ratios. Transparency additionally removes the exposure-duration ratio. The same scalar scale/ADU sky offset applies to all future RGB channels. Exposure is not applied twice. Final noise is scale times source noise; final weights use inverse normalized noise squared and HFR squared, normalized to maximum accepted weight one. The nested inspection report intentionally retains M2 provisional weights; the outer registration rows contain final weights. Every real row uses direct master photometric matches; the documented session-bridge product fallback was not needed on either real dataset. Finite-aperture green flux and noise estimates retain the M2 limitations.

Canvas footprints represent strict inverse-bicubic sensor support. The common polygon is an exact convex intersection; rectangular canvases select the largest axis-aligned rectangle of safe integer pixel centers. Coverage uses accepted final weights, not a fraction of frame count or equal per-pixel acquisition time. The 80% rectangle may have variable coverage; final inverse sampling and clipping remain M4 responsibilities. No full-frame RGB image, registered intermediate or master was generated by M3.

## Owner Review and Reproduction

Open the DS-B CSV or linked manual table. Check the smooth rotation trend, high inlier counts, residuals below 1.5 px, unchanged five exclusions, and the two proposed crop sizes. The manual table includes separated source/anchor coordinate examples for checking the same stars in PixInsight. It uses an automatic anchor different from the original PI template; compare transforms only after accounting for reference coordinates.

From the project directory in PowerShell, choose unused report names:

```powershell
cargo run --release --bin fulgurstack -- register --input "SeestarS50 Subs" --report "outputs/M3_owner_review.json" --csv "outputs/M3_owner_review.csv"
cargo run --release --bin fulgurstack -- register --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --report "outputs/M3_full_owner_review.json" --csv "outputs/M3_full_owner_review.csv"
```

Use `--min-coverage 0.80` (default) to specify the weighted coverage threshold. Existing outputs are never overwritten. M3/G3 requires owner review before M4 resampling/integration and its final PixInsight master-quality comparison.
