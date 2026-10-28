# M5: Hot-Pixel-Robust Star Measurement and Scale/Deformation Fallback (2026-10-28)

Owner-requested engine changes ADR-073 and ADR-074; M5 remains open. Published in the owner-authorized consolidated commit of 2026-10-28 (ADR-075). Current behavior is summarized in [current state](CURRENT_STATUS.md).

## Trigger

A real uncalibrated Dwarf3 mini set (target C 20, Duo-Band, 1098 × 60 s, nine nights, sensor 27–35 °C, no darks) stacked **1 of 1098** frames with geometric correction enabled and a manually selected reference. The report showed successful rigid registration statistics but 836 of 837 quality-accepted frames refused by absolute whole-field validation (`native_insufficient_field_evidence`, 0–9 of ≈245 witnesses).

## Diagnosis

1. **Hot pixels in the star catalog.** About half of the 250 catalog "stars" were single-sample defects at identical sensor positions in frames hours and weeks apart. Their neighbour share (median excess of the four nearest measurement-plane neighbours / peak excess) stayed below 0.04 (99th percentile), against ≈0.4 for real stars. Sequential registration locked onto this fixed pattern (near-identity transforms), so real stars did not land on the reference.
2. **Hot pixels inside real stars.** After filtering detections, defects inside stellar apertures still pulled flux-weighted centroids by 2–3 px (one reference star with a defect 4 px from its core caused 315 of 751 errors above 3 px).
3. **Plate-scale change between nights.** The remaining 107 refusals had normal registration but 1–2 px residuals in opposite corners. A direct rigid refit changed nothing (≤ 0.06 px); a similarity fit passed 88 of them. Scale differs from the reference night by +2…+6·10⁻⁴ (refocus/temperature, growing towards morning: 1–4% refusals at 22–00 h, 20–30% at 02–03 h), slightly anisotropic (refraction). Single-night Seestar data do not show it.

## Changes

- **ADR-073.** Star measurement (quality inspection and native registration) works on a measurement-only copy in which isolated plane-sample defects (> 10 σ above the neighbour median, neighbours < 10% of the excess) are replaced by the neighbour median; every detection must also show a natural multi-sample profile (neighbour share ≥ 0.1). Sources, sky background/noise statistics and integration are unchanged. Inspection JSON reports `suppressed_hot_pixels` and `refused_defect_detections`.
- **ADR-074.** With geometric correction enabled, a frame whose rigid mapping fails absolute validation is retried with a similarity (30–333%) fitted on training witnesses, then with the bounded cubic candidate; a frame that rigid registration cannot register is retried with the external-reference similarity search. All use the unchanged absolute gates on fixed witness identities. Frames passing rigid validation are bit-identical.

## Results (Windows 11, release)

| Run | Stacked | Notes |
| --- | --- | --- |
| Dwarf3, before | 1 / 1098 | 836 refused, hot-pixel lock |
| Dwarf3, detection filter only | 308 / 1098 | centroids still biased |
| Dwarf3, ADR-073 | 771 / 1098 | 107 refused by corner geometry; 156.2 s, peak 2.75 GiB |
| Dwarf3, ADR-073 + ADR-074 | **859 / 1098** | 88 `native_similarity_validated` (scale 1.00024–1.00070), previous 771 unchanged; 150.5 s, peak 3.01 GiB |

- Quality acceptance 837 → 878 (eccentricity rejections 259 → 220); about 37 700 suppressed samples per frame (median); inspection 19.1 → 22.5 s, peak 721 → 1027 MiB. The optimized suppression pass produces catalogs identical to the unoptimized reference pass.
- 19 Dwarf3 frames remain refused, each by a single mismatched witness (4–12 px); the limits were not relaxed.
- **DS-B (Seestar S50, precalibrated):** 0–8 suppressed samples per frame; identical quality decisions; the same 27 frames stacked natively and against the 200% template. Transforms move ≤ 0.002 px (Auto) / ≤ 0.018 px (template); master RMS difference 6.6e-6 / 8.7e-6 at a mean level of 0.034, so DS-B products are no longer byte-identical to ADR-070/071. With geometric correction enabled, no ADR-074 fallback is triggered.
- Gates: format, default/all-feature Clippy PASS; release suites 177/177 default and 180/180 all-feature with real FITS, required AVX2 and actual GPU (0 ignored). New synthetic gates cover CFA/mono defect clusters and in-aperture defects (all suppressed, all stars kept, centroid < 0.1 px) and +0.12%, 200% and 50% scale recovery with < 1e-6 scale/rotation error; 400% is never accepted.
- No source file changed. Sanitized numbers: [evidence](M5_HOT_PIXEL_AND_SCALE_EVIDENCE.json).

## Not verified / limits

- Owner PixInsight review of the Dwarf3 master corners; no darks were available, so calibrated Dwarf3 behavior is untested.
- The cubic fallback was not needed on real data (exercised only by construction); 30–333% native recovery was verified synthetically, not with real mixed-focal-length sets (different sensor sizes are still refused at inspection).
- Windows only; macOS not run.
