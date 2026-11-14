# Automated testing — suites, inventory and review (2026-11-08)

This document is the single entry point for automated testing. It replaces the scattered
test counts in older reviews (those remain historical). Related: [Development](DEVELOPMENT.md),
[Current state](CURRENT_STATUS.md), [Engineering contract](../AGENTS.md), [Benchmarks](BENCHMARKS.md).

All verification runs on the owner's computer, in release mode. GitHub Actions stay disabled;
`.github/workflows/ci.yml` is retained, starts only manually (ADR-180), and is not run. Windows 11 is the only platform exercised.

## 1. The two suites

| | **Quick regression** | **Full** |
| --- | --- | --- |
| Command | `scripts\test-quick.ps1` | `scripts\test-full.ps1` |
| Purpose | After every change; before a commit | Before an engine delivery, a release or a publication batch |
| Contents | `cargo fmt --check`, `cargo clippy -D warnings`, all tests except the 19 slow ones in `scripts/tests/slow-tests.txt` | Everything in quick **plus** the slow tests, all-feature (GPU) build and lint, real DS-B frames, AVX2 and GPU hardware gates, PixInsight adapter tests (`node --test`), CLI smoke on DS-B with PixInsight comparison |
| Data needed | none | `SeestarS50 Subs\` (32 frames), optional `PixInsight Files\integration.fit`, AVX2 CPU, GPU adapter |
| Tests run | 297 (of 316) | 320 passed, 3 ignored (they need private data, section 4) |
| Measured time | **7 s** of test time with a warm release build (cold: about 4 min, build only) | about 30 s of test time (measured 29 s with GPU and real data) plus 30 s CLI smoke; measured end to end with warm builds: 330 s, PASS; a cold all-feature build adds 2–5 min |
| Verdict | exit 0 = PASS | 0 = PASS, 1 = FAIL, 2 = INCOMPLETE (a gate could not run; never reported as PASS) |

UI-only changes still follow the owner's policy in `AGENTS.md` section 6: no automatic functional
tests, release build for manual review. Engine changes need the full suite.

Selection rule (how a test gets into the slow list): it needs more than about 6 s while the whole
suite runs in parallel **and** its behaviour is also guarded by a faster test or by a full-suite gate.
Move a test in or out by editing `scripts/tests/slow-tests.txt`; no source change is needed.

Manual commands without the scripts:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release                      # all 316 tests, ~25 s warm
cargo test --workspace --release -- --skip <name>     # one libtest filter per slow test
cargo test --workspace --release --all-features       # + GPU code paths (hardware gates need the env vars below)
.\scripts\test-cli-smoke.ps1                          # CLI on DS-B, ~30 s, -Strict also fails on known defects
```

Per-test timing (needs a stable toolchain with the bootstrap switch, no nightly):
`$env:RUSTC_BOOTSTRAP=1; cargo test --workspace --release -- -Z unstable-options --report-time`.

## 2. Test layers and inventory (316 tests, default features)

| Layer | Where | Tests | What it protects |
| --- | --- | --- | --- |
| FITS/XISF I/O, output | `fulgur-core` (`tests/fits_io.rs` 26, unit 27) | 53 | Big-endian `i16` + `BZERO`, no `BIAS` subtraction, header derivation, atomic/no-clobber writes, XISF codecs and checksums, FITS/XISF master parity, GPS handling |
| Pipeline engine | `fulgur-pipeline` (unit 174, `tests/reconstruction.rs` 18) | 192 | Star measurement (HFR, eccentricity), registration (rigid, Auto reference, cameras, sky/Gaia, distortion), calibration, cosmetics, VNG, resampling, rejection (winsorized, linear fit, ESD, median), tiling and RAM plan, integration |
| CLI end to end | `fulgur-cli/tests/commands.rs` | 29 | Every command on synthetic frames: no intermediates, no GPS, no overwrite, validation before publishing, stacks, calibration, XISF, tile options |
| Catalog / Gaia | `fulgur-catalog` | 10 | Object table, name look-up, XPSD reading, download fall-backs |
| GPU | `fulgur-gpu` | 4 | WGSL shaders validate; reducer plans (hardware gates live in `fulgur-pipeline`) |
| GUI | `fulgur-gui` | 28 (+3 ignored) | Settings migration, set handling, job orchestration, views render at Windows scales |
| Scripts | `scripts/tests` | Node 4, Python tools | PixInsight adapter contract; offline comparison tools (section 7) |
| CLI smoke | `scripts/test-cli-smoke.ps1` | 35 checks + 2 known-defect probes | Release binary on DS-B: exit codes, RAM, privacy, immutability, PixInsight quality comparison |

Counts change; regenerate with `cargo test --workspace --release -- --list` (or the timing command above).

## 3. Measured baseline of this review (Windows 11, 24 threads, 32 GB)

* Default features: 316 passed (317 after the D-01 test), 0 failed. All features with real DS-B frames, AVX2 and GPU: 320 passed,
  0 failed, 3 ignored. Format and Clippy (`-D warnings`, default and all features): PASS. Identical before
  and after the code simplification in section 8.
* Sum of per-test times 292 s, but they run in parallel: wall 22 s for `fulgur-pipeline`, 3 s for the CLI tests.
  The slow list is 19 tests = about 221 s of that sum (76 %). Timings depend on contention: one GPU test took 124 s while another build ran and 20 s on an idle machine, so measure on a quiet PC.
* CLI on DS-B (32 frames, 27 accepted): survey 0.2 s, debayer-single 0.4 s, inspect 0.4 s, register 0.6 s,
  stack 4.5 s (ESD), median with coverage canvas 2.9 s; peak RAM 1.17 GB; the PixInsight comparison
  reports PASS (median flux residual 0.4 %, FWHM ratio 0.996, sky-noise ratio 0.994); inputs byte-identical.

## 4. Environment variables (real data and hardware)

A test that needs data returns early and **passes** when its variable is unset. This is why the full
suite sets them and refuses to start without the data.

| Variable | Used by | Meaning |
| --- | --- | --- |
| `FULGURSTACK_TEST_FITS_DIR` / `FULGURSTACK_TEST_FITS` | core, pipeline | DS-B folder / one real frame |
| `FULGURSTACK_TEST_SIMD=1` | pipeline | Require the AVX2 kernels instead of skipping |
| `FULGURSTACK_TEST_GPU=1` | pipeline, `gpu_parity.rs` | Require a real GPU adapter |
| `FULGURSTACK_TEST_XISF_DIR` | core | Folder with real XISF units |
| `FULGURSTACK_TEST_FLAT`, `_FLAT_PNG`, `_LIGHT`, `_ASTROMETRY_DIR/_GAIA/_OUT/_OBJECT/_SELECTION` | GUI, `#[ignore]` | Private data (flats, a Gaia database); run by hand with `-- --ignored` |

## 5. Slow tests (skipped by the quick suite)

Measured 2026-11-08 under parallel load (seconds): frozen-planner equivalence 20.4; four sky-registration
tests 12–17; vector VNG versus scalar and frozen 15.6; cosmetics versus scalar 12.8; Auto reference with unmapped
nights 12.7 and progress 8.8; sparse wide anchor 12.1; scaled inverse warp 9.1; fallback solver bits 8.7;
sample accounting 8.7; ROI scratch reuse 8.4; distortion recovery 7.9 and 7.7; rendered-star centroids 6.9;
generated VNG 6.7. The complete list with reasons is in `scripts/tests/slow-tests.txt`.

## 6. Review: which tests stay, change or go

### Still valid and wanted (keep in both suites)

* All of `fits_io.rs`, `xisf_tests.rs`, `fits_cache.rs`: they guard the data contract (immutability, atomic writes, no `BIAS`, big-endian).
* All CLI tests: each maps to a contract rule (section 7). They cost only 3 s together.
* Rejection, calibration, inspection, tile plan, header derivation, camera catalogue, settings and set handling.
* Hard-error paths (invalid tile, existing output, destinations checked before publishing).

### Still valid but expensive: keep in the full suite only

The "frozen oracle" tests compare the current kernel bit for bit with a retained copy of the earlier
implementation (`vng_reference.rs`, `cosmetic_reference.rs`, `geometry_fallback_oracle.rs`,
`register/tests/serial_oracle.rs`). They are the evidence behind every "byte-identical master" claim in
the speed rounds (ADR-111 to ADR-152) and must run before any further kernel optimization. They add
593 lines of oracle code. Retire an oracle together with its tests once the optimized kernel has been
stable for a release and the DS-B/DS-A master hashes are the accepted gate (proposal P-4).

### Obsolete, duplicate or weak: action taken or proposed

| ID | Item | Finding | Action |
| --- | --- | --- | --- |
| T-1 | `calibration::mismatch_warnings` and its four assertions | Pure duplicate of `Calibration::light_warnings` (the real path); the test checked dead code | **Removed.** The gain-differs case was only covered through the duplicate, so it was added to the `light_warnings` test |
| T-2 | `inspect_inputs_with_progress` test | Exercised a wrapper nobody calls | **Test rewritten** onto `inspect_sets` (same assertions) |
| T-3 | CLI progress test via `register_inspected_with_progress` | Wrapper nobody else calls | **Test rewritten** onto `register_inspected_with_options` (same assertions) |
| T-4 | `vng::generated_vng_matches_prior_scalar_bits_for_phases_fields_and_edges` (6.7 s) and `vector_vng_matches_scalar_and_frozen_bits_for_phases_widths_and_encodings` (15.6 s) | Appear to overlap: both compare the generated kernel with the scalar reference over phases and edges (verify before merging) | Proposal P-1: merge into one parametrized test, saves ~7 s of CPU time |
| T-5 | `corrected_geometry…`, `integrate_distortion_tests`, `distortion/tests`, `register/tests/distortion` (19 tests) | Optional geometric correction is off by default; three test files cover overlapping ground | Proposal P-2: keep, but move the two 8 s registration tests behind the slow list (done) and review for merging when the feature changes |
| T-6 | Destination-validation tests: `invalid_report_path…`, `existing_report…`, `inspect_checks_all_report_destinations…`, `stack_manual_reference_and_map_destinations…`, `using_input_as_output…` | One rule ("validate before publishing") is tested in five places; each is cheap (< 2 s) and covers a different command | Keep; do not add a sixth. Proposal P-3: table-drive them into one CLI test with a case list |
| T-7 | `an_xisf_master_and_its_maps_hold_exactly_the_fits_samples` (CLI) versus `xisf_master_holds_the_fits_samples…` and `xisf_maps_follow_the_fits_maps…` (core) | Same invariant at two levels | Keep the core pair, keep the CLI test as the only end-to-end proof; no change |
| T-8 | Vacuous real-data tests (about 20 across 20 files) | Return early and pass when the data variable is unset (false green, contradicts "never silently skip") | Full suite sets the variables and aborts without data; proposal N-2 makes them fail on request |

### Scripts

| Script | Status | Recommendation |
| --- | --- | --- |
| `scripts/tests/compare-master.py` | Valid; used by the CLI smoke | Keep (needs NumPy) |
| `scripts/tests/verify-stack-equivalence.py` | Valid; exact product comparison for optimization batches | Keep |
| `scripts/tests/open-fulgurstack.test.cjs` | Valid contract test of the PixInsight adapter; needs Node | Keep in the full suite |
| `verify-gpu-stack.py`, `verify-integration-profile.py` | Valid but tied to long runs on private data | Keep as manual tools |
| `verify-full-stack.py`, `verify-reference-maps.py`, `verify-scaled-reference.py`, `verify-gui-stack.py` | Obsolete: hard-coded M4-era output names, superseded by the ADR-gated hash comparison and `verify-stack-equivalence.py` | Move to `scripts/tests/archive/` (historical reviews link them, so the move needs a link edit; not done here) |
| `pixinsight-display-regression.js`, `pixinsight-master-import.js`, `pixinsight-reference-maps.js` | Obsolete: native PixInsight diagnostics of M1/M4 (ADR-013 workaround failed and is superseded; the master opens with File > Open) | Archive with the Python scripts above |
| `analyze-geometric-mixture.py` | Obsolete: diagnosis of a regression that ADR-066 repaired | Archive |
| `.github/workflows/ci.yml` | Manual start only; Actions disabled by the owner | Do not run it (owner policy, ADR-180) |

## 7. Requirement traceability (contract rule to automated check)

| Rule (source) | Automated check | Suite |
| --- | --- | --- |
| Big-endian `i16` + `BZERO`; never subtract `BIAS` (AGENTS 3.3) | `signed_big_endian_bzero_and_bias_are_correct`, CLI `stack_cli_integrates_mixed_exposures_without_bias_or_intermediates` | quick |
| Inputs immutable (AGENTS 3.1) | `using_input_as_output_fails_without_modifying_source`, `existing_source_and_hardlink_alias_are_never_overwritten`, smoke hash check | quick, full |
| No intermediate files, atomic write (3.2) | CLI `…without_bias_or_intermediates`, `partial_write_failure_removes_only_its_temporary_file`, smoke (`only masters and their maps`) | quick, full |
| Output `BITPIX -32`, 0..1 range, GPS from reference anchor, NCOMBINE/TEXPTIME | `rgb_export_is_planar_big_endian_linear…`, `export_bounds_reconstruction…`, `integrated_master_records_crop_exposure_and_anchor_gps…`, smoke | quick, full |
| GPS never in console/default reports (3.5) | `header_debug_redacts_observing_coordinates`, CLI `inspect_measures…without…gps`, `registration_cli_preserves_sources_redacts_gps…`, smoke | quick, full |
| HFR accuracy within 3 % on Gaussian stars (G2) | `master_quality::known_gaussian_stars…`, `stars::tests` | quick |
| Inspect 1,140 frames < 15 s (G2) | smoke checks 32 frames < 15 s only; DS-A not re-run (benchmark policy) | full (partial) |
| Rotation recovery < 0.15 px, pseudo second night (G3) | `register::tests::real_cfa_pseudo_second_night_rotation_and_shift`, rotation sweeps in `register/tests` | full / quick |
| Satellite trail rejected, star flux preserved ≤ 1 % (T-REJECT-01, T-PHOTO-01) | `integrate::tests` (rejection and flux), `reconstruction.rs`, `rejection_*_tests` | quick |
| Mixed exposures: flux ≤ 1 %, finite weights, saturated stars excluded | `stack_cli_integrates_mixed_exposures…`, `inspect`/`stars` tests | quick |
| Memory ≤ 12 GB, tiles independent of frame count | `tile_plan::tests`, smoke (peak 1.17 GB on 32 frames); no test varies the frame count (N-5) | quick, full |
| Compensated f64 accumulation (Neumaier) | exercised through integration tests; no direct unit test of `Sum` (N-3) | quick (indirect) |
| Quality not worse than PixInsight (M4) | smoke + `compare-master.py` on DS-B; full DS-A is a manual owner review | full |
| GPU equals CPU | `tests/gpu_parity.rs`, `hardware_gpu_*` tests (need `FULGURSTACK_TEST_GPU=1`) | full |

## 8. Dead code removed in this review

Found with a cross-workspace reference scan (public items with no caller outside tests), checked by hand, removed,
then verified with the full suite (section 3). No behaviour changed; no scientific output changed.

| Item | Where | Reason |
| --- | --- | --- |
| `inspect_inputs`, `inspect_inputs_with_progress` | `fulgur-pipeline/src/inspect.rs` | Wrappers over `inspect_sets` without callers (GUI and CLI use `inspect_sets`) |
| `register_inputs`, `register_inputs_with_reference`, `register_inputs_with_options` | `fulgur-pipeline/src/register.rs` | Three-step wrapper chain over `register_sets_with_options`, no callers |
| `register_inspected`, `register_inspected_with_progress` | `register.rs` | Wrappers over `register_inspected_with_options` (kept then; replaced by the new `register_inspected` of ADR-179) |
| `RegistrationReport::accepted_monochrome_frames` | `register.rs` | No callers |
| `calibration::mismatch_warnings` | `fulgur-pipeline/src/calibration.rs` | Duplicate of `Calibration::light_warnings` (ADR-080 warnings), only a test called it |
| dev-dependency `sha2` | `fulgur-gui/Cargo.toml`, `Cargo.lock` | Not used by any GUI test |

Net change: about 150 lines of source removed. Candidates **not** removed (judgement needed): the retained MHC
demosaic (spec requires it as a diagnostic), `fits_cache` (off by default but a documented option), the example
programs, the experiment tools (archived since ADR-180), the frozen oracles (section 6).

## 9. Unmet or open requirements and defects found

Open items from this section and section 10 are also tracked, with owner decisions and reviews, in [OPEN_ITEMS.md](OPEN_ITEMS.md).

| ID | Finding | Evidence | Status |
| --- | --- | --- | --- |
| D-01 | The master's FITS `REJECT` card always reads `WINSOR-MAD`, whatever method ran (Auto picks ESD for 27 frames; median and ESD runs show the same card). The report JSON is correct. Violates the "metadata stays truthful" rule | `output.rs` writes the literal card; smoke check D-01 | **Fixed 2026-11-08** (`MasterMetadata.rejection`; test `master_header_names_the_rejection_actually_applied…`, smoke check) |
| D-02 | `CREATOR = 'FulgurStack 0.1.0'` in FITS while the application is 0.12.0 | `output.rs`; smoke check | **Fixed 2026-11-08** (`CARGO_PKG_VERSION`) |
| D-03 | Real-data/hardware tests pass vacuously when their variable is unset | Section 4 | Mitigated by `test-full.ps1`; N-2 closes it |
| U-1 | macOS Apple Silicon: the spec requires compile and test there; never verified, CI disabled | `CURRENT_STATUS.md` | Open by owner policy; needs a Mac |
| U-2 | Real multi-night data and real mixed-exposure data not available; only synthetic and artificial-date gates | spec, ADR-067 | Open, waiting for data |
| U-3 | Full DS-A (1,140 frames) gates (inspect < 15 s, stack ≤ 18 min, quality) not re-run on the current build; per the benchmark policy the stored evidence is used. DS-B re-run today passes | `BENCHMARKS.md` | Accepted limitation |
| U-4 | Owner decision pending: ESD below 25 frames in Auto (ADR-102) | `CURRENT_STATUS.md` | Open, owner |
| U-5 | CLI options without any CLI-level test: `--registration-engine sky`, `gaia-check`, `--accuracy`, `--partial-frames`, `--geometric-correction` (engine-level tests exist) | `commands.rs` | See N-6 |
| U-6 | Documentation drift: `PROJECT_SPEC.md` still describes Winsorized rejection as the default and XISF as out of scope (amended elsewhere); test counts in `DEVELOPMENT.md` (213/217), `BENCHMARKS.md` (210/213) and `CURRENT_STATUS.md` (315) are old | Documents | **Closed 2026-11-13** (spec amendments index; counts now point here and to ADR-174) |

## 10. Proposed new automated test cases

| ID | Test | Why | Suite |
| --- | --- | --- | --- |
| N-1 (done: core test for all four methods + smoke) | Truthful metadata: for each of `auto/winsorized/linear-fit/esd/median` check FITS `REJECT`, `CREATOR` and report JSON agree (synthetic CLI stack) | D-01, D-02; write together with the fix | quick |
| N-2 | `require_real_data()` test helper: with `FULGURSTACK_REQUIRE_REAL_DATA=1` a missing variable fails instead of passing; set by `test-full.ps1` | D-03 | full |
| N-3 | Unit test of the compensated sum: `1e16 + 1 − 1e16`, 10⁶ small terms next to a large one, order independence | Spec: Neumaier accumulation | quick |
| N-4 | Bicubic Keys (a = −0.5): weights sum to 1; footprint partly outside the sensor gives `valid = false`, never zero-blended | Spec 4.5.3 (confirm what `resample_tests` already covers first) | quick |
| N-5 | Memory independence: stack 24 and 96 tiny synthetic frames, assert the planned tile bytes are identical and peak RSS grows by less than a set margin | AGENTS 3.4 | quick |
| N-6 | CLI tests for `gaia-check` (tiny fake XPSD fixture), `--registration-engine sky` with a synthetic catalogue, `--accuracy` presets, `--partial-frames`, `--geometric-correction` rejection of bad values | U-5 | quick |
| N-7 | Golden master: store the sample hash of the DS-B master (ESD) and compare in the smoke script; update only through an ADR | Catches silent numeric drift cheaply; replaces the manual byte-identical procedure | full |
| N-8 | Interrupt test: start a stack, terminate it, assert no `.tmp`, no partial master | Spec: cleanup on cancellation (library tests exist, CLI level missing) | full |
| N-9 | Report schema snapshot: required keys of `*_report.json` (`schema_version 2`) | Protects GUI/script consumers | quick |
| N-10 | Run `test-quick.ps1` on macOS aarch64 when a Mac is available | U-1 | n/a |
| N-11 | Real mixed-exposure and multi-night sets as a data-gated test once the owner has them | U-2 | full |

Retirement proposals: P-1 merge the two VNG-versus-scalar tests (T-4); P-2 review distortion tests when the
feature changes (T-5); P-3 table-drive the destination-validation CLI tests (T-6); P-4 retire a frozen oracle
and its tests after the golden hash (N-7) has been accepted for a release; P-5 archive the obsolete scripts.
None of P-1 to P-5 has been executed; each is a behaviour-neutral clean-up for the owner's go-ahead.

## 11. Owner (manual) tests

Not automatable and unchanged: visual review of masters in PixInsight (File > Open, SPCC with the WCS), the
Windows UI at 1366 × 768 / 125 %, packaging (`WINDOWS_DISTRIBUTION.md`), comparison with other stacking programs.

## 12. Conventions for new tests

* Synthetic data first, in memory or in `tempfile`; a test never writes next to real data.
* Name tests as a sentence stating the rule. Keep a test under 2 s; if it cannot be, add it to `slow-tests.txt` in the same change.
* Real-data tests read their path from a `FULGURSTACK_TEST_*` variable and are listed in section 4.
* Bit-exactness claims need an oracle or a golden hash, never a tolerance that was tuned until it passed.
* Update the counts in section 2 and 3 when you add or remove tests in bulk.
