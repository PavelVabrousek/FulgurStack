# M5: Reconstruction of Missing Header Values and Camera Catalogue (2026-10-29)

Owner-requested work, ADR-084. Local and unpublished. M5 remains open. Current behaviour is summarized in [current state](CURRENT_STATUS.md).

## Problem

Lights from a ZWO ASI2600MC Air had already been calibrated by another program (`c_Light_M 42_270deg_30.0s_Bin1_2600MC_gain100_…_SQA_0001.fit`). The program kept only SIMPLE, BITPIX, NAXIS, BZERO/BSCALE, CREATOR, INSTRUME, DATE(-OBS), XPIXSZ/YPIXSZ and CCD-TEMP in the header. EXPTIME, BAYERPAT, GAIN and OBJECT were missing. Inspection stopped with "invalid FITS" because a light without EXPTIME could not be read.

## What is reconstructed, and from where

| Value | 1. header | 2. other header card | 3. file name | 4. camera catalogue | Needs confirmation |
| --- | --- | --- | --- | --- | --- |
| Exposure | EXPTIME | EXPOSURE | `30.0s`, `300s`, `120sec`, `500ms` | — | yes |
| Colour layout | BAYERPAT | COLORTYP, CFAPAT, CFA-PAT, CFATYPE | `RGGB`/`GRBG`/`GBRG`/`BGGR` token | INSTRUME, CAMERA, TELESCOP, CREATOR, then file-name tokens such as `2600MC` | yes |
| Gain | GAIN | — | `gain100` | — | no (label) |
| Object | OBJECT | — | after `Light_` | — | no (label) |

- A file name gives a value only when it names exactly one. For example, `a_30s_60s.fit` gives no exposure.
- A catalogued mono camera keeps the ADR-078 mono default with nothing to confirm.
- A colour camera without a catalogued pattern must be declared by the owner.
- A recorded but non-positive or non-numeric EXPTIME is still invalid.
- A light with no exposure anywhere is rejected (`missing_exposure`). The CLI then asks for `--exposure`.
- Calibration frames are not reconstructed; the ADR-080 warnings are unchanged.

## Camera catalogue

`crates/fulgur-core/data/cameras.json` is embedded in the program. It has 69 entries (55 colour, 14 mono).

**Coverage:**
- Astro cameras: ZWO ASI, QHY, Player One, SVBONY, ToupTek-based OEM bodies, Moravian.
- Smart telescopes: Seestar S30/S50 (Pro), DWARF II/3/mini, Vespera 3, Celestron Origin.
- DSLR/mirrorless families: Canon EOS, Nikon, Sony Alpha, Fujifilm X-Trans.

**Fields of each entry:** manufacturer, model, match tokens, sensor, colour/mono, Bayer pattern, native size, pixel size, ADC bits, confidence level and sources.

**Pattern convention.** The pattern is the one the vendor's capture software writes as BAYERPAT for a full-sensor, bin-1 frame. It is applied to the FITS pixel array in stored order.

**Confidence levels:**

| Level | Entries |
| --- | --- |
| `verified_local` (owner's own headers) | 4: ASI2600MC, ASI2600MM, Seestar S50 Pro, DWARF 3 |
| `vendor` | 2 |
| `retailer` | 6 |
| `community` | 9 |
| `typical` (sensor family) | 48 |

**Disagreements.** Sources disagree in several places, for example:
- an ASI2600MC unit read as GRBG;
- ASI533MC needing GRBG in one report;
- the SVBONY SV605CC being GRBG on the same IMX533 sensor as the ASI533MC.

So a catalogue pattern is never applied without confirmation. Where reports conflict, no pattern is catalogued: QHY5III, SVBONY SV705C/SV405CC/SV305, Nikon and ASI120MC. A frame whose size differs from the native sensor size is flagged, because an ROI or crop can shift the pattern. Fujifilm X-Trans (6×6) is recorded as unsupported.

## Owner confirmation

- **CLI:** `stack`, `register` and `inspect` print the derived values grouped by value and source, then stop. Re-run with `--accept-derived` to use them, or correct them with `--sensor <mono|rggb|grbg|gbrg|bggr>` and `--exposure <seconds>`. The report records `derived_metadata_confirmed`, `exposure_override_seconds` and, per frame, `frame.derived` (field, value, source).
- **GUI:**
  - The light-set panel lists "Values derived for missing FITS header cards".
  - Exposure can be entered with "Use this exposure for every frame".
  - The layout radio shows "Derived · colour RGGB (confirm below)", and the layout radios correct it.
  - Measure/Stack stays blocked until "I checked these values · use them for this set" is ticked.
  - The confirmation resets when the set's derived values change and is not kept between sessions. An entered exposure is kept.

## Verification (Windows 11, release, local)

**Gates:**
- `cargo fmt --check`: PASS.
- `cargo clippy -D warnings` (default and all features): PASS.
- `cargo test --workspace --release`: 203/203 default, 206/206 all features (after ADR-085) (SIMD and GPU checks on).

**Synthetic tests:**
- ASIAIR file name gives exposure, gain, object and the `2600MC` camera token.
- Header cards win over the file name and the catalogue.
- A mono camera, an unknown name or ambiguous tokens derive nothing.
- Catalogue tokens are unique, and the longest match separates MC from MM.
- The FITS decoder path and the planning metadata derive the same values, and the source file hash is unchanged.
- Owner corrections remove the values they replace.
- CLI: an unconfirmed stack is refused and writes nothing; `--accept-derived` gives an RGB master with `mosaic_source = camera_catalog`; `--sensor mono --exposure 12` needs no confirmation; a frame with no exposure asks for `--exposure`.
- GUI set model: confirmation, corrections, unknown colour pattern, missing exposure.

**Real data, ASI2600MC Air M42** (`C:\Astrofoto\2026\01 January\kvilda\m42`, 351 lights). Derived values:
- Exposure from file names: 30 s × 155, 60 s × 100, 120 s × 30, 300 s × 66.
- RGGB from the catalogue (identified by INSTRUME 'ZWO ASI2600MC Air'; `verified_local`).
- Gain 100 and object "M 42" from the file name.

Results with `--accept-derived`:

| Step | Result |
| --- | --- |
| Inspect | 23.1 s, peak 3.08 GiB; 255 accepted, 84 excessive HFR (75 of them 60 s frames), 12 excessive eccentricity |
| Stack | 255/351 frames, 255/255 registered (100%), 144.9 s, peak 3.01 GiB, 6248 × 4176 RGB |
| Colour check | central sky median R:G:B = 0.71 : 1 : 0.47 vs 0.84 : 1 : 0.68 for the ASI2600MC Flaming Star master with BAYERPAT RGGB (same channel order; a wrong pattern would swap R and B) |

**Regression.** Seestar S50 DS-B native and 200% external-template stacks: master, coverage and high-rejection maps are byte-identical to the calibration release (ADR-082).

## Follow-up: GUI stack failure with geometric correction (ADR-085)

The owner's GUI stack of M42 failed with "Auto reference found no rectangle covered by 80% of Use-on frames". The GUI has geometric correction switched on; the CLI run above did not. Two causes were found and fixed (see ADR-085):

1. **Misidentified witness star.** One star was paired with a neighbour about 10 px away while the true RMSE was 0.14 px. Only 131/255 frames were mapped.
2. **Too-deep automatic reference.** A 300 s reference mapped only 76/255 frames: 30–60 s frames and the night of 2026-01-18 shared too few unsaturated stars with it.

Result with `--accept-derived --geometric-correction` (the GUI settings):

| Measure | Value |
| --- | --- |
| Frames stacked | 235/351 |
| Registered | 235/255 (92%) |
| Time | 127.4 s |
| Peak RAM | 3.27 GiB |
| Reference | 30 s frame from 2026-01-19 (preliminary reference kept) |

The 20 frames still refused are 300 s frames from 2026-01-21. They pass registration without geometric correction, but their catalogs cover only 64–74% of the 30 s anchor (80% rule).

Regression after ADR-085:

| Dataset | Result |
| --- | --- |
| Flaming Star masters | byte-identical |
| Sadr full calibration | byte-identical |
| Seestar DS-B | byte-identical |
| Dwarf3 with darks | 856/856 registered (was 851/856) |

## Not verified / limits

- Owner review of the M42 master in PixInsight. The frames are already calibrated and their background is clipped at 0 (median ≈ 0.001), so do not assign another calibration set to them.
- GUI derived-values panel: release-compiled for manual review only.
- Catalogue patterns rated `typical`, `community` or `retailer` are not confirmed on real files.
- Windows only; macOS not run.

Sanitized numbers: [evidence](M5_HEADER_RECONSTRUCTION_EVIDENCE.json). Master metrics and rejection methods that followed are in the [master quality and rejection review](M5_MASTER_QUALITY_AND_REJECTION_REVIEW.md).
