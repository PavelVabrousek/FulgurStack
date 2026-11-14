# M5: Light-Frame Calibration — Bias, Darks, Flats (2026-10-29)

Owner-requested calibration work, ADR-076 to ADR-082 (with the registration fixes ADR-081 and ADR-082 found by the calibration tests). M5 remains open. Published in the owner-authorized consolidated commit of 2026-10-29 (ADR-083). Current behaviour is summarized in [current state](../CURRENT_STATUS.md).

## What is applied

For every light of a light set with a calibration set, before star measurement (quality, registration catalogs, photometry) and before cosmetic correction and demosaicing in integration (CPU and GPU):

```
calibrated = (light − bias − (dark − bias) · t_light / t_dark) / normalized flat + pedestal
```

| Masters present | Offset term | Notes |
| --- | --- | --- |
| bias + dark | `bias + (dark − bias) · t_light/t_dark` | thermal signal scaled by exposure; `dark_scaled_by_exposure` noted per frame |
| bias + dark, dark exposure unknown | `dark` | unscaled; `dark_exposure_unknown` |
| dark only | `dark` | unscaled; another exposure gives `dark_exposure_differs` (ADR-080) |
| bias only | `bias` | |
| flat | divided by the flat normalized per 2×2 Bayer phase (mono: harmless) | the flat lamp's colour does not tint the lights; vignetting and dust divide out |

- **Pedestal:** the rounded median of the master bias (else of the master dark). It keeps the calibrated sky in its original offset range, so the 0..1 export never truncates background noise; a constant cancels in the per-frame background normalization.
- **Saturation:** raw samples at or above 65000 ADU stay saturated (65535) instead of receiving an exposure-scaled, possibly also saturated, dark or a flat division.
- **Dark defects:** a master-dark hot pixel (scaled thermal signal > 50 ADU) whose light holds less than half of that signal, or whose over-subtraction would go negative, is replaced by the median of its calibrated same-phase neighbours. This is the cosmetic correction a master dark makes possible; ordinary pixels stay linear and untouched.
- **Flat defects:** normalized flat values outside 0.05–20 are not divided by.
- **Masters:** built in RAM (Average, Median, Average sigma-kappa 2), never written unless saved. Master flats from frames subtract the master dark flat (else the master bias) before normalization. Loaded master flats are treated as raw when they are integer and lack `OFFSETRM = T`; the offset master is then subtracted when calibration is prepared. Master files may be 8/16/32/−32/−64-bit, one plane or three identical planes; float masters normalized to 0..1 are scaled by 65535.

## Real data (Windows 11, release)

| Dataset | Calibration | Stacked / registered | Negative samples per frame | Corner/centre sky (offset removed) |
| --- | --- | --- | --- | --- |
| ASI2600MM Sadr DualBand, 40 × 300 s | 100 bias, 10 darks 180 s (scaled ×1.667), 61 flats 20 ms minus bias | 40/40 (100%), 47.9 s, 3.27 GiB | 0 (median 1 dark defect repaired) | left corners 0.93–0.94 → 1.00–1.01; right corners 1.13 → 1.15–1.18 (sky gradient, not vignetting) |
| ASI2600MC Flaming Star, 30 × 300 s | ASIAIR master bias, master dark (no EXPTIME → unscaled), master flat (integer → bias subtracted here) | 30/30 (100%), 24.1 s, 3.04 GiB | 0 | R 0.90–0.93 → 0.95–0.98; G 0.91–0.96 → 0.97–1.01; B 0.91–0.95 → 0.96–1.00 |
| same, flat built from 31 flat frames minus the master bias | as above | 30/30, 27.8 s, 3.91 GiB | 0 | identical to the master-flat run; phase medians 13885/34594/34618/28613 ADU vs 13886/34594/34618/28613 |
| Dwarf3 mini C 20, 1098 × 60 s | 37 darks (no bias) | 851 stacked, 851/856 registered (99.4%), 183.8 s | ≤ 1 (median 2420 dark defects repaired, max 28 281) | — |
| Seestar S50 DS-B, no calibration | — | 27/32 | — | master and both maps byte-identical to ADR-074 (native and 200% template) |

The colour flat has very different phase levels (R 13.9k, G 34.6k, B 28.6k ADU); per-phase normalization keeps the camera's colour response (channel levels unchanged) while removing vignetting.

## Problems found and fixed during the tests

1. **Negative samples (mono).** A naive `light − bias − 1.667 (dark − bias)` gave about 30 negative samples per Sadr frame: saturated hot pixels saturated in both light and dark, and dark frames hit by cosmic rays. Fix: saturated raw samples stay saturated; the sigma-kappa master rejects the cosmic rays. Remaining: one unstable pixel, now repaired.
2. **Negative samples (Dwarf3).** Up to 3700 per frame, strongly dependent on the light's sensor temperature (25 °C: 2550, 31–36 °C: ~330) against 26–31 °C darks recorded a year earlier. A global dark optimization (one scale factor per light) helped cold frames but made warm ones far worse (up to 12 000), because part of the dark's hot pixels no longer exist in the lights. Fix: the dark-defect repair above; negatives ≤ 1 per frame.
3. **Registration below 90% (colour).** 23/30: coverage was complete (99%), but two 250-star catalogs of a 25 000-star field shared only 32–47 accuracy witnesses (minimum 50). Fix (ADR-082): when the ordinary witnesses are too few although coverage passes, the anchor's 1000 precisely measured stars serve as accuracy witnesses within 3 px; 30/30, worst RMSE 0.64 px, worst error 1.24 px. Frames that pass with the ordinary witnesses are unchanged.
4. **Calibration data notes for the owner.** The Sadr flats were taken at rotator 179° while the lights were at 2°: vignetting divides out, but dust shadows may not match (the GUI now warns about rotator differences above 5°). The ASIAIR master dark records no exposure; it is assumed to match the lights.

## Not verified / limits

- Owner PixInsight review of the calibrated masters.
- Dark scaling by temperature, dark optimization and flat-dark scaling are not implemented; bias is required for exposure scaling.
- Per-phase normalization applies only the spatial shape of each Bayer phase of the flat; the overall colour balance of the lights is left to colour calibration in post-processing.
- GUI calibration screens are release-compiled for manual review. Windows only; macOS not run.

Sanitized numbers: [evidence](M5_CALIBRATION_EVIDENCE.json).
