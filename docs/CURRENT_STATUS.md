# Current delivery state — 2026-10-26

This is the current-state companion to [PROJECT_SPEC.md](../PROJECT_SPEC.md). Dated reviews preserve what was implemented, measured and accepted at the time. Their old test counts, hashes, local-only statements and design proposals are historical; later amendments supersede behavior without rewriting measurements. See the [documentation index](README.md).

## Delivery and acceptance

The owner authorized one consolidated commit/push of the completed local batch to `main`, including documentation. It contains native registration parallel walks (ADR-064), optional geometric correction (ADR-065), its required whole-field acceptance repair (ADR-066), and Auto multi-night reference selection (ADR-067), on top of Compact Desk and CPU integration improvements. ADR-068 records publication scope. Future publishing remains opt-in. GitHub Actions stay disabled and their configuration is unchanged.

**M1/G1–M4/G4 are owner-accepted. M5 remains open.** Current automated Windows engine gates PASS; this is not owner acceptance of all latest images or native UI. Real multi-night observations, real mixed-exposure data, latest native 1366×768/125% interaction/PixInsight acceptance and macOS are unverified. Earlier analytic CFA-to-RGB photometry and unmatched PixInsight workload limitations remain in the M4 reviews.

## Current application

- Compact Desk: centered normal-window startup, top navigation and separate Stack setup. Physical target 1366×768 at Windows 125%; maximization supported. Wide virtual Quality table, 20-point rows, Folder first, pinned trailing Use, all-column filters, sort/resize/content fit, Ctrl/Shift marking and visible-row batch actions. Folder/Frame/Captured clip leading text when narrowed. UI times omit fractional seconds; saved metadata retains precision.
- Highlighting/filtering does not change Use. Remove hides/excludes without deleting sources; restore brings rows back Use-off. Unusable inputs cannot be forced in. Inspection reuse requires unchanged source membership/size/mtime; measurements and overrides are session-local. Footer owns status/counts; no duplicate Quality progress or Use commands.
- Processing owns scientific progress and phase Time/Status/outcomes. Fixed STOP is in the modal, or the footer when the modal is dismissed. Results retain their completed-run configuration, phase times, Master/Coverage/High rejection/report views and file actions.
- Both map previews are computed independently of GUI save checkboxes, off by default. Unsaved maps are labeled and cannot open unrelated files. High Rejection defaults to display-only 0–5%, with peak-preserving overview and per-channel statistics. FITS fractions are unchanged; native map detail requires saved maps.
- Approved lightning/stars/photo-layer icon and Windows ProductName/FileDescription/AppID are embedded; PID is available for Task Manager lookup. Native Task Manager grouping has not been visually verified.

## Current scientific behavior

CPU is default, GPU reconstruction explicit. CPU pool cap is 24, GPU cap 16; memory planning accounts bounded worker allocations within the 12288 MiB ceiling and system reserve. Automatic verified tiles remain 128/256/512 square; original-byte FITS cache defaults off. Sources remain immutable; no intermediate images, full registered-frame cache, half-precision pixels or default BIAS/dark subtraction.

Registration defaults to effective Use-on frames. Excluded diagnostics are opt-in and cannot influence selected prediction history, weights, Auto voting or integration. Native session fits retain rigid geometry, source-order scientific reductions and two concurrent chronological outward walks, rather than arbitrary independent per-frame fitting.

Reference selection defaults to **Auto**. One Use-on acquisition group retains exact chronological-median behavior. More than six hours between accepted timestamps separates groups, independent of midnight. Multiple groups use catalog-only preliminary registration and a bounded coverage estimate to choose a real accepted source. Priority is largest estimated rectangle covered by ≥80% of all Use-on frames, then mean coverage, representative center/orientation (180° framing period), quality and filename. Unmapped selected frames remain in the denominator; an entirely unmapped selected group fails clearly. Explicit chronological/manual/external choices remain. No automatic crop is introduced.

Auto uses ≤1024² count cells (4 MiB) and ≤128-side candidate grids. These approximate cell-center predictions are neither an exact global pixel optimum nor final post-rejection Coverage. Results/JSON identify chosen source, groups, prediction and timer scopes. A winning preliminary plan is reused; otherwise a final direct-anchor pass reruns with existing scientific gates. STOP remains active throughout.

Geometric correction is **optional, off by default**. Independent held-out distributed stars must establish useful improvement and absolute whole-field validity. Try direct cubic, then separately validated anchor-calibration/source-pose composition. Ordinary linear mapping is eligible only if it independently passes absolute gates. Insufficient evidence cannot authorize unsafe fallback; failed sources are excluded and unconfirmed anchor geometry stops integration. Accepted mapping uses one tiled interpolation, conservative ROI bounds and native photometric/exposure/GPS provenance. See [repair evidence](M5_GEOMETRIC_ACCEPTANCE_REPAIR.md); the [earlier mixed-geometry failure](M5_GEOMETRIC_MIXTURE_DIAGNOSIS.md) remains a historical FAIL, not current readiness evidence.

GUI writes master/report and requested map files, with a common non-overwriting version suffix. CLI stack always writes both final maps. Coverage is surviving per-channel samples / all original deduplicated inputs; High Rejection is upper-rejected samples / that same count. Auto's ≥80% estimate instead uses Use-on inputs before rejection. Display stretch does not change scientific output.

## Verified evidence and limits

| Evidence | Result | Scope / limitation |
| --- | --- | --- |
| Latest release suites | 160/160 default; 163/163 all-feature; format/Clippy PASS | Local Windows, actual FITS/AVX2/GPU; no macOS or remote CI |
| Native parallel walks | 17.0449013 → 10.1033996 s median; 940 successes, 200 skipped; exact serial/parallel science | Catalog replay, not a full stack; total CPU cost increased |
| Repaired distorted-reference replay | 937/940 selected accepted; 3 geometric failures; 200 QC excluded; 766 direct +171 anchor corrections | Stored catalog replay; zero unsafe linear fallbacks; median frame RMS 0.311609 px |
| Repaired real-pixel smoke | 27/32 retained, 23 direct +4 anchor corrections; 14.5014043 s; 724.0 MiB peak | Bounded 32-source integration, no full reference rerun |
| Auto artificial three-night dates on real pixels | All 27 selected register; exact footprint ≥80% rectangle 3,252,600 → 4,191,129 px (+28.85%) | Artificial dates, not real multi-night observations; mean overlap falls 0.902446 percentage point |
| Same Auto smoke | 4.8123964 → 5.6044817 s; Auto peak 651.3 MiB | Small sequential observations; do not extrapolate full-set overhead |
| Auto external-reference control | Entire repaired master + both maps byte-identical; scientific reports exact | Explicit reference bypasses Auto; timings/OS headroom may differ |
| Historical full CPU pipeline | 247.6727114 s; 3397.9 MiB peak | 1,140 inputs, external 200% reference; not latest native/Auto workload |
| Prior frozen integration optimization | 162.7911621 → 135.0441689 s; exact whole master/maps | Integration replay only; not full-pipeline time |

Source/reference fingerprints remain unchanged in the applicable runs. Negative results and detailed gate definitions remain in reviews and sanitized evidence. Latest logs/products are private under `outputs/auto_reference_20261026/`; no full reference benchmark was rerun for this batch.

## Local release and manual review

The desktop shortcut targets `target/release/fulgurstack-gui.exe` with the approved icon. Current hashes/gates are in [Auto evidence](M5_AUTO_REFERENCE_EVIDENCE.json). Earlier temporary/versioned release paths are superseded. From the repository root:

```powershell
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference-strategy auto --output "outputs/current_review_new.fits" --report "outputs/current_review_new.json"
```

Use unused names. For real multi-night data, replace the input folder and compare `--reference-strategy chronological-median` with different output names. Review reference, accepted/failed counts, estimated rectangle, master/corner stars and final Coverage in PixInsight. Different chosen grids intentionally do not yield identical images. UI-only iterations retain the owner's manual-review exception; publication does not rerun scientific stacks or remote CI.
