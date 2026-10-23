# M5 Native Windows Workspace — Progress and Review

Date: 2026-10-23. Status: native Workspace implemented; latest UI additions release-compiled and awaiting owner manual acceptance. **M5 remains open.** The owner authorized one consolidated progress commit and push. GitHub Actions remain disabled; no new automatic functional tests, scientific stacks or benchmarks were run for publication.

## Current progress snapshot

- Workspace navigation and Stack settings share the left panel. Quality uses a continuous virtualized table without pagination, with the selected subexposure preview on the right. The window uses a very dark sky-blue background.
- Quality, registration and integration report percentages. Processing is a compact modal without scrolling, with fixed STOP/Close actions, elapsed time, an approximate remaining-time estimate and Inspect/Register/Integrate/Write outputs durations. Successful stacking closes the modal and opens Results. Animation is limited to actual background work.
- Completed quality measurements are retained for the current application session. Before reuse, inputs are rediscovered and canonical membership, byte lengths and modification timestamps are compared. Changed inputs cause fresh inspection and clear stale manual choices; explicit Inspect always measures again. Changing output, reference or processing options does not itself invalidate source measurements. Reused Inspect is labeled Reused and contributes zero new inspection time.
- Per-frame Use checkboxes allow manual inclusion/exclusion; Use automatic selection resets overrides. Soft HFR, eccentricity and relative-star-count rejection can be overridden when measurements remain usable. Unreadable, incompatible or unusable frames cannot be forced in. Registration still enforces fit/photometric validity. All original deduplicated inputs remain the denominator of both maps, and the report records manual decisions and original automatic reasons.
- Existing output families are preserved. The GUI chooses a common _v2, _v3, etc. suffix when needed, retaining atomic no-clobber publication. Save coverage map and Save high rejection map are independent options, both off by default; master and JSON report are always saved. Unsaved map thumbnails are labeled Preview only; map computation remains in memory.
- Last-used input/output selection, master filename, reference, map flags and stack preferences persist in %LOCALAPPDATA%/FulgurStack/settings.json. Explicit startup input arguments override restored inputs. Opening the application never starts inspection/stacking. Inspection catalogs and manual choices are not persisted across restarts.
- CPU remains default; GPU remains an explicit choice. The scientific reference grid, no-crop policy, bounded memory and disabled FITS-byte cache remain intact. Retained quality metadata is not an image-data cache.

### Verification boundary for this publication

The latest release build with optional GPU support succeeded locally. Formatting and patch hygiene are checked for publication. Under the owner's continuing UI/UX policy, the latest percentage/timing/modal/output/settings/reuse/manual-selection iterations have **not** received new automatic functional tests, scientific parity runs or benchmarks. Their manual acceptance is pending. The checkpoint evidence later in this document belongs to earlier revisions and must not be reported as a pass for the current snapshot.

M1–M4 acceptance and the published CPU optimization evidence are unchanged. Windows is the platform exercised so far; macOS, full native DPI/accessibility acceptance, packaging and completion of M5 are not claimed.

### Manual review of the current application

1. Launch without arguments to restore settings, or supply SeestarS50 Subs to replace the restored input selection. Inspect quality, then change Use checkboxes and stack. Unchanged inputs should go straight to registration with Inspect marked Reused. Inspect explicitly again to reset measurements/overrides.
2. Confirm that all Processing content and STOP fit without scrolling; completion should close the modal and show Results. Reopen Processing intentionally to view its frozen summary. STOP requests cooperative cancellation and shows Stopping during cleanup.
3. Repeat into the same output folder: earlier products must remain unchanged and the new family must receive a version suffix. Enable each map-saving option independently. Restart to check settings restoration; quality measurements/manual choices intentionally require a new session assessment.

These are owner review steps, not checks executed during this documentation/publication task.

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

This is the first connected application, not full M5 completion. Initial source preview is fit-to-panel and capped at a 640-pixel edge; full-resolution pan/zoom, project save/load, localization, session editing and advanced crop policies are future work. The first GUI intentionally exposes the accepted reference-size canvas only. Quality plots/weights before registration are provisional; completed-run rows use final registered weights. Changing settings retains the saved result's provenance instead of claiming a recomputation.

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
