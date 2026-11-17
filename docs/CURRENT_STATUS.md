# Current delivery state — 2026-11-17

This is the current-state companion to [PROJECT_SPEC.md](../PROJECT_SPEC.md) and its owner amendments index. Dated reviews in [reviews/](reviews/) preserve what was implemented, measured and accepted at the time; their test counts, hashes and "local" statements are historical. The version history is in [CHANGELOG.md](../CHANGELOG.md), and the dated status log up to 0.14 is in [archive/STATUS_HISTORY_2026-10-26_to_2026-11-09.md](archive/STATUS_HISTORY_2026-10-26_to_2026-11-09.md). See the [documentation index](README.md).

## Delivery and acceptance

- **Published:** version **0.15** on `main` (ADR-186, 2026-11-17), with every decision up to ADR-186, and as the GitHub pre-release `v0.15.0` with the Windows package ([distribution](WINDOWS_DISTRIBUTION.md)). New in 0.15 after the code review: sharp zoomed detail of XISF masters and maps (ADR-183), the master's WCS in the coverage and high-rejection maps (ADR-184) and the median capture time as DATE-OBS/DATE-AVG of the master and maps (ADR-185).
- **Published on `main` 2026-11-14 (ADR-182), part of 0.15:** the work packages from the [code review](CODE_REVIEW_2026-11-13.md):
  - WP-1: application correctness (ADR-173), release-compiled and awaiting the owner's manual check;
  - WP-2: robustness without numeric change (ADR-174). Full verification passed, and the DS-B masters, maps and master darks are byte-identical to 0.14;
  - WP-3: this documentation refresh (ADR-175);
  - WP-4: mechanical engine clean-up (ADR-176). Masters, maps and master darks are byte-identical to 0.14;
  - Blind plate solving is now `fulgurstack solve` and `fulgurstack solve-index`; the separate program is gone (ADR-181);
  - WP-7: tools, build and error hygiene (ADR-180): experiment tools archived, errors name the file, one exposure per light set on the CLI. Masters byte-identical to 0.14;
  - WP-6 part 2: code structure (ADR-179): one file per CLI command, two registration entry points, the integration function split, the application's stacking settings grouped. Masters byte-identical to 0.14;
  - WP-6 part 1: one science path for application and CLI (ADR-178). DS-B masters from both are byte-identical to 0.14;
  - WP-5: registration consistency (ADR-177). 16-bit masters are unchanged; sensors below 16 bits change slightly (Dwarf3: 2 of 885 frame scales, master within one noise unit).

- **Milestones:** M1/G1–M4/G4 are owner-accepted; **M5 remains open**.
- **Owner reviews passed** (recorded in the ADRs):
  - repaired geometric correction (2026-10-26);
  - Dwarf3 master (2026-10-28);
  - sky registration, XISF masters and the Settings, sky and split screens (2026-11-05, ADR-143).
- **Awaiting the owner:**
  - the application screens since ADR-157 (ADR-157, 159, 161–163, 169–171, 173);
  - the 0.15 masters in PixInsight: XISF zoom detail, WCS of the maps and the median DATE-OBS (ADR-183–185);
  - the WCS check in PixInsight SPCC (ADR-155/158);
  - the Olyhen L master (ADR-156).

  The full list, with test gaps and proposals, is in [open items](OPEN_ITEMS.md).
- **Not verified:**
  - Auto reference on actual multi-night observations;
  - real mixed-exposure data;
  - native 1366×768/125 % interaction review;
  - macOS Apple Silicon.

## Planned next

Owner decision 2026-10-31. Separate masters on one pixel grid are done (ADR-101, released in 0.12); this is what follows.

- **Linear channel mix of finished masters.** Weights with SHO, HOO and RGB presets, applied after stacking, with sky levels matched first. No mixing during rejection. Not started; waits for the owner's word.
- **Open owner decision:** whether Auto should use ESD below 25 frames. ADR-102 records the evidence on one 22-frame set.
- **Code review work packages WP-4 to WP-7** ([code review](CODE_REVIEW_2026-11-13.md)), one at a time with the owner's go-ahead.

Later idea, not scheduled: separating dual-band colour data into Ha and OIII. Drizzle is set aside until a comparison on real data shows a benefit.

## Current application

- **Steps.** Frames, Settings, Processing, Results and About. The primary action (Measure quality / Stack N frames), its menu and STOP sit at the right of the top bar. The status bar shows the current message in white, yellow or red.
- **Frames.**
  - Light sets: one per folder, or one per sub-folder with frames. They are split by camera, gain and filter on request. Mono and colour cameras are never mixed.
  - Derived header values, confirmed by the owner.
  - Calibration sets: bias, darks, flats, dark flats and hot pixel maps, from frames or masters, with previews and in-memory masters. Calibration is offered to fitting light sets; an explicit "None" is kept (ADR-173).
  - The quality table and plot, with quick selections.
- **Settings.** One column of panels with info markers:
  - registration reference and method (Stars, or Sky · Gaia plate solving with field centre and blind-solving index);
  - accuracy level and geometric correction (on by default);
  - stacking and rejection method, masters (combined, per filter, per set) and partial frames;
  - output format (FITS or XISF) and optional maps;
  - Advanced: engine, tiles and memory ceiling.
- **Warnings.** Warnings that can be put right have a red fix button, and warnings that may be wanted have "Keep as is" (ADR-170, ADR-171).
- **Processing and Results.**
  - Processing shows phase progress, times and outcomes.
  - Results show the master, coverage and high-rejection views, master quality, rejection and calibration details, sky registration details and the report.
- **Target and lifecycle.**
  - The application targets 1366×768 physical pixels at Windows 125 %.
  - Preferences restore sets, output and settings. Files opened with the program are shown for that session only (ADR-173).
  - Closing during a run, an index build or an in-memory master build asks first.

## Current scientific behavior

CPU is default, GPU explicit: `--engine gpu` runs pixel rejection and the weighted mean on the GPU, overlapped with CPU reconstruction (ADR-090); `--gpu-stages reconstruction|all` keeps GPU cosmetics/VNG. CPU pool cap is 24 (16 with GPU reconstruction); memory planning accounts bounded worker allocations within the 12288 MiB ceiling and system reserve. Automatic tiles follow ADR-118 (1024 × 256 on frames wider than 4000 px, else 512 × 256, then 256 × 256 and 128 × 128; GPU stacks keep squares); original-byte FITS cache defaults off. Sources remain immutable; no intermediate images, full registered-frame cache, half-precision pixels or default BIAS/dark subtraction.

Pixel rejection defaults to **Auto**: median below 6 frames, winsorized sigma clipping (3.0/2.5 σ) below 25, generalized ESD (α 0.05, ≤ 30%) from 25; winsorized, ESD, linear fit (5/4 σ) and median can be chosen explicitly. Survivors keep frame weights and f64 Neumaier accumulation (median excepted); fewer than 5 samples are not clipped. The applied method and its parameters are recorded in the report. The finished master is measured (HFR, FWHM, eccentricity, sky, per-channel noise) without changing it.

Registration defaults to effective Use-on frames. Excluded diagnostics are opt-in and cannot influence selected prediction history, weights, Auto voting or integration. Native session fits retain rigid geometry, source-order scientific reductions and two concurrent chronological outward walks, rather than arbitrary independent per-frame fitting.

Reference selection defaults to **Auto**. One Use-on acquisition group retains exact chronological-median behavior. More than six hours between accepted timestamps separates groups, independent of midnight. Multiple groups use catalog-only preliminary registration and a bounded coverage estimate to choose a real accepted source. Priority is largest estimated rectangle covered by ≥80% of all Use-on frames, then mean coverage, representative center/orientation (180° framing period), quality and filename. Unmapped selected frames remain in the denominator; an entirely unmapped selected group fails clearly. Explicit chronological/manual/external choices remain. No automatic crop is introduced.

Auto uses ≤1024² count cells (4 MiB) and ≤128-side candidate grids. These approximate cell-center predictions are neither an exact global pixel optimum nor final post-rejection Coverage. Results/JSON identify chosen source, groups, prediction and timer scopes. A winning preliminary plan is reused; otherwise a final direct-anchor pass reruns with existing scientific gates. STOP remains active throughout.

Geometric correction is **on by default in the application** and an opt-in flag (`--geometric-correction`) in the CLI (ADR-103). Independent held-out distributed stars must establish useful improvement and absolute whole-field validity. Try direct cubic, then separately validated anchor-calibration/source-pose composition. Ordinary linear mapping is eligible only if it independently passes absolute gates. Insufficient evidence cannot authorize unsafe fallback; failed sources are excluded and unconfirmed anchor geometry stops integration. Accepted mapping uses one tiled interpolation, conservative ROI bounds and native photometric/exposure/GPS provenance. See [repair evidence](reviews/M5_GEOMETRIC_ACCEPTANCE_REPAIR.md); the [earlier mixed-geometry failure](reviews/M5_GEOMETRIC_MIXTURE_DIAGNOSIS.md) remains a historical FAIL, not current readiness evidence.

GUI writes master/report and requested map files, with a common non-overwriting version suffix. CLI stack always writes both final maps. Coverage is surviving per-channel samples / all original deduplicated inputs; High Rejection is upper-rejected samples / that same count. Auto's ≥80% estimate instead uses Use-on inputs before rejection. Display stretch does not change scientific output.

## Verified evidence and limits

| Evidence | Result | Scope / limitation |
| --- | --- | --- |
| Latest release suites (2026-11-14, ADR-181) | All-feature workspace release tests 365 passed, 3 ignored (default features 361 passed), with DS-B real data, AVX2 and GPU required; format/Clippy PASS; CLI smoke on DS-B PASS; DS-B masters, maps and master darks byte-identical to 0.14 | Local Windows only; all-feature run with 4 test threads (two manual-tile tests refuse their tile when free RAM is low, see [open items](OPEN_ITEMS.md)); no macOS or remote CI |
| DS-A full run against other programs (2026-11-05) | FulgurStack 941/1,140 frames in 105.0 s median (CPU, peak 1.88 GiB); logged runs on the same data: PixInsight FBPP 34 min 56 s, StackingWizard 29 min 52 s, Siril 1.4.5 18 min 52 s | Other programs not rerun, times from their own logs; frame counts differ; [DS-A log](reviews/M5_PERFORMANCE_DSA_20261105.md) |
| Master WCS (ADR-155/158) | DS-A 298/300 master stars on Gaia, RMS 0.16–0.23 px; Seestar S30 M 45 with a stale "M 33" field 294/300, RMS 0.37 px | Master pixels unchanged; PixInsight SPCC check by the owner pending |
| GPU pixel rejection (ADR-090) | Integration −33/−33/−35% (M42, Dwarf3, Flaming Star); master RMS ≤ 9.7e-8, max ≤ 4.3e-4 vs CPU; 8–41 map pixels differ per set | Radeon 890M only; discrete GPU/Metal untested; [review](reviews/M5_GPU_REDUCTION_REVIEW.md) |
| Pixel rejection methods (ADR-087) | ESD lowest master noise on DS-B (G 13.23 → 12.74 ADU), Flaming Star, M42 and Dwarf3; integration +40–50% vs winsorized on large stacks; explicit winsorized byte-identical to earlier masters | Owner PixInsight comparison pending; [review](reviews/M5_MASTER_QUALITY_AND_REJECTION_REVIEW.md) |
| Header reconstruction and dense-field fixes (ADR-084/085) | ASI2600MC M42 c_ frames: exposure from names, RGGB from catalogue; 255/255 registered without, 235/255 with geometric correction; Dwarf3 856/856 | Owner review of M42 master pending; [review](reviews/M5_HEADER_RECONSTRUCTION_REVIEW.md) |
| Windows distribution package (ADR-089) | No CRT DLL imports; DS-B Auto master and maps byte-identical to the standard build; GUI starts | Windows 11 x64 only; [distribution](WINDOWS_DISTRIBUTION.md) |
| Hot-pixel-robust star measurement and scale/deformation fallback (ADR-073/074) | Dwarf3 mini 9 nights: 1 → 859/1098 stacked (150.5 s, peak 3.01 GiB); DS-B same 27 frames, master RMS change 6.6e-6, no ADR-074 fallback | Owner PixInsight review of the Dwarf3 master PASS (2026-10-28); no darks; cubic fallback unused on real data; [review](reviews/M5_HOT_PIXEL_AND_SCALE_REVIEW.md) |
| Light sets, calibration sets, declared sensor layout, four-step UI (ADR-071) | DS-B master/maps byte-identical to ADR-070; real Seestar S30 Pro IR+LP as two sets 75/87 in 11.8 s; synthetic mono gates PASS | Calibration assigned but not applied; no real mono/calibration/cross-camera data; native visual review pending; [review](reviews/M5_LIGHT_SETS_REVIEW.md) |
| Reused integration buffers (ADR-070) | Frozen native-plan integration 112.7421006 → 105.8856032 s vs ADR-069; kernel CPU about halved | Paired same-session controls; products identical to ADR-069; Windows only; [review](reviews/M5_CPU_BUFFER_REUSE_REVIEW.md) |
| AVX2 VNG (ADR-069) | Frozen native-plan integration 162.2608958 → 130.0071238 s; central-region median −27.421%; DS-B CLI ≈6.6 → ≈5.5 s | Paired same-session controls; identical raw samples and whole master/map FITS; Windows/AVX2 only; [review](reviews/M5_CPU_VNG_SIMD_REVIEW.md) |
| Native parallel walks | 17.0449013 → 10.1033996 s median; 940 successes, 200 skipped; exact serial/parallel science | Catalog replay, not a full stack; total CPU cost increased |
| Owner review of repaired geometric correction | PASS, explicitly confirmed 2026-10-26 | Acceptance limited to this repaired feature; no additional run metrics or product identities supplied |
| Repaired distorted-reference replay | 937/940 selected accepted; 3 geometric failures; 200 QC excluded; 766 direct +171 anchor corrections | Stored catalog replay; zero unsafe linear fallbacks; median frame RMS 0.311609 px |
| Repaired real-pixel smoke | 27/32 retained, 23 direct +4 anchor corrections; 14.5014043 s; 724.0 MiB peak | Bounded 32-source integration, no full reference rerun |
| Auto artificial three-night dates on real pixels | All 27 selected register; exact footprint ≥80% rectangle 3,252,600 → 4,191,129 px (+28.85%) | Artificial dates, not real multi-night observations; mean overlap falls 0.902446 percentage point |
| Same Auto smoke | 4.8123964 → 5.6044817 s; Auto peak 651.3 MiB | Small sequential observations; do not extrapolate full-set overhead |
| Auto external-reference control | Entire repaired master + both maps byte-identical; scientific reports exact | Explicit reference bypasses Auto; timings/OS headroom may differ |
| Historical full CPU pipeline | 247.6727114 s; 3397.9 MiB peak | 1,140 inputs, external 200% reference; not latest native/Auto workload |
| Prior frozen integration optimization | 162.7911621 → 135.0441689 s; exact whole master/maps | Integration replay only; not full-pipeline time |

Source/reference fingerprints remain unchanged in the applicable runs. Negative results and detailed gate definitions remain in reviews and sanitized evidence. Latest logs/products are private under `outputs/rejection_20261029/` and `outputs/dist_check_20261030/`; no full reference benchmark was rerun for this batch.

## Local release and manual review

The desktop shortcut targets `target/release/fulgurstack-gui.exe` with the approved icon. The portable package for other computers is the 0.15 ZIP in `outputs/dist/` (also on the GitHub pre-release `v0.15.0`); its hashes and checks are in [Windows distribution](WINDOWS_DISTRIBUTION.md). Earlier temporary/versioned release paths are superseded. From the repository root:

```powershell
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference-strategy auto --output "outputs/current_review_new.fits" --report "outputs/current_review_new.json"
```

Use unused names. For real multi-night data, replace the input folder and compare `--reference-strategy chronological-median` with different output names. Review reference, accepted/failed counts, estimated rectangle, master/corner stars and final Coverage in PixInsight. Different chosen grids intentionally do not yield identical images. UI-only iterations retain the owner's manual-review exception; publication does not rerun scientific stacks or remote CI.
