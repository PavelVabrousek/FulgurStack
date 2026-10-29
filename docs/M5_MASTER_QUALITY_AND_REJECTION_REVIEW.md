# M5: Master Quality Metrics and Pixel Rejection Methods (2026-10-29/30)

> **Documentation context — 2026-10-30:** dated review of ADR-086 and ADR-087 and the GUI iterations published with them (ADR-088). See [current state](CURRENT_STATUS.md) for authoritative behavior. M5 remains open.

## What was delivered

- **Master quality (ADR-086).** The finished master is measured with the same star detector as the frames:
  - median HFR, FWHM and eccentricity, each next to the median of the stacked frames;
  - sky background in ADU and on the 0..1 FITS scale;
  - per-pixel sky noise per colour channel (R/G/B, or one value for mono).
  - Output: the CLI line `Master quality: …`, the report block `master_quality` and GUI Results. The master FITS and maps are unchanged.
- **Pixel rejection methods (ADR-087).** Each method decides which samples of a pixel survive, and the survivors are averaged with their frame weights, as before:
  - winsorized sigma clipping (3.0 / 2.5 σ, unchanged);
  - generalized ESD (α 0.05, at most 30% of the stack);
  - linear fit clipping (5 / 4 σ);
  - median (no weights).
  - **Auto** chooses by frame count: median below 6 frames, winsorized below 25, ESD from 25. It is the CLI and GUI default. `IntegrationConfig::default()` stays winsorized for library callers.
  - CLI: `stack --rejection auto|winsorized|linear-fit|esd|median`. The report records both the requested method (`config.rejection`) and the applied one with its parameters (`integration.rejection`, `integration.rejection_rule`).
- **GUI.**
  - Settings card **Stacking and outlier rejection**: one radio button and hint per method, plus a line showing the method Auto will apply to the next stack (Use-on frames once measured, else all frames of the included light sets). The choice is saved with the preferences and shown in Results. It was first placed under Advanced and has moved out of it.
  - Frames:
    - quick selections All / best 95 / 90 / 80 / 50 % per light set;
    - a three-lane quality plot (HFR, eccentricity, sky) that follows the current list order and filter and redraws on every list change;
    - collapsible light-set details;
    - a "Review and confirm…" button for derived header values;
    - a wider preview divider (up to 70% of the window).
  - All Settings, Processing and Results cards scroll instead of overlapping when the window is small.

## Real-data comparison (Windows 11, release)

Integration time, rejected samples, master noise per channel and master HFR (ADR-086 metric).

| Stack | Method | Integration s | Rejected low / high | Noise R / G / B ADU | HFR px |
| --- | --- | --- | --- | --- | --- |
| Seestar DS-B, 27 × 10 s | winsorized | 4.6 | 0.59% / 2.35% | 18.96 / 13.23 / 17.25 | 1.714 |
| | linear fit | 7.6 | 0.11% / 0.67% | 18.10 / 12.76 / 16.69 | 1.725 |
| | **ESD** (Auto) | 6.8 | 0.19% / 0.52% | **18.11 / 12.74 / 16.54** | 1.717 |
| | median | 4.0 | 0 / 0 | 22.01 / 15.41 / 19.93 | 1.710 |
| ASI2600MC Flaming Star, 30 × 300 s, calibrated | winsorized | 16.0 | 0.90% / 1.21% | 11.36 / 10.68 / 9.67 | 1.402 |
| | linear fit | 24.3 | 0.23% / 0.44% | 11.09 / 10.62 / 9.30 | 1.407 |
| | **ESD** (Auto) | 19.7 | 0.26% / 0.19% | **11.07 / 10.45 / 9.27** | 1.398 |
| | median | 14.7 | 0 / 0 | 12.86 / 11.67 / 11.83 | 1.432 |
| ASI2600MC M42, 235 frames 30–300 s, 4 nights | winsorized | 109.1 | 0.67% / 2.54% | 0.99 / 0.92 / 0.78 | 1.643 |
| | linear fit | 239.3 | 0.86% / 2.16% | 0.99 / 0.91 / 0.78 | 1.833 |
| | **ESD** (Auto) | 153.9 | 0.07% / 0.17% | **0.98 / 0.88 / 0.75** | 1.831 |
| Dwarf3 C 20, 856 × 60 s, darks | winsorized | 117.3 | 0.66% / 2.49% | 0.61 / 0.47 / 0.39 | 1.463 |
| | **ESD** (Auto) | 173.3 | 0.15% / 0.40% | **0.60 / 0.47 / 0.38** | 1.472 |

**Findings:**
- **Noise.** ESD gives the lowest master noise in every set. It rejects 0.2–0.7% of samples, against about 3% for winsorized clipping.
- **Cost.** ESD needs about 40–50% more integration time than winsorized on the large stacks. Peak RAM is unchanged (≤ 3.6 GiB).
- **Median.** The median costs 16–22% more noise.
- **M42 HFR.** On M42 the master HFR differs by method: winsorized clipping trimmed the wider star profiles of the long exposures. ESD keeps them, which matches the true mean profile.
- **Reproducibility.** Explicit `--rejection winsorized` reproduces earlier masters byte for byte (DS-B and Flaming Star verified). With Auto, stacks of 25 or more frames change.
- **Distribution build.** The CLI from the Windows distribution package (ADR-089) stacked DS-B with Auto. Its master, coverage map and high-rejection map are byte-identical to the standard release build.

Master quality examples (winsorized, before ADR-087):

| Set | HFR px | Noise R / G / B ADU |
| --- | --- | --- |
| DS-B | 1.71 | 18.96 / 13.23 / 17.25 |
| Flaming Star | 1.40 | 11.36 / 10.68 / 9.67 |
| Sadr (mono) | 1.29 | 13.04 |
| M42 | 1.64 | 0.99 / 0.92 / 0.78 |

The test for this method is `known_gaussian_stars_give_their_size_shape_and_sky`: synthetic Gaussian stars of known size, shape and sky are recovered within tolerance.

## Verification

- Gates:
  - `cargo fmt --all -- --check`: PASS.
  - `cargo clippy -D warnings`, default and all features: PASS.
  - `cargo test --workspace --release`: 210/210 default and 213/213 with all features. The all-features run had the real FITS, SIMD and GPU variables set.
- Synthetic rejection tests:
  - Student-t quantiles and ESD critical values (NIST n = 54: λ₁ = 3.159, λ₁₀ = 3.085);
  - satellite trails rejected;
  - an even level spread survives linear fit;
  - Gaussian noise loses at most 2 of 200 samples with ESD;
  - stacks under 5 samples are not clipped;
  - Auto thresholds and dispatch.
- GUI test: quick selections keep the best share of each light set.
- The Settings card is UI only. It is release-compiled and clippy-clean, without automatic functional tests (owner UI policy).

## Owner action steps

```powershell
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --rejection esd --output "outputs/review_esd_new.fits" --report "outputs/review_esd_new.json"
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --rejection winsorized --output "outputs/review_winsorized_new.fits" --report "outputs/review_winsorized_new.json"
```

In PixInsight, compare the background noise and the faint stars of the two masters. Also compare the stored M42 masters `outputs\rejection_20261029\m42_esd.fits` and `m42_winsorized.fits`, especially the star sizes. In the GUI, check:
- the Settings card;
- the "Next stack" line after measuring quality;
- the Pixel rejection and master quality rows in Results.

## Not verified / limits

- The owner has not yet compared the ESD and winsorized masters in PixInsight.
- GUI changes are awaiting manual acceptance.
- Linear fit was not run on Dwarf3; median was not run on M42 and Dwarf3.
- The master metrics follow the frame detector. They are comparable with the frame values, but they are not a PixInsight FWHMEccentricity or NoiseEvaluation measurement.
- Windows only; macOS not run.

Sanitized numbers: [evidence](M5_MASTER_QUALITY_AND_REJECTION_EVIDENCE.json).
