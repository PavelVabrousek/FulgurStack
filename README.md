# FulgurStack

CPU-first astrophotography stacking for immutable Seestar CFA FITS exposures. Produces a linear, sensor-bounded 32-bit RGB FITS master for normal opening in PixInsight, without calibrated, debayered or registered intermediate files.

## Current status — 2026-10-26

M1–M4 are owner-accepted; **M5 remains open**. The current delivery includes Compact Desk, selected-only registration, CPU performance improvements, optional geometric correction with whole-field acceptance, and automatic multi-night reference selection. The owner authorized this consolidated publication to `main`; GitHub Actions remain disabled and future publication still requires authorization.

Local Windows gates: formatting and default/all-feature Clippy PASS; release tests **160/160 default, 163/163 all-feature**, including actual FITS, AVX2 and GPU checks. The owner confirmed the repaired geometric correction on 2026-10-26 (PASS). Auto review on actual multi-night data is deferred until suitable observations are available. Remaining native DPI/interaction, Auto result review and macOS remain unverified. See [current state and limitations](docs/CURRENT_STATUS.md) and the [documentation index](docs/README.md).

## Windows application

```powershell
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui
```

The local release is `target/release/fulgurstack-gui.exe`, also reached by the FulgurStack desktop shortcut. Launching does not start a job. Preferences restore inputs, output and settings; positional input paths replace restored inputs.

Compact Desk targets **1366×768 physical pixels at Windows 125% scaling**. It opens centered in a normal window and supports maximization. Top navigation separates Frames, Quality, Stack setup, Processing and Results. Quality retains the wide, continuous 20-point-row table and optional narrow preview: Folder first, pinned trailing Use, column filters/sorting/resizing, Ctrl/Shift marking and batch Use/automatic/remove actions. Marking and filtering are separate from inclusion; removing a row never deletes its source. Status belongs to the footer. Folder/Frame/Captured preserve their readable right ends when narrowed; UI times show whole seconds.

Processing owns phase progress, times and outcomes, with fixed **STOP**. Dismissing its modal retains STOP in the main footer. Results retain the completed run's times, reference and Master/Coverage/High rejection/report views. Map previews exist independently of the two optional GUI map-saving checkboxes, both off by default. High Rejection defaults to a display-only 0–5% range; exported fractions are unchanged. See [current UI and manual review](docs/M5_WINDOWS_APP_REVIEW.md).

### Hot-pixel-robust star measurement (ADR-073)

Quality measurement and registration ignore hot pixels: isolated defects are suppressed in a measurement-only copy and only detections with a natural star profile are used. Source files, background/noise statistics and the stacked pixels are unaffected. This lets uncalibrated data (for example Dwarf3 mini lights without darks) register on real stars; the inspection report lists `suppressed_hot_pixels` and `refused_defect_detections` per frame.

### Light sets, calibration sets and sensor layout (ADR-071)

The application now has four steps: **Frames, Settings, Processing, Results**. The primary action (Measure quality / Stack N frames), its menu and STOP sit at the right of the top bar; the bottom bar shows status only. Every added folder becomes a **light set** with its own quality limits; sets may differ in filter (warned, with a confirmation before stacking), gain and exposure, but must show the same target on the same sensor size. **Calibration sets** (bias, darks, flats, dark flats; frames or masters, folders sorted by IMAGETYP) are assigned per light set with header-based compatibility checks; calibration is **not applied yet** and reports say so. When headers carry no Bayer pattern, the set must be declared colour (with pattern) or mono; mono frames skip demosaicing and a mono-only master is written as one plane. See [review](docs/M5_LIGHT_SETS_REVIEW.md).

## Registration and reference selection

- **Auto** is default. One observing group uses the exact chronological median. Multiple Use-on groups separated by more than six hours use catalog-based framing selection, prioritizing the estimated rectangle covered by at least 80% of selected frames, then mean coverage and representative framing/quality. Midnight alone does not split a group. The bounded estimate is approximate; it does not automatically crop output or predict post-rejection Coverage.
- **Chronological median**, selected accepted input and external geometry-only FITS remain available. Explicit `--reference` takes precedence. External templates support 30–333% scale (ADR-071; earlier 33–300%), may use any filter (geometry only) and retain native source photometry/exposure/GPS provenance.
- With geometric correction enabled, a frame whose shift+rotation mapping fails the whole-field check (for example a refocused night with a slightly different image scale) is retried with a validated scale fit (30–333%) and then a validated local-deformation model; a frame that shift+rotation cannot register at all is retried with the external-reference scale search. Every fallback must pass the same accuracy limits; frames that pass with shift+rotation are unchanged (ADR-074).
- Registration normally fits **Use-on frames only**. `--diagnose-excluded` opts into excluded-frame diagnostics without adding those frames to integration or reference voting.
- **Correct geometric distortion** / `--geometric-correction` is optional and **off by default**. Independent held-out stars must confirm improvement and absolute whole-field accuracy. A skipped nonlinear fit may use linear mapping only if that mapping passes absolute gates; otherwise the frame is excluded. Unconfirmed anchor geometry stops the run. Accepted mapping is applied in one existing tiled interpolation.

See [Auto reference](docs/M5_AUTO_REFERENCE_REVIEW.md), [geometry acceptance repair](docs/M5_GEOMETRIC_ACCEPTANCE_REPAIR.md) and [the diagnosed historical regression](docs/M5_GEOMETRIC_MIXTURE_DIAGNOSIS.md).

## Release CLI examples

Run from the repository root and choose unused output/report names. Inputs may be files or directories; repeat `--input` for more folders.

```powershell
cargo run --release --bin fulgurstack -- inspect --input "SeestarS50 Subs" --report "outputs/quality_review_new.json" --csv "outputs/quality_review_new.csv"
cargo run --release --bin fulgurstack -- register --input "SeestarS50 Subs" --reference-strategy auto --report "outputs/registration_review_new.json"
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference-strategy auto --output "outputs/master_review_new.fits" --report "outputs/master_review_new.json"
cargo run --release --bin fulgurstack -- stack --input "night1" --input "night2" --input-sets --sensor mono --output "outputs/mono_sets_new.fits" --report "outputs/mono_sets_new.json"
```

`--sensor auto|mono|RGGB|GRBG|GBRG|BGGR` declares the sensor layout (auto uses `BAYERPAT` and never assumes mono); `--input-sets` makes each `--input` its own light set.

CLI `stack` exports master plus coverage/high-rejection FITS; optional `--coverage-output` / `--high-rejection-output` change destinations. Unlike GUI saving options, CLI maps are always written. Outputs use atomic no-clobber publication. GUI repeats choose a common version suffix.

Coverage is surviving channel samples after selection, geometry and rejection divided by **all original deduplicated inputs**. High rejection uses upper-rejected samples with the same denominator. Fractions 0..1 mean 0..100%; neither map is exposure or quality weight. Auto framing instead uses all **Use-on** frames before pixel rejection. Display stretch never modifies exported linear FITS.

CPU is default; GPU reconstruction requires `--features gpu` and explicit `stack --engine gpu`, without silent fallback. CPU pool defaults to available parallelism capped at 24; GPU is capped at 16. Automatic tiles choose a verified 128/256/512 square within available RAM, system reserve and the 12288 MiB ceiling. Manual tile controls remain available. Original-byte FITS cache stays off (`--fits-cache-mib 0`); stored measurements found no benefit from enabling it. GPU accelerates reconstruction; registration/resampling/rejection/compensated accumulation retain CPU paths.

## Measured evidence

These measurements have different scopes and must not be combined into one speedup:

| Scope | Stored observation | Evidence |
| --- | --- | --- |
| Historical full 1,140-input CPU pipeline, external 200% reference | 247.673 s; peak 3397.9 MiB | [CPU batch](docs/M5_CPU_OPTIMIZATION_SUMMARY.md) |
| Frozen-plan integration replay | 162.791 → 135.044 s; exact complete master/maps | [Integration profile](docs/M5_INTEGRATION_PROFILE_REVIEW.md) |
| Native catalog registration, 940 selected / 1,140 originals | Median 17.045 → 10.103 s; exact serial/parallel science | [Native walks](docs/M5_NATIVE_REGISTRATION_WALKS_REVIEW.md) |
| Auto on real pixels with artificial three-night dates | Usable ≥80% rectangle +28.85%; mean overlap −0.90 percentage point; 4.812 → 5.604 s | [Auto review](docs/M5_AUTO_REFERENCE_REVIEW.md) |

The Auto fixture is not real multi-night observing evidence. No new full reference integration was run for the latest reference/geometry batch. Timing conditions, adverse metrics and acceptance limits remain in each review.

## Data, development and documentation

Real FITS/XISF, observing directories, outputs, raw telemetry, preferences and build artifacts stay local and excluded from Git. Sources are never modified. Final source-normalized RGB remains linear in 0..1; no default BIAS/dark subtraction is applied to precalibrated Seestar lights. Valid native-reference GPS is retained in final master FITS with provenance and redacted from default reports/logs.

See [development](docs/DEVELOPMENT.md), [datasets and benchmark rules](docs/BENCHMARKS.md), [project contract](PROJECT_SPEC.md), [engineering instructions](AGENTS.md), and [architecture decisions](docs/ARCHITECTURE_DECISIONS.md). The [documentation index](docs/README.md) separates current guidance from historical milestone evidence. Project artifacts are English; owner communication is Czech. Windows 11 x86_64 is locally tested; macOS Apple Silicon remains a required target without new local verification.
