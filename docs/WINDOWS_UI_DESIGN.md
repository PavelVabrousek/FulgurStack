# Windows UI Design Proposal

Status: historical design proposal, superseded where amended by the current native implementation. Initial proposal: 2026-10-22. Native implementation authorized: 2026-10-23. The owner selected **quick stacking with optional quality review**. Conversation is Czech; all design artifacts and UI labels are English. A consolidated progress commit/push was authorized on 2026-10-23; verification remains local and GitHub Actions remain disabled.

## Processing and diagnostic map clarification (2026-10-26)

Completed Register/Integrate Status cells show their real phase populations and distinguish failures from excluded inputs. Geometric registration may include quality-rejected diagnostic frames; only eligible frames enter integration. Keep the wrapped status, retained saved summary and fixed STOP.

Both diagnostic maps always have in-memory previews; Output flags save their FITS files independently. Use identical saved/unsaved labels and open only the selected saved map. Coverage defaults to display 0–100%, High Rejection to 0–5%, with explicit alternative percent ranges. This replaces the former fixed 0–100% display assumption only; scientific fractions/FITS are unchanged. High Rejection has a labeled peak-preserving overview and full-map per-channel statistics, while saved native detail follows the selected display scale. See ADR-059 and the current native review for read-only last-run evidence and manual acceptance limits.

## Compact quality table amendment (2026-10-25)

Header correction: the current Quality surface has a single control strip. Use on/off/toggle appear directly, and Marked contains only automatic-for-marked/remove. Preview/HFR toggles are available once in global View. Remove the repeated heading, measurement/weight description, inline counts and review prompt; the fixed main footer now owns Shown/Marked/Use/exposure, measurement/weight/filter/removal status and review messaging. Preserve compact 50-point footer geometry, truncated status labels with full hover text, fixed STOP/Continue and modal ownership (ADR-058).

Follow-up correction: Folder precedes Frame and shows the full parent directory; Frame shows the standalone filename. Folder participates in filtering, sorting, resizing and content fit. When Folder, Frame or Captured is narrower than its text, clip the leading portion and preserve the end. Keep left alignment when the value fits, and retain full source/Captured information in the row tooltip. Existing Use, selection, STOP and DPI behavior remain unchanged (ADR-056).

The owner approves the native Excel/PixInsight-style quality table: 20-point virtualized rows, fixed header/filter row, alternating row backgrounds and full-row highlighting separate from Use. Columns support sorting, dragging dividers and double-click content fit. The filename column initially fits complete names. Use follows all quality fields and remains pinned at the right while the data scrolls horizontally.

Each column filters independently: text contains, numeric inclusive min/max, state or Use on/off. Active filters combine, show indicators and can be cleared together. Highlight with click, Ctrl-click, Shift range, Ctrl+Shift range, Ctrl+A for visible rows and Escape. The active row controls the existing preview. Filters clear highlighting without changing Use. Batch Use on/off/toggle, automatic reset and remove are available above the table and from the row context menu, applying only to marked visible rows. Unusable inputs remain protected.

Remove hides and excludes the frame from the current inspection list; More > Restore removed brings it back with Use off. Original files and original-input scientific map denominators remain intact. A new inspection/input change resets the session-local list removal. Shown/Marked/Use counts are distinct; measurements and final/provisional weights remain labeled. Preserve the 1366 x 768 / 125% Compact Desk target, centered normal-window startup (ADR-054) and separate fixed STOP surfaces (ADR-052). Release preparation/manual review only, without automated functional/scientific/performance runs or publishing. See ADR-055.

## Current implementation amendments (2026-10-23)

### Compact Desk supersession (2026-10-24)

The owner approved Compact Desk for native implementation after reviewing the 1366 x 768 / 125% proposals. The selected revision restores the larger continuous subframe table and narrow right preview; the later enlarged-preview proposal was rejected. Top navigation and a dedicated Stack setup screen replace the combined left sidebar. Basic, Alignment, Rejection and Performance organize existing supported controls; this UI slice does not introduce new crop modes, cache controls or configurable scientific rejection settings. A maximized native window uses the OS work area instead of a fixed physical-pixel scale or a 650-point minimum height.

Owner correction (2026-10-25): phase progress is rendered only in Processing, including inspection preparation. Quality does not repeat the inspection progress bar. Background workspace run controls/timing are hidden while a modal owns the actions, restoring bottom-bar STOP/elapsed/review continuation on dismissal. Processing has one launcher in top navigation; Quality review has one Continue/STOP set in the bottom bar. Exit confirmation temporarily replaces the Processing modal. Source-preview loading and completed-result summaries retain their separate meanings (ADR-052).

Processing remains a compact modal without scrolling. STOP stays in its header, and Back to workspace or Escape dismisses the presentation while the active run continues with STOP in the persistent bottom bar. A terminal summary has Close; successful completion opens Results. Results retain their completed-run phase times/configuration across later runs, show truthful saved/unsaved map state and expose master/maps/report files. All displayed telemetry comes from real worker/results data; prototype example timings are never copied into the application. Local release/manual review only, no automated functional/scientific/performance runs or publishing. See ADR-051 and M5_WINDOWS_APP_REVIEW.md.

The [native application progress report](M5_WINDOWS_APP_REVIEW.md) describes the current delivered behavior and verification limits. It takes precedence over the original proposal below. Navigation/settings share the left panel; Quality has no pagination and the selected-source preview occupies the right panel. Processing is a compact modal without scrolling, with a fixed STOP/Close header, per-phase percentages, elapsed/remaining estimates and phase times. Background work alone animates, and successful completion opens Results.

Quality now supports session-local Use checkboxes and reset to automatic selection. Usable frames rejected by soft quality thresholds can be included manually; invalid or incompatible frames cannot be forced in, and registration can still reject a selected frame. Completed inspection is reused only after source-identity validation. Output filename families are versioned automatically; coverage/high-rejection file saving is independently optional, off by default. Last-used settings persist locally. Project save/load and native accessibility/macOS acceptance remain future work. Latest UI changes are release-compiled for manual owner review, without automatic functional tests or a new benchmark.

Source/master wheel zoom, drag panning, Fit and physical-pixel 1:1 are now implemented for manual review, using bounded native viewport detail. Saved diagnostic maps share those controls; unsaved maps remain overview-only. Project save/load and native accessibility/macOS acceptance remain future work. See ADR-050.

The remaining sections preserve the original workflow/layout proposal and its historical prototype verification; they are not a claim that every proposed control has been implemented or that those gates cover the latest native application.

## Accepted Workspace revision (2026-10-23)

The owner selected Workspace as the starting layout with two changes: Quality uses a continuously scrollable, virtualized table with no pagination; Stack settings share the left navigation panel, freeing the right panel for the selected subexposure. The initial native application follows this arrangement. The historical Focused candidate remains a proposal, not an additional native layout. See [native application review](M5_WINDOWS_APP_REVIEW.md) for the implemented scope and verification.

## Main workflow

1. Add FITS folders or files for one target and filter; deduplicate immutable inputs. Different exposure lengths are supported.
2. Choose the output folder and master filename. New projects default to CPU, automatic accepted chronological-median reference, reference-size canvas without crop, automatic RAM-aware tiles, 12 GiB ceiling and FITS cache off.
3. Select **Stack images**. Inspection, automatic quality selection, registration, integration and output publication run without mandatory review dialogs.
4. Optionally enable **Review quality before stacking**. Stop after inspection, before registration; the owner can examine quality and choose an accepted reference, then select **Continue stacking**.
5. Show the master, coverage map, high-rejection map and measured report. Open the final FITS normally in PixInsight.

The interactive example starts from the measured M33 project with its existing external 200% reference. This is a restored example, not the default reference choice for a new project. Results and thumbnails belong to that saved CPU run; changing example settings does not generate a new scientific image.

## Layout candidates

| Candidate | Navigation | Intended use |
|---|---|---|
| Workspace | Persistent left navigation; central work area; stack settings on the right | Repeated use, switching between frame review and results; recommended starting point |
| Focused | Horizontal navigation and a short workflow strip; central work area; settings on the right | More horizontal space for quality tables and previews |

Both candidates use the same quick workflow and the same science settings. Neither makes quality review mandatory. The interactive proposal has appearance and density controls for comparison; they are design controls, not additional scientific parameters.

## Screens

### Frames and setup

- Show target, filter, instrument, input count, exposure durations and total input exposure. Distinguish summed exposure from elapsed acquisition span and processing time.
- Support add folder, add files, drag/drop, removal from the input list, multiple sessions and duplicate feedback. Do not implement calibration-frame controls before their processing model exists.
- Keep one prominent **Stack images** action. Keep reference, engine, output and optional review visible. Put resource and rejection details in an Advanced section.
- With no inputs, show a single add-files/folders prompt. Incompatible or unreadable inputs have explicit reasons and prevent accidental mixing of targets or filters.
- A previous result may be previewed, clearly labeled with its run and geometry. Configuration changes mark it as a previous result; they must never relabel it as a new result.

### Quality

- HFR, star-count and weight charts use actual acquisition order/time and quantities with units. Never invent a quality score.
- A sortable, virtualized frame table shows filename, exposure, HFR, star count, eccentricity, weight and acceptance/rejection reasons. Filters: All, Accepted, Rejected.
- Initial quality-only weights must be labeled provisional; normalized final weights appear after registration. The prototype uses saved final measurements.
- Choosing an accepted row as reference sets the manual-reference choice visibly. Automatically rejected rows cannot be forced into the reference role.
- The first design does not add force-inclusion or manual quality overrides. If manual exclusions are added later, keep the complete original input denominator for maps and record exclusion reasons; passing only selected paths to the current engine would silently change coverage semantics.
- Optional review precedes registration so reference selection does not cause an undisclosed repeated registration. Retain only validated catalogs/plans, never reconstructed image caches.

### Processing

- Show Inspect, Register, Integrate and Write outputs as distinct phases, with the current phase emphasized.
- Use actual phase counts where known: inspected/total, registered/total and completed/total output tiles. No invented overall percentage across unequal stages.
- Show elapsed wall time and observed working set; distinguish the configured memory ceiling from actual usage. Offer an ETA only once sufficiently supported by observed phase work; otherwise show no ETA.
- Keep the window responsive with one active job. The current engine supports cancellation, not pause/resume; the first GUI must not promise pause or crash recovery.
- **Cancel run** requests cooperative cancellation, joins work and reports a clear Cancelled state. Originals remain unchanged. Disable mutation of active job settings. Navigation and display-only controls remain usable.
- Closing the application during a running job requires a clear choice between continuing and cancelling the job before exit.
- Publication is not an all-files transaction today. If cancellation/failure happens after some final files have been published, identify the files that exist; do not claim that every output was rolled back. Temporary files must still be cleaned up.

### Results

- Tabs: Master, Coverage, High rejections. Add fit-to-window and 1:1/pan controls in the native implementation.
- Auto stretch affects the preview only. The initial proposal uses an explicitly labeled unlinked-RGB stretch; native UI should provide linked/unlinked RGB display control. This is not color calibration or an export transform.
- Maps use fixed 0..100% legends and an explicit R/G/B channel selection. No auto stretch of quantitative maps and no false monochrome merge of channel-dependent rejection.
- Coverage means surviving samples divided by all original deduplicated inputs. High rejection means upper-rejected samples divided by the same denominator. Neither means geometric overlap, confidence or exposure time.
- Keep saved-run settings/provenance with the result. Show processing time, peak RAM, input/accepted/rejected counts, accepted exposure, reference and dimensions.
- Provide Reveal output folder and system Open actions for the files that actually exist; do not require or assume a particular PixInsight installation path.
- Preserve valid scientific-anchor observing location in the master FITS; default tables, logs, recent-project lists and reports show no GPS coordinates.

## Settings and validation

| Setting | Default / behavior |
|---|---|
| Engine | CPU; explicit GPU option with availability/reason. No silent fallback or promise that GPU is faster. |
| Reference | Accepted chronological median using header DATE-OBS, earlier central frame for even counts. Manual accepted input or external geometry-only FITS at 33..300% scale. |
| Canvas | Exact reference dimensions and crop at (0,0). Common intersection and coverage-optimized crops are explicit advanced choices. |
| Coverage threshold | Relevant only to coverage-optimized crop; does not silently crop or mask reference mode. |
| Output | New master filename plus derived coverage/high-rejection paths and JSON report. Validate every destination before processing; no overwrite action. |
| Memory and tiles | Existing 12 GiB ceiling and automatic available-RAM selection with system reserve; explicit tested manual sizes remain advanced. |
| FITS cache | Off. No promotion to a basic control without a demonstrated product benefit. |
| Pixel rejection | Existing 3.0 / 2.5 sigma defaults; advanced controls must map to the same validated engine configuration. |

Validate missing inputs, malformed/unsupported reference, incompatible metadata, invalid Windows/reserved filenames, duplicate/colliding paths, existing outputs, unwritable destinations, insufficient RAM and unavailable GPU. Errors should explain a concrete recovery step. Preserve input/output identity checks and read-only source handling.

## Windows interaction and accessibility

- Target a desktop workspace around 1280x800 logical pixels, with a compact arrangement for smaller windows. Test native layouts at 100%, 125%, 150% and 200% scaling; distinguish logical and physical pixels.
- Use familiar Windows spacing, Segoe UI/system text, restrained accent colors and system/light/dark appearance. Color never carries acceptance or error status alone.
- Keep output actions reachable, support keyboard navigation and visible focus, and avoid tiny icon-only targets. Native tables must expose meaningful row/cell semantics and virtualized accessibility.
- Proposed shortcuts: Ctrl+O add inputs, F5 start/continue, Escape dismiss a dialog. Escape alone must not unexpectedly abort a running scientific job.
- Use native file/folder dialogs. English is the initial artifact/UI language; externalize strings for later Czech localization. This design task does not authorize a translation deliverable.

## Engineering boundary

Implement the planned `fulgur-gui` with the existing eframe/egui choice from PROJECT_SPEC. The prototype is an interface review surface, not the delivery technology for Windows.

Use a thin orchestration layer over existing pure-Rust pipeline APIs and a bounded background-job event stream. Do not parse console strings or spawn a fresh complete CLI job when the user changes views. Current inspection/registration APIs return reports, integration logs only coarse tile progress, and the CLI owns report/output orchestration; these are explicit integration gaps for the next slice.

Add structured, throttled progress/cancellation events without changing scientific arithmetic. Support inspection -> optional review -> registration -> integration -> final publication using validated immutable inputs and a single configuration snapshot. Retain catalogs/plans only while their source/header/config identity remains valid. Display the provenance of a previously computed result after edits.

Keep large pixel buffers owned by the worker; use bounded downsampled previews and account for UI textures/buffers in the same memory ceiling. Do not duplicate all final full-resolution RGB maps just to draw the interface. A future project file may store settings and input paths, but must not embed raw images or GPS values; its explicit save/load format and versioning require implementation tests.

## Implementation sequence and acceptance

1. Native shell, file selection, settings, immutable job snapshot and validation. Render all designed states without connecting a second scientific implementation.
2. Connect the existing CPU/GPU options and structured progress/cancellation. Add optional quality review without a redundant inspection run.
3. Native quality tables/charts, image STF preview and channel-correct diagnostic maps; bounded memory and keyboard/accessibility checks.
4. Verify exact CLI-versus-GUI CPU master/map parity on 32 inputs, resource/cancellation/error paths, then measure any UI overhead against stored full-set evidence only when a full run is authorized.

Acceptance requires all sources unchanged, no intermediate images, correct reference/no-crop defaults, honest cancellation/publication status, responsive UI, unchanged scientific outputs, local format/lint/release gates and real hardware checks when applicable. Visual approval of this proposal does not by itself establish native accessibility, packaging, performance or completion of M5.

## Prototype scope and verification

The interactive design uses read-only downsampled samples from the accepted full M33 master and both maps, plus the saved 1,140-frame measurement table. No observing coordinates are embedded. Display samples are quantized/downsampled and must not be used for scientific measurements or parity claims.

Only the prototype's navigation, controls, example run states and browser layout are exercised in this task. Its run sequence is explicitly simulated; it does not read new inputs, process FITS, launch PixInsight or write final outputs. The Rust engine, dependencies, datasets and published branch are unchanged.

Local prototype checks on 2026-10-22:

- PASS: both layout variants; 1024-pixel desktop and 320-pixel narrow browser layouts, without root horizontal overflow.
- PASS: saved 1,140-frame table, accepted/rejected filters, sorting, row details, accepted manual reference selection and rejected-reference prevention.
- PASS: master stretch toggle; coverage/high-rejection switching with explicit B channel and fixed percentage legends.
- PASS: quick simulated completion; optional review before registration; continue and cancel states; editable settings after cancellation.
- PASS: reserved Windows filename CON.fits blocks the simulated run; correcting the filename permits completion.
- PASS: empty-project disabled start and automatic-reference default; example-load dialog and Escape dismissal.
- PASS: JavaScript syntax check; fragment below 1 MB with self-contained display samples and no observing coordinates.
- PASS: unchanged-engine local formatting, lint and 118/118 default release tests. A sandbox-restricted first test attempt failed Windows output-publication operations with access denied; rerunning the same unchanged suite outside the sandbox passed. Lint was rerun with incremental compilation disabled after a stale incremental-directory cleanup warning. No dataset benchmark or new GPU hardware validation was needed for this design-only slice.

Native dialogs, file permissions/collisions, DPI scaling, screen-reader integration, actual progress/cancellation, GPU availability and CLI-versus-GUI scientific parity remain future native implementation gates. These prototype checks do not establish those passes. All work stays local without commits, pushes or GitHub Actions.

### Selected registration and aligned phase outcomes (2026-10-26)

Register only effective Use-on frames by default. In Stack setup's registration controls, expose the remembered, default-off `Also register Use-off frames for diagnostics` option; freeze it for the active run. Optional diagnostics do not change selected registration or stack membership. Register Status reports scheduled successes/failures and skipped exclusions separately. Preserve all original rows and map denominators. Center wrapped multiline Status with Time and Progress in bounded 34-point-minimum rows; retain fixed STOP and Compact Desk's 1366 x 768 / 125% target. See ADR-060 for local accuracy gates and verification limits.
