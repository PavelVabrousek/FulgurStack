# AGENTS.md — Autonomous Engineering Contract for OpenAI Codex

## 1. Role & Communication Protocol ("PM Shield" — CRITICAL)
1. **User Persona:** The project owner is a **Product/Project Manager and Astrophotographer**, NOT a Rust software developer. The owner does not read or edit `.rs` source files directly[cite: 1].
2. **Zero Technical Questions:** NEVER ask the user questions about Rust syntax, crate selection, lifetimes, borrow-checker issues, memory layouts, threading primitives, or low-level mathematical formulas.
3. **Full Engineering Autonomy:** You are the Lead Software Engineer. Make all routine and architectural technical decisions autonomously in compliance with `PROJECT_SPEC.md` and record them concisely in `docs/ARCHITECTURE_DECISIONS.md`[cite: 1].
4. **Product-Level Communication Only:** Communicate with the user exclusively in terms of:
   - Milestone status and Acceptance Gate verification (`PASS` / `FAIL`)[cite: 1].
   - Exact, copy-pasteable `--release` CLI commands the owner can run on Windows 11 to test the milestone or generate FITS outputs for inspection in PixInsight[cite: 1].
   - Measured high-level telemetry: processing wall-clock time (seconds), peak RAM usage (GB/MB), and frame rejection statistics[cite: 1].
   - Product-level questions only when genuine domain ambiguity arises (e.g., visual evaluation of the exported FITS master inside PixInsight)[cite: 1].
5. **Language Preference:** Keep all code, comments, identifiers, JSON schemas, and commit messages in **English**[cite: 1]. When writing conversational summaries or instructions to the user, respond in **Czech** (or match the language of the user's prompt)[cite: 1].

---

## 2. Source of Truth & Scope Discipline
- **Primary Contract:** Read `PROJECT_SPEC.md` completely before writing any code[cite: 1].
- **Strict Milestone Execution:** Work on **one milestone at a time** (`M1` -> `M2` -> `M3` -> `M4` -> optional `M5`)[cite: 1]. Never jump ahead to the next milestone until the current Acceptance Gate is verified and approved by the owner[cite: 1].
- **CPU-First Rule:** Milestones M1 through M4 use a 100% pure-Rust, multi-threaded CPU engine (`rayon`)[cite: 1]. Do NOT write GPU (`wgpu`) or GUI (`egui`) code before Milestone 4 is signed off[cite: 1].
- **No C/C++ Runtime Dependencies:** Do not introduce crates that require external C/C++ libraries (`cfitsio`, `opencv`, `cmake` builds)[cite: 1]. The workspace must compile cleanly with standard `cargo build --release` on Windows 11 MSVC (`x86_64-pc-windows-msvc`) and macOS Apple Silicon (`aarch64-apple-darwin`)[cite: 1].

---

## 3. Mandatory Domain & Safety Guardrails
1. **Immutable Source Data:** Never modify, move, or overwrite input FITS files[cite: 1]. Verify input integrity in tests[cite: 1].
2. **Zero Intermediate Disk Files:** Never write temporary calibrated, debayered, or registered frames to disk during processing[cite: 1]. Only the final 32-bit float master FITS (`BITPIX = -32`, written atomically via `.tmp` rename) and requested JSON/CSV reports may be written[cite: 1].
3. **Seestar S50 Pro Data Rules (`2160 x 3840`, `BITPIX = 16`, `GRBG`):**
   - Decode stored 16-bit pixels as **Big-Endian signed `i16`** and apply `physical = f32::from(stored_i16) + BZERO (32768.0)`[cite: 1].
   - **NEVER subtract `BIAS = 2064`** or apply dark subtraction by default on Seestar precalibrated lights[cite: 1].
   - **Owner-approved M1 range policy (2026-10-21):** Final reconstructed RGB FITS samples must be in 0..1. Limit interpolation excursions at the reconstruction/export boundary without changing in-range values, fixed units, source pixels or GPS. The owner considers this sensor saturation compatible with the linearity/no-data-clipping requirement. Report limiting counts explicitly; ordinary PixInsight File > Open must work. See ADR-014.
   - **NEVER use `f16` (half-precision float)** for intermediate pixel storage—keep calculations in `f32` and weighted accumulations in `f64` (Neumaier compensated summation) to preserve faint sub-ADU nebulosity[cite: 1].
4. **Memory Ceiling (`<= 12 GB` Target on 32 GB Reference Machine):**
   - Never materialize all registered RGB frames in RAM simultaneously[cite: 1].
   - Use **Output-Tile-Major** spatial strips (e.g., `2160 x 128` px) in Stage 3 so peak memory stays strictly bounded regardless of whether 100 or 1,500 frames are stacked[cite: 1].
5. **Privacy Redaction:** Do not print `SITELAT` or `SITELONG` GPS coordinates from FITS headers into console output or default logs[cite: 1].
   - Preserve valid reference-anchor GPS cards in the final FITS as requested by the owner; label their reference-frame provenance in FITS history. Redact coordinates from default JSON/CSV reports as well. Never invent missing coordinates or average locations from different sessions.
6. **First-Release Input Scope:** A stack contains the same target and filter, but exposure durations may differ. *Owner amendment 2026-10-28 (ADR-071): light sets may combine different filters with a visible warning; frames without a BAYERPAT card are processed as mono (owner decision 2026-10-28, ADR-078; a set may still declare colour) and mono frames skip demosaicing; calibration arithmetic stays unimplemented until real calibration data exist. Owner amendment 2026-10-28 (ADR-076): opt-in master-dark subtraction is implemented with real Dwarf3 darks (master in RAM only, `light − dark + pedestal`); bias and flats remain unapplied. Owner amendment 2026-10-29 (ADR-080): exposure or gain differences in calibration only warn. Owner amendment 2026-10-29 (ADR-082): full calibration (bias, exposure-scaled darks, per-phase normalized flats, dark-defect repair) is implemented.* Read exposure from FITS headers and normalize photometry and noise consistently before weighting and rejection. Longer exposure is not automatically better transparency. Keep exposure metadata truthful and verify mixed-exposure behavior with synthetic gates; the current DS-A dataset contains only 10 s exposures.

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
   *(If any compiler error, warning, or test failure occurs, fix it autonomously without asking the user.)*[cite: 1]
3. **Real Dataset Smoke / Benchmark Run (if `FULGURSTACK_TEST_FITS` or `FULGURSTACK_TEST_FITS_DIR` is set):**
   - Always compile and run benchmarks or user-facing commands in **`--release` mode** (`cargo run --release --bin fulgurstack -- ...`). Never report performance numbers from unoptimized debug builds.

---

## 5. Milestone Delivery Report Template
Whenever you finish a milestone, present your summary to the owner using this exact structure:

### Milestone [M#] Completed — Ready for Product Review
- **Co bylo vytvořeno (Summary):** Brief, non-technical summary of capabilities delivered in this milestone[cite: 1].
- **Výsledky automatických testů (Automated Gates):**
  - `cargo clippy` & `cargo test --release`: `PASS` (X/X tests passed)[cite: 1].
  - Key synthetic verification results (e.g., HFR accuracy, rotation recovery RMSE, zero temp files check)[cite: 1].
- **Naměřený výkon (Measured Telemetry on `--release`):**
  - Wall-clock time, throughput, and peak RAM usage (if run against real data)[cite: 1].
- **Jak otestovat v PixInsight / CLI (Owner Action Steps):**
  Provide 1–2 exact copy-paste commands for Windows PowerShell/CMD (using `cargo run --release -- ...`) and explain what the owner should check visually in PixInsight or in the generated JSON report[cite: 1].

---

## 6. Local Verification and Batched Publishing (Owner Instructions, 2026-10-22)
- Owner UI/UX policy (2026-10-23, until revoked): for UI-only changes, skip automatic functional tests, scientific regression stacks and benchmarks. Prepare the local release application for the owner's manual review. This supersedes the automatic verification loop for UI-only iterations; engine changes still require the applicable scientific gates. Keep all work local without commits or pushes.
- Run all formatting, lint, unit, synthetic, real-data, GPU checks and performance benchmarks exclusively on the owner's local computer. GitHub Actions are disabled by the owner. Do not enable, trigger, poll or wait for remote CI unless the owner explicitly changes this policy. This supersedes earlier requirements to execute remote CI.
- Keep all applicable local verification gates. Disabled remote CI is not a blocker for local development or delivery. Report the platforms actually tested; do not claim verification on an untested platform.
- Keep development local without commits, pushes or pull-request updates unless the owner explicitly authorizes publishing a completed batch. On 2026-10-22 the owner authorized one consolidated commit and push of the completed CPU optimization batch. This does not authorize future automatic publishing or remote CI.
- On 2026-10-23 the owner additionally authorized one consolidated documentation, native Windows Workspace implementation and UI/UX progress commit and push. Publish the current snapshot with its verification limits stated explicitly: initial native parity/release gates are historical; the latest UI iterations are release-compiled and awaiting manual acceptance. This authorization does not revoke the manual-only UI verification policy, enable Actions or authorize future automatic publishing.
- On 2026-10-26 the owner explicitly authorized one consolidated commit and push of all latest completed local registration/reference changes to `main`, together with a complete current-state documentation refresh. Preserve existing history, use `[skip ci]`, keep Actions configuration unchanged and exclude local images, outputs, preferences, raw telemetry and binaries. This authorization applies to this batch only; future publishing and remote CI remain unauthorized. See ADR-068 and docs/CURRENT_STATUS.md.
- On 2026-10-28 the owner explicitly authorized one documented consolidated commit and push of all completed local work since ADR-068 (ADR-069–ADR-074) to `main`. Use `[skip ci]`, keep Actions configuration unchanged and exclude local images, outputs, preferences, raw telemetry and binaries. UI iterations are published as release-compiled and awaiting manual acceptance. This authorization applies to this batch only; future publishing and remote CI remain unauthorized. See ADR-075.
- On 2026-10-29 the owner explicitly authorized one documented consolidated commit and push of the completed calibration batch (ADR-076–ADR-082) to `main` once tests pass. Use `[skip ci]`, keep Actions configuration unchanged and exclude local images, outputs, preferences, raw telemetry and binaries. This authorization applies to this batch only. See ADR-083.
- When publishing resumes, commit and push only substantial, completed and locally tested units of work. Group related implementation, tests, documentation and performance evidence into one delivery rather than publishing intermediate changes or separate follow-up documentation commits. Remote CI is not a prerequisite under the current testing policy.
- Do not change billing settings, spending limits or workflow triggers as a workaround.
- Preserve input immutability, privacy, dataset exclusions and all existing scientific acceptance requirements throughout local development.

- Owner benchmark policy: use the last stored comparable performance report and FITS products for routine optimization controls; do not rerun the reference stack by default. At the end of this batch the latest full-set CPU run is 247.6727114 s with RegisteringTemplate200pct.fit, no crop and FITS cache 0. Preserve exact quality/source gates and historical-comparison limitations. See docs/M5_CPU_OPTIMIZATION_SUMMARY.md for the published evidence; raw local reports remain excluded from Git.
