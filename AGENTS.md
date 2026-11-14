# AGENTS.md — Autonomous Engineering Contract for AI Coding Agents

## 1. Role & Communication Protocol ("PM Shield" — CRITICAL)
1. **User Persona:** The project owner is a **Product/Project Manager and Astrophotographer**, NOT a Rust software developer. The owner does not read or edit `.rs` source files directly.
2. **Zero Technical Questions:** NEVER ask the user questions about Rust syntax, crate selection, lifetimes, borrow-checker issues, memory layouts, threading primitives, or low-level mathematical formulas.
3. **Full Engineering Autonomy:** You are the Lead Software Engineer. Make all routine and architectural technical decisions autonomously in compliance with `PROJECT_SPEC.md` and record them concisely in `docs/ARCHITECTURE_DECISIONS.md`.
4. **Product-Level Communication Only:** Communicate with the user exclusively in terms of:
   - Milestone status and Acceptance Gate verification (`PASS` / `FAIL`).
   - Exact, copy-pasteable `--release` CLI commands the owner can run on Windows 11 to test the milestone or generate FITS outputs for inspection in PixInsight.
   - Measured high-level telemetry: processing wall-clock time (seconds), peak RAM usage (GB/MB), and frame rejection statistics.
   - Product-level questions only when genuine domain ambiguity arises (e.g., visual evaluation of the exported FITS master inside PixInsight).
5. **Language Preference:** Keep all code, comments, identifiers, JSON schemas, and commit messages in **English**. When writing conversational summaries or instructions to the user, respond in **Czech** (or match the language of the user's prompt).

---

## 2. Source of Truth & Scope Discipline
- **Primary Contract:** Read `PROJECT_SPEC.md` completely before writing any code.
- **Strict Milestone Execution:** Work on **one milestone at a time** (`M1` -> `M2` -> `M3` -> `M4` -> optional `M5`). Never jump ahead to the next milestone until the current Acceptance Gate is verified and approved by the owner.
- **CPU-First Rule:** Milestones M1 through M4 use a 100% pure-Rust, multi-threaded CPU engine (`rayon`). Do NOT write GPU (`wgpu`) or GUI (`egui`) code before Milestone 4 is signed off.
- **No C/C++ Runtime Dependencies:** Do not introduce crates that require external C/C++ libraries (`cfitsio`, `opencv`, `cmake` builds). The workspace must compile cleanly with standard `cargo build --release` on Windows 11 MSVC (`x86_64-pc-windows-msvc`) and macOS Apple Silicon (`aarch64-apple-darwin`).

---

## 3. Mandatory Domain & Safety Guardrails
1. **Immutable Source Data:** Never modify, move, or overwrite input FITS files. Verify input integrity in tests.
2. **Zero Intermediate Disk Files:** Never write temporary calibrated, debayered, or registered frames to disk during processing. Only the final 32-bit float master FITS (`BITPIX = -32`, written atomically via `.tmp` rename) and requested JSON/CSV reports may be written. *Owner amendment 2026-11-05 (ADR-142): the master and its diagnostic maps may instead be written as 32-bit float XISF, by the owner's choice of output format.*
3. **Seestar S50 Pro Data Rules (`2160 x 3840`, `BITPIX = 16`, `GRBG`):**
   - Decode stored 16-bit pixels as **Big-Endian signed `i16`** and apply `physical = f32::from(stored_i16) + BZERO (32768.0)`.
   - **NEVER subtract `BIAS = 2064`** or apply dark subtraction by default on Seestar precalibrated lights.
   - **Owner-approved M1 range policy (2026-10-21):** Final reconstructed RGB FITS samples must be in 0..1. Limit interpolation excursions at the reconstruction/export boundary without changing in-range values, fixed units, source pixels or GPS. The owner considers this sensor saturation compatible with the linearity/no-data-clipping requirement. Report limiting counts explicitly; ordinary PixInsight File > Open must work. See ADR-014.
   - **NEVER use `f16` (half-precision float)** for intermediate pixel storage—keep calculations in `f32` and weighted accumulations in `f64` (Neumaier compensated summation) to preserve faint sub-ADU nebulosity.
4. **Memory Ceiling (`<= 12 GB` Target on 32 GB Reference Machine):**
   - Never materialize all registered RGB frames in RAM simultaneously.
   - Use **Output-Tile-Major** spatial strips (e.g., `2160 x 128` px) in Stage 3 so peak memory stays strictly bounded regardless of whether 100 or 1,500 frames are stacked.
5. **Privacy Redaction:** Do not print `SITELAT` or `SITELONG` GPS coordinates from FITS headers into console output or default logs.
   - Preserve valid reference-anchor GPS cards in the final FITS as requested by the owner; label their reference-frame provenance in FITS history. Redact coordinates from default JSON/CSV reports as well. Never invent missing coordinates or average locations from different sessions.
6. **Input Scope (current rules; the original first-release rule allowed one target and one filter with differing exposures):**
   - **Same sky field.** A different OBJECT name only warns and the frames stay in the stack; star matching decides what stacks, and frames of another sky field fail registration (ADR-093).
   - **Filters.** Light sets may combine different filters, with a visible warning (ADR-071).
   - **Exposure.** Durations may differ. Read exposure from FITS headers and normalize photometry and noise consistently before weighting and rejection. Longer exposure is not automatically better transparency. Keep exposure metadata truthful and verify mixed-exposure behavior with synthetic gates; the current DS-A dataset contains only 10 s exposures.
   - **Missing header values.** Values missing from light headers are reconstructed from other cards, then the file name, then the camera catalogue (`crates/fulgur-core/data/cameras.json`). Exposure and colour layout are used only after the owner confirms or corrects them (ADR-084).
   - **Colour layout.** Frames without a BAYERPAT card are processed as mono, and a set may still declare colour (ADR-078). Mono frames skip demosaicing.
   - **Calibration (opt-in).**
     - Full calibration is implemented: bias, exposure-scaled darks, per-phase normalized flats and dark-defect repair (ADR-082). It started with master-dark subtraction, `light − dark + pedestal` with the master in RAM only (ADR-076).
     - Exposure or gain differences in calibration only warn (ADR-080).
     - The Seestar rule in 3.3 still applies: nothing is subtracted by default.
   - **Formats.** Light and calibration input is FITS or XISF only; other formats such as camera RAW are converted outside FulgurStack (ADR-104).

---

## 4. Autonomous Verification Loop (Before Reporting Back)
Before telling the owner that a task or milestone is ready, you MUST autonomously execute and pass this loop:

1. **Compile & Lint Check:**
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```
2. **Unit & Synthetic Gate Tests:**
   ```bash
   cargo test --workspace --release
   ```
   *(If any compiler error, warning, or test failure occurs, fix it autonomously without asking the user.)*
3. **Real Dataset Smoke / Benchmark Run (if `FULGURSTACK_TEST_FITS` or `FULGURSTACK_TEST_FITS_DIR` is set):**
   - Always compile and run benchmarks or user-facing commands in **`--release` mode** (`cargo run --release --bin fulgurstack -- ...`). Never report performance numbers from unoptimized debug builds.

---

## 5. Milestone Delivery Report Template
Whenever you finish a milestone, present your summary to the owner using this exact structure:

### Milestone [M#] Completed — Ready for Product Review
- **Co bylo vytvořeno (Summary):** Brief, non-technical summary of capabilities delivered in this milestone.
- **Výsledky automatických testů (Automated Gates):**
  - `cargo clippy` & `cargo test --release`: `PASS` (X/X tests passed).
  - Key synthetic verification results (e.g., HFR accuracy, rotation recovery RMSE, zero temp files check).
- **Naměřený výkon (Measured Telemetry on `--release`):**
  - Wall-clock time, throughput, and peak RAM usage (if run against real data).
- **Jak otestovat v PixInsight / CLI (Owner Action Steps):**
  Provide 1–2 exact copy-paste commands for Windows PowerShell/CMD (using `cargo run --release -- ...`) and explain what the owner should check visually in PixInsight or in the generated JSON report.

---

## 6. Local Verification and Batched Publishing (Owner Instructions, 2026-10-22; condensed 2026-11-13)
- **UI-only changes (owner policy 2026-10-23, until revoked).**
  - Skip automatic functional tests, scientific regression stacks and benchmarks.
  - Prepare the local release application for the owner's manual review.
  - This supersedes the automatic verification loop for UI-only iterations. Engine changes still require the applicable scientific gates.
  - Keep all work local without commits or pushes.
- **Local verification only.**
  - Run all formatting, lint, unit, synthetic, real-data and GPU checks and performance benchmarks exclusively on the owner's local computer.
  - GitHub Actions are disabled by the owner. Do not enable, trigger, poll or wait for remote CI unless the owner explicitly changes this policy. This supersedes earlier requirements to execute remote CI.
  - Keep all applicable local verification gates. Disabled remote CI is not a blocker for local development or delivery.
  - Report the platforms actually tested; do not claim verification on an untested platform.
- **Publishing only on explicit authorization.**
  - Keep development local without commits, pushes or pull-request updates unless the owner explicitly authorizes publishing a completed batch.
  - Each authorization covers its named batch only. It does not authorize future publishing, remote CI, or uploading a Windows package; packages built for a batch stay local unless their publication is authorized too.
  - Record each authorization and its scope in an ADR.
- **How an authorized batch is published (the conditions of every past authorization).**
  - One documented, consolidated commit to `main` with `[skip ci]`. Preserve existing history and never force-push.
  - Keep the Actions configuration unchanged.
  - Exclude local images, outputs, preferences, raw telemetry, Gaia data and binaries.
  - State the verification limits explicitly; for example, UI iterations are published as release-compiled and awaiting manual acceptance.
- **Past one-time authorizations.** They are recorded in the decision log:
  - the publication record of 2026-10-22 (CPU optimization batch);
  - the documentation and native Workspace publication of 2026-10-23 (git history);
  - ADR-061, ADR-063, ADR-068, ADR-075, ADR-083 and ADR-088; the two commits of 2026-10-30 for ADR-089 (documentation refresh) and ADR-090; then ADR-100 (first alpha package), ADR-115 (version 0.12), ADR-143, ADR-153, ADR-164 (version 0.13), ADR-172 (version 0.14) and ADR-182 (code-review work, no version).

  None of them is still in force.
- **What to publish.** Commit and push only substantial, completed and locally tested units of work. Group related implementation, tests, documentation and performance evidence into one delivery rather than publishing intermediate changes or separate follow-up documentation commits. Remote CI is not a prerequisite under the current testing policy.
- **No workarounds.** Do not change billing settings, spending limits or workflow triggers as a workaround.
- **Guardrails stay in force.** Preserve input immutability, privacy, dataset exclusions and all existing scientific acceptance requirements throughout local development.
- **Owner benchmark policy.**
  - Use the last stored comparable performance report and FITS products for routine optimization controls; do not rerun the reference stack by default.
  - The stored references are:
    - the DS-A log of 2026-11-05: 941/1,140 frames in 105.0 s median, see docs/reviews/M5_PERFORMANCE_DSA_20261105.md;
    - for the external 200 % reference workload, 247.6727114 s with RegisteringTemplate200pct.fit, no crop and FITS cache 0, see docs/reviews/M5_CPU_OPTIMIZATION_SUMMARY.md.
  - Preserve exact quality and source gates and historical-comparison limitations. Raw local reports remain excluded from Git.
