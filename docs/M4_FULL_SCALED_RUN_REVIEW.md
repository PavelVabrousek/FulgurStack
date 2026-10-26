# M4 Full DS-A Run with the 200% External Reference

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. Program revision: `bbb316350f7a4bc0c40827793c8cd2757d4ade4e`.

The owner approved the 32-frame scaled-reference products visually and requested the full 1,140-frame set against `RegisteringTemplate200pct.fit`, without additional cropping. Execution, source integrity, geometry/map verification and native PixInsight import: **PASS**. The owner visually approved this full master and closed **M4/G4 on 2026-10-22**, authorizing M5 GPU acceleration. No production algorithm was changed for this run. The documented analytic-photometry and unmatched PixInsight-workload limitations remain limitations.

## Delivered products

| Product | Local path | SHA-256 |
| --- | --- | --- |
| master | `outputs/M4_DS-A_external200_stack.fits` | `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc` |
| coverage | `outputs/M4_DS-A_external200_stack_coverage.fits` | `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5` |
| high_rejection | `outputs/M4_DS-A_external200_stack_high_rejection.fits` | `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970` |

All three files are **4000 x 4000 planar RGB Float32 FITS**, `BITPIX=-32`, top-down, output origin **(0,0)**. They retain the complete existing template grid/crop; no further automatic crop was applied. The template itself covers a cropped field at approximately 200% image magnification relative to the native sources; this does not imply retention of the full uncropped source sensor field.

- Main detailed report: `outputs/M4_DS-A_external200_stack.json`.
- Compact performance/delivery summary: `outputs/M4_DS-A_external200_summary.json`.
- Independent sample/map/geometry/hash validation: `outputs/M4_DS-A_external200_stack_validation.json`.
- Complete command timer: `outputs/M4_DS-A_external200_stack_timing.json`.
- Native import proof: `outputs/M4_DS-A_external200_stack_PI_validation.json`.
- Pre-run source/template integrity check: `outputs/M4_DS-A_external200_integrity_before.json`, checked against `outputs/M4_DS-A_full_input_integrity_before.json`.

## Input selection and reference

Input folder: `C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs`, **1,140 original M33/IRCUT 10 s frames**, totaling 18,917,804,160 bytes.

- Successfully registered: **1,140/1,140**.
- Quality accepted and integrated: **940**; rejected: **200**. External registration introduced no additional frame rejection.
- Source quality reason counts (overlapping, do not sum): `{'excessive_eccentricity': 193, 'excessive_hfr': 86, 'low_star_count_in_exposure_group': 2}`.
- Total original acquisition exposure: **11,400 s**; accepted acquisition exposure: **9,400 s**. Output EXPTIME remains **10 s** and TEXPTIME is **9,400 s**. These are acquisition metadata, not processing time or uniform per-pixel exposure.
- Geometry reference: `RegisteringTemplate200pct.fit`, **4000 x 4000 RGB Float32**, SHA-256 `3d1f0c73f04749391d18f7764912eac07f642589ac4a401f27eb1f9c3a6f6d61`.
- Native photometric/exposure/GPS anchor: `Light_M 33_10.0s_IRCUT_20261006-232530.fit`, FITS DATE-OBS `2026-10-06T21:25:09.047658`; accepted chronological median under the existing policy.
- Native-anchor-to-template scale: **199.998266%**. Accepted per-frame fitted scales: **199.937376..200.041244%**.
- Accepted inlier RMSE range: **0.259524..0.647703 reference pixels**; minimum **79** unique matched stars. Gate: at least ten pairs and <=1.5 reference-pixel RMSE. Each serialized match residual was independently recomputed from source/template star coordinates.

The external template supplies geometry only. Its brightness, pixels, exposure and any observing-location metadata do not contribute to the scientific stack. Native source normalization and weights remain in force; GPS in the scientific master belongs to the native photometric anchor. Output FITS records SIM(2)/GEOMSCL and external-grid crop provenance, omits stale native detector-pitch cards and contains no invented WCS. The scientific output uses the accepted fixed linear source ADU / 65,535 range with final sensor bounding; no image-extrema normalization or BIAS subtraction.

## Measured release performance

- **Complete command: 632.991 s = 10 min 32.991 s.** Includes prebuilt cargo startup, processing, three final FITS writes and JSON serialization.
- **Processing through FITS publication: 631.724 s.** Excludes JSON serialization and startup.
- **Throughput: 1.805 original frames/s** using the processing interval.
- **Peak process working set through FITS publication: 990.1 MiB = 0.967 GiB**, below the 12 GiB ceiling.
- Planned integration buffers/runtime reserve: **1737.7 MiB**. **512** output tiles, nominal **256 x 128**, at most **16** concurrent source ROIs. No full registered-frame collection.
- Host: AMD Ryzen AI 9 HX 370, 12 physical/24 logical processors, 16 CPU workers. GPU unused.

The <=18 minute full-run target passes. This is a single measured run with uncontrolled OS cache/background load. Input SHA-256 hashing preceded execution, so it is not a controlled cold-cache benchmark. Compilation, hashing, post-run validation and PixInsight checks are outside the timed processing interval. The previous native-grid run used a different field/grid and cannot establish a like-for-like performance regression or quality-equivalent PI speedup.

### Non-overlapping processing phases

| Phase | Wall seconds | Processing share |
| --- | ---: | ---: |
| inspection | 17.408 | 2.76% |
| registration | 26.957 | 4.27% |
| reconstruction_batches | 442.753 | 70.09% |
| rejection_accumulation_maps | 132.635 | 21.00% |
| output_copy | 0.430 | 0.07% |
| three_fits_writes | 1.166 | 0.18% |
| other_processing | 10.374 | 1.64% |

Integration totals **585.869 s**. Reconstruction batches (raw ROI read/decode, cosmetics, VNG, inverse warp/normalization, scheduling/waits) dominate at **70.09%**. Gathering samples, clipping, compensated accumulation and maps account for **21.00%**. Other processing includes allocation, buffer lifecycle and small setup overhead.

The detailed JSON also contains per-ROI elapsed read/cosmetic/VNG/resampling sums. Those tasks overlap and can include nested scheduling waits: they are neither additive wall phases nor sampled CPU percentages. Decoded ROI area is 7,454,242,911 pixels. No GPU implementation or new optimization benchmark was introduced for this run.

## Coverage and high-rejection maps

Both maps use **all 1,140 original inputs** as denominator. Fractions 0..1 mean 0..100%; maps are channel-dependent counts, not weights or brightness. Coverage counts samples surviving whole-frame selection, geometry and low/high pixel rejection. High rejection counts only upper-rejected channel samples; it excludes frame rejection, missing geometry, low rejection and export sensor bounding.

- Coverage minimum **39.298245%**, maximum **82.456142%**, full-image/channel mean **79.472048%**. Maximum is consistent with 940/1,140 accepted inputs.
- High-rejection maximum **19.736843%**, mean **1.275699%**.
- Geometrically valid frame counts before pixel rejection range from **492 to 940** across the grid; no zero-geometry-coverage output pixels.

Independent integer count reconstruction from every Float32 map sample verifies:

**43,487,104,590 used + 85,995,586 low rejected + 698,062,316 high rejected = 44,271,162,492 valid channel samples.**

The scientific writer bounded 170 channel samples above the normalized sensor limit and 0 below zero under the owner-approved range policy. These events are recorded separately from high/low rejection. All final files contain finite samples in 0..1, with direct fraction metadata on maps and truthful native exposure/GPS provenance on the master.

## Verification and owner review

- fmt / Clippy warnings denied / **77 release Rust tests**: **PASS**, real-FITS gates enabled.
- [Windows MSVC and macOS Apple Silicon CI for this exact program revision](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39753296482): **PASS**.
- All 1,140 input SHA-256 hashes and the external template hash match their pre-run values. Input inventory unchanged. No source file modified.
- Exact template dimensions/origin, accepted scale range/residuals, every map count, exposure metadata, private GPS preservation and memory budget: **PASS**.
- Zero calibrated/debayered/registered intermediate images; no `.tmp` remains.
- Ordinary native PixInsight opening of all three files: **PASS**, zero probe/value/orientation mismatches or out-of-range samples. Validation used a separate hidden automation instance, then closed it without modifying the owner's session.

Open the delivered master with ordinary PixInsight **File > Open** and inspect stars, faint structure and edges against the template's geometry. Maps can use a display STF; underlying values remain fractions. The owner subsequently approved this full master visually and closed M4. Automated import/geometry checks alone are not image-quality sign-off; the previously documented quantitative/absolute-photometry and unmatched PI-workload limitations remain explicit.

To repeat from the project directory, use an unused output prefix (the delivered files are never overwritten):

```powershell
cargo run --release --bin fulgurstack -- stack --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --memory-budget-mib 12288 --output "outputs/M4_DS-A_external200_repeat.fits" --report "outputs/M4_DS-A_external200_repeat.json"
```

The coverage/high-rejection names are generated automatically beside the master. The local independent checker for this run is `target/verify-full-external.py`, adapted from the tracked full-run checker; it writes JSON only and requires development NumPy, not a product runtime dependency.
