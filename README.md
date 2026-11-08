# FulgurStack

CPU-first astrophotography stacking for immutable FITS and XISF light frames, from smart telescopes such as the Seestar S50 to mono and colour astro cameras. Produces a linear, sensor-bounded 32-bit master, as FITS or XISF with a plate solution, for normal opening in PixInsight, without calibrated, debayered or registered intermediate files. A short overview for other developers is in [project summary](docs/PROJECT_SUMMARY.md).

## Current status — 2026-11-08, version 0.12

M1–M4 are owner-accepted; **M5 remains open**. The current delivery adds the following to the earlier work (Compact Desk, selected-only registration, CPU optimizations, validated geometric correction, Auto reference, light/calibration sets, full calibration):
- reconstruction of missing header values with a camera catalogue and owner confirmation (ADR-084);
- dense-field registration fixes (ADR-085);
- quality metrics of the stacked master (ADR-086);
- the pixel rejection methods winsorized, ESD, linear fit and median, with Auto by frame count (ADR-087);
- separate masters per filter or light set on one pixel grid (ADR-101) and sky offset per colour channel (ADR-102);
- XISF input, bottom-up FITS and sensors below 16 bits (ADR-104 to ADR-106);
- dense-field registration with flux-ranked lists and an Auto reference that continues (ADR-107, ADR-108);
- experimental partial frames (ADR-109) and one stack from several cameras on any reference (ADR-110, ADR-120);
- faster ESD, inspection, reconstruction and median with byte-identical masters (ADR-111 to ADR-114).

It was published to `main` with ADR-115. The work since, published with ADR-143, adds:
- registration of several cameras and optics in both directions, mirrored cameras and adjustable accuracy (ADR-116 to ADR-130);
- experimental sky registration: every frame plate-solved against Gaia DR3 and registered through the sky, from the CLI or the application (ADR-131 to ADR-138);
- splitting a light set by camera, gain and filter, and light folders that read only their own files (ADR-139, ADR-140);
- a one-column Settings page with info markers (ADR-141) and masters and maps written as FITS or XISF (ADR-142).

The DS-A speed rounds were published with ADR-153 (DS-A full run 105 s median, masters byte-identical). Local since, not published:
- masters carry their plate solution as FITS WCS (TAN with SIP), written only when the master's own stars match Gaia through it; the solve falls back to the frames' own field when Settings name another target, and a failed solve is recorded (ADR-155, ADR-158);
- plate solving adds its star lists to an existing quality measurement (ADR-154);
- a light set without stars in common with the reference, such as broadband L against narrowband OIII, keeps a brightness link of its own (ADR-156);
- calibration sets filled kind by kind are offered to light sets, and a stack without calibration is flagged (ADR-157);
- a folder dropped on the start screen gives one light set per sub-folder with frames, and the drag hint says the outcome (ADR-159).

GitHub Actions remain disabled, and future publication still requires authorization.

Local Windows gates: formatting and default/all-feature Clippy PASS; workspace release tests **315 passed, 0 failed** (ADR-158), including actual FITS, AVX2 and GPU checks, and byte-identical reference masters. The latest GUI iterations are release-compiled and await the owner's manual acceptance. Auto review on actual multi-night data, native DPI/interaction review and macOS remain unverified. See [current state and limitations](docs/CURRENT_STATUS.md) and the [documentation index](docs/README.md).

## Windows application

```powershell
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui
```

The local release is `target/release/fulgurstack-gui.exe`, also reached by the FulgurStack desktop shortcut. Launching does not start a job. Preferences restore inputs, output and settings; positional input paths replace restored inputs.

The application targets **1366×768 physical pixels at Windows 125% scaling**, opens centered in a normal window and supports maximization. It has four steps and an About page; the primary action (Measure quality / Stack N frames), its menu and **STOP** sit at the right of the top bar, and the bottom bar shows status only.
- **Frames:**
  - light sets (one per added folder, or one per sub-folder with frames when the folder has none of its own) with collapsible details, derived header values and their confirmation;
  - calibration sets with master previews;
  - the frame table: Folder first, pinned trailing Use, column filters, sorting and resizing, Ctrl/Shift marking, batch actions, and the quick selections All / best 95 / 90 / 80 / 50 %;
  - a resizable preview and a quality plot (HFR, eccentricity, sky) in list order.

  Marking and filtering are separate from inclusion; removing a row never deletes its source.
- **Settings:**
  - registration reference and output;
  - **Stacking and outlier rejection** (method and the method Auto applies to the next stack);
  - Registration geometry (geometric correction, reference frame).
  - Advanced (engine, tiles, memory ceiling).
- **Processing:** phase progress, times and outcomes with fixed STOP.
- **Results:**
  - the completed run's times and reference, the pixel rejection used and the master quality;
  - Master / Coverage / High rejection / report views. Map previews exist even when the two optional map-saving checkboxes (off by default) are off. High rejection defaults to a display-only 0–5% range.

Cards scroll instead of overlapping in small windows. See [current UI and manual review](docs/M5_WINDOWS_APP_REVIEW.md) and [UI design history](docs/WINDOWS_UI_DESIGN.md).

### Windows distribution package (ADR-089)

A portable ZIP (`outputs/dist/FulgurStack-<version>-<commit>-windows-x64.zip`, not versioned) contains both executables with the Visual C++ runtime linked in, plus a Czech guide `CTI_ME.txt`; nothing needs to be installed. The current package is built from 97caf3b. See [build and check procedure](docs/WINDOWS_DISTRIBUTION.md).

### Pixel rejection (ADR-087)

`stack --rejection auto|winsorized|linear-fit|esd|median` (GUI: Settings › Stacking and outlier rejection). Auto uses the median below 6 frames, winsorized sigma clipping below 25 and the generalized ESD test from 25 frames; the method applied is printed and recorded in the report. On the CPU, ESD sorts neighbouring pixels together (ADR-111).

### Master quality metrics (ADR-086)

`stack` measures the finished master with the same star detector as the frames and prints, for example, `Master quality: HFR 1.40 (frames 1.35) px, FWHM 3.41 (frames 3.34) px, eccentricity 0.284 (frames 0.297), sky 1399.4 ADU (0.02135), noise R 11.36 / G 10.68 / B 9.67 ADU`. The report carries `master_quality`; the GUI shows the same values in Results.

### Missing header values (ADR-084)

Lights whose header lost cards (for example frames calibrated by another program) are still usable: exposure, Bayer pattern, gain and object are reconstructed from other header cards, then from file-name tokens such as `30.0s`, `gain100` or `2600MC`, then from the camera catalogue in `crates/fulgur-core/data/cameras.json`. The values are listed with their source; confirm them with `--accept-derived` (GUI: **Review and confirm…** in the set overview opens the light-set details with the confirmation checkbox) or correct them with `--sensor` and `--exposure`. See [review](docs/M5_HEADER_RECONSTRUCTION_REVIEW.md).

### Calibration: bias, darks, flats and master files (ADR-076–082)

Assign a calibration set (bias, darks, flats, dark flats; frames or master files) to a light set in the GUI, or pass `--bias`, `--dark`, `--flat`, `--dark-flat` (or their `-master` variants) to `stack`, `register` or `inspect`. Masters are combined in RAM and every light becomes `(light − bias − (dark − bias)·t_light/t_dark) / normalized flat + pedestal` before quality measurement and stacking (ADR-082): darks are scaled by exposure when a bias is present, flats are offset-corrected (dark flats, else bias) and normalized per Bayer phase so the flat lamp's colour does not tint the lights, saturated samples stay saturated, and master-dark hot pixels missing from a light are repaired from their neighbours. The constant pedestal (master-bias median) keeps the sky above zero. See [calibration review](docs/M5_CALIBRATION_REVIEW.md). Lights whose size differs from the darks are rejected; a different exposure or gain only produces a warning and the dark is subtracted unscaled (ADR-080). Nothing is subtracted unless darks are assigned, so Seestar lights stay as recorded.

Bias, flats and dark flats can also be stacked, previewed (with 1:1 detail) and saved as masters (ADR-079); since ADR-082 they are applied to lights. Darks are combined by **Average**, **Median** or **Average, sigma-kappa 2** (default; needs at least 6 frames to reject a single outlier). In the GUI, **Stack master** builds the master in memory and **Save master…** writes it as a 32-bit FITS in ADU that later runs can use directly; master files from other programs (16-bit ADU or float normalized 0..1) can be added with *Add master…*. CLI: `master-dark --dark <folder> --combine median --output MasterDark.fits`, then `stack --dark-master MasterDark.fits`.

```powershell
cargo run --release --bin fulgurstack -- stack --input "C:\Astrofoto\Dwarf3mini Haspl\Light" --dark "C:\Astrofoto\Dwarf3mini Haspl\Dark" --geometric-correction --output outputs\dwarf3_dark.fits --report outputs\dwarf3_dark.json
```

### Hot-pixel-robust star measurement (ADR-073)

Quality measurement and registration ignore hot pixels: isolated defects are suppressed in a measurement-only copy and only detections with a natural star profile are used. Source files, background/noise statistics and the stacked pixels are unaffected. This lets uncalibrated data (for example Dwarf3 mini lights without darks) register on real stars; the inspection report lists `suppressed_hot_pixels` and `refused_defect_detections` per frame.

### Light sets, calibration sets and sensor layout (ADR-071)

The application now has four steps: **Frames, Settings, Processing, Results**. The primary action (Measure quality / Stack N frames), its menu and STOP sit at the right of the top bar; the bottom bar shows status only. Every added folder becomes a **light set** with its own quality limits; sets may differ in filter (warned, with a confirmation before stacking), gain, exposure, camera and optics, but must show the same sky field. **Calibration sets** (bias, darks, flats, dark flats; frames or masters, folders sorted by IMAGETYP) are assigned per light set with header-based compatibility checks; **darks are subtracted** (ADR-076: a master built in RAM, `light − dark + pedestal`, before measurement and stacking); bias and flats followed with ADR-082. When headers carry no Bayer pattern, the frames are processed as mono (ADR-078; a set can still be declared colour); mono frames skip demosaicing and a mono-only master is written as one plane. See [review](docs/M5_LIGHT_SETS_REVIEW.md).

## Registration and reference selection

- **Auto** is default. One observing group uses the exact chronological median. Multiple Use-on groups separated by more than six hours use catalog-based framing selection, prioritizing the estimated rectangle covered by at least 80% of selected frames, then mean coverage and representative framing/quality. Midnight alone does not split a group. The bounded estimate is approximate; it does not automatically crop output or predict post-rejection Coverage.
- **Chronological median**, selected accepted input and external geometry-only FITS remain available. Explicit `--reference` takes precedence. External templates support 30–333% scale (ADR-071; earlier 33–300%), may use any filter (geometry only) and retain native source photometry/exposure/GPS provenance.
- Several cameras or lenses can share one stack: give each camera its own light set (`--input a --input b --input-sets`). The reference may be a frame of any set or an external file; without a choice, the camera with most usable frames leads. Each camera registers on its own, star flux sets one brightness scale between them, and every frame passes the usual accuracy limits. A camera that matches only as a mirror image joins in reversed rows, unless it has calibration masters. A frame that failed quality or is switched off in Use cannot be the reference (ADR-110, ADR-120). A camera with a wider field joins a narrower grid too: it is fitted and judged only on the part of its sensor that the grid shows (ADR-122).
- Registration accuracy has three levels (Settings, or `--accuracy`): Maximum accuracy, the default, needs 1 px RMS, no star above 3 px, 80 % of the stars the weaker of frame and reference shows in their shared field and at least 50 matched stars; Compromise 2 px, 6 px, 60 %, 30 stars; Maximum coverage 3 px, 9 px, 40 %, 20 stars (ADR-121).
- With geometric correction enabled, a frame whose shift+rotation mapping fails the whole-field check (for example a refocused night with a slightly different image scale) is retried with a validated scale fit (30–333%) and then a validated local-deformation model; a frame that shift+rotation cannot register at all is retried with the external-reference scale search. Every fallback must pass the same accuracy limits; frames that pass with shift+rotation are unchanged (ADR-074).
- In dense star fields the whole-field coverage check compares each frame with the reference's 5000 brightest detections (saturated stars included), so differently chosen 250-star catalogs no longer cause refusals; accuracy limits are unchanged (ADR-081).
- Registration normally fits **Use-on frames only**. `--diagnose-excluded` opts into excluded-frame diagnostics without adding those frames to integration or reference voting.
- **Correct geometric distortion** is **on by default** in the application; `--geometric-correction` stays an opt-in flag in the CLI (ADR-103). Independent held-out stars must confirm improvement and absolute whole-field accuracy. A skipped nonlinear fit may use linear mapping only if that mapping passes absolute gates; otherwise the frame is excluded. Unconfirmed anchor geometry stops the run. Accepted mapping is applied in one existing tiled interpolation.

See [Auto reference](docs/M5_AUTO_REFERENCE_REVIEW.md), [geometry acceptance repair](docs/M5_GEOMETRIC_ACCEPTANCE_REPAIR.md) and [the diagnosed historical regression](docs/M5_GEOMETRIC_MIXTURE_DIAGNOSIS.md).

## Release CLI examples

Run from the repository root and choose unused output/report names. Inputs may be files or directories; repeat `--input` for more folders.

```powershell
cargo run --release --bin fulgurstack -- inspect --input "SeestarS50 Subs" --report "outputs/quality_review_new.json" --csv "outputs/quality_review_new.csv"
cargo run --release --bin fulgurstack -- register --input "SeestarS50 Subs" --reference-strategy auto --report "outputs/registration_review_new.json"
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference-strategy auto --output "outputs/master_review_new.fits" --report "outputs/master_review_new.json"
cargo run --release --bin fulgurstack -- stack --input "night1" --input "night2" --input-sets --sensor mono --output "outputs/mono_sets_new.fits" --report "outputs/mono_sets_new.json"
```

Inputs are FITS (`.fit`, `.fits`, `.fts`, 16-bit two-axis lights) and XISF (`.xisf`, `.xish`). Other formats, such as camera RAW, are converted outside FulgurStack. XISF covers the whole 1.0 format: monolithic and distributed units, attached, embedded, path and local `file:` blocks, all codecs, checksums and pixel layouts; remote `http` blocks are refused. Debayered XISF frames stack as colour, without calibration. Masters and diagnostic maps are 32-bit float FITS, or XISF when the master name ends in `.xisf` (ADR-142).

`--sensor auto|mono|RGGB|GRBG|GBRG|BGGR` declares the sensor layout (auto uses `BAYERPAT` and never assumes mono); `--input-sets` makes each `--input` its own light set. `--masters combined|per-filter|per-set` registers all sets together and stacks them into one master (default), one master per filter, or one master per light set; each extra master adds its label to the output, map and report names (`_Ha`, `_OIII`).

CLI `stack` exports master plus coverage/high-rejection FITS; optional `--coverage-output` / `--high-rejection-output` change destinations. Unlike GUI saving options, CLI maps are always written. Outputs use atomic no-clobber publication. GUI repeats choose a common version suffix.

Coverage is surviving channel samples after selection, geometry and rejection divided by **all original deduplicated inputs**. High rejection uses upper-rejected samples with the same denominator. Fractions 0..1 mean 0..100%; neither map is exposure or quality weight. Auto framing instead uses all **Use-on** frames before pixel rejection. Display stretch never modifies exported linear FITS.

CPU is default; the GPU engine requires `--features gpu` and explicit `stack --engine gpu`, without silent fallback. It runs pixel rejection and the weighted mean on the GPU while the CPU reconstructs the next tile, about one third shorter integration on the reference machine (ADR-090, [review](docs/M5_GPU_REDUCTION_REVIEW.md)); `--gpu-stages reconstruction|all` keeps the earlier GPU cosmetics/VNG. CPU pool defaults to available parallelism capped at 24 (16 with GPU reconstruction). Automatic tiles choose 1024 × 256 on frames wider than 4000 px and 512 × 256 on others, then smaller shapes, within available RAM, system reserve and the 12288 MiB ceiling; GPU stacks keep 128/256/512 squares (ADR-118). Manual tile controls remain available. Original-byte FITS cache stays off (`--fits-cache-mib 0`); stored measurements found no benefit from enabling it. Registration, calibration, demosaicing and resampling stay on the CPU.

## Measured evidence

These measurements have different scopes and must not be combined into one speedup:

| Scope | Stored observation | Evidence |
| --- | --- | --- |
| Full DS-A run, 1,140 inputs, current build (2026-11-05) | 941 frames stacked in 105.0 s median (103.3–109.8 s), peak 1.88 GiB; logged runs of other programs on the same data: PixInsight FBPP 34 min 56 s, StackingWizard 29 min 52 s, Siril 1.4.5 18 min 52 s (not rerun, frame counts differ) | [DS-A log](docs/M5_PERFORMANCE_DSA_20261105.md) |
| Historical full 1,140-input CPU pipeline, external 200% reference | 247.673 s; peak 3397.9 MiB | [CPU batch](docs/M5_CPU_OPTIMIZATION_SUMMARY.md) |
| Frozen-plan integration replay | 162.791 → 135.044 s; exact complete master/maps | [Integration profile](docs/M5_INTEGRATION_PROFILE_REVIEW.md) |
| Native catalog registration, 940 selected / 1,140 originals | Median 17.045 → 10.103 s; exact serial/parallel science | [Native walks](docs/M5_NATIVE_REGISTRATION_WALKS_REVIEW.md) |
| Auto on real pixels with artificial three-night dates | Usable ≥80% rectangle +28.85%; mean overlap −0.90 percentage point; 4.812 → 5.604 s | [Auto review](docs/M5_AUTO_REFERENCE_REVIEW.md) |
| GPU pixel rejection, Radeon 890M (ADR-090) | Integration M42 153.9 → 103.8 s, Dwarf3 173.3 → 115.7 s, Flaming Star 19.7 → 12.9 s; masters within T-NUM-01 | [GPU review](docs/M5_GPU_REDUCTION_REVIEW.md) |
| Pixel rejection methods on four real sets (27–856 frames) | ESD lowest master noise in every set; integration +40–50% vs winsorized on large stacks (M42 109 → 154 s, Dwarf3 117 → 173 s); RAM unchanged | [Rejection review](docs/M5_MASTER_QUALITY_AND_REJECTION_REVIEW.md) |

The Auto fixture is not real multi-night observing evidence. No new full reference integration was run for the latest reference/geometry batch. Timing conditions, adverse metrics and acceptance limits remain in each review.

## Data, development and documentation

Real FITS/XISF, observing directories, outputs, raw telemetry, preferences and build artifacts stay local and excluded from Git. Sources are never modified. Final source-normalized RGB remains linear in 0..1; no default BIAS/dark subtraction is applied to precalibrated Seestar lights. Valid native-reference GPS is retained in final master FITS with provenance and redacted from default reports/logs.

See [development](docs/DEVELOPMENT.md), [datasets and benchmark rules](docs/BENCHMARKS.md), [project contract](PROJECT_SPEC.md), [engineering instructions](AGENTS.md), and [architecture decisions](docs/ARCHITECTURE_DECISIONS.md). The [documentation index](docs/README.md) separates current guidance from historical milestone evidence. Project artifacts are English; owner communication is Czech. Windows 11 x86_64 is locally tested; macOS Apple Silicon remains a required target without new local verification.
