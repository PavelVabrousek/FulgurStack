# Code and Documentation Review — 2026-11-13 (version 0.14.0, HEAD 7317e67)

Read-only review of the whole workspace (8 crates, 3 tools, about 72,000 lines of Rust) and of the documentation
(README, AGENTS, PROJECT_SPEC, 70 files in `docs/`). Nothing was changed, built or run. This file lists what
should be cleaned up, refactored or fixed, with the risk of each change and a suggested order.

**Risk** says what a change can do to the science:
* **Low**: a mechanical change with no numeric effect. The quick suite is enough.
* **Medium**: behaviour changes in an edge case. Add a test, then run the full suite.
* **High**: the output can change. This needs the scientific gates: a byte-identical master or an ADR with new
  evidence.

**Effort** is S (hours), M (about a day) or L (several days).

A ✔ marks a finding the reviewer confirmed by reading the code a second time. The others come from one careful read and
should be confirmed when the work is picked up. Line numbers refer to HEAD 7317e67.

---

## 1. Defects and latent bugs (do first)

These are not style issues. Each one can give a wrong result, lose a user choice, or crash the application.

| ID | Where | Finding | Risk of fix | Effort |
| --- | --- | --- | --- | --- |
| B-01 ✔ | `fulgur-gui/src/app.rs:2395-2407` | **An explicit "None" calibration is not kept.** "None · Not recommended" (`app/frames.rs:1013`) stores `calibration = None`. That is also the state that `poll_scans` treats as "not chosen yet", so every rescan quietly assigns the best calibration again. Rescans happen at app start, on a layout change and on a folder drop. Needs a three-way choice (Auto / None / Set) that is saved with the settings. | Medium | S |
| B-02 ✔ | `fulgur-gui/src/app.rs:1466-1475`, `:3367` | **Opening the app with files wipes the saved workspace.** Command-line inputs ("Open with", drag onto the exe) replace `light_sets` after the "last saved" snapshot is taken, and `on_exit` saves the result. Any unknown argument also becomes a light-set path (`main.rs:44`). | Medium | S |
| B-03 ✔ | `fulgur-pipeline/src/register.rs:594`, `register/cameras.rs:13`, `register/set_photometry.rs:53`, `stars.rs:419,1009` | **Star-engine saturation is hard-coded at 62000 ADU.** The sky engine uses the camera's own level (`sky_engine.rs:328`, `0.95 × saturation`). For a camera that saturates lower, saturated stars enter the brightness scale of the star engine. | **High** (photometric scale can change on such cameras) | S |
| B-04 ✔ | `register.rs:1504-1505` | **A missing background value is handled three different ways.** The master's value is `unwrap()`ed (a panic in the GUI), the source's falls back to `0.` (a wrong offset), and `adopt_direct_fit` (`:1760`) refuses the frame. `sky_engine.rs:349` and `set_photometry.rs:189` use `NAN` or `0.`. One rule is needed. | Medium | S |
| B-05 ✔ | `fulgur-solve/src/index.rs:412`, header check `:485` | **A corrupt or half-written blind-solve index can abort the process.** The header allows `n_entries` up to 2 × u32::MAX, and `Vec::with_capacity(count)` is called before any data is read. That is an allocation of up to about 200 GB, and failing it aborts the process instead of returning an error. Settings loads this file. Check the header totals against the file length first. | Low | S |
| B-06 | `fulgur-solve/src/index.rs:437` | Index save uses `path.with_extension("tmp")`. Two outputs that differ only in extension share one temp file. The temp file is not opened with `create_new`, and it is not removed on error. Copy the `fulgur_core::output::atomic_write` pattern. | Low | S |
| B-07 | `fulgur-pipeline/src/register/cameras.rs:716-726` | `settle` recomputes weights as `1/(noise² · size²)` without a finite check (`sky_engine.rs:762` has one). If HFR is 0, the weight is infinite and every normalized weight becomes 0 or NaN. | Medium | S |
| B-08 | `register.rs:1274-1293` vs `auto_reference.rs:97-111` | The session split and the night split disagree. One uses all frames and stable path order, the other uses accepted frames only and ties on file name. A rejected frame inside a gap can merge two sessions that the other function keeps apart. The 6 h constant is duplicated. | Medium | S |
| B-09 | `fulgur-cli/src/main.rs:1334` → `register_with_options` (`register.rs:706-758`) | **The CLI `register` command takes a different engine path from `stack` and the GUI.** It has no camera grouping or mirroring and no stranded-set second pass, so other cameras in a mixed folder fail. This may be intentional ("relative source names"), but it is not recorded. | Medium | S |
| B-10 | `fulgur-pipeline/src/calibration.rs:533-556` | **Building a calibration master has no memory bound.** Every Rayon strip holds `frames × width × 32 rows × 4 B`. With 500 bias frames at 6248 px and 16 threads that is about 6 GB, outside the 12 GB budget and outside the GUI memory setting. Each strip also reopens every file. Bound the concurrency by bytes (`map_bounded`). | Low (output identical) | M |
| B-11 | `fulgur-gui/src/job.rs:131,184,257-260` | Process-wide calibration caches (up to 4 GiB of masters, plus 8 calibrations and 8 hot-pixel maps) live for the whole session and do not count against the memory setting. Two builders can make the same master at once. `lock().unwrap()` means one poisoned lock breaks every later run. | Medium | M |
| B-12 | `fulgur-gui/src/app.rs:2074-2116`, `:1972` | Calibration previews and the in-app master build cannot be cancelled: the cancel flag is local and never set. Several clicks put several full-resolution planes in RAM at once. `PreviewTask` (`preview.rs:277`) already has the right cancel-on-drop pattern. | Medium | S |
| B-13 | `fulgur-gui/src/app/blind.rs:176,200,545`, `app/sky.rs:263,282` | Six background tasks write `Done` only on success. A panic leaves `Running` forever, and Auto-solve and Build stay disabled until a restart. Results carry no request id, so an older lookup can overwrite a newer one. | Medium | S |
| B-14 | `fulgur-gui/src/sets.rs:1010-1028` | Light-set scans carry no generation number. A slow old scan can overwrite a newer one, for example after two quick layout changes. | Medium | S |
| B-15 | `fulgur-gui/src/app.rs:3375` | Closing during an index build or an in-app master build gives no warning, and leaves the index `.tmp` behind. | Low | S |
| B-16 | `fulgur-pipeline/src/calibration.rs:748-773` | The FITS header parser for loaded masters is hand-written. `from_utf8_lossy` followed by `text[..8]` can panic on a non-ASCII byte, and `split(" /")` cuts string values. Reuse `fulgur_core::fits`. | Low | M |
| B-17 | `fulgur-pipeline/src/calibration/hot_pixels.rs:258-306` | The hot-pixel map writer has its own `exists()` + `rename` sequence (a race; on Windows the rename replaces the file) and its own `card()`. Use `fulgur_core::output::atomic_write`, which `calibration.rs:707` already uses. | Low | S |
| B-18 | `fulgur-core/src/xisf.rs:278-289`, `:1095` | Sub-block sizes and `width × height × channels × bytes` are summed without overflow checks. A crafted XISF can wrap the sum in release and then panic on the slice. Use `checked_` arithmetic. | Low | S |
| B-19 | `fulgur-pipeline/src/integrate.rs:972`, `:1386` | GPU path: `next().unwrap()` panics if the device returns fewer images than it was given, and `samples - low - high` (u64) can wrap. Return an `Error`. | Low | S |
| B-20 | `fulgur-solve-cli/src/main.rs:229`, `tools/blindspike/src/main.rs:658` | `decimal_year` is copied three times. The two CLI copies fall back to epoch 2026.0 when DATE-OBS is missing, which affects the proper-motion correction for old frames. | Low | S |
| B-21 | `fulgur-cli/src/main.rs:962`, `:628-645` | With `--input-sets`, one `--exposure` value is applied to every set, although the help text says exposures may differ between sets. Every catalogue failure is reported as "needs Gaia stars … and a field centre", and the real cause appears only on stdout. | Low | S |
| B-22 | `fulgur-pipeline/src/inspect.rs:1051,1054` | Stale strings in every inspection JSON: "M3 must finalize…", "registration-failure rejection belongs to M3". | Low | S |

Checked and **not** a bug: the pipeline reviewer suspected that the global XISF decode limit
(`integrate.rs:1688-1700`) leaks between runs when no compressed XISF is present. It is set on every run where
`band_row_bytes > 0`, so it does not leak in practice. It is still process-wide mutable state, and passing it
through the block cache would be cleaner (Low, S).

Worth recording but **not** to be "fixed" without gates:
* There are three ESD implementations with different floating-point arithmetic: scalar
  (`rejection_methods.rs:213-227`), column (`esd_columns.rs:341-360`) and the GPU f32 λ table
  (`integrate.rs:1287-1309`). Tied columns fall back to the scalar path (`esd_columns.rs:446`), so decisions at the
  threshold boundary inside one master can come from either. Add a test that pins decisions near λ and document the
  difference.
* Dark-defect repair uses neighbours inside the tile ROI only (`calibration.rs:1356-1385`). Today the 8-9 px margin
  in `integrate.rs:558-629` keeps those pixels out of every sample footprint. Add an assertion that the margin is at
  least the repair radius plus the VNG and Keys footprints, so that future margin tuning cannot silently break this.

---

## 2. Engine refactoring — registration (`fulgur-pipeline`, about 17,700 lines)

### 2.1 Duplicated logic (high value, mostly mechanical)
| ID | Finding | Risk | Effort |
| --- | --- | --- | --- |
| R-01 | The "finalize" block (normalize weights, accepted polygons, intersection, `geometry::canvas`, recount) appears 4 times: `register.rs:1611-1640`, `reference.rs:820-862`, `cameras.rs:728-757`, `sky_engine.rs:1291-1310`. Extract one `finalize_canvas(...)`. The operations already run in the same order. | Low | S |
| R-02 | Photometric normalization (scale, ratio, offset, noise, weight and the finite gate) appears 6 times: `register.rs:1501-1531`, `:1759-1771`, `sky_engine.rs:334-372`, `set_photometry.rs:180-200`, `reference.rs:360-374`, plus the weight-only copies. One `Normalization::new(...)` would remove the drift behind B-04 and B-07. Keep the floating-point operation order exactly as it is. | Medium | M |
| R-03 | Coverage validation is copy-pasted 6 times (`register.rs:687,713,837,898`, `sky_engine.rs:814,1412`), and some of them validate twice. | Low | S |
| R-04 | There are two "sharpest frame" pickers with different tie rules (`cameras.rs:99/153` vs `sky_engine.rs:648`), and progress construction is duplicated (`register.rs:974-981`, `sky_engine.rs:834-841`). | Low | S |
| R-05 | The anchor's FITS file is reread and re-measured several times per run (`register.rs:567,1588`, `reference.rs:746`). `WitnessCatalog` has two `impl` blocks (`register.rs:104`, `:556`). | Low | S |

### 2.2 API surface and parameter sprawl
* **R-06: eight public entry points** for registration.
  * `register` and `register_with_reference` (`register.rs:682-705`) have only one test caller.
  * `register_inspected_with_options` is used only by one CLI test.
  * `register_sets_with_options` / `register_sets_sky` and `register_inspected_with_calibration` / `register_sky` are parallel pairs.
  * Proposal: one `RunContext { root, min_coverage, reference, options, cancel, start, progress, calibrations }` and an `Engine::{Stars, Sky(&SkyOptions)}` value. (Low, M)
* **R-07:** 18 `#[allow(clippy::too_many_arguments)]`. `validate_external_row` takes **15** parameters (`reference.rs:22`) and `align_rows_observed` takes **17** (`reference.rs:945`). Two structs would remove most of the allows: a `FitContext` (sensor, field, reference size, `AccuracyLimits`, deep list, witnesses, cancel) and the `RunContext` above. (Low, M)
* **R-08:** test-only wrappers in production files: `register_catalogs`/`_observed` (`register.rs:1070-1113`), `reference::align` (`:503`), `align_rows` (`:913`), `geometry::solve` (`#[cfg(test)] pub`, `:437`), `stars::analyze` (`:718`). Move them under `tests`. (Low, S)
* **R-09:** the `pub mod cameras/set_photometry/sky_engine` modules (`register.rs:16-18`), the `sky::check` constants and `sky::solve::LADDER` are wider than their callers need. (Low, S)

### 2.3 Oversized functions
| Function | Location | Lines | Natural split |
| --- | --- | --- | --- |
| `register_catalogs_witnessed` | `register.rs:1152` | 572 | multi-night wrapper (1163-1266), session grouping (1274-1293, share it with `nights`, B-08), per-frame walk → `register_row` (1400-1545), report assembly (1659-1722) |
| `validate_external_row` | `reference.rs:22` | ~460 | becomes smaller once `FitContext` exists |
| `register_sky` | `sky_engine.rs:802` | 431 | |
| `apply_native_correction` | `register.rs:1805` | 308 | |
| `scan_plane` | `stars.rs:1014` | 235 | |
| `align_with_progress` | `reference.rs:695` | 215 | |
| `map_rows` | `sky_engine.rs:444` | 201 | |
| `sky::solve::solve` | `sky/solve.rs:708` | 199 | |

All are Low risk if done as pure moves (Effort M each).

### 2.4 Smaller items
* **R-10: magic numbers.**
  * The held-out split `m.source % 5 == 0` appears **11 times** across register, reference and distortion. Make it one `fn calibration_held_out`.
  * The 1.5 px match radius appears 5 times.
  * `window = 12.` (`register.rs:1879`) has the same value as `TIGHT_ANCHOR_RADIUS_PX`.
  * The "≥ 10 stars" minimum appears 9 times.
* **R-11: stringly-typed control flow.** `provenance == "direct_master_matches"` (`register.rs:1498`, `reference.rs:57`), a match on `"external_longer_cubic_validated"` (`reference.rs:352`) and reason-string filters (`:1779`). Use enums whose `Serialize` keeps the same JSON strings.
* **R-12:** `clone_row` (`sky_engine.rs:1238-1271`) is a 33-line hand-written clone. The `Fallback` 5-tuple alias (`register.rs:2114`) and `Attempt` (`cameras.rs:504`) should become named structs.
* **R-13:** about 15 invariant `frame.as_ref().unwrap()` calls in library code (register, reference, sky_engine, auto_reference) turn a broken invariant into a GUI crash. Convert them to `Error::Invalid`.
* **R-14:** `WitnessCatalog::measure`/`raw` (`register.rs:157-180`) turn `Error::Cancelled` into "no witnesses" with `.ok()?`. Cancellation should be propagated.
* **R-15: inline test blocks that could move to sibling files.**
  * Locations: `register.rs:2311-2675` (365 lines), `stars.rs:1250-1863` (613), `cameras.rs:899-1186`, `geometry.rs:824-1101`.
  * Moving them cuts `register.rs` to about 2,300 lines and `stars.rs` to about 1,250.
* **R-16:** about 70 inline ADR-number comments. Move the rationale into doc comments on named constants and keep the ADR number once per item.
* **R-17:** a stale doc comment at `register.rs:182`.

---

## 3. Engine refactoring — integration, calibration, inspection

| ID | Finding | Risk | Effort |
| --- | --- | --- | --- |
| I-01 | `integrate.rs` is 3,441 lines, and about 1,360 of them are inline tests (2071-3427). Move them to `integrate_tests.rs`, as the crate already does for other modules. Put the test-only reference code woven into production (`keys` :631, `warp_rgb` :843, `median` :1022, `combine` :1099-1166, test fields in `ClippingScratch` and `RoiScratch`) into a `#[cfg(test)] mod reference`, like `vng_reference`. Later, turn the `#[path]` siblings into a `src/integrate/` directory. | Low | M |
| I-02 | `integrate_common_progress` is a 520-line function (`integrate.rs:1550-2069`). Split it into plan validation, memory/XISF/cache setup, the GPU reduction worker, per-tile reconstruction and CPU reduction. Other long functions: `calibration::build` (189), `load_master` (134), `Calibration::new` (119), `inspect::one` (136), `inspect_paths` (123), `esd_columns::survivor_windows` (126), `master_groups::runs` (121). | Low | L |
| I-03 | Four functions (`reduce_band`, `reduce_band_with`, `integrate_common_progress`, `GpuTile::reduce`) pass the same 5 values: weights, input count, total weight, config, cancel. Make them one `ReductionContext<'a>`. `RowStats { … }` is built three times; add `RowStats::new(frames)`. The coverage and high-rejection writes are duplicated three times. `master_from` takes 10 arguments. | Low | M |
| I-04 | Redundant entry points: `IntegrationEngine` has three methods, and the default `integrate_with_progress` **silently drops the progress callback** (`integrate.rs:308-318`). `integrate_common` is a pure forwarder. | Low | S |
| I-05 | The CPU and GPU reconstruction paths have drifted apart. `gpu_batch` (`integrate.rs:926-1002`) allocates fresh buffers per source and tile, without `RoiScratch` and without the ADR-144 spans. Either share the read/calibrate step or document the GPU path as frozen. | Medium | M |
| I-06 | `inspect.rs:447-480` and `:968-1010` re-implement `cpu_schedule::map_bounded`, each with a `Mutex` counter whose `lock().unwrap()` panics if the mutex is poisoned. | Low | S |
| I-07 | The worker cap of 24 is coupled across files: `integrate.rs:1645` and `cpu_schedule.rs:14` (and 16 at `inspect.rs:447,968`). Raising one alone breaks integration. Use one shared const. | Low | S |
| I-08 | Dead or unreachable code: the `Median`/`Esd` arms of `combine_dense` (`rgb_rejection.rs:112-118`); `rejection_methods::median` (test-only); `bounds_with_margin`'s `margin` (always zero, `integrate.rs:579`); a duplicated comment (`:769-772`); `strip_count` computed twice (`calibration.rs:482,518`). `mhc_demosaic` is reachable only from tests, but the spec keeps it as a diagnostic, so that is an owner decision. `debayer_reusing`, `cosmetic_correct_into` and `vng_demosaic_into` can become `pub(crate)`. | Low | S |
| I-09 | Duplicate helpers: Neumaier sum (`calibration.rs:267` = `integrate.rs:1004 Sum`), `median_f32` three times (`calibration.rs:281`, `integrate.rs:1023`, `sky/detect.rs:19`), `median8` twice. There are three ESD λ caches (`rejection_methods.rs:254`, a per-scratch `HashMap`, `Tester.limits`). | Low | S |
| I-10 | Per-pixel allocations in the hot path: `linear_fit` allocates `alive` on each iteration (`rejection_methods.rs:139`), and `survivors` and `esd` allocate too. Move these buffers into `MethodScratch`. This could give some speed with no numeric change. | Low | S |
| I-11 | Scattered constants: `MIN_SAMPLES` exists, but a literal `5` is used at `rgb_rejection.rs:124` and `integrate.rs:1293`. The Winsorized constants are inline (`1.4826`, `1e-6`, 3 iterations), and so are the memory-model constants in `tile_plan.rs:105-131`. | Low | S |

No `f16`, no intermediate frame files and no C dependencies were found in this part of the code. The guardrails hold.

---

## 4. GUI (`fulgur-gui`, about 17,000 lines)

The defects are in section 1 (B-01, B-02, B-11 to B-15). Structural items:

* **G-01: `Workspace` is a god struct with 92 fields** (`app.rs:129-237`) and a 100-line `Default`. The fields fall into natural groups:
  * `RunState`
  * `ResultState`
  * `SourcePreview`
  * `CalibrationPreviewState`
  * `StackSettings`
  * `SettingsPersistence`
  * pending dialogs

  `StackSettings` alone would replace the hand-copying in `restore_settings` (124 lines) and `settings_snapshot` with one `From`/`Into` pair. Do it one group at a time. (Medium, L)
* **G-02: pipeline orchestration lives in the GUI crate.** `job.rs` holds:
  * calibration master building and the caches
  * request validation and destinations
  * `InspectionSession`
  * `finish`: register, integrate, export and report, through its own `BorrowedReport`

  The CLI has a second orchestration with its own `StackReport` (`fulgur-cli/src/main.rs:266`), so **GUI and CLI masters and reports can drift apart**. Moving the non-UI half of `job.rs` into `fulgur-pipeline` (a `run` module) gives one engine with one report schema. `sets::compatibility` and `classify_calibration` are also domain logic. (Medium, L. This is the most valuable structural change in the whole review.)
* **G-03: oversized functions.**
  * `calibration_view` (`app/frames.rs:1099`, 575 lines). Its preview is taken at 1133 and put back at 1627, so an early return in between loses it.
  * `job::finish` (481 lines, 11 parameters)
  * `light_header` (470)
  * `quality_table_ui` (399)
  * `field_row` (254)
  * `image_view::show` (233)
  * `poll` (218)
* **G-04:** phase names are plain strings (`"Inspect"`, `"Integrate"`; about 17 uses, compared in `app.rs:3291` and `guide.rs:180`). A typo fails silently. Use an enum. (Low, S)
* **G-05:** there are 82 inline colour literals, and `MUTED` is defined twice. Put them in one `theme.rs`. (Low, S)
* **G-06:** the native smoke-test harness and the guide tour run inside the production `draw` (`app.rs:239`, `1216-1400`). Put them in their own module behind `cfg` or a feature. (Low, S)
* **G-07: dead code.**
  * `job::master_dark` (`job.rs:264`) has no caller.
  * `calibration_master` → `_with_progress` → `_offset` is a chain of three wrappers.
  * `Worker::start` is used only by tests.
  * `pub mod job` hides these from the dead-code lint. Make it `pub(crate)`.
* **G-08:** settings migration is split between `settings.rs:236` (v1→v2) and the clamps in `restore_settings`. `accepted_warnings` grows without limit. (Low, S)
* **G-09: no tests** cover the non-UI logic:
  * `InspectionSession::rows_with_selection`
  * `set_calibration`
  * `Destinations::published`
  * cache keys and eviction
  * settings clamps
  * shift-range selection

  Add regression tests for B-01, B-02 and B-14 first.

---

## 5. Core, CLI, solver, tools, build

* **C-01: `.github/workflows/ci.yml` still triggers on every push and pull request.** Actions are off only in the GitHub settings. The file mirrors `scripts/test-full.ps1` and is drifting from it. Reduce the triggers to `workflow_dispatch` with a header comment, or delete the file. **Changing the workflow needs the owner's explicit word** (AGENTS §6). (Medium, S)
* **C-02: the experiment tools are obsolete or unchecked.**
  * `tools/synthref` duplicates star detection, the triangle solver, Gaia/Sesame access and a FITS writer, all productized since (ADR-131/138 supersede ADR-127). Its Gaia client lacks the ESA→GAVO fallback. Archive it.
  * `tools/blindspike` was ported into `fulgur-solve` (ADR-168). Only `--diag` is unique, so move it into `fulgur-solve-cli` and archive the tool.
  * `tools/skyreg` keeps a second Gaia TAP client (`catalog.rs:8-19`). It should use `fulgur_catalog::download`.
  * None of the three tools is built by the test scripts, so any API change breaks them silently. If a tool is kept, add `cargo check --manifest-path tools/X/Cargo.toml` to `test-full.ps1`.
  * (Medium, M)
* **C-03: CLI structure.** `run()` is one 800-line `match` (`main.rs:830-1640`), and the `Stack` arm alone is 435 lines.
  * Six options are strings that are re-matched by hand (`engine`, `gpu_stages`, `registration_engine`, `canvas`, `reference_strategy`). Make them `clap::ValueEnum` so the `expect("clap validates…")` and the `_ =>` fallbacks go away.
  * `DarkArgs` is boxed in one command and not in the others.
  * Split the commands into `cmd/*.rs`.
  * (Low, M)
* **C-04: error types are inconsistent across crates.** core uses `Invalid(&'static str)` with no path, gpu, catalog and solve-cli use `String`, and solve uses `io::Error`. The user sees "file not found" without a file name. Add `Io { path, source }` and `Invalid(Cow<'static, str>)`. (Low, M)
* **C-05: crate boundaries.**
  * `fulgur-catalog` depends on the whole `fulgur-pipeline` for `sky.rs` glue. Move that glue out, to a small crate or into the pipeline.
  * There are now two solvers: `pipeline::sky::solve` (near a hint) and `fulgur-solve` (blind). The second ships as a separate binary rather than `fulgurstack solve`. Decide on one front end.
  * (Low, M)
* **C-06: dependency hygiene.**
  * `clap`, `roxmltree`, `lz4_flex`, `ureq` and `windows-sys` are not in `[workspace.dependencies]`.
  * `Cargo.lock` carries duplicate versions: `sha1`/`digest` 0.10 and 0.11, `base64`, `miniz_oxide`, three `getrandom`, three `windows-sys`.
  * `ureq` → `rustls` → `ring` compiles C and assembly **at build time**. There is no runtime C library, so the rule holds, but a C toolchain is needed. Note this in `DEVELOPMENT.md`.
  * `rfd` uses the deprecated `async-std` feature.
  * (Low, S)
* **C-07: public surface and naming.**
  * `fulgur-core` exports every module `pub`.
  * `FitsHeader` also carries XISF data. Rename it `ImageHeader`.
  * `read_reference_plane` has its own decode loop next to `read_raw_with_decode` (`fits.rs:880-912`).
  * `output::card()` panics on an over-long card. Its callers are safe today, but it is a latent panic.
  * (Low, M)
* **C-08: test fixtures.** Synthetic FITS writers are re-implemented in `fulgur-cli/tests/commands.rs` and in the pipeline test fixtures. Make one shared test-support module. (Low, S)
* **C-09: repository root.**
  * `Pepa_stacked_result.fits`, `RegisteringTemplate200pct.fit`, `outputs/` and `outputs2/` sit in the root. They are git-ignored, so nothing leaks.
  * Move the benchmark template under a data folder, and update the AGENTS benchmark line with it.
  * Ask the owner before deleting `outputs2/`: it may hold stored comparison reports.

---

## 6. Documentation

### 6.1 Wrong facts in the entry points (fix first)
* **D-01:** `README.md`
  * `README.md:5` says "version 0.12"; the workspace is 0.14.0.
  * Lines 27-35 call ADR-154–159 "not published"; they went out with 0.13.
  * Line 39 quotes old test counts.
  * Line 79 names an old package commit.
  * Blind solving, hot-pixel maps, the mono/colour split, the plate-solving default, the fix buttons and the saturated-star bridge are missing.
* **D-02:** `docs/README.md:3` still says ADR-154–159 await review.
* **D-03:** `docs/CURRENT_STATUS.md`
  * The title is dated 2026-11-08, but it has 11-09 entries.
  * Line 9 ("ADR-168 local only") is superseded by line 7 (0.14 published).
  * "Planned next" calls ADR-101 local.
* **D-04:** `docs/DEVELOPMENT.md:9` lists the crates without `fulgur-solve`, `fulgur-solve-cli` and `tools/blindspike`, and its test counts are old.
* **D-05:** `docs/archive/SKY_REGISTRATION_PLAN.md:3` says "implementation not started", but phases 1-4 shipped with owner PASS.
* **D-06:** `docs/archive/WINDOWS_UI_DESIGN.md:7` says geometric correction is off by default, which contradicts ADR-103.
* **D-07:** `docs/BENCHMARKS.md` (2026-10-30) predates the 105 s DS-A result, and its test counts are old.
* **D-08:** the AGENTS benchmark-policy line quotes 247.67 s as "the latest full-set CPU run", while README and PROJECT_SUMMARY quote 105 s.

### 6.2 Contradictions and noise in the governing documents
* **D-09:** `PROJECT_SPEC.md` still says XISF is out of scope (`:87`, `:234`) and Winsorized is the default (`:161`). Both were amended (ADR-104/142, ADR-087). This is already logged as U-6 in `TESTING.md`. Add inline "superseded by ADR-xxx" notes, or an **amendments index** (rule → ADR) at the top of the spec.
* **D-10:** there are **85 `[cite: 1]` artifacts** (25 in AGENTS.md, 60 in PROJECT_SPEC.md). They are generation leftovers that mean nothing to a reader. Strip them; this is a mechanical edit.
* **D-11:** AGENTS.md is titled "… for OpenAI Codex". Use a tool-neutral title.
* **D-12:** AGENTS.md §6 has about 21 dated one-time publishing authorizations, each "for this batch only". They carry no standing instruction, and the list is already incomplete: it lacks 0.13 (ADR-164) and 0.14 (ADR-172). Keep the standing rules and replace the dated bullets with one line pointing to ADR-068/075/083/088/100/115/143/153/164/172. **This is the owner's contract, so it needs the owner's approval.**
* **D-13:** the AGENTS §3.6 input-scope paragraph is 8 italic amendments in one run-on paragraph. Restate it as the current rules, with ADR references.

### 6.3 `ARCHITECTURE_DECISIONS.md` (2,710 lines, ADR-001…172)
* Numbering is clean: no gaps and no duplicates.
* The order is mixed:
  * 172→068 runs newest first.
  * A "Decision history" heading follows, then 067.
  * 001→066 runs oldest first.
  * A stray "Publication Record" heading sits at line 2428.
* Two heading styles are used (`ADR-NNN:` / `ADR-NNN —`).
* There is no index.
* **53 headings still say "local"** although everything up to 172 is published.
* There is no uniform "Superseded by" field.

Recommendations:
* Add an index table at the top (number, title, date, status, superseded-by).
* Drop "local" from the headings.
* Use one order and one heading style.
* Tag the publication-record ADRs separately from the technical decisions.
* Consider splitting the file by range.

### 6.4 Layout of `docs/` (70 flat files)
* 42 milestone/M5 reviews and 15 evidence JSONs sit next to the current documents. Move the historical ones to `docs/archive/` (or `docs/history/m1-m4/`, `docs/history/m5/`), each paired with its JSON:
  * M1–M4 reviews and the test plan
  * the 13 CPU micro-optimization reviews, already summarized by `M5_CPU_OPTIMIZATION_SUMMARY.md` and `M5_PERFORMANCE_DSA_20261105.md`
  * `SKY_REGISTRATION_PLAN.md`
  * `WINDOWS_UI_DESIGN.md`
* Keep at the top level: `CURRENT_STATUS`, `DEVELOPMENT`, `TESTING`, `BENCHMARKS`, `WINDOWS_DISTRIBUTION`, `PROJECT_SUMMARY`, the ADR log and the reviews that describe current behaviour.
* No broken relative links today. Links must be rewritten when files move.
* Six evidence JSONs and `tools/blindspike/README.md` are not in the `docs/README.md` index.

### 6.5 Missing documents
* **D-14: `docs/ARCHITECTURE.md`** for developers. It should cover:
  * the 8 crates and 3 tools, and the dependency graph
  * the data flow: read → inspect → register/plate-solve → tile-major calibrate/debayer/resample/reject → FITS/XISF + WCS
  * where each ADR family lives in the code
* **D-15: `CHANGELOG.md`** per version (0.11-0.14). Release notes exist only on the About page and inside ADRs. A changelog would absorb the history paragraphs that make README and CURRENT_STATUS long.
* **D-16:** no `LICENSE` or `CONTRIBUTING` at the root. Data licences (Gaia, deep-sky list) are covered only in `crates/fulgur-catalog/data/README.md`.
* **D-17: tester guide.** The Czech tester guide `CTI_ME.txt` shipped in the ZIP is not version-controlled. There is no user guide for the 0.14 features: building the 1.12 GB blind-solve index, Auto-solve, Keep as is, hot-pixel maps. Version it as `docs/user/GUIDE_cs.md`.
* **D-18:** the `fulgur-solve` CLI has no usage document.
* **D-19:** open items (D-03, U-1…U-6, N-2…N-11, P-1…P-5) are tracked only inside `TESTING.md` §9-10. Promote them to `docs/OPEN_ITEMS.md`, linked from CURRENT_STATUS, or to GitHub issues. Add this review's B-items to the same register.

---

## 7. Suggested work packages

Each package is one coherent, testable delivery under the existing rules: local work, the gates that apply, and
publishing only with the owner's word.

| # | Package | Contents | Gates | Effort |
| --- | --- | --- | --- | --- |
| WP-1 ✅ done locally 2026-11-13 (ADR-173), awaiting manual review | **GUI correctness** | B-01, B-02, B-14, B-13, B-12, B-15, with regression tests (G-09) | quick suite + owner's manual UI check | M |
| WP-2 ✅ done locally 2026-11-13 (ADR-174) | **Robustness, no numeric change** | B-05, B-06, B-10, B-16, B-17, B-18, B-19, B-22, I-04, I-06, I-07, R-13, R-14 | quick + full suite; DS-B master byte-identical | M |
| WP-3 ✅ done locally 2026-11-13 (ADR-175), D-12/D-13 with the owner's approval | **Documentation refresh** | D-01…D-08, D-10, D-11, D-13, PROJECT_SPEC notes (D-09), ADR index and status cleanup, `docs/archive/`, `ARCHITECTURE.md`, `CHANGELOG.md`, open-items register | none (docs only); D-12 needs the owner's approval | M |
| WP-4 ✅ done locally 2026-11-13 (ADR-176); C-08 moved to WP-7 | **Mechanical engine cleanup** | R-01, R-03, R-04, R-08, R-10, R-15, R-17, I-01, I-08, I-09, I-11, C-08 | full suite; DS-B byte-identical | M |
| WP-5 ✅ done locally 2026-11-13 (ADR-177) except R-02 | **Registration consistency** (owner-visible) | B-03, B-04, B-07, B-08, B-09; then R-02 | scientific gates; ADR per behaviour change | M |
| WP-6 ✓ done locally 2026-11-13 (ADR-178, ADR-179); other `Workspace` groups after `StackSettings` remain | **One orchestration for GUI and CLI** | G-02, R-06, R-07, I-02, I-03, C-03, G-01 (`StackSettings` first) | full suite; DS-B byte-identical; GUI manual check | L |
| WP-7 ✓ done locally 2026-11-14 (ADR-180, ADR-181); C-01 manual start only and `fulgurstack solve` (owner) | **Tools, build and CI hygiene** | C-01 (owner's word), C-02, C-05, C-06, C-04, C-07, B-20, B-21 | quick suite | M |
| later | Engine-numerics questions | three ESD arithmetics (test first), GPU path drift (I-05), ROI repair margin assertion, I-10 hot-path allocations | scientific gates | M |

Recommended order: **WP-1 → WP-2 → WP-3 → WP-4 → WP-5 → WP-6 → WP-7.**
* WP-1 and WP-2 remove real user-facing risk cheaply.
* WP-3 makes the entry points truthful again.
* WP-4 shrinks the code before the larger WP-6 refactor.
* WP-5 is separate because it can change masters on some cameras and needs the owner's review.

### Observation from the WP-2 verification
Two manual-tile tests in `fulgur-pipeline` failed when they ran together with the rest of the suite and free RAM was about 4.3 GB:
* `scaled_inverse_warp_matches_full_reconstruction_and_is_tile_invariant`
* `hardware_gpu_integration_preserves_flux_masks_and_rejection`

They correctly refuse a tile that exceeds the RAM budget. However, their outcome then depends on what else runs on the PC, so `test-full.ps1` can fail for a reason that has nothing to do with the code.

Proposed follow-up: either mark these tests to run serially, or let them pass a fixed memory budget instead of reading the machine's free RAM. (Low, S)
