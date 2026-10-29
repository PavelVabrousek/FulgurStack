# M5 Native Windows Workspace — Progress and Review

## Current review entry point — 2026-10-30

> Since ADR-071 the application uses four steps (Frames, Settings, Processing, Results); later iterations added collapsible light-set details, derived-value confirmation, quick selections, the list-order quality plot, scrollable cards and the Settings card "Stacking and outlier rejection" (ADR-084–ADR-087, published with ADR-088, awaiting manual acceptance). Current gates are 210/210 default and 213/213 all-feature. The Compact Desk description below remains valid for the frame table, previews and Results maps. See [master quality and rejection review](M5_MASTER_QUALITY_AND_REJECTION_REVIEW.md).

See [current delivery state](CURRENT_STATUS.md) for authoritative behavior, release paths, test counts and acceptance boundaries. The current native release is `target/release/fulgurstack-gui.exe`, reached by the desktop shortcut. Earlier `target/local-release` paths/hashes below are historical and superseded.

Compact Desk now opens centered in a normal window, with top navigation, separate Stack setup, wide 20-point Quality rows, Folder first, trailing pinned Use, all-column filtering/resizing, Ctrl/Shift marking and batch actions. Footer owns status; Processing alone owns scientific progress, with phase outcomes/times and visible STOP in the modal or footer. Results retain completed-run master/maps/report, display-only scaling and phase summary. Physical target remains 1366×768 / 125%.

Registration defaults to Use-on only; excluded diagnostics are optional. Reference modes are Auto (default), explicit Chronological median, selected source and external FITS. Optional geometric correction is off; accepted mappings must pass the repaired absolute whole-field gates. Auto and correction preserve STOP, source immutability and scientific map denominators. See [Auto implementation](M5_AUTO_REFERENCE_REVIEW.md) and [acceptance repair](M5_GEOMETRIC_ACCEPTANCE_REPAIR.md).

Current Windows engine suites PASS 160/160 default, 163/163 all-feature, with format/Clippy and actual FITS/AVX2/GPU. These do not establish native visual/DPI or PixInsight owner acceptance of the latest changes. UI-only revisions retain manual-review policy; real multi-night observations and macOS remain unverified. This batch is included in the owner-authorized `main` publication; remote CI stays disabled and M5 stays open.

Manual review: maximize at the target resolution/scaling, inspect/filter/mark a large Quality list, compare Auto and explicit median on actual multi-night inputs, confirm reference/outcome counts and phase times, verify STOP with modal open/dismissed, and inspect master/corner stars plus maps. New report/output names are required; do not overwrite earlier products.

## Dated implementation and review history

All entries below describe their particular revision, including publication-time restrictions and older packaging paths. They do not override the current entry above.

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-26. Status: Compact Desk, phase frame counts and diagnostic map preview corrections locally release-compiled, awaiting manual review. The earlier owner PASS for quality-assessment reuse, manual frame selection and Processing without scrolling remains historical acceptance of that slice. **M5 remains open.** Work remains local without commits or pushes. GitHub Actions remain disabled.

## Phase outcomes and diagnostic maps (2026-10-26)

Register/Integrate Status now retain completed frame counts (ADR-059). Register reports geometric successes and failures across all original inputs; quality-rejected frames may still register diagnostically. Integrate reports frames supplied to its successful completion, processing failures and separately excluded inputs. Frame-level failures abort integration; cancelled/failed phases do not invent completion totals. Pixel rejection is separate. Counts survive in Completed while a new job runs, with wrapped Status cells and fixed STOP preserved.

Read-only evidence from the latest stored `outputs/master_v8_report.json`: **1140 registered, 0 registration failures; 981 integration inputs and 159 excluded by quality**. Both optional map filenames are null, with only `master_v8.fits` and its report in that output family. The report records **725586597 upper rejected channel samples**, equivalent to about **1.326% RGB-wide mean** of the all-original-input denominator. Upper rejection is therefore not globally zero. The old fixed 0–100% display maps this mean to about 3/255 gray levels; point-sampled overviews can also miss narrow features. Exact old selected-channel appearance is not recoverable from this report, and no new stack was run to reconstruct the unsaved maps. Current setup flags are not evidence of what an earlier completed run saved.

Both map previews continue to be computed in memory regardless of saving. Output now explains that the independent checkboxes save extra FITS files. Results consistently label each map as an in-memory preview with FITS saved/not saved. The selected saved map is opened by Open map FITS; unsaved maps cannot open an unrelated master through that action. Their native-detail availability remains tied to whether their FITS was saved.

Coverage defaults to display 0–100%, High Rejection to 0–5%; both offer 0–1/5/10/100% display ranges. The saved FITS and scientific rejection are unchanged. High Rejection uses a labeled peak-preserving overview. Full-map per-channel range/mean/nonzero statistics distinguish true zeros from low contrast and are independent of the preview's maxima. Saved native detail uses the same chosen display range. No full map is retained beyond the existing worker lifetime.

Manual review after a newly completed owner-run stack: check Register/Integrate counts in live and saved summaries; review both maps with saving off/on, including selected-map file opening; compare 0–100% and 0–5% rejection display, R/G/B statistics and saved native zoom. Unsaved maps from a previous application session cannot be recovered after restart. Keep 1366 x 768 / 125%, fixed STOP and unchanged source/output science. No automatic functional tests, new scientific stacks, benchmarks or publishing.

Local Windows gates for this slice: formatting PASS, GUI/all-target GPU-enabled lint PASS and full GPU-enabled Cargo release build PASS. Current executable: `target/release/fulgurstack-gui.exe`, reached by the FulgurStack desktop shortcut. This validates compilation/packaging only; no new native interaction, output parity or performance pass is claimed. The investigation above reads existing reports/files only and is not a new stack/benchmark.

## Compact Quality header and footer status (2026-10-25)

Owner screenshot correction (ADR-058): Quality now starts with one compact control strip. Use on/off/toggle occur directly; Marked contains only automatic reset for marked rows and remove. More retains distinct list-level reset/restore/fit actions. The full right-click row context menu remains available. The repeated Quality heading, CFA description, inline Shown/Marked/Use counts, Preview/HFR toggles and review prompt are removed from above the table. Preview/HFR remain in the global View menu.

Quality status now belongs to the main footer: Shown/listed, Marked, Use and Use-selected acquisition exposure on the first line; CFA/weight state, active filters, removed count where relevant and the current status/settings notice on the second. Registered acceptance is distinguished only when it differs from Use choices. The shared predicate keeps footer visibility counts consistent with the table. Long lines truncate with full hover text, and compact local spacing preserves the fixed STOP/Continue region. Waiting for review is stated once in the footer. Processing/exit modals continue to suppress underlying footer contents.

Manual review at 1366 x 768 / 125%: check the single toolbar and two-item Marked menu; filter/mark/remove/restore rows and verify footer counts; use View to toggle Preview/HFR; confirm review status/Continue/STOP fit without header duplication. Whole-second times, tail clipping, pinned Use and existing scientific behavior remain intact. Verification is local formatting/lint and release preparation only; no automatic functional tests, scientific stacks, benchmarks, commits, pushes or remote CI.

Final local Windows gates: formatting PASS, GUI/all-target GPU-enabled lint PASS and complete GPU-enabled Cargo release build PASS. Packaged executable matches the freshly linked artifact by SHA-256 and its embedded Windows icon loads. The desktop shortcut has been updated/read back to point to `target/release/fulgurstack-gui.exe`. No application/scientific run was started. Native interaction/DPI acceptance remains manual.

**Current manual-review executable:** `target/release/fulgurstack-gui.exe`, available from the FulgurStack desktop shortcut. This supersedes the temporary `target/local-release` launch path in the earlier whole-second update below. A source launch from the repository root is:

```powershell
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui
```

## Whole-second time display (2026-10-25)

Captured, exposure and processing/phase time displays omit fractional seconds consistently across Quality, source preview, row tooltip, Processing, Results and completion status. Timestamps retain their date/timezone; phase clocks use HH:MM:SS. Original measurements, timestamps, exposure, sort/filter values, FITS and JSON/CSV retain their precision. Other quality values and RAM/percentage formatting are unchanged (ADR-057).

Manual review: confirm Captured ends at whole seconds, Exposure shows e.g. 10 instead of 10.00, and Processing/Results phase times have no fractional suffix. Preserve 1366 x 768 / 125%, fixed STOP and existing source/list behavior. No automatic functional tests, scientific stacks, benchmarks or publishing.

Local verification: formatting PASS and GUI/all-target GPU-enabled lint PASS. Release compilation/linking produced the updated executable, but Cargo's final replacement of `target/release/fulgurstack-gui.exe` failed with Windows access denied, including with normal local access. Do not report the complete Cargo build command as PASS. Preserve the old executable and do not terminate a user session. Package the newly linked `target/release/deps/fulgurstack_gui.exe` as `target/local-release/fulgurstack-gui.exe`; source/artifact timestamp ordering, identical SHA-256 after copying and extraction of the embedded Windows icon PASS. The existing desktop shortcut now targets this new release and its icon, verified by reading the shortcut back. Native appearance/interaction remains manual.

**Current manual-review executable:** `target/local-release/fulgurstack-gui.exe`. This supersedes the older launch paths in historical sections below. Start the FulgurStack desktop shortcut, or run from the repository root:

```powershell
& ".\target\local-release\fulgurstack-gui.exe"
```

## Folder column and text clipping correction (2026-10-25)

Folder is now the first Quality column, with full parent-directory display, text filtering, sorting, resizing and double-click content fit. Frame shows the standalone filename; original report/source identities stay unchanged. Folder, Frame and Captured remain left aligned while they fit; when narrowed, their right edge stays anchored and their beginning is clipped, preserving the directory tail, filename suffix and acquisition time. The row tooltip retains the full source path and Captured value (ADR-056).

Keep 20-point rows, column/row selection behavior, pinned trailing Use, centered Compact Desk sizing and fixed STOP. Manual review: narrow/expand each of these three columns, hover to inspect full values, and combine Folder/Frame filters and sorting on sources from different directories. Review at 1366 x 768 and 125%. No automatic functional tests, scientific stacks, benchmarks or publishing.

Local Windows verification for this correction: formatting PASS, GUI/all-target GPU-enabled lint PASS, GPU-enabled release compilation PASS. Updated executable: `target/release/fulgurstack-gui.exe`, reached by the existing desktop shortcut. Native appearance and interaction remain pending owner review; no macOS verification is claimed.

## Compact quality table local release (2026-10-25)

Implemented owner-approved table behavior (ADR-055): 20-point virtualized rows, fixed header and filter row, alternating backgrounds, full-row marking, a distinct preview focus marker, and a pinned final Use column. Every column sorts/filters; drag dividers to resize and double-click to fit. Complete filenames set the initial filename width. Background and noise use existing measurements in ADU and ADU/s, respectively. Widths/filters/marking remain session-local.

Click selects one row, Ctrl-click toggles a row, Shift selects an inclusive displayed range and Ctrl+Shift adds a range. Ctrl+A/Escape act on the focused table, not filter text fields. Filter changes clear marking. Shown/Marked/Use counts are separate. Use on/off/toggle, marked-only automatic reset and remove act on marked visible paths through existing manual decisions; invalid inputs cannot be forced in. Removal hides and excludes without touching input files or changing original-input scientific denominators. More > Restore removed returns rows with Use off; an explicit new inspection/input change resets the list. More also retains an all-listed automatic reset, which cannot re-enable hidden removed frames.

STOP remains outside the table in the fixed bottom bar or active Processing modal. No duplicate phase UI, engine arithmetic, science output format or new processing dependency is introduced. Final local Windows formatting PASS, GUI/all-target GPU-enabled lint PASS and GPU-enabled release compilation PASS. The release is `target/release/fulgurstack-gui.exe`, reached by the existing FulgurStack desktop shortcut. No automated functional tests, scientific stacks, benchmarks, commits, pushes or remote CI. Native layout/interaction acceptance remains manual.

Manual review at 1366 x 768 and 125%:

1. Start the desktop FulgurStack shortcut, inspect quality and confirm compact rows, complete filenames, fixed header and trailing pinned Use while scrolling both axes. Drag/double-click dividers and hide/show Preview as needed.
2. Combine filename/reason/status/Use and numeric min/max filters, including a missing measurement or invalid range. Confirm Clear filters and active indicators. Sorting must keep values aligned with their source names.
3. Click, Ctrl-click and Shift-click ranges after sorting/filtering. Ctrl+A selects visible rows; Escape clears marks. Right-click an unmarked row and an already marked group. Marking alone must not change Use; changing filters must clear marking.
4. Apply Use on/off/toggle and marked-only automatic reset. Unusable frames must remain off. Remove a subset, restore it via More and confirm Use stays off until explicitly changed. Inspect again to reset the session-local list.
5. With quality review enabled, change marked Use and Continue stacking via the fixed bottom bar. Inspect phase reuse should remain available for unchanged sources, STOP must remain accessible, and saved Results/phase times must retain their existing behavior.

## Centered startup local release (2026-10-25)

Owner correction: open the application in the center of the primary screen, in the normal window state, using the framework's OS/DPI-aware centering. This supersedes automatic maximization in the initial Compact Desk revision. The 1060 x 520 logical client size, 800 x 460 minimum, app icon and desktop shortcut remain intact; maximize is still available manually (ADR-054).

Local Windows verification: formatting PASS, GUI/all-target GPU-enabled lint PASS and GPU-enabled release compilation PASS. The desktop shortcut still targets the updated release and uses the normal window style. Actual native placement and taskbar clearance at 1366 x 768 / 125% remain manual review. No automatic functional tests, scientific stacks, benchmarks or publishing.

## Application icon local release (2026-10-25)

The owner approved the three photographic layers with white stars and a continuous golden lightning bolt in front. The native window now explicitly uses this icon. The Windows executable embeds a matching multi-resolution icon for Explorer, desktop shortcuts and the running application. Approved source artwork, PNG/ICO exports, provenance and the export command are in `crates/fulgur-gui/assets/README.md` (ADR-053).

Local Windows formatting, GUI/all-target GPU-enabled lint and GPU-enabled release compilation PASS. The executable's packaged icon is inspected separately from the UI; actual desktop/title-bar appearance at 125% remains manual owner review. No automatic functional tests, scientific runs, benchmarks, commits, pushes or remote CI. M5 remains open.

Windows successfully extracted the new 32-pixel icon directly from the release executable; its appearance matches the approved design. The retained source image is byte-identical to the selected generated artwork. Created and read back the current-user `Desktop/FulgurStack.lnk`, targeting this repository's `target/release/fulgurstack-gui.exe` with embedded icon index 0. No application job was started for these packaging checks.

## Owner acceptance update (2026-10-23)

The owner manually confirmed **PASS** for all three requested behaviors: completed quality assessments are reused, manual frame selection works, and the full Processing modal is visible without scrolling. This accepts the reviewed UI slice, not all of M5 or a new scientific/performance gate. No new automatic functional tests, scientific regression stacks or benchmarks were run for this update. The owner explicitly retained local-only development without automatic tests, commits or pushes.

## Current progress snapshot

- Compact Desk supersedes the former shared left navigation/settings panel: top navigation, a separate tabbed Stack setup screen, a wider continuous virtualized Quality table and a narrow right preview. HFR plot and source preview can be toggled; the plot is initially collapsed to preserve list height. The window uses a very dark sky-blue background.
- Quality, registration and integration report percentages. Processing is a compact modal without scrolling, with fixed STOP/Close actions, elapsed time, an approximate remaining-time estimate and Inspect/Register/Integrate/Write outputs durations. Successful stacking closes the modal and opens Results. Animation is limited to actual background work.
- Completed quality measurements are retained for the current application session. Before reuse, inputs are rediscovered and canonical membership, byte lengths and modification timestamps are compared. Changed inputs cause fresh inspection and clear stale manual choices; explicit Inspect always measures again. Changing output, reference or processing options does not itself invalidate source measurements. Reused Inspect is labeled Reused and contributes zero new inspection time.
- Per-frame Use checkboxes allow manual inclusion/exclusion; Use automatic selection resets overrides. Soft HFR, eccentricity and relative-star-count rejection can be overridden when measurements remain usable. Unreadable, incompatible or unusable frames cannot be forced in. Registration still enforces fit/photometric validity. All original deduplicated inputs remain the denominator of both maps, and the report records manual decisions and original automatic reasons.
- Existing output families are preserved. The GUI chooses a common _v2, _v3, etc. suffix when needed, retaining atomic no-clobber publication. Save coverage map and Save high rejection map are independent options, both off by default; master and JSON report are always saved. Unsaved map thumbnails are labeled Preview only; map computation remains in memory.
- Last-used input/output selection, master filename, reference, map flags and stack preferences persist in %LOCALAPPDATA%/FulgurStack/settings.json. Explicit startup input arguments override restored inputs. Opening the application never starts inspection/stacking. Inspection catalogs and manual choices are not persisted across restarts.
- CPU remains default; GPU remains an explicit choice. The scientific reference grid, no-crop policy, bounded memory and disabled FITS-byte cache remain intact. Retained quality metadata is not an image-data cache.
- New navigation slice, awaiting manual review: source/master previews support wheel zoom anchored at the cursor, drag panning, Fit/double-click reset and 1:1 (100% = one image pixel per physical screen pixel). Native visible detail loads in a bounded background task after navigation settles; Loading native detail distinguishes the temporary overview from actual pixels. Existing source VNG and display-only STF are reused; exported FITS are unchanged. Saved maps support native detail as well; unsaved maps remain overview-only with 1:1 disabled.

## Compact Desk local implementation (2026-10-24)

Owner-approved scope: wider subframe list, compact Processing and Results, targeting 1366 x 768 at Windows 125%. Startup is maximized; the previous 1000 x 650 logical minimum is replaced with 800 x 460 so it no longer exceeds the reference work area. Windows determines the actual client area; do not claim a measured DPI/layout pass from a release compilation.

- Navigation: Frames, Quality, Stack setup, Processing and Results. Supported settings are organized into Basic, Alignment, Rejection and Performance; reference-size canvas and existing science defaults remain unchanged.
- Quality: continuous virtualized rows, per-frame Use, automatic reset and a Manual filter; a narrow resizable preview preserves table width. Optional HFR chart is off initially.
- Processing: per-phase progress/time/status, elapsed/approximate remaining time and fixed header STOP. Back to workspace and Escape keep work running; STOP remains in the bottom bar. Cancellation still joins existing workers/cleanup, without pause/resume promises. Completion closes the modal and opens Results.
- Results: master, coverage, high-rejection and Report & files tabs; completed-run timing snapshot survives new runs, cancellation and changing setup. Scientific output/reference provenance and optional saved-file states come from the completed result. Unsaved maps remain overview-only. Opening the completed summary during a new run does not replace live Processing telemetry.
- Verification policy: local formatting/lint and release compilation only. Automatic functional tests, scientific stacks and benchmarks are not run. No commit, push, PR update or remote CI. Windows native 1366 x 768 / 125% review remains pending with the owner; macOS is unverified.

Local Windows verification: formatting PASS (`cargo fmt --all -- --check`), GUI/all-targets GPU-enabled lint PASS (`cargo clippy -p fulgur-gui --all-targets --features gpu -- -D warnings`) and final GPU-enabled native release PASS (`cargo build --release -p fulgur-gui --features gpu --bin fulgurstack-gui`). No automated functional tests, scientific runs or benchmarks were performed. Release executable: `target/release/fulgurstack-gui.exe`. Actual 1366 x 768 / 125% native layout, interaction and STOP acceptance remain pending owner review; no macOS verification or new scientific parity is claimed.

### Compact Desk owner review

2026-10-25 follow-up: Processing is the only scientific-progress display. Quality no longer repeats inspection progress/preparation. Underlying run actions/timing are hidden while a modal is open and return in the bottom bar on dismissal. Review continuation/STOP are consolidated in that bar; Processing is reopened from top navigation. Source-preview loading remains independently labeled. This UI-only correction awaits owner review under the same local-only policy (ADR-052).

Follow-up local verification (2026-10-25): formatting and GUI/all-target GPU-enabled lint PASS; GPU-enabled release build PASS. Final lint used task-local `CARGO_INCREMENTAL=0` to avoid unsupported hard-link cache warnings from the restricted filesystem; no source-warning suppression was added. Static UI audit finds a single scientific ProgressBar renderer (the Processing phase grid), with only the separate source-preview loading spinner remaining. No automatic functional tests, scientific stacks, benchmarks, commits, pushes or remote CI were run; native interaction/DPI acceptance remains manual.

Launch from the repository root with the existing settings (no job starts automatically):

```powershell
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui
```

For the existing small input set, explicit startup inputs replace the restored selection:

```powershell
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui -- "SeestarS50 Subs"
```

At 1366 x 768 with Windows scaling set to 125%, review the maximized window. Confirm the wider continuously scrolling Quality list, its optional chart/preview, and Basic/Alignment/Rejection/Performance setup. After manually starting a run, check all four Processing phase times and header STOP without scrolling. Back to workspace must retain bottom-bar STOP; reopen Processing to inspect the same job. On completion inspect Master/Coverage/High rejection/Report & files, saved-map labels, phase times and actual file-opening actions. The completed summary should stay tied to its saved result during a later run. This manual checklist is a pending owner acceptance step, not an automated verification claim.

### Verification boundary for this publication

The latest release build with optional GPU support succeeded locally. Formatting and patch hygiene were checked for the consolidated publication. Under the owner's continuing UI/UX policy, the latest percentage/timing/modal/output/settings/reuse/manual-selection iterations have **not** received new automatic functional tests, scientific parity runs or benchmarks. The subsequent owner manual acceptance covers the three behaviors listed above; it does not establish other unreported checks. The checkpoint evidence later in this document belongs to earlier revisions and must not be reported as a pass for the current snapshot.

M1–M4 acceptance and the published CPU optimization evidence are unchanged. Windows is the platform exercised so far; macOS, full native DPI/accessibility acceptance, packaging and completion of M5 are not claimed.

### Manual review of the current application

1. Launch without arguments to restore settings, or supply SeestarS50 Subs to replace the restored input selection. Inspect quality, then change Use checkboxes and stack. Unchanged inputs should go straight to registration with Inspect marked Reused. Inspect explicitly again to reset measurements/overrides.
2. Confirm that all Processing content and STOP fit without scrolling; completion should close the modal and show Results. Reopen Processing intentionally to view its frozen summary. STOP requests cooperative cancellation and shows Stopping during cleanup.
3. Repeat into the same output folder: earlier products must remain unchanged and the new family must receive a version suffix. Enable each map-saving option independently. Restart to check settings restoration; quality measurements/manual choices intentionally require a new session assessment.

These are owner review steps, not checks executed during this documentation/publication task.

### Manual review of image navigation

Load a selected subexposure, move the pointer over a star and turn the wheel. The star should remain under the pointer while zoom changes. Drag to inspect another area, select 1:1 for native-pixel detail, and select Fit or double-click to restore the complete image. Repeat with a completed master in Results, with Auto stretch on/off and at the chosen Windows display scaling. Source/result views keep separate navigation positions. Allow the Loading native detail label to clear before judging individual pixels. No automatic functional checks or new science/performance measurements cover this slice yet.

## Initial implementation scope (historical checkpoint)

- Native FulgurStack window with Frames, Quality, Processing and Results.
- Navigation and Stack settings together on the left; selected source preview on the right of Quality.
- Continuous virtualized Quality table without pagination; accepted/rejected filters, sortable measurements, HFR plot and accepted source selection as reference.
- Native file/folder dialogs, file/folder drag-and-drop, deduplicated immutable processing inputs and explicit validation errors.
- Quick stacking or optional review after inspection and before registration; continuation reuses the measured catalogs and verifies source identities.
- Real phase states and integration progress; cooperative cancellation, close-during-run confirmation and truthful partial final-output publication. Current user-facing progress is a percentage.
- Same accepted CPU pipeline, optional GPU build/selection, accepted chronological-median automatic reference, exact reference dimensions without crop, automatic RAM-aware tiles and cache off.
- One bounded native VNG source preview; display-only unlinked RGB STF. Master and RGB-channel coverage/high-rejection tabs use downsampled display buffers, with fixed 0..100% map scales.
- Final FITS and requested maps/report use the existing atomic no-clobber writers. Source GPS remains private in default reports/UI and is preserved only in the master under the existing provenance policy.

## Launch locally

Run from the repository root in Windows PowerShell:

```powershell
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui -- "SeestarS50 Subs"
```

Or launch the already built application:

```powershell
.\target\release\fulgurstack-gui.exe "SeestarS50 Subs"
```

Normal launch uses supplied inputs, or restores the previous selection when no inputs are supplied, without starting a job. Choose an existing output folder and a master filename. Select Inspect quality or Stack images. For an external template select External FITS and choose RegisteringTemplate200pct.fit. Enabled maps and the report use derived sibling names in the selected versioned family. Existing files are never overwritten.

The CPU-only build omits --features gpu; selecting GPU then explains that it is unavailable. In a GPU-enabled build actual adapter initialization can still fail clearly; there is no silent CPU fallback.

## Implementation boundary

This is a connected application, not full M5 completion. The source/master viewer combines a 640-edge overview with bounded native-resolution viewport crops for pan/zoom and 1:1; it does not retain full reconstructed datasets. Project save/load, localization and advanced crop policies remain future work. The GUI intentionally exposes the accepted reference-size canvas only. Quality plots/weights before registration are provisional; completed-run rows use final registered weights. Changing settings retains the saved result's provenance instead of claiming a recomputation.

The scientific worker and preview task are bounded. Source preview is cancelled/joined before integration allocations; full-resolution maps remain worker-owned and are released after final publication/downsampling. Display data/queues/textures fit the existing catalog/runtime reserve. Source identity checks cover canonical inputs, length and modification time before/after inspection and after review; they are not a cryptographic lock against concurrent external modification. Source integrity is verified independently against saved dataset hashes in real-data validation.

Processing time excludes human review wait, report serialization and UI thumbnails. Peak working set includes the native process and its UI. It is not directly comparable to a fresh controlled CLI benchmark; this task reuses saved CLI products and does not rerun a baseline or all 1,140 frames.

## Historical verification — initial native checkpoint

Platform: Windows 11 x86_64 only. Headless layout runs at 100%, 125%, 150% and 200% check rendering/virtualization, not a full native DPI or screen-reader audit. AccessKit is enabled; actual assistive-technology behavior remains an owner/native acceptance gate. macOS build/runtime validation has not been performed in this task.

| Gate | Result |
|---|---|
| Local formatting and default/all-feature lint | PASS |
| Default release suite | PASS, 124/124 tests |
| All-feature release suite | PASS, 126/126 tests; actual GPU, AVX2 and real FITS explicitly required |
| Native window flow | PASS: measured Quality, single-source preview, continue, real 64-tile integration and all three result tabs |
| Scientific output parity | PASS: entire master, coverage and high-rejection FITS byte-identical to stored accepted CLI products |
| Scientific report parity | PASS: selection, registration, photometry, reconstruction/rejection/export/work counters unchanged |
| Integrity and I/O | PASS: all 32 source, two PI reference and external-template hashes unchanged; no intermediate images or .tmp leftovers |
| Quality table | PASS: 10,000-row headless test renders only visible table rows without pagination |
| Review cancellation and validation | PASS: cancelled review publishes nothing; invalid/reserved names and existing destinations fail without source changes |

The initial sandbox test attempt could not perform Windows no-clobber publication. The unchanged operations were verified with normal local Windows access. Initial native smoke attempts exposed a Windows GUI-without-console problem after registration: the integration observer path now avoids console writes, and worker panics produce an explicit terminal error instead of silently leaving stale UI status. Failed attempts remain in outputs/M5_GUI_native_review and outputs/M5_GUI_native_review_v2; they are not successful measurements. The final successful run is outputs/M5_GUI_native_review_v3. No CLI baseline was rerun.

### Native 32-frame result

Reference: RegisteringTemplate200pct.fit. CPU, automatic RAM-aware 512 x 512 tiles, 12288 MiB ceiling, cache 0, exact 4000 x 4000 reference grid at (0,0), no crop.

- Input / accepted / rejected: **32 / 27 / 5**; accepted exposure **270 s**.
- Processing: **11.9451358 s**; automatic QA review/preview wait **0.5856406 s**, reported separately.
- Peak process working set: **818.1328125 MiB**, including the native UI process.
- Stored CLI processing time is historical. This is a correctness/native-flow check, not a controlled UI-overhead or speedup benchmark. Full 1,140-frame processing was not run.
- Native client screenshots: 1800 x 1125 pixels; Quality, master, coverage and high-rejection views were visually inspected. Their captions distinguish source data, saved result, display-only STF and percentage map channels.

Local artifacts:

- outputs/M5_GUI_native_review_v3/native_master.fits
- outputs/M5_GUI_native_review_v3/native_master_coverage.fits
- outputs/M5_GUI_native_review_v3/native_master_high_rejection.fits
- outputs/M5_GUI_native_review_v3/native_master_report.json
- outputs/M5_GUI_native_review_v3/equivalence.json and native_smoke.json
- outputs/M5_GUI_native_review_v3/delivery_evidence.json: final source/binary fingerprints and local gate totals
- outputs/M5_GUI_native_review_v3/quality.png, master.png, coverage.png and high-rejections.png

Reproduce parity without another reference calculation:

```powershell
python scripts/tests/verify-gui-stack.py --before outputs/M5_fallback_DS-B_after_external200.json --after outputs/M5_GUI_native_review_v3/native_master_report.json --output outputs/M5_GUI_native_review_v3/owner_parity_new.json
```

Choose an unused verification report path. The verifier permits only the frontend/timing/RSS fields and explicitly checks the automatic-versus-manual tile-policy/RAM-snapshot differences; selected geometry, allocation estimates, all work/science fields and FITS bytes remain strict.

## Historical verification — review-wait correction (2026-10-23)

The owner's apparent 180-second quality hang was the optional approval wait: inspection was already complete, and registration had not started. The old Processing view incorrectly showed a spinner and increasing elapsed time; the checkbox explaining the pause could be below the visible settings area.

The review option now sits above the main stack action and remains off by default. When requested, both Quality and Processing show **Waiting for your confirmation**, measured acceptance counts and a prominent **Continue stacking** action. Processing explicitly shows inspection complete and the remaining stages not started. Approval wait has no spinner, elapsed counter or repaint heartbeat. Displayed processing elapsed excludes accumulated/current review wait after continuation. F5 continues the existing inspected job; it does not inspect the sources again. Cancellation/exit uses the same cancellation transition and remains responsive.

Per the owner's instruction, animations indicate actual background work only. Source loading, active inspection/registration/integration/publication and cancellation cleanup may animate; idle, completed and human-review states do not. Ordinary widget transition animation is disabled. Source-preview loading remains separately indicated if active during a review.

Regression checks cover a real main-area button click into the parked worker, a simulated 180-second human wait, quiet idle/review redraw behavior, automatic continuation with review off and existing review cancellation. Native QA adds a waiting Processing screenshot; **--ui-smoke-quick** exercises the unattended path. Neither mode writes intermediate astronomical images or changes scientific arithmetic.

Native validation of the correction passed on the same 32-frame dataset, CPU, external 200% reference, automatic 512-square tiles, no crop and cache off:

| Flow | Processing time | Separate review wait | Peak process RAM | Selection |
|---|---:|---:|---:|---|
| Review, explicit continuation | 10.7490924 s | 0.6198819 s | 801.2421875 MiB | 27/32 accepted |
| Quick, no approval stop | 12.0354467 s | 0 s | 807.0625 MiB | 27/32 accepted |

Both entire masters and both diagnostic maps are byte-identical to the stored accepted CLI products; strict science/work/allocation fields and all source/PI/template hashes are unchanged. These are native-flow checks on 32 inputs, not a measured optimization claim or a full-set benchmark. No reference calculation was rerun.

Local evidence: outputs/M5_GUI_review_wait_fix and outputs/M5_GUI_quick_wait_fix contain final FITS, reports, native QA traces/screenshots and equivalence.json. The corrected static approval screen is outputs/M5_GUI_review_wait_fix/waiting.png. Release suite logs are outputs/M5_GUI_review_state_default_final.log and outputs/M5_GUI_review_state_all_final.log. This checkpoint was verified locally before the manual-only UI policy and consolidated publication authorization. M5 stays open.

Final correction gates: format/default/all-feature lint PASS, **126/126 default** and **128/128 all-feature** release tests PASS, including actual local FITS/AVX2/GPU checks. The two added GUI regressions verify a real main-area continuation click, no review/idle animation heartbeat, exclusion of a simulated 180-second review wait and an unattended no-review path. Source/binary fingerprints and both native parity reports are recorded in outputs/M5_GUI_review_state_evidence.json. Scientific engine code and the accepted FITS products remain unchanged by this UI correction.

## Use-selected registration and Processing alignment (2026-10-26)

Owner-authorized local engine/UI slice, ADR-060. Registration now schedules only effective Use-on frames by default. Stack setup > Basic / Reference exposes `Also register Use-off frames for diagnostics`, off by default and fixed during a run. Optional diagnostics never change selected-frame prediction, photometry or stack membership. Original rows/session grouping and original-input map denominators are retained. Register Status separates scheduled successes/failures and excluded/skipped rows; its multiline text is vertically centered with Time and Progress. STOP remains fixed.

The stored full-run CFA-catalog replay PASS: 981/981 selected frames registered; 159 skipped by default; 1140 registered with diagnostics. Every selected report row is exactly identical between the two new modes. Against the historical report, maximum corner displacement is 5.533e-12 reference px (native 1.890e-12 px), maximum external inlier RMSE 0.647704 px, maximum RMSE increase 1.051e-13 px and maximum normalization/weight difference relative to max(1,abs(old)) 4.548e-13. Historical rows are not claimed bit-identical (stored decimal JSON replay); all preregistered accuracy gates pass. Source header/metadata checks pass. Replay peak working set 656.90 MiB; the 65.53 s test envelope includes JSON loading, header validation and both registration modes and is not a registration benchmark or end-to-end stack time. Private evidence: outputs/registration-selection-gate.json.

No new full 1140-frame integration or reference benchmark was run; existing synthetic integration, source immutability, geometry, photometry and actual hardware gates remain required for this engine change. Native visual/interaction review at 1366 x 768 / Windows 125% remains manual. Check the default Register summary `981 registered / 0 failed / 159 excluded` with unchanged Use choices, optionally repeat with diagnostics on, confirm unchanged eligible count/results and inspect Status alignment and fixed STOP. Existing preferences restore the diagnostic option as false until explicitly changed. All work stays local without commit, push or remote CI; M5 remains open.

Final local verification for ADR-060: formatting and default/all-feature workspace Clippy PASS; release workspace suites PASS (131/131 default, 133/133 all-feature), with actual real-FITS, AVX2 and GPU hardware gates enabled. The focused headless Processing test verifies Status/Time/Progress center alignment at 1060 x 520 logical client size. Private replay evidence remains excluded from Git. Verification-script profile checks support both historical all-input and new scheduled-only timings and reject inconsistent skipped counts; exact before/after science/FITS comparisons remain strict and were not relabeled as historical bitwise parity. Native DPI appearance and the newly prepared application remain for owner manual review; macOS was not tested.


## Integration worker choice and Windows identity (2026-10-26)

Owner-authorized local profile/optimization, ADR-062. CPU Automatic now uses available parallelism up to 24; Stack setup > Performance offers remembered Automatic/12/16/24 CPU worker choices, fixed during a run. GPU retains its 16-worker bound. Memory planning includes every active worker and a bounded 64 KiB original-byte read buffer; source/image cache remains off. Fixed STOP, original reference geometry, selection and the 1366 x 768 / 125% design are preserved.

Actual local profiling of the frozen native plan (940 accepted of 1140) found the three-run central-region medians 23.362894/20.804144/18.301232 s for original 12/16/24 workers. With read-ahead they are 22.531762/20.096449/17.716736 s. The complete native-reference pair is 162.791162 to 135.044169 s integration-only, 17.044533% shorter. Whole master/maps, raw samples and scientific counters remain exact; all 1140 source hashes/metadata remain unchanged. These are explicit requested profiling runs, not a historical full external-reference baseline rerun or a portable speed guarantee. See M5_INTEGRATION_PROFILE_REVIEW.md for increased byte traffic, CPU-efficiency/cache limitations and measurement scope. Windows denied WPR profiling privilege; process CPU/user/kernel and application I/O counters provide the profile, with no ETW stack-analysis claim.

The executable embeds FileDescription/ProductName FulgurStack and version 0.1.0, sets a stable Windows application ID and exposes its PID/version in File. Verify the native process in Task Manager > Details as fulgurstack-gui.exe with the same PID; friendly-name/grouping appearance remains manual. No changes to Windows power mode, priority, affinity or security were made. Local release only, no commit/push/remote CI; M5 remains open and macOS has not been run.

Final local gates for ADR-062: formatting, default/all-feature workspace Clippy and release suites PASS (134/134 default, 136/136 all-feature), with real FITS, AVX2 and actual GPU hardware gates enabled. The new gates cover CPU counter monotonicity, buffered-vs-frozen-row reader bits/source integrity and 12/16/24-worker full sample/map/source equality. Complete frozen-reference master/map/raw-sample/science equality and all 1140 source fingerprints PASS. No macOS or native Task Manager grouping acceptance is claimed. Local release preparation only; no commits, pushes or remote CI.

Local release preparation PASS: cargo build --release -p fulgur-gui -p fulgur-cli --features gpu --bins. GUI SHA256 5e6fcfdc5cb9aa85a97f90b84873e15d38ae64771eda9fed3f04a2e51d4b41d6; root/deps executable hashes agree and the desktop shortcut targets the updated standard release. Windows executable metadata query confirms FileDescription/ProductName FulgurStack, version 0.1.0 and original filename fulgurstack-gui.exe. Actual 32-source CFA inspection at 16 and 24 threads preserves every measurement/catalog/selection field exactly (27 accepted / 5 rejected); only timings, concurrency and memory telemetry differ. No additional performance claim is drawn from this short inspection gate. Native Task Manager grouping/DPI appearance remains manual. No commit, push or remote CI.


## Parallel native registration review (2026-10-26)

ADR-064 retains Compact Desk, fixed STOP, centered startup and Windows identity. Only the two independent chronological walks of native registration now run together; each preserves its original prediction history and direct-anchor fitting. Three paired registration-only real-catalog replays give serial/parallel medians 17.0449013/10.1033996 s (40.724798% shorter), with 940/940 selected successes, 200 skipped, exact all-field scientific plans and unchanged 1140 source fingerprints. These are same-parsed-input replay timings, not historical GUI or total-stack times. Format/default-all-feature lint and release suites PASS 137/137 and 139/139 with actual real FITS/AVX2/GPU checks; GPU-enabled GUI/CLI release build PASS. Existing desktop shortcut targets the prepared release. For manual review, repeat the existing stack selection, check Register counts/elapsed and fixed STOP; output/map settings and integration are unchanged. See M5_NATIVE_REGISTRATION_WALKS_REVIEW.md for scope, resource tradeoffs and limitations. No full reference integration rerun, macOS, commit, push or remote CI; M5 remains open.


## Optional geometric correction (2026-10-26)

ADR-065 adds Stack setup > Registration > Advanced registration > Correct geometric distortion, off by default and restored false from older preferences. Enabling requests independent held-out/outer-field validation; insufficient or non-improving models keep ordinary integration. JSON stores per-frame decisions/metrics and accepted models; Results shows corrected/skipped integrated-source counts. A source skipped for correction is not excluded from the stack. The native correction-validation pass contributes actual completed work to the existing single Register progress display. Compact Desk dimensions, STOP, phase alignment, maps/save choices, Windows identity and centered startup are preserved.

The owner's final artificial reference is outputs/geometric_correction/RegisteringTemplate200pct_radial3pct.fit: geometry-only monochrome Float32 green plane, 4120-square, corner radius +3%, unchanged original reference. A real 32-source test integrates 27, with 25 corrections and two ordinary fallbacks. Processing off/on is 7.6690582/12.7537752 s in one local CPU-16 pair; retain the +66.301714% small-set overhead instead of the earlier rough estimate. All 34 source/reference fingerprints match. Format/default-all-feature lint and release tests PASS 148/148 and 151/151 with actual FITS/AVX2/GPU. GPU-enabled GUI/CLI release build and existing desktop-shortcut target verification PASS. Review the checkbox, Results counts, corner alignment and visible STOP manually at 1366 x 768 / 125%. See M5_GEOMETRIC_CORRECTION_REVIEW.md for metrics, model limits and artifacts. No full reference integration rerun, macOS, commit, push or remote CI; M5 remains open.
# Optional geometry acceptance repair (2026-10-26)

Automatic reference follow-up: Stack setup > Registration now offers Auto,
explicit Chronological median, selected input and external FITS. Auto retains
the median for a single observing group and selects count-based framing for
multiple groups. Results distinguishes predicted selected-frame geometric
coverage from the final original-input/post-rejection Coverage map. Local
format/lint and 160/163 default/all-feature release tests PASS; the 32-source
artificial-date end-to-end case retains 27/27 selected frames, while external
reference master/maps remain exactly equal to the prior control. See
[Auto review and limits](M5_AUTO_REFERENCE_REVIEW.md). Standard release/desktop
shortcut updated; no native visual or real multi-night acceptance is claimed.

The owner-authorized ADR-066 repair prevents incompatible linear fallback after
nonlinear rejection. Results distinguishes corrected, validated linear and failed
geometry counts; Quality/report reasons expose registration failures separately
from Use selection. Native scientific provenance, fixed STOP, centered startup,
Compact Desk dimensions, desktop shortcut, icon and Windows identity are retained.
Stored owner-run catalogs now accept 937/940 selected frames (766 direct cubic,
171 calibrated compositions); 136 of the earlier 139 unsafe similarities recover
and three fail safely. See [repair evidence and manual review](M5_GEOMETRIC_ACCEPTANCE_REPAIR.md).
No full reference integration benchmark, commit, push or remote CI is authorized.
Native visual/DPI/PixInsight review and macOS remain unverified.
