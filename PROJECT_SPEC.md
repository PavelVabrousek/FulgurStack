# FulgurStack — Project Specification & Implementation Contract (v2.5)

**Version:** 2.5 (Owner Scope Clarifications; supersedes v2.4)\
**Date:** 2026-10-21\
**Language:** English (code, identifiers, normative specs); UI localization ready (CS/EN).\
**Primary Platform:** Windows 11 x86_64 (Reference: AMD Ryzen AI 9 HX, 32 GB RAM, 2 TB NVMe SSD, Radeon 890M)[cite: 1]. Must also compile and pass tests on macOS Apple Silicon[cite: 1].\
**Reference Sample:** ZWO Seestar S50 Pro FITS (`Light_M 33_10.0s_IRCUT_20261006-204534.fit`, 2160 × 3840, 16-bit CFA GRBG)[cite: 1].

> **Instructions for AI Coding Agents:** Treat `MUST` and acceptance tests as binding. Do not invent unrequested abstractions, academic simulators, or disk intermediates[cite: 1]. Work strictly one milestone at a time[cite: 1]. Write clean, multi-threaded Rust (`rayon`) first; keep GPU (`wgpu`) and GUI (`egui`) strictly out of scope until the M4 CPU release candidate passes all quality and speed benchmarks[cite: 1].

---

## 1. Executive Summary & Core Architecture Decisions

FulgurStack is an ultra-fast, memory-bounded astrophotography preprocessor, star-registrar, and integrator tailored for **hundreds to thousands of ZWO Seestar S50 Pro exposures** (single or multi-night, Alt-Az with field rotation or EQ mode)[cite: 1]. It outputs a photometrically accurate, **linear 32-bit float (`BITPIX = -32`) FITS master** ready for PixInsight[cite: 1].

### Baseline & Speedup Targets
- **Owner-Confirmed PixInsight Baseline (2026-10-21):** Approximately 3 hours for the complete process from source FITS to the supplied master. The available DS-A folder contains 1,140 frames; its exact accepted reference input list is not yet verified. The inherited stage split (Debayer ~3%, StarAlignment ~15%, Integration with rejection ~80%) is indicative only. Report speedup against the approximate owner-reported baseline with that qualification, and verify equivalent input selection and processing scope. Acquisition exposure time is not processing wall-clock time.
- **Target Tiers:** Minimum **≥10×** (≤18 min) | Good **~30×** (~6 min) | Stretch **~100×** (~110 s)[cite: 1].
- **Quality Rule:** A faster stack that is blurrier, noisier, or photometrically degraded compared to the PixInsight reference master is a failure[cite: 1].

### Confirmed First-Release Scope and Local Validation Data
- **Product identity:** FulgurStack is the product and repository name; `fulgurstack` is the CLI binary. Documentation, source code, comments, schemas, and reports are English. Owner-facing conversation is Czech.
- **Supported stack:** One target and one filter per run, across one or more sessions. Exposure durations may differ. Camera geometry, CFA pattern, gain, and pixel units must be compatible; incompatible inputs must receive a clear diagnostic rather than silently mixing them.
- **Exposure source:** Read a finite, positive `EXPTIME` from each FITS header. Never infer exposure from a filename or assume all frames have the reference frame's duration.
- **DS-A inputs:** `C:\Astrofoto\2026\10 October\SeeStar50P M33\M33 Subs`.
- **PixInsight reference:** `C:\Astrofoto\2026\10 October\SeeStar50P M33\integration.xisf`. This is an immutable external comparison artifact; XISF import/export is not a first-release feature.
- **Read-only inventory (2026-10-21):** 1,140 FITS files, each 10 s, target `M 33`, filter `IRCUT`, 2160 x 3840, `BITPIX=16`, `GRBG`, gain 200, `BZERO=32768`, `BSCALE=1`, and informational `BIAS=2064`. The named reference sample exists. The XISF header identifies a 2160 x 3840 three-channel RGB Float32 image and declares 11,400 s total exposure per channel, consistent with the dataset's summed acquisition exposure. Header inspection alone does not establish linearity, quality parity, processing duration, or the exact accepted input list.
- **Mixed-exposure verification:** DS-A provides real 10 s data only. Synthetic mixed-exposure gates are mandatory; validation on real mixed-exposure data remains explicitly unverified until such data is available.
- **DS-B quick regression set:** The project-local `SeestarS50 Subs/` directory contains 32 verified 10 s M33/IRCUT source FITS files. Immutable PI references are in `PixInsight Files/`, including `integration.fit` and the selected registration anchor source. The owner reports approximately 32 s debayer, 35 s registration, and 48 s integration (115 s summed). The PI master uses ESD rejection; FulgurStack retains the specified Winsorized Sigma Clipping, with image-quality comparison rather than bitwise identity. See [benchmark evidence and settings](docs/BENCHMARKS.md) and [source/reference fingerprints](docs/ds-b-manifest.json). DS-B supplements, and does not replace, full DS-A acceptance gates. PI-provided intermediate images are immutable reference artifacts, not FulgurStack outputs.

### Frozen Architectural Choices (No Further Spike Needed)
1. **CPU-First (`rayon`), GPU-Ready Trait (`wgpu` in M5):** Because the reference machine uses shared LPDDR5x memory (32 GB), a tiled, cache-friendly multi-threaded CPU engine avoids PCIe bottlenecks and is vastly simpler to validate[cite: 1]. GPU acceleration sits behind `trait IntegrationEngine` as an optional P1 milestone[cite: 1].
2. **Zero Intermediate Disk Files:** Source FITS files are immutable[cite: 1]. No calibrated, debayered, or registered intermediate files are ever written to disk[cite: 1].
3. **Output-Tile-Major Integration (128–256 rows per tile):** Storing 1,100 full RGB `f32` frames requires ~109 GB RAM (and `f16` requires ~50 GB while destroying faint sub-ADU signal due to mantissa quantization)[cite: 1]. Therefore, the integration engine processes the output canvas in **spatial horizontal tiles (e.g., 2160 × 128 px)**. For each tile, only the required raw CFA source bounding boxes (via inverse transform) are decoded, hot-pixel cleaned, demosaiced, and warped into an in-memory `f32` tile buffer (~0.9–1.8 GB RAM for 1,100 frames), followed immediately by exact **Winsorized Sigma Clipping (Median + MAD)** and weighted accumulation.
4. **Direct Anchor Registration (No Global Bundle-Adjustment Graph):**
   - *Single session/night:* All frames register directly to the single best **Master Reference Anchor** frame (using sequential prediction first, falling back to Groth triangle/asterism matching).
   - *Multi-night:* Each night registers to its own **Session Anchor**; each Session Anchor is then matched directly to the **Master Reference Anchor** via global triangle matching (handling up to 180° flips), and the two rigid SE(2) transforms are composed analytically (`T_final = T_session_to_master ∘ T_frame_to_session`). Zero chain drift.
5. **Drizzle-Compatible Boundary (`D-1..D-5`):** Registration operates purely on CFA star catalogs and outputs geometric transforms[cite: 1]. The `IntegrationEngine` receives raw CFA views + transforms + weights, never pre-demosaiced RGB images[cite: 1].

---

## 2. Confirmed Seestar S50 Pro FITS Contract

Inspected directly from reference file `Light_M 33_10.0s_IRCUT_20261006-204534.fit`[cite: 1]:

| Keyword | Value | Mandatory Handling Rule |
|---|---|---|
| `BITPIX`, `NAXIS1`, `NAXIS2` | `16`, `2160`, `3840` | Big-Endian signed `i16`; 8,294,400 px per frame (~16.59 MB raw)[cite: 1]. |
| `BZERO`, `BSCALE` | `32768`, `1` | Physical linear value: `f32::from(i16::from_be_bytes(b)) + 32768.0`[cite: 1]. |
| `BAYERPAT` | `'GRBG'` | 2×2 CFA phase; verify actual row order against PixInsight reference[cite: 1]. |
| `CREATOR` | `'ZWO Seestar S50 Pro'` | Onboard dark subtraction is already applied when Dark Correction is ON[cite: 1]. |
| `BIAS` | `2064` | **DO NOT SUBTRACT.** Informational header card only; subtracting causes clipping[cite: 1]. |
| `FOCALLEN`, `XPIXSZ` | `260` mm, `2.9` µm | Pixel scale ≈ **2.30″/px** (under-sampled stars, typical FWHM ~1.5–2.2 px)[cite: 1]. |
| `DATE-OBS`, `EXPTIME` | ISO timestamp, `10.0` s | Preserve verbatim; do not rely on filename timestamps for session math[cite: 1]. |
| `SITELAT`, `SITELONG` | GPS coordinates | **Privacy Rule:** Never print observing site coordinates to console/logs by default[cite: 1]. |

### Output Master FITS Contract
- **Format:** Uncompressed IEEE-754 `f32` (`BITPIX = -32`), Big-Endian, planar RGB cube (`NAXIS = 3`, `NAXIS1 = W`, `NAXIS2 = H`, `NAXIS3 = 3`)[cite: 1].
- **Photometry:** Strictly linear, unstretched, preserving relative flux[cite: 1]. Internal reconstruction remains in physical source ADU. Export samples as source ADU / 65,535, with zero offset and one fixed divisor across images and RGB channels, matching the supplied PI single-frame reference's units. Preserve negative and above-one values; never normalize by image extrema or clamp. Record the divisor in descriptive `FLXDIV` metadata and FITS history; this is not an automatic FITS scaling keyword. Do not copy input `BZERO`/`BSCALE` cards to the float output[cite: 1]. Do not write false WCS headers if plate-solving was not performed[cite: 1].
- **PixInsight import:** Supply an optional PJSR import adapter with explicit `ignore-out-of-range` and `top-down` hints. It must preserve the exported samples without arithmetic or global preference changes. Ordinary PI File > Open remains subject to the user's float-range policy; FITS alone cannot force that policy. The core CLI must compile and run without PixInsight or JavaScript installed. This adapter is an M1 interoperability aid, not a FulgurStack GUI or XISF implementation.
- **Atomic Write:** Write to `<output>.tmp` and rename upon completion; clean up on error/cancellation[cite: 1].
- **Exposure metadata:** The master uses the reference anchor's intensity scale. Set output `EXPTIME` to that anchor's exposure duration, `NCOMBINE` to the number of accepted frames supplied to integration, and `TEXPTIME` to their summed acquisition exposure in seconds. These metadata do not describe per-pixel exposure where coverage or rejection differs. Report input, accepted, and rejected counts and acquisition exposure totals separately; never label the weighted mean as a summed exposure image.
- **Observing location:** Preserve the reference anchor's valid `SITELAT` and `SITELONG` cards in the final master, including single-frame M1 exports. State in FITS `HISTORY` that the location belongs to the reference anchor and may not represent every contributing session. Do not average observing coordinates or invent missing coordinates. Continue redacting coordinates from console output, default logs, and default JSON/CSV reports.

---

## 3. Core Processing Pipeline (4 Stages)

```text
[Immutable 16-bit FITS on SSD]
       │ (memmap2 / buffered read + rayon parallel scan)
       ▼
┌─────────────────────────────────────────────────────────────────┐
│ STAGE 1: FITS Scan & CFA Subframe Inspector (CPU rayon)         │
│ - Decode i16 BE + BZERO -> extract native 1080x1920 Green grid  │
│   G_super = 0.5 * (G1 + G2) for fast sky/star detection         │
│ - 16x16 mesh sky background median (BG_i) & local noise (σ_bg)  │
│ - Star detection + full-res CFA green centroid, True HFR & Ecc  │
│ - Apply Hard Rejection gates & compute frame Soft Weight (W_i)  │
└─────────────────────────────────────────────────────────────────┘
       │ (Accepted Frames + Star Catalogs + Weights W_i)
       ▼
┌─────────────────────────────────────────────────────────────────┐
│ STAGE 2: Direct-to-Anchor Rigid Registration (CPU nalgebra)     │
│ - Select best-quality Reference Anchor near session midpoint    │
│ - Match stars (Sequential prediction -> Triangle fallback)      │
│ - RANSAC + Weighted Procrustes SVD -> Rigid SE(2) (θ, dx, dy)   │
│ - Compute photometric scale (a_i) & sky offset (b_i) from stars │
│ - Compute exact polygon footprints & Output Canvas crop         │
└─────────────────────────────────────────────────────────────────┘
       │ (IntegrationPlan: Transforms, Normalization, CanvasSpec)
       ▼
┌─────────────────────────────────────────────────────────────────┐
│ STAGE 3: Tiled CFA Reconstruction & Integration (CPU Engine)    │
│ For each horizontal Output Tile (e.g., 2160 x 128 px):          │
│ 1. Inverse-map tile bounds to each frame's CFA ROI (+8px halo)  │
│ 2. Hot-Pixel Cosmetic Filter (3x3 same-color CFA 5σ replacement)│
│ 3. Malvar-He-Cutler (5x5) Demosaicing + Bicubic Inverse Warp    │
│ 4. Apply Photometric Normalization: y_i = a_i * x_i + b_i       │
│ 5. Per-pixel Winsorized Sigma Clipping + Neumaier Weighted Mean │
└─────────────────────────────────────────────────────────────────┘
       │
       ▼
[Stage 4: Atomic 32-bit Float RGB FITS Writer + JSON Telemetry]
```

---

## 4. Mathematical & Algorithmic Specifications

### 4.1 Sky Background & Noise (§6.2)
Divide the green channel into a **16 × 16 grid**[cite: 1]. Within each cell, compute the median and Median Absolute Deviation ($\text{MAD}$) after basic star clipping[cite: 1]:
$$\tilde{b}_{\text{cell}} = \operatorname{median}(x), \qquad \hat{\sigma}_{\text{cell}} = 1.4826 \cdot \operatorname{median}(\vert{}x - \tilde{b}_{\text{cell}}\vert{})$$
- Global sky background $\text{BG}_i$ = median of cell medians[cite: 1].
- Global sky noise $\sigma_{\text{bg}, i}$ = median of cell $\hat{\sigma}_{\text{cell}}$ (never take the MAD of just the 16×16 cell medians, which understates pixel noise)[cite: 1].

### 4.2 Star Detection, True HFR & Eccentricity (§6.3, §6.4)
- **Detection:** Find local maxima above $\text{BG}_i + 6\sigma_{\text{bg}, i}$ on the $1080 \times 1920$ averaged green grid $(G_1+G_2)/2$, excluding saturated peaks ($> 62,000\text{ ADU}$)[cite: 1]. Select up to 250 well-distributed stars across a $4 \times 4$ spatial bucket grid (preventing all stars from clustering in a galaxy core)[cite: 1].
- **Refinement:** Map peak coordinates back to full $2160 \times 3840$ CFA space[cite: 1]. Using locally sky-subtracted positive green fluxes $f_k = \max(0, I_k - b_{\text{local}})$ within an adaptive radius ($R = 6\text{ px}$):
  - **Sub-pixel Centroid:** $(x_c, y_c) = \frac{1}{\sum f_k} \left(\sum f_k x_k, \sum f_k y_k\right)$[cite: 1]
  - **True Half-Flux Radius (HFR):** Interpolate the radial cumulative flux curve to find the exact radius $r_{50}$ where enclosed flux equals $0.5 \sum f_k$[cite: 1].
  - **Eccentricity:** From the 2nd central moment covariance matrix $\Sigma$, compute eigenvalues $\lambda_{\max} \ge \lambda_{\min} > 0$ and eccentricity $e = \sqrt{1 - \lambda_{\min}/\lambda_{\max}}$[cite: 1].

### 4.3 Subframe Hard Rejection & Soft Weights (§6.5)
- **Hard Rejection (Configurable Defaults):**
  1. `HFR_i > median(HFR) + 2.5 * 1.4826 * MAD(HFR)` (Defocus / seeing spike)[cite: 1]
  2. `StarCount_i < 0.45 * median(StarCount)` (Clouds / heavy haze)[cite: 1]
  3. `Eccentricity_i > 0.60` (Wind gust / mount tracking jump)[cite: 1]
  4. `RegistrationFailed`: `< 10` inlier star pairs, or inlier $\text{RMSE} > 1.5\text{ px}$.
- **Soft Weighting (for accepted frames):**
  $$w_i = \frac{T_i^2}{\sigma_{\text{bg}, i}^2 \cdot \text{HFR}_i^2}$$
  where $T_i$ is the relative atmospheric transparency (median flux ratio of matched stars relative to the Reference Anchor)[cite: 1]. Normalize $w_i$ so $\max(w_i) = 1.0$.

#### Mixed-Exposure Extension (Binding)
- For exposure duration $t_i$ and matched unsaturated stellar fluxes, distinguish total flux ratio $S_i = \operatorname{median}(F_{i,k}/F_{\text{ref},k})$ from exposure-corrected transparency $T_i = S_i t_{\text{ref}}/t_i$. A longer exposure must not be reported as improved atmospheric transparency.
- Use the final photometric scale from Section 4.4 and noise measured in the same normalized units: $\sigma_{\text{norm},i} = |a_i|\sigma_{\text{bg},i}$. Final quality weights are $w_i = 1/(\sigma_{\text{norm},i}^2\,\text{HFR}_i^2)$, normalized to a maximum of 1. This reduces to the original equal-exposure formula and incorporates exposure and transparency once.
- M2 cannot compute matched-star transparency before M3 registration. Its standalone report must label weights as provisional; M3 finalizes transparency, photometric scale, and weights before integration. Do not reject an entire shorter-exposure group solely because its expected detected star count is lower. Compare star-count rejection within compatible exposure-duration groups; do not apply the population-relative rule when a group has insufficient comparison frames.
- Exclude stars saturated in either member of a photometric match. Reject frames with invalid exposure, non-finite normalization, or unusable noise estimates with an explicit reason; never emit infinite weights.

### 4.4 Rigid SE(2) Registration & Photometric Normalization (§7, §8.4)
- Fit strictly **Rigid SE(2)** transformations (rotation $\theta_i$ and translation $t_{x,i}, t_{y,i}$, scale fixed to $1.0$) using **RANSAC + Weighted Procrustes SVD** (`nalgebra`)[cite: 1]:
  $$\mathbf{r} = \begin{bmatrix}\cos\theta_i & -\sin\theta_i \\ \sin\theta_i & \cos\theta_i\end{bmatrix} \mathbf{p}_i + \mathbf{t}_i$$
- From the matched inlier stars, compute the relative flux scale $a_i = \operatorname{median}(F_{\text{ref}, k} / F_{i, k})$ and background offset $b_i = \text{BG}_{\text{ref}} - a_i \cdot \text{BG}_i$ so every frame is normalized before pixel rejection: $y_i(p) = a_i x_i(p) + b_i$[cite: 1].
- These matched-flux scales already include exposure-duration differences. Do not apply a second exposure correction to normalized pixels. Preserve relative RGB flux with the same scalar normalization in all channels.

### 4.5 Pre-Demosaic Hot-Pixel Filter, Demosaic & Warp (§8.1–§8.3)
1. **CFA Cosmetic Correction (Hot-Pixel Removal):** Before demosaicing a source ROI tile, inspect each raw CFA pixel against its 8 nearest neighbors of the **same Bayer color** (at step $\pm 2\text{ px}$). If a pixel exceeds the median of its same-color neighbors by $> 5\sigma_{\text{local}}$, replace it with that median.
   - **M1 noise/star safeguards:** Confirm candidates using the larger of the 8-neighbor MAD noise and a 24-neighbor same-phase MAD noise estimate (5 x 5 same-phase grid at step 2, excluding the center). Use a 1 ADU minimum noise estimate for constant/quantized data. Preserve candidates supported by at least two orthogonally adjacent CFA samples above their same-phase local median plus `min(3 * neighbor_noise, candidate_excess / 8)`, where candidate excess is its value minus its same-phase local median. The contrast cap prevents stellar gradients and saturated plateaus from inflating the support threshold and erasing real star cores. Prefer preserving an ambiguous defect to damaging a star; single-frame correction is heuristic, not a verified defect map. Leave the outer four sensor rows/columns unmodified by cosmetics. Validate saturated/colored stars over a width and subpixel-phase sweep as well as isolated defects and ordinary noise.
2. **Demosaicing:** 5×5 **Malvar-He-Cutler (MHC)** linear filter respecting exact CFA phase on the ROI + 2 px border halo[cite: 1].
3. **Inverse Resampling:** For each output pixel center $\mathbf{q}$, compute source coordinate $\mathbf{p}_i = T_i^{-1}(\mathbf{q})$ and sample the demosaiced RGB ROI using **Bicubic Keys ($a = -0.5$)** interpolation[cite: 1]. If the $4 \times 4$ bicubic footprint falls outside the valid sensor boundary, mark the sample `valid = false` (never blend zero-valued border padding into the stack)[cite: 1].
   - The composed raw-data support requires an 8-pixel halo: four for cosmetic noise confirmation, two for MHC, and two for bicubic sampling. M1 full-frame MHC uses parity-preserving reflection at sensor boundaries; future ROI processing must not treat internal tile boundaries as sensor boundaries.

### 4.6 Winsorized Sigma Clipping & Stable Accumulation (§8.5, §8.6)
For each output pixel and RGB channel with $N_{\text{valid}}$ valid samples:
- If $N_{\text{valid}} == 0$: output `0.0` and coverage `0`[cite: 1].
- If $1 \le N_{\text{valid}} < 5$: skip clipping, compute weighted mean directly[cite: 1].
- If $N_{\text{valid}} \ge 5$: run up to 3 iterations of **Winsorized Sigma Clipping** (`sigma_low = 3.0`, `sigma_high = 2.5`) around the median and $\hat{\sigma} = 1.4826 \cdot \text{MAD}$[cite: 1].
- Accumulate surviving samples using **Neumaier compensated summation** in `f64` on CPU (or tree-reduced `f32` on future GPU) for both numerator $\sum w_i y_i$ and denominator $\sum w_i$[cite: 1].

---

## 5. Workspace Structure & CLI Contract

### 5.1 Simplified Cargo Workspace
```text
fulgurstack/
  Cargo.toml
  PROJECT_SPEC.md
  crates/
    fulgur-core/      # Core types, FITS reader/writer, CFA phase, calibration provenance
    fulgur-pipeline/  # Stage 1 (Inspect), Stage 2 (Register), Stage 3 (Tile Integrator)
    fulgur-cli/       # clap CLI binary (`fulgurstack`)
```
*(Optional crates `gpu-engine` and `fulgur-gui` will be added only in M5)[cite: 1].*

### 5.2 CLI Commands (`fulgur-cli`)
```bash
# 1. Fast header survey & quality inspection (outputs JSON/CSV report)
fulgurstack inspect --input "D:/Astro/M33" --report quality.json

# 2. Full end-to-end stack (Inspect -> Register -> Integrate -> FITS)
fulgurstack stack \
  --input "D:/Astro/M33_night1" \
  --input "D:/Astro/M33_night2" \
  --canvas coverage-optimized \
  --min-coverage 0.80 \
  --sigma-low 3.0 --sigma-high 2.5 \
  --memory-budget-mib 12288 \
  --output "D:/Astro/M33_master.fits" \
  --report "D:/Astro/M33_run_report.json"
```

---

## 6. Streamlined Implementation Plan (5 Milestones)

Execute strictly in order. Each milestone must compile cleanly (`cargo test`, `cargo clippy`) and pass its verification gate on both synthetic unit tests and the real `DS-A` dataset (`FULGURSTACK_TEST_FITS_DIR`)[cite: 1].

### Milestone 1 (M1): Workspace, FITS I/O, CFA Cosmetic + MHC Demosaic
- **Deliver:**
  - Workspace setup (`fulgur-core`, `fulgur-pipeline`, `fulgur-cli`).
  - Pure-Rust FITS reader (`BITPIX=16`, Big-Endian + `BZERO`, header preservation, privacy redaction for `SITELAT`/`SITELONG`) and atomic 32-bit float RGB FITS writer (`BITPIX=-32`)[cite: 1].
  - 3×3 same-channel CFA hot-pixel filter + 5×5 Malvar-He-Cutler demosaicer.
  - CLI command `fulgurstack debayer-single --input <sample.fit> --output <test_rgb.fits>` + dataset header/I/O throughput scanner (`fulgurstack survey`).
- **Acceptance Gate (G1):**
  - Unit tests pass for Big-Endian `i16` decoding and MHC color reconstruction.
  - Single-frame `test_rgb.fits` from `Light_M 33_10.0s_IRCUT_20261006-204534.fit` opens in PixInsight with correct orientation, valid RGB colors, no hot-pixel crosses, and zero `BIAS=2064` subtraction[cite: 1].
  - Cosmetic correction must not introduce dark holes into spatially supported bright/saturated star cores. The owner-reported failure of the earlier normalized export requires a fresh visual review after the stellar-support fix.
  - Record baseline metadata (`B-01`) and SSD read speed on the 1,140-frame `DS-A` folder[cite: 1].
  - Preserve source exposure and valid GPS metadata in the single-frame master while redacting coordinates from default diagnostics and reports.

### Milestone 2 (M2): CPU Subframe Inspector & Quality Filtering (Stage 1)
- **Deliver:**
  - Parallel `rayon` scanner extracting the $1080 \times 1920$ green grid, 16×16 sky background/noise estimator, spatially bucketed star detector, full-res centroiding, True HFR interpolation, and moment eccentricity[cite: 1].
  - Hard-rejection classifier + soft-weight calculator ($w_i$)[cite: 1].
  - CLI command `fulgurstack inspect --input <dir> --report quality.json`.
- **Acceptance Gate (G2):**
  - Synthetic Gaussian star test recovers analytical HFR ($\approx 1.1774\sigma$) within 3%[cite: 1].
  - Scans all 1,140 frames of `DS-A` in **< 15 seconds** on Ryzen AI 9 HX, outputting `quality.json` with realistic HFR (~1.5–3.0 px), star counts, and flagged defective frames[cite: 1].

### Milestone 3 (M3): Direct-to-Anchor Rigid Registration & Canvas Planner (Stage 2)
- **Deliver:**
  - Reference Anchor auto-selector, sequential star predictor, and Groth rotation-invariant triangle matcher fallback (handling arbitrary rotation up to 180°)[cite: 1].
  - RANSAC + Weighted Procrustes SE(2) solver (`nalgebra`), relative photometric scale/offset ($a_i, b_i$) estimator, and polygon footprint coverage calculator (`CommonIntersection` and `CoverageOptimized` with 0.80 effective-weight threshold)[cite: 1].
- **Acceptance Gate (G3):**
  - Synthetic rotation sweep ($0^\circ, 15^\circ, 90^\circ, 179.5^\circ$) and pseudo-second-night test (real `DS-A` subset rotated $180^\circ$ + shifted) recover exact transforms within $< 0.15\text{ px}$ RMSE[cite: 1].
  - Registers all accepted frames of `DS-A` smoothly across the full night's Alt-Az field rotation[cite: 1].

### Milestone 4 (M4): Tiled CPU Integration Engine & Release Candidate (Stage 3 & 4)
- **Deliver:**
  - `IntegrationEngine` trait implementation using spatial output tiles (`2160 × 128`), ROI inverse bounding-box extraction, hot-pixel cleaning, MHC demosaic, Bicubic Keys inverse warp with strict validity masking, photometric normalization, iterative Winsorized Sigma Clipping, and `f64` Neumaier weighted accumulation[cite: 1].
  - End-to-end `fulgurstack stack` CLI command with memory-budget enforcement (`<= 12 GB` RSS) and JSON benchmark report[cite: 1].
- **Acceptance Gate (G4 — MVP Sign-off):**
  - Zero intermediate files written to disk (`T-I/O-01`)[cite: 1].
  - Synthetic satellite trail removed without clipping star cores (`T-REJECT-01`); isolated star flux preserved within $\le 1\%$ (`T-PHOTO-01`)[cite: 1].
  - Full 1,140-frame `DS-A` run completes on the reference machine, records accepted/rejected counts, and matches PixInsight reference master quality (background RMS, star FWHM, color balance). Target <=18 min and >=10x speedup against the owner-confirmed approximately 3-hour full-process baseline (`T-BASE-01`, `T-PERF-01`). Qualify speedup as approximate and verify equivalent input selection and processing scope. Define quality measurement regions and numerical tolerances before the M4 comparison, then obtain owner visual sign-off.
  - Synthetic mixed-exposure stacks preserve normalized unsaturated stellar flux within <=1%, report exposure-corrected transparency correctly, and do not reject good shorter exposures simply for having fewer detected stars. Test saturated-star exclusion, invalid exposure handling, and finite final weights.
  - Verify accepted exposure totals and reference-anchor GPS provenance in the final FITS without leaking coordinates into default telemetry.

### Milestone 5 (M5 — Optional P1/P2): GPU Backend (`wgpu`) & Native GUI (`egui`)
- **Trigger:** Started *only* after M4 benchmark identifies the exact bottleneck stage[cite: 1].
- **Deliver:**
  - `gpu-engine` crate implementing `IntegrationEngine` via `wgpu` (DX12/Vulkan on Windows, Metal on macOS) with `f32` tree/Kahan reduction and CPU parity tests (`T-NUM-01`)[cite: 1].
  - `fulgur-gui` crate (`eframe`/`egui`) providing interactive session management, HFR/weight plots, coverage crop preview, and non-destructive STF auto-stretch display[cite: 1].
