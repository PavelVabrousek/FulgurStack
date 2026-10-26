# M4 Revision: Scaled External Registration References

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. Scientific implementation: `23915f48ebedf287679ef909e5c286e8fd9d30af` (subsequent rendered-endpoint test and documentation do not change production calculations).

**Full-run follow-up:** The owner approved the DS-B scaled products visually. The subsequent 1,140-frame run registered 1,140/1,140, integrated 940 frames on the complete 4000 x 4000 template grid and passed automated verification in 632.991 s complete-command time / 990.1 MiB peak working set. See [full scaled-reference run and performance report](M4_FULL_SCALED_RUN_REVIEW.md). Full-master visual review remains pending.

## Delivered behavior

Registration now accepts a separate FITS geometry reference at **33..300%** of the source pixel geometry. It fits positive uniform scale, rotation and translation; no shear/reflection. Existing native science frames still use the accepted CFA inspector and rigid session/photometric anchor pipeline. Default no-reference behavior and native manual references remain supported. Existing quality-rejected input references cannot bypass their rejection through the external path.

Use `--reference <external FITS path>` on either `register` or `stack`. A separately supplied reference is not counted as an exposure or integrated frame. It may be cropped or have different dimensions and need not carry EXPTIME. The reference supplies its pixel grid/crop only. Scientific brightness normalization, weights, acquisition exposure and observing-location provenance remain tied to the native accepted-median source anchor. Template brightness/color values are never integrated or used for photometric normalization. This also permits geometry from a processed reference without importing its brightness processing.

The provided `RegisteringTemplate200pct.fit` is a **4000 x 4000 planar RGB Float32** FITS with TOP-DOWN rows, not a raw CFA frame despite a leftover BAYERPAT card. Geometry uses its green channel. The general geometry importer also supports monochrome/RGB signed16 with BZERO/BSCALE, validates dimensions/padding/finiteness/cancellation and honors explicit row order. Absent ROWORDER defaults to BOTTOM-UP for reconstructed external FITS. It retains one plane, capped at 96 million pixels. Source CFA import rules remain unchanged.

## Real 32-frame test — PASS

- Source folder: `SeestarS50 Subs`, **32/32 geometrically registered**, **27 accepted / 5 rejected** by unchanged source quality selection.
- Geometry reference: project-root `RegisteringTemplate200pct.fit`, unchanged SHA-256 `3d1f0c73f04749391d18f7764912eac07f642589ac4a401f27eb1f9c3a6f6d61`.
- Native scientific photometric anchor: `Light_M 33_10.0s_IRCUT_20261006-233727.fit`; DATE-OBS `2026-10-06T21:37:05.722764`.
- Anchor-to-template scale: **199.990538%**. Per-frame fitted range: **199.948722..200.040424%**.
- Per-frame inlier RMSE: **0.314325..0.698889 reference pixels**; minimum **108** unique matched stars. Gate: at least ten matches and <=1.5 reference-pixel RMSE. Every residual was independently recomputed from serialized source/template coordinates and transforms.
- Registration-only processing: **1.735 s**, peak **573.0 MiB** (32-frame source inspection included; JSON/CSV serialization excluded).
- Stack processing: **18.752 s**. Complete prebuilt release command including startup, three FITS writes and JSON serialization: **18.962 s**. Process peak through FITS publication: **573.8 MiB**. Single runs, OS cache/background load uncontrolled; compilation and independent validation excluded.
- Master and maps: exact reference **4000 x 4000**, origin **(0,0)**, **no automatic crop**, accepted exposure **270 s**, native intensity-scale EXPTIME **10 s**. The template's crop defines the output field, so this is not the same workload as a full-native-sensor stack.

## Local review products

| Product | Path |
| --- | --- |
| Scientific master | `outputs/M4_DS-B_external200_stack.fits` |
| Surviving original-sample coverage | `outputs/M4_DS-B_external200_stack_coverage.fits` |
| Upper pixel rejection | `outputs/M4_DS-B_external200_stack_high_rejection.fits` |
| Registration manual-review table | `outputs/M4_DS-B_external200_registration.csv` |
| Registration catalog/transforms | `outputs/M4_DS-B_external200_registration.json` |
| Full stack report | `outputs/M4_DS-B_external200_stack.json` |
| Independent validation | `outputs/M4_DS-B_external200_stack_validation.json` |
| Complete command timer | `outputs/M4_DS-B_external200_stack_timing.json` |
| Native ordinary PI import check | `outputs/M4_DS-B_external200_stack_PI_validation.json` |

CSV contains geometric_scale separately from photometric_scale, rotation/translation, inlier count/RMSE, source quality, weight and rejection reasons. JSON distinguishes the native reference_anchor/photometry from external_reference. Source transforms map native pixels to the external grid, with source_to_photometric_anchor preserved separately; external match indices resolve against external_reference.stars. Session transforms and photometric matches stay in their explicitly labeled native catalogs.

The scientific master is fixed linear source ADU / 65,535, bounded at final export under the accepted policy. It records REGMODEL=SIM(2), GEOMSCL (native anchor-to-reference scale), external-grid crop provenance and native exposure/GPS provenance. Native XPIXSZ/YPIXSZ are omitted on this resampled grid; no plate solution/WCS is invented. Maps remain per-channel fractions of ALL 32 originals. Maximum coverage is **84.375% = 27/32**; these are sample counts, not weights. Image magnification does not create additional optical detail or change the number of collected exposures.

## Automated evidence

- fmt and Clippy with denied warnings: PASS. All **77 release Rust tests** and four legacy adapter tests pass locally, with real-FITS gates enabled.
- [Windows MSVC and macOS Apple Silicon CI](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39752324071) pass for scientific implementation `23915f4` (76 tests there). The subsequent added rendered-endpoint test and documentation do not change production calculations.
- Rendered Gaussian reference images at the actual 33% and 300% endpoints also produce at least 20 matched stars with <0.15 reference-pixel RMSE; this checks reference image detection as well as catalog geometry.
- Release gates include scale sweeps **0.33/0.5/1/2/3** crossed with **0/15/90/179.5 degrees**, missing/outlier stars, proper-rotation/reflection rejection, unsupported-scale rejection, insufficient matches and cancellation.
- Scaled inverse/composed geometry and strict masks agree with an independent full reconstructed source at fractional coordinates. Alternate output tile shapes produce identical pixels/maps for scales 0.33/0.5/2/3, including tile and sensor edges.
- RGB geometry import verifies green-channel selection, signed/scaled units, top/bottom orientation, truncation, nonfinite pixels and immutable fixtures. CLI verifies different template intensities do not change source photometry, EXPTIME/counts/weights or source hashes.
- Every real template residual, scale, map count, dimensions, range and metadata independently checked by `scripts/tests/verify-scaled-reference.py` (NumPy development-only).
- **Unit-scale regression PASS:** all three `M4_DS-B_scale_unit_regression` FITS are byte-identical to the owner-approved `M4_DS-B_reference_median` master/maps. Source acceptance, photometry, noise, transparency and weights are also exactly unchanged between the native and external-reference runs.
- Sample accounting: **1,234,322,968 used + 7,034,427 low rejected + 29,046,854 high rejected = 1,270,404,249 valid channel samples**. Map counts reconstruct these totals exactly.
- All 32 original sources, both existing PI reference fixtures and the new external template retain their original SHA-256 hashes. No calibrated/debayered/registered intermediate images or `.tmp` files remain.
- Native ordinary PixInsight import of all three new outputs: **PASS**, zero probe/value/orientation mismatches and all samples in 0..1. A separate hidden automation instance was closed without modifying the owner's session.

## Reproduction and visual review

Delivered files already exist and are never overwritten. Run from the repository directory with unused filenames:

```powershell
cargo run --release --bin fulgurstack -- register --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --report "outputs/M4_scaled_registration_review.json" --csv "outputs/M4_scaled_registration_review.csv"
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --output "outputs/M4_scaled_master_review.fits" --report "outputs/M4_scaled_master_review.json"
```

Open the delivered master normally in PixInsight and compare star positions/orientation/crop with `RegisteringTemplate200pct.fit`. Use display STF as desired. Review bright cores/colors and small stars; a geometric template with different brightness processing is not a photometric reference. Default maps are generated beside the master automatically.

This revision validates registration over the requested scale range and supplies the real 200% review image. It does not claim new optical resolution, a new absolute analytic CFA photometric guarantee or alias-free downsampling from the existing point-sampled Keys kernel. The earlier disclosed photometry/full-reference-quality limitations remain open; the owner approved this DS-B scaled master, while full-master visual review and final G4 sign-off remain pending. No GPU/GUI code was added. This focused 32-frame reference test is supplemented by the full DS-A run linked above.
