# Benchmark Datasets and PixInsight References

## Current measured evidence — 2026-10-26

Current product/defaults and verification are in [CURRENT_STATUS.md](CURRENT_STATUS.md). M1–M4 are owner-accepted; M5 remains open. Latest Windows release gates are 194 default / 197 all-feature tests with actual FITS/AVX2/GPU (ADR-082: calibrated Sadr 40/40 in 47.9 s, Flaming Star 30/30 in 24.1 s, Dwarf3 dark-only 851/856 registered in 183.8 s, DS-B byte-identical; ADR-081: Sadr 40/40 in 21.2 s; Dwarf3 and DS-B unchanged; ADR-079 adds no performance claim; ADR-078: ASI2600MM IC 1805 22 × 300 s with master dark 22/22 in 11.5 s; ADR-077: Dwarf3 master dark build+save 0.48–1.11 s by method, saved master stacks byte-identically to its frames; ADR-076: Dwarf3 dark-calibrated stack 852/1098 in 92.5 s, DS-B without darks byte-identical to ADR-074; ADR-074 adds no performance claim; ADR-073: hot-pixel-robust star measurement: DS-B uses the same 27 frames, master RMS change 6.6e-6, so products are no longer byte-identical to ADR-070; Dwarf3 inspection 19.1 → 22.5 s). Do not substitute old gate counts or combine these distinct timing scopes:

| Scope | Stored observation | Evidence |
| --- | --- | --- |
| Historical complete 1,140-input CPU pipeline, external 200% reference | 247.6727114 s; peak 3397.9 MiB | [CPU summary](M5_CPU_OPTIMIZATION_SUMMARY.md) |
| Frozen-plan integration, 12/16/24-worker comparison and reader optimization | Full integration replay 162.7911621 → 135.0441689 s; exact whole master/maps | [Integration profile](M5_INTEGRATION_PROFILE_REVIEW.md) |
| AVX2 VNG, frozen native-plan integration, 24 threads | Paired 162.2608958 → 130.0071238 s; central median 21.8704137 → 15.8733048 s; exact whole master/maps | [AVX2 VNG](M5_CPU_VNG_SIMD_REVIEW.md) |
| Reused integration buffers vs AVX2 VNG control, 24 threads | Paired 112.7421006 → 105.8856032 s; central median 14.6702947 → 13.6289897 s; kernel CPU −48.8%; exact whole master/maps | [Buffer reuse](M5_CPU_BUFFER_REUSE_REVIEW.md) |
| Native 940-selected catalog replay | Paired median 17.0449013 → 10.1033996 s; 200 Use-off skipped; exact frozen-serial science | [Native walks](M5_NATIVE_REGISTRATION_WALKS_REVIEW.md) |
| Distortion repair | 937/940 catalog fits valid, 3 rejected; bounded 27/32-source integration 14.5014043 s, 724.0 MiB peak | [Acceptance repair](M5_GEOMETRIC_ACCEPTANCE_REPAIR.md) |
| Auto: real pixels with artificial three-night timestamps | ≥80% rectangle +28.854732%; mean overlap −0.902446 percentage point; chronological 4.8123964 s, Auto 5.6044817 s, Auto peak 651.3 MiB | [Auto reference](M5_AUTO_REFERENCE_REVIEW.md) |

The last fixture is not evidence from real multiple observing nights. Auto predictions use Use-on frame counts before rejection; final Coverage uses all original inputs after rejection. No latest full reference integration was rerun. Reuse stored comparable controls for routine work unless the owner explicitly authorizes a new reference benchmark. Preserve adverse results and timing/environment qualifications. All benchmark execution is local and release-only; Actions remain disabled.

## Historical dataset inventories and owner references

The dated inventories/settings below remain source/reference evidence, not current application defaults or new timing measurements.

## Evidence and Comparison Rules

Times below were reported by the owner on 2026-10-21, rather than measured by FulgurStack. Keep approximate values labelled as approximate. FulgurStack performance measurements must use release builds and report wall-clock time, accepted/rejected frame counts, and measured peak process memory separately.

The small and large datasets are independent benchmark cases. Do not extrapolate throughput, rejection behavior, memory use, or stage proportions linearly from 32 to 1,140 frames. Check equivalent input selection, output coverage, and image quality when comparing performance. Do not compare a single-frame M1 export directly with a 32-frame PixInsight batch time.

All source FITS and owner-supplied PixInsight artifacts are immutable. The supplied PI intermediates are reference material, not permission for FulgurStack to write intermediate images. Keep newly generated outputs outside both reference directories and never reuse reference output paths. GPS values are not included in this document or the manifest.

## DS-B: Project-Local 32-Frame Regression Dataset

Paths are relative to the FulgurStack project root:

| Artifact | Path |
| --- | --- |
| Raw source directory | `SeestarS50 Subs/` |
| PixInsight intermediate directory | `PixInsight Files/InterimFiles/` |
| Reference anchor source copy | `PixInsight Files/Registration template Light_M 33_10.0s_IRCUT_20261006-232807.fit` |
| PixInsight integrated master | `PixInsight Files/integration.fit` |
| Source and FITS reference fingerprint manifest | `docs/ds-b-manifest.json` |

### Read-Only Inventory

Verified on 2026-10-21:

- 32 source FITS files; 531,025,920 bytes in total.
- All source headers identify M33 (`M 33`), `IRCUT`, 10 s exposure, gain 200, 2160 x 3840 pixels, `BITPIX=16`, and `GRBG` CFA.
- The original M1 reference sample, `Light_M 33_10.0s_IRCUT_20261006-204534.fit`, is included.
- The selected registration anchor's original source filename is `Light_M 33_10.0s_IRCUT_20261006-232807.fit`; that source is also included. The separately named template file is raw CFA (`BITPIX=16`, two axes), not an RGB image. Use the corresponding debayered image when comparing RGB registration results.
- Source header timestamps span `2026-10-06T18:45:12.145756` to `2026-10-07T00:15:22.431284`; this is acquisition span, not processing duration. Use header timestamps rather than filename clock times.
- The PI intermediate directory contains 64 XISF files. Their detailed processing settings and completeness at each stage have not been independently verified.
- The integrated master is a 2160 x 3840 x 3 FITS cube with `BITPIX=-32`, without `BZERO` or `BSCALE`. Header inspection does not verify pixel orientation, linearity, color accuracy, or image quality.
- `ds-b-manifest.json` records SHA-256 fingerprints and byte sizes for all 32 source files, the separate anchor copy, and the integrated FITS master. This establishes a reference for future integrity checks; it does not prove correspondence to the full DS-A dataset or fingerprint the 64 intermediate files.

### Owner-Reported Processing Times

| PixInsight stage | Approximate time |
| --- | ---: |
| Debayer, 32 frames | 32 s |
| Registration, selected middle-of-series anchor | 35 s |
| Integration, 32 images | 48 s |
| Sum of the three stages | 115 s |

The sum excludes manual file transfers and unreported steps. These small-dataset proportions must not replace the separate large-dataset timing assumptions. PixInsight version, debayer algorithm/settings, registration interpolation/settings, cache state, repeated-run variability, and peak process memory were not supplied.

### Owner-Reported Integration Configuration

| Setting | Value |
| --- | --- |
| Input image count | 32 |
| Pixel combination | Average |
| Output normalization | Additive + scaling |
| Weighting mode | PSF Signal Weight |
| Scale estimator | Biweight midvariance |
| Range rejection | `range_low=0.000000` |
| Pixel rejection | Generalized extreme Studentized deviate (ESD) |
| Rejection normalization | Scale + zero offset |
| Rejection clippings | Low enabled; high enabled |
| Rejection parameters | `esd_outliers=0.30`, `esd_alpha=0.05` |

FulgurStack retains the specified Winsorized Sigma Clipping method and quality weights. The PI reference does not change that implementation contract. Quality comparisons must evaluate stellar flux, star shape, background noise, color, and rejection artifacts after aligning geometry and intensity units; bitwise-identical output is not an acceptance criterion. Report any difference between FulgurStack's accepted input set and PI's 32 integrated images.

### Owner-Provided PixInsight Memory Diagnostics

| Diagnostic | Reported value |
| --- | --- |
| Available physical memory | 10.901 GiB |
| Allocated pixel buffer | 3840 rows; 0.988 GiB |
| Concurrent pixel stacks | 3840; 3.985 GiB |

These are individual diagnostics, not a measurement of peak process RSS or total physical RAM. Do not add them together and label the result as measured peak RAM. No rejected-pixel statistics were provided.

### Use Within Milestones

- **M1:** Compare single-frame CFA reconstruction against the supplied debayered references, check orientation and color in PixInsight, and verify source/reference integrity. The full DS-A I/O baseline remains required.
- **M2:** Use all 32 frames for rapid real-data inspection regressions. Keep the full DS-A performance gate.
- **M3:** Compare registration in the supplied anchor's coordinate system. Account explicitly for coordinate-system changes if the automatic anchor differs; retain the synthetic transform-recovery gates.
- **M4:** Compare the integrated result with the supplied master using matched geometry, intensity scale, measurement regions, and documented input selection. Record overall and stage timings without replacing the full DS-A quality, memory, and speed gates.

## DS-A: Full 1,140-Frame Dataset

| Artifact | Absolute local path |
| --- | --- |
| Raw sources | `C:\Astrofoto\2026\10 October\SeeStar50P M33\M33 Subs` |
| PixInsight master | `C:\Astrofoto\2026\10 October\SeeStar50P M33\integration.xisf` |

The verified header inventory contains 1,140 compatible 10 s M33/IRCUT FITS files. The owner confirmed approximately three hours for the complete PixInsight workflow. The exact accepted reference input list remains unverified. The target remains <=18 minutes and >=10x speedup with comparable image quality, qualified against the approximate owner-reported baseline. DS-A provides no real mixed-exposure validation.
