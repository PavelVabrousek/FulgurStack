# FulgurStack — Project Specification & Implementation Contract (v3.3)

**Version:** 3.3 (M4 Owner Acceptance and M5 GPU Reconstruction; supersedes v3.2)\
**Date:** 2026-10-22\
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
- **M1 quality revision:** The owner rejected the MHC candidates for dark core defects and chromatic satellites. Preserve the MHC baseline for diagnostics, but use VNG with final sensor-range bounding as the default reconstruction candidate under the binding quality rule. This is an engineering revision within M1, recorded in ADR-012, not owner visual approval or permission to start M2. The owner explicitly approved limiting reconstructed values to 0..1 as sensor saturation compatible with linear-light output (ADR-014). VNG remains adaptive, with fixed photometric units and no tone stretch.
- **Milestone authorization:** The owner visually approved M1/G1 on 2026-10-21 and explicitly requested M2 implementation and a 32-frame manual-review table. The owner approved M2/G2 on 2026-10-22 and authorized M3 implementation and registration testing on DS-B. The owner approved M3/G3 on 2026-10-22 and authorized M4 implementation and a DS-B integrated FITS for PixInsight review. The owner visually approved the full 1,140-frame external-200% master and explicitly closed M4/G4 on 2026-10-22, authorizing M5 GPU acceleration. Known analytic-photometry/reference-workload limitations remain recorded as limitations, not retroactive mathematical passes.
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
- **Photometry:** Strictly linear, unstretched, preserving relative flux[cite: 1]. Internal reconstruction remains in physical source ADU. Export samples as source ADU / 65,535, with zero offset and one fixed divisor across images and RGB channels, matching the supplied PI single-frame reference's units. Complete reconstruction at export by saturating normalized samples to 0..1, matching PI's handling of reconstructed channels. This owner-approved sensor-range policy supersedes the earlier prohibition on limiting interpolation excursions and is not considered a violation of the product's linearity/no-data-clipping requirement. Preserve every in-range sample exactly at Float32 precision; never normalize by image extrema. Retain unbounded reconstruction internally for diagnostics. Record lower/upper limiting counts separately from the final out-of-range counts. Record the divisor in descriptive `FLXDIV` metadata and FITS history; this is not an automatic FITS scaling keyword. Do not copy input `BZERO`/`BSCALE` cards to the float output[cite: 1]. Do not write false WCS headers if plate-solving was not performed[cite: 1].
- **PixInsight import:** Current bounded FITS must open through ordinary **File > Open**, without a FulgurStack loading script, forced STF or manual intensity conversion. Verify loaded pixel values and orientation against the serialized FITS and supplied single-frame reference. Historical import scripts are diagnostic tools only. ADR-013's display workaround failed owner review and is superseded by ADR-014; its in-memory transfer test did not verify the actual viewport path. FITS as a format permits out-of-range floats; the 0..1 rule is FulgurStack's explicit product contract, not a FITS standard restriction.
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
│ 3. VNG (5x5; MHC diagnostic) Demosaic + Bicubic Inverse Warp   │
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
- **M2 sampling convention:** Use at most a stratified 16 x 16 sample within each of the 16 x 16 spatial cells, then one 3-sigma clipping pass and recompute median/MAD. This retains pixel-scale noise while bounding estimator cost. Report noise in source ADU of the two-green average, without subtracting informational BIAS.

### 4.2 Star Detection, True HFR & Eccentricity (§6.3, §6.4)
- **Detection:** Find local maxima above $\text{BG}_i + 6\sigma_{\text{bg}, i}$ on the $1080 \times 1920$ averaged green grid $(G_1+G_2)/2$, excluding saturated peaks ($> 62,000\text{ ADU}$)[cite: 1]. Select up to 250 well-distributed stars across a $4 \times 4$ spatial bucket grid (preventing all stars from clustering in a galaxy core)[cite: 1].
- **M2 detection counts:** Count uncapped supported, unsaturated peaks for cloud/count rejection; report this separately from raw local maxima and the capped 250-star photometry catalog. Require support in at least three neighboring green-grid samples above local 3-sigma sky. Exclude a candidate if any CFA color in its central neighborhood exceeds 62,000 ADU. Keep 4 x 4 bucket selection deterministic, with at most 64 candidates per bucket and round-robin photometry selection.
- **Refinement:** Map peak coordinates back to full $2160 \times 3840$ CFA space[cite: 1]. Using locally sky-subtracted positive green fluxes $f_k = \max(0, I_k - b_{\text{local}})$ within an adaptive radius ($R = 6\text{ px}$):
  - **Sub-pixel Centroid:** $(x_c, y_c) = \frac{1}{\sum f_k} \left(\sum f_k x_k, \sum f_k y_k\right)$[cite: 1]
  - **True Half-Flux Radius (HFR):** Interpolate the radial cumulative flux curve to find the exact radius $r_{50}$ where enclosed flux equals $0.5 \sum f_k$[cite: 1].
  - **Eccentricity:** From the 2nd central moment covariance matrix $\Sigma$, compute eigenvalues $\lambda_{\max} \ge \lambda_{\min} > 0$ and eccentricity $e = \sqrt{1 - \lambda_{\min}/\lambda_{\max}}$[cite: 1].

M2 uses a 6 px circular measurement aperture and a 7..10 px native-green sky annulus. Native-green centroids are refined before cumulative photometry. For HFR and covariance only, interpolate the rotated native-green lattice with normalized Lanczos-3 phase tables, evaluate half-pixel quadrature, split pixel-area contributions into 2 x 2 subcells and interpolate cumulative flux in 0.01 px radial bins. No RGB image is materialized or saved. This is a finite-aperture HFR; broad/faint/blended sources can be biased and require manual review. The reported FWHM is a Gaussian-equivalent second-moment value, not a fitted profile FWHM.

### 4.3 Subframe Hard Rejection & Soft Weights (§6.5)
- **Hard Rejection (Configurable Defaults):**
  1. `HFR_i > median(HFR) + 2.5 * 1.4826 * MAD(HFR)` (Defocus / seeing spike)[cite: 1]
     - M2 uses an explicit configurable 0.05 px floor on robust HFR scatter to avoid a zero-MAD population rejecting tiny measurement fluctuations. Evaluate this population gate only with at least five measurable, compatible frames. HFR is compared across exposure durations; star-count rejection alone uses exposure groups.
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
- Standalone M2 weights use `1 / ((sigma_bg / exposure_seconds)^2 * HFR^2)`, normalized across accepted frames only. These are provisional unit-transparency, exposure-normalized weights, not measurements of atmospheric transmission. Exposure-count groups use a relative duration tolerance of 1e-6 and at least five comparison frames; at least ten measured stars and positive finite noise are required. Registration and its failure rejection are explicitly deferred to M3.
- Exclude stars saturated in either member of a photometric match. Reject frames with invalid exposure, non-finite normalization, or unusable noise estimates with an explicit reason; never emit infinite weights.

### 4.4 Native Rigid Registration, External Similarity & Photometric Normalization (§7, §8.4)
- Fit strictly **Rigid SE(2)** transformations (rotation $\theta_i$ and translation $t_{x,i}, t_{y,i}$, scale fixed to $1.0$) using **RANSAC + Weighted Procrustes SVD** (`nalgebra`)[cite: 1]:
  $$\mathbf{r} = \begin{bmatrix}\cos\theta_i & -\sin\theta_i \\ \sin\theta_i & \cos\theta_i\end{bmatrix} \mathbf{p}_i + \mathbf{t}_i$$
- From the matched inlier stars, compute the relative flux scale $a_i = \operatorname{median}(F_{\text{ref}, k} / F_{i, k})$ and background offset $b_i = \text{BG}_{\text{ref}} - a_i \cdot \text{BG}_i$ so every frame is normalized before pixel rejection: $y_i(p) = a_i x_i(p) + b_i$[cite: 1].
- These matched-flux scales already include exposure-duration differences. Do not apply a second exposure correction to normalized pixels. Preserve relative RGB flux with the same scalar normalization in all channels.

#### M3 Registration and Canvas Conventions
- `register` reruns the immutable CFA inspector, preserves M2 quality decisions, and attempts geometric registration of rejected but measurable compatible frames for diagnostics only. Rejected frames have zero final weight and no integration footprint.
- Use header `DATE-OBS`, sorted chronologically; gaps over six hours start a session. If any timestamp is missing/unparseable, explicitly report the single-session sorted-path fallback. Select the chronological median of quality-accepted frames using FITS DATE-OBS (including subsecond precision and timezone offsets); choose the earlier central frame for even counts and source-name order for equal timestamps. Do not use quality weights or filenames as timestamps. Missing accepted acquisition times prevent automatic selection and require explicit --reference. The master is selected globally and serves as its session anchor. Other sessions use their accepted chronological median. An explicit --reference selects one uniquely identified quality-accepted input frame by path or unique filename, or an external geometry-only FITS as specified in the scaled-reference revision below. Reject missing, ambiguous or quality-rejected input choices; never silently fall back.
- Predict angular velocity and transformed sensor-center velocity from two successful direct fits using actual header times. Every local solution is still fitted to its session anchor; prediction never creates transform chains. Match unique nearest stars, deterministic two-pair RANSAC (up to 512 hypotheses), then SNR-weighted orthogonal Procrustes SVD with proper-rotation correction and three inlier-refinement passes. Require at least ten unique pairs inside 1.5 px. Global fallback uses canonical local triangles (eight nearest neighbors), side-ratio descriptors, rigid-length checks and transform voting; Native source/session fits have unit scale and no shear/reflection; external template alignment additionally fits uniform scale as specified below.
- Compose each source-to-session transform with its session-to-master transform. Report both bridge and local matched pairs and residuals, with the named reference catalog. Final coordinates are zero-based original sensor pixel centers, top-down; translations refer to the sensor origin, not the image center.
- Prefer unsaturated direct master matches for separate medians of reference/source flux (scale) and source/reference flux (total flux ratio). If fewer than ten direct photometric matches exist, compose the two session-bridge photometric ratios and label this approximation. Report matched photometric pairs when direct. Compute exposure-corrected transparency and final normalized-noise weights exactly once; background offsets remain in physical ADU.
- Footprints are exact transformed convex sensor polygons with the strict Keys 4x4 sample domain: source x/y >=1 and x<width-2, y<height-2 (upper bounds inset by 1e-6 px). CommonIntersection is the exact convex polygon intersection. Each proposed rectangular canvas is the largest axis-aligned rectangle of integer pixel centers within its mask; half-open upper scanline intervals conservatively exclude boundary centers. CoverageOptimized uses the sum of accepted final weights covering each center divided by total accepted weight (default >=0.80), not frame-count coverage. Exact polygon-edge scanline events and an O(width) histogram avoid storing full-frame masks for every source. Empty overlap returns no common canvas. The output origin may be negative in anchor coordinates.
- M3 produces only JSON/CSV plans. Resampling and integrated FITS export belong to M4 after owner G3 approval. CLI: `fulgurstack register --input <dir> --report registration.json --csv registration.csv --min-coverage 0.80`.

#### Owner Revision: Reference Canvas and Final Diagnostic Maps (2026-10-22)
- Default `stack --canvas reference` uses the registration reference FITS dimensions, original pixel grid and origin (0,0), with no automatic crop. Existing common/coverage-optimized canvases remain explicit opt-in policies. A coverage threshold does not mask or crop reference mode. Invalid resampling footprints remain invalid, not padded into the average; uncovered master pixels are zero and identified by the coverage map.
- Both `register` and `stack` accept `--reference <input-path-or-unique-filename>`; absent this option, use the accepted chronological median rule above.
- Always export the final master plus two owner-requested final diagnostic FITS maps: `<master-stem>_coverage.fits` and `<master-stem>_high_rejection.fits`. Optional `--coverage-output` and `--high-rejection-output` override paths. All destinations must be distinct, new, validated before processing and written atomically. The owner explicitly authorizes these diagnostic images in addition to the final scientific master; they are not calibrated/debayered/registered intermediate frames.
- Both maps are RGB Float32 on exactly the master grid. Coverage is the surviving per-channel sample count after frame selection, geometry and low/high pixel rejection divided by ALL original deduplicated input frames. It measures used original samples, not quality weight, acquisition exposure or pre-rejection geometric overlap. High rejection is the count rejected above the upper clipping threshold per channel divided by the same original input count. It excludes low rejection, geometric absence, whole-frame rejection and final export sensor limiting. Never merge channel-dependent rejection counts into a false monochrome value.
- Map samples are direct normalized fractions 0..1 representing 0..100%. Store BUNIT='fraction', MAPTYPE, NINPUT and descriptive PCTFACT=100. Never divide map values by 65535, apply FITS scaling or min/max stretching, or label maps scientific ADU. With 27/32 accepted inputs, maximum used coverage is 84.375%. Reject invalid map values instead of silently clipping them. Maps do not copy observing coordinates.
- Memory planning includes all three final RGB buffers and current-tile diagnostics; source ROI concurrency remains bounded. JSON schema 2 records map names, denominator, definitions and reference choice. Keep previous milestone and absolute-photometry limitations explicit; this owner revision does not close G4.

#### Owner Revision: Scaled External Geometry References (2026-10-22)
- The owner explicitly requests registration references at **33% through 300%** of the native source pixel geometry, including cropped external FITS. This supersedes the input-only reference restriction and the scale=1 requirement for the external alignment step. Native science input compatibility, CFA quality inspection and internal session registration remain unchanged.
- `--reference` on `register` and `stack` accepts either the existing quality-accepted input selection or an existing external FITS path. An external reference is geometry-only: it is never counted as an input, accepted exposure, weighted sample or source of pixel photometry. Existing input references that failed quality checks must still fail; never bypass quality via the external path.
- Read external uncompressed primary monochrome or planar RGB `BITPIX=-32`/16 FITS, respecting signed big-endian decoding and BZERO/BSCALE. RGB geometry uses the green plane regardless of leftover BAYERPAT cards. Explicit TOP-DOWN/BOTTOM-UP determines reference orientation; absent ROWORDER on these reconstructed references means standard BOTTOM-UP. Retain only one reference plane, capped at 96 million pixels, validate dimensions/padding/finiteness, honor cancellation and redact GPS. No XISF support is implied.
- Use distributed geometry-only reference centroids and scale-invariant triangle ratios to seed a positive uniform-scale rotation/translation fit. Proper-rotation weighted Procrustes and deterministic RANSAC exclude shear/reflection. Fit and constrain final source-to-reference scale to [0.33,3.0]; require at least ten unique star matches and <=1.5 reference-pixel RMSE. Endpoints use constrained least-squares scale; unsupported scales must not be silently accepted as unrestricted models.
- Retain the native accepted chronological median as the scientific photometric/exposure/GPS anchor. Fit it to the external catalog, then independently refine each source directly against that external catalog using the composed prediction. Record native photometric transforms, external matches/catalog, geometric scale and residuals separately. Reject external alignment failures without silently reverting to the native grid. Native session transforms remain in native anchor coordinates and are explicitly labeled.
- Reference canvas uses the external image's exact dimensions and crop at (0,0); all final maps share it. Scale-aware inverse transforms, footprints and memory estimates must be used throughout integration. Resampling preserves reference-normalized surface brightness; do not multiply intensities by area, import template intensities, or silently change exposure/weights. Existing Keys reconstruction remains the specified interpolation; synthetic scale tests verify geometry, inverse sampling, strict masks and tile invariance, not a new absolute photometric guarantee for undersampled downscales.
- External-grid FITS records GEOMSCL and geometry-only provenance, retains native scientific exposure/GPS provenance, omits native XPIXSZ/YPIXSZ on the resampled grid and writes no false WCS. CROPX/CROPY refer to external reference centers. The reference may have no exposure metadata because it supplies no science samples.
- Validation: geometry sweeps including 0.33/0.5/1/2/3 and 0/15/90/179.5 degrees, missing/outlier stars, inverse/composition correctness, reflection/out-of-range rejection, scaled tile invariance, malformed/oriented RGB imports, unchanged unit-scale products, source integrity and real 32-frame registration/stacking against `RegisteringTemplate200pct.fit`. No GPU or GUI scope is added and outstanding G4 quality limitations remain explicit.

### 4.5 Pre-Demosaic Hot-Pixel Filter, Demosaic & Warp (§8.1–§8.3)
1. **CFA Cosmetic Correction (Hot-Pixel Removal):** Before demosaicing a source ROI tile, inspect each raw CFA pixel against its 8 nearest neighbors of the **same Bayer color** (at step $\pm 2\text{ px}$). If a pixel exceeds the median of its same-color neighbors by $> 5\sigma_{\text{local}}$, replace it with that median.
   - **M1 noise/star safeguards:** Confirm candidates using the larger of the 8-neighbor MAD noise and a 24-neighbor same-phase MAD noise estimate (5 x 5 same-phase grid at step 2, excluding the center). Use a 1 ADU minimum noise estimate for constant/quantized data. Preserve candidates supported by at least two orthogonally adjacent CFA samples above their same-phase local median plus `min(3 * neighbor_noise, candidate_excess / 8)`, where candidate excess is its value minus its same-phase local median. The contrast cap prevents stellar gradients and saturated plateaus from inflating the support threshold and erasing real star cores. Prefer preserving an ambiguous defect to damaging a star; single-frame correction is heuristic, not a verified defect map. Leave the outer four sensor rows/columns unmodified by cosmetics. Validate saturated/colored stars over a width and subpixel-phase sweep as well as isolated defects and ordinary noise.
2. **Demosaicing:** Default to independently implemented **VNG** on a 5x5 CFA neighborhood, respecting the exact CFA phase and a 2 px reconstruction halo. Generate eight directional stencils by rotation/reflection. Select gradients `g <= min(g) + 0.5 * max(g)`, including flat/tied directions, then reconstruct missing channels from directional mean color differences. Preserve the native sample exactly after cosmetics. Keep f32 computations and unbounded interpolation internally; complete reconstruction at final FITS export by saturating source ADU / 65,535 to 0..1 (ADR-014). Never stretch or normalize by image extrema. Generate symmetric diagonal stencils. Retain the original 5x5 **MHC** filter behind `--demosaic mhc` for reproducible diagnostics; it is not the default quality candidate after owner rejection. Record the selected method in JSON and FITS `DEBAYER`/history. Adaptive direction selection is not a tone curve; test common gain/offset equivariance and native-sample preservation.
3. **Inverse Resampling:** For each output pixel center $\mathbf{q}$, compute source coordinate $\mathbf{p}_i = T_i^{-1}(\mathbf{q})$ and sample the demosaiced RGB ROI using **Bicubic Keys ($a = -0.5$)** interpolation[cite: 1]. If the $4 \times 4$ bicubic footprint falls outside the valid sensor boundary, mark the sample `valid = false` (never blend zero-valued border padding into the stack)[cite: 1].
   - The composed raw-data support requires an 8-pixel halo: four for cosmetic noise confirmation, two for demosaicing, and two for bicubic sampling. M1 full-frame reconstruction uses parity-preserving reflection at sensor boundaries; future ROI processing must not treat internal tile boundaries as sensor boundaries.

### 4.6 Winsorized Sigma Clipping & Stable Accumulation (§8.5, §8.6)
For each output pixel and RGB channel with $N_{\text{valid}}$ valid samples:
- If $N_{\text{valid}} == 0$: output `0.0` and coverage `0`[cite: 1].
- If $1 \le N_{\text{valid}} < 5$: skip clipping, compute weighted mean directly[cite: 1].
- If $N_{\text{valid}} \ge 5$: run up to 3 iterations of **Winsorized Sigma Clipping** (`sigma_low = 3.0`, `sigma_high = 2.5`) around the median and $\hat{\sigma} = 1.4826 \cdot \text{MAD}$[cite: 1].
- Accumulate surviving samples using **Neumaier compensated summation** in `f64` on CPU (or tree-reduced `f32` on future GPU) for both numerator $\sum w_i y_i$ and denominator $\sum w_i$[cite: 1].

#### M4 Integration Conventions
- `stack` accepts repeated `--input` files/directories, deduplicates canonical paths, classifies one combined quality population, and uses one master anchor across sessions. Source IDs are relative to their common filesystem root; different roots without a shared filesystem root fail explicitly. A single directory retains its original relative source names.
- The integration boundary consumes immutable raw CFA paths/headers, rigid transforms, scalar photometry and final weights. Process output tiles in row-major order; the original M4 default is 256 x 128 px, superseded by the verified M5 default in Section 7.1. These are spatial tiles rather than full-width strips: at large field rotations their inverse ROI bounding boxes avoid repeatedly debayering much of a sensor. Decode only the required rectangular raw ROI with an 8 px composed halo. Round ROI origins down to even coordinates to preserve Bayer phase. Sensor-edge reflection follows accepted M1; internal ROI boundaries never enter the required 4x4 sampling footprint.
- Bound concurrently resident raw/cleaned/RGB ROIs to at most 16, including nested Rayon reconstruction. Retain only frame-major registered samples for the current output tile and one final master. Per-pixel invalid samples are excluded before statistics. Track geometric coverage separately from surviving weight after rejection; 0.80 constrains geometric input coverage, not a post-rejection weight guarantee.
- Default memory ceiling is 12288 MiB, allowed 1024..12288 MiB. Account for all three final RGB buffers, tile samples, bounded ROI workers, scratch and 768 MiB of inspection/catalog/runtime overhead. Section 7.2 supersedes height shrinking with RAM-aware selection of verified square sizes or an explicit manual geometry; fail before image allocations that cannot fit. On Windows also stop if measured process peak working set exceeds the budget. On other platforms RSS remains unavailable, with allocation planning still enforced.
- For >=5 valid channel samples, compute unweighted median and 1.4826 MAD, winsorize a scratch copy to the configured low/high sigma bounds, repeat up to three times, then reject excursions from the original samples and average original survivors. Use a 1e-6 source-ADU sigma floor for constant/quantized populations. With <5 samples bypass rejection. Empty coverage produces zero. Use f64 Neumaier sums for weighted numerator, denominator and coverage; intermediate image samples remain f32.
- Keep reconstruction and resampling excursions internally; bound only the final normalized FITS to 0..1 under ADR-014. Apply photometric scale/ADU offset once before rejection. No BIAS or dark subtraction. Master EXPTIME is the anchor exposure, NCOMBINE is the accepted frame count, TEXPTIME is accepted acquisition total. CROPX/CROPY locate output (0,0) in anchor coordinates; they are not celestial WCS. Copy valid anchor GPS into FITS only, with provenance history; redact from JSON/console.
- Scientific checks distinguish M4 integration flux preservation relative to the accepted M1 VNG reconstruction from absolute analytic CFA-to-RGB profile accuracy. The former <=1% gate passes; the latter has a documented narrow colored-star counterexample and remains unestablished (see M4_TEST_PLAN.md). Never label the counterexample an absolute photometric pass. Real PI comparison thresholds are defined before measurements and remain unchanged. Owner visual approval and full-dataset reference parity are required before G4 sign-off.

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
    fulgur-gpu/       # Optional M5 bounded cosmetics/VNG backend
```
*(`fulgur-gpu` is the first M5 slice; `fulgur-gui` remains later M5 work.)*

### 5.2 CLI Commands (`fulgur-cli`)
```bash
# 1. Fast header survey & quality inspection (outputs JSON/CSV report)
fulgurstack inspect --input "D:/Astro/M33" --report quality.json

# 2. Full end-to-end stack (Inspect -> Register -> Integrate -> FITS)
fulgurstack stack \
  --input "D:/Astro/M33_night1" \
  --input "D:/Astro/M33_night2" \
  --canvas reference \
  --min-coverage 0.80 \
  --sigma-low 3.0 --sigma-high 2.5 \
  --memory-budget-mib 12288 \
  --output "D:/Astro/M33_master.fits" \
  --report "D:/Astro/M33_run_report.json"
```

---

## 6. Streamlined Implementation Plan (5 Milestones)

Execute strictly in order. Each milestone must compile cleanly (`cargo test`, `cargo clippy`) and pass its verification gate on both synthetic unit tests and the real `DS-A` dataset (`FULGURSTACK_TEST_FITS_DIR`)[cite: 1].

### Milestone 1 (M1): Workspace, FITS I/O, CFA Cosmetic + Demosaic
- **Deliver:**
  - Workspace setup (`fulgur-core`, `fulgur-pipeline`, `fulgur-cli`).
  - Pure-Rust FITS reader (`BITPIX=16`, Big-Endian + `BZERO`, header preservation, privacy redaction for `SITELAT`/`SITELONG`) and atomic 32-bit float RGB FITS writer (`BITPIX=-32`)[cite: 1].
  - Conservative same-channel CFA hot-pixel filter, 5x5 VNG default and the retained MHC diagnostic baseline.
  - CLI command `fulgurstack debayer-single --input <sample.fit> --output <test_rgb.fits>` + dataset header/I/O throughput scanner (`fulgurstack survey`).
- **Acceptance Gate (G1):**
  - Unit tests pass for Big-Endian `i16` decoding and MHC color reconstruction.
  - Single-frame `test_rgb.fits` from `Light_M 33_10.0s_IRCUT_20261006-204534.fit` opens in PixInsight with correct orientation, valid RGB colors, no hot-pixel crosses, and zero `BIAS=2064` subtraction[cite: 1].
  - Cosmetic correction must not introduce dark holes into spatially supported bright/saturated star cores. The owner-reported failure of the earlier normalized export requires a fresh visual review after the stellar-support fix.
  - Reconstruction must not introduce the reported isolated green/blue satellites near bright stars. Verify a failing neighborhood, native-sample preservation, color/CFA phases, common gain/offset behavior, quarter-turn symmetry, pre-export excursions, final 0..1 saturation and a numerical comparison of the reported star pair. These checks do not replace renewed owner visual approval.
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
  - `IntegrationEngine` trait implementation using spatial output tiles (`2160 × 128`), ROI inverse bounding-box extraction, hot-pixel cleaning, the accepted demosaicing method (current candidate VNG), Bicubic Keys inverse warp with strict validity masking, photometric normalization, iterative Winsorized Sigma Clipping, and `f64` Neumaier weighted accumulation[cite: 1].
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

## 7. M5 Start: Profile-Driven GPU Reconstruction (Owner-Authorized 2026-10-22)

M4 is owner-accepted after the 632.991 s / 990.1 MiB full external-reference run and visual review. Start M5 with GPU acceleration only; GUI and GPU rejection/reduction are later slices. The first hybrid IntegrationEngine accelerates cosmetics and VNG on bounded ROI batches, retaining CPU f64 inverse geometry/photometric normalization, existing Keys sampling, rejection, maps and Neumaier accumulation. It targets the measured 442.753 s reconstruction phase (70.09% of processing), not the fast inspection or final writer.

- Default engine remains the approved CPU path. `stack --engine gpu` requires the optional `gpu` build feature; unavailable/unsupported GPU must fail clearly rather than silently produce a CPU benchmark. Report hardware adapter/backend and actual accelerated stages. No software adapter may be presented as GPU acceleration.
- Share the existing CPU-generated VNG direction coefficients with GPU WGSL. Intermediate source/cleaned/RGB storage is f32, never f16; raw CFA decoding remains unchanged. Batch at most 16 ROIs, split against actual storage/dispatch limits, reuse bounded buffers and include conservative GPU/readback memory in allocation planning. No full dataset/frame cache or image intermediates; Section 7.3 permits only bounded, on-demand original-byte FITS blocks.
- Include GPU initialization, upload, dispatch/wait/readback and CPU work in end-to-end measurements. Record reserved device buffers separately from process working set; overlapping task timers are not CPU or GPU execution percentages. State device/driver and cache/run scope. No speed claim before measured parity/performance gates.
- Initial T-NUM-01 gates, fixed before measurement: stage reconstruction max absolute difference <=0.05 source ADU; native post-cosmetic samples and correction counts must agree; image finiteness/phase/orientation/halos must pass. End-to-end CPU/GPU normalized-master RMS <=1e-5, maximum <=1e-3, isolated-star aperture flux difference <=0.1%, no quality/registration-selection differences. Report rejection-map count differences explicitly; f32 shader rounding need not be byte-identical. Preserve existing <=1% integration-relative synthetic flux gates and disclosed stronger analytic limitation.
- Compile/lint/test default CPU and all-feature builds. GPU-independent shader syntax/validation must run in CI; actual hardware parity is explicitly required locally via FULGURSTACK_TEST_GPU=1 and may not silently skip when requested. Validate real DS-B native and 200% templates before promoting GPU output for owner review. Keep full DS-A scaling evidence distinct from the first DS-B GPU benchmark.

### 7.1 Owner-Authorized Tile Geometry Optimization (2026-10-22)

The measured and verified M5 preferred geometry is **512 x 512 output pixels**, now selected subject to available RAM by Section 7.2, with `--tile-width` (1..1024) and `--tile-height` (1..512) manual overrides. The old geometry is explicitly reproducible with `--tile-width 256 --tile-height 128`. Preserve the same 12288 MiB ceiling and conservative allocation planning; report requested/effective geometry separately. Section 7.2 supersedes automatic height shrinking. Never crop or change the reference canvas to fit a tile, cache whole registered datasets, reduce precision or change pixel arithmetic.

Selection used 26 DS-B search runs across 11 shapes, then native/200% exact before/after FITS checks for both CPU and GPU. Full DS-A CPU verification passed exact master/map equality to both the new control and owner-accepted M4 products, with source integrity and scientific counts unchanged. Same-binary full processing was 676.741 s at 256x128 and 558.198 s at 512x512; reconstruction 481.696/367.423 s; peak RSS 993.746/3412.633 MiB, below the 12 GiB ceiling. This is one measured sequential pair on the reference machine, not a universal optimum or GPU speedup claim. See [tile geometry review](docs/M5_TILE_GEOMETRY_REVIEW.md).

### 7.2 Owner-Authorized Available-RAM Tile Selection (2026-10-22)

Default `stack` (or `--tile-size auto`) measures total/available physical RAM immediately before integration, after registration and before allocating the three final image buffers. Reserve the greater of 2 GiB and 10% of total physical RAM (rounded up). The effective allocation budget is `min(configured ceiling, available RAM minus system reserve)`, floored at zero. Do not count swap as available RAM or add the process's existing allocations back to the available measurement. Keep the existing conservative 768 MiB pipeline-overhead model in addition to the system reserve. Account for accepted source count, all three final buffers, worker count, inverse reference scale and CPU/GPU ROI storage.

Choose the first fitting square in the measured preference order 512x512, 256x256, 128x128, capped only at canvas edges. The owner's 265x256 entry is interpreted as the previously tested 256x256. No new shape sweep or full 1,140-frame benchmark is required for this policy change. Fail clearly if none fits or if physical RAM cannot be measured in automatic mode; never crop, reduce precision or alter scientific arithmetic to fit.

`--tile-size 128|256|512` selects a manual square. Existing width/height overrides select a manual rectangle; an omitted companion dimension defaults to 512. The square selector and rectangular controls are mutually exclusive. Manual geometry also respects measured available RAM minus reserve and the configured ceiling, but is never silently shrunk. When OS measurement is unavailable, manual mode explicitly reports configured-ceiling-only enforcement. Keep configured peak-working-set enforcement on Windows. The available-RAM snapshot is a conservative starting allocation decision, not a reservation against other applications consuming memory later. Tile dimensions remain fixed for the run; dynamic resizing is outside this request.

Record the mode, RAM snapshot, system reserve, configured/effective ceilings, all three candidate estimates and selected dimensions in `integration.memory_plan`, and print the selection before image allocation. Keep requested dimensions in config and actual canvas-capped dimensions in integration statistics. No observing coordinates enter this telemetry. Use memory-only system queries through pure-Rust sysinfo 0.33.1 (system feature only, MSRV 1.74, no external build runtime) for Windows/macOS support. Verification covers selection thresholds, reserve/ceiling precedence, insufficient/unknown RAM, manual refusal, GPU/scale estimates, overflow handling, CLI conflicts and unchanged scientific outputs; prior geometry/performance evidence remains valid.

### 7.3 Owner-Authorized Bounded Original FITS Block Cache (2026-10-22)

The owner authorizes an on-demand cache of immutable original FITS bytes up to 1 GiB, exact 32-frame master/map verification and a full 1,140-frame same-binary comparison against RegisteringTemplate200pct.fit without cropping. This is an explicit exception to the earlier general cache prohibition only for bounded original-byte blocks, never calibrated, decoded-float, reconstructed or registered frame caches. No intermediate images or source writes.

`stack --fits-cache-mib` accepts 0..1024, default 0 after the full-set performance comparison; 0 retains the direct reader for performance controls. Select tile geometry first using Section 7.2, then size the cache from the remaining effective budget after conservative integration and transient-read buffers. Cache must not force a smaller tile. Unknown available RAM disables the cache even in manual tile mode. Insufficient space disables it without changing output. Include accounted capacity and transient buffers in planned allocation; preserve the system reserve and 12 GiB ceiling.

Cache 128x128 original signed-16 big-endian byte blocks. Split capacity into independent source quotas with LRU eviction, preventing a short whole-frame LRU from thrashing across 940 sources and avoiding a global read lock. Read adjacent missing blocks together by source row; do not read large row gaps or preload whole frames. Account for payload, conservative entry/map overhead and source state within the cap. Decode only requested ROI samples using exactly the current signed-i16+BZERO calculation. Keep original ROI dimensions, Bayer phase, reflection/halo, cosmetics, VNG, geometry, clipping, maps and f64 accumulation unchanged.

Open files read-only on misses and close them per ROI; all-hit requests avoid open/read calls. Check source size and modification timestamp even on all-hit requests; fail if changed. No observing coordinates or paths enter cache telemetry. Report requested/effective capacity, retained payload/accounted bytes, conservative source-peak sum, hits/misses/evictions, requested/hit ROI bytes, successful source-read bytes, read calls, opens and metadata checks. These are application I/O counters, not physical SSD traffic or OS-cache misses. Existing ROI decode counts remain scientific workload counters independent of block prefetch.

Quality requires entire master/coverage/high-rejection FITS byte equality, unchanged selection, registration, photometry, exposure and rejection/export counts, source/reference hashes, no image intermediates and no temporary leftovers. Compare cache 0 and up-to-1024 MiB with the same release executable, 16 workers, fixed 512x512 tiles, 3.0/2.5 clipping, 12 GiB ceiling, same reference grid and sequential runs. Retain unfavorable measurements. Record capacity reduction if available RAM differs; claim only observed end-to-end differences, not isolated causality or portable speedup. Default/all-feature lint/release gates and locally required real GPU parity must pass. M5 remains open.

### 7.4 Owner-Authorized Exact CPU Resampling Optimization (2026-10-22)

Keep FITS caching disabled by default and explicitly set --fits-cache-mib 0 in the before/after performance controls. Optimize only redundant CPU inverse-warp work and safe contiguous 4x4 footprint access. Eliminate redundant floor on valid positive coordinates and select the unchanged Keys polynomial by known footprint position, verified bit-for-bit at integer/adjacent-representable boundaries. Reuse inverse rotation and f64 per-column/per-row products while preserving the original arithmetic order, exact Keys weights, sixteen row-major f32 contributions, strict validity masks, scalar photometry, final saturation and rejection/Neumaier/map behavior. Never step coordinates, reassociate interpolation sums, approximate coefficients or reduce precision. Temporary geometry storage is bounded by tile width (16 KiB at the maximum width per worker), covered by the existing runtime reserve; it contains no image pixels.

Require differential bit/mask gates against the retained prior scalar implementation, default/all-feature release/lint checks including locally required GPU hardware parity, entire DS-B master/map byte equality, immutable inputs and fresh paired full DS-A measurement against RegisteringTemplate200pct.fit without cropping. Fix 16 CPU workers, 512x512 tiles, 12 GiB ceiling and all scientific settings; retain the exact prior release executable and record both binary hashes. Ignore only explicitly permitted OS available-RAM snapshots/derived headroom when comparing otherwise strict reports. Keep total/reserved RAM, chosen geometry, allocation estimates, cache-off settings/counters and all scientific data strict. Report processing/reconstruction wall time separately from summed overlapping resampling task elapsed time; retain any unfavorable result. See ADR-026. This is an M5 optimization slice, not closure of all M5 deliverables.


Validation completed: exact DS-B and full DS-A master/map/source gates PASS, including equality to owner-accepted products. Retain the exact CPU optimization with cache default 0. Fresh full processing 523.466/501.305 s, reconstruction 344.081/329.472 s, peak RSS 3409.789/3416.391 MiB. Summed overlapping resampling task elapsed fell 6.091%; it is not additive wall or CPU execution time. Existing OS caches/load are uncontrolled and 6.094 s of total variation is in unchanged inspection/registration, so no isolated causal or universal speedup is claimed. Keep the slower preliminary DS-B result. Default/all-feature release gates and Windows/macOS CI pass. See [CPU resampling review](docs/M5_CPU_RESAMPLING_REVIEW.md).

### 7.5 Owner-Authorized Exact CPU VNG Optimization (2026-10-22)

Optimize CPU VNG with cache default/benchmark setting 0. Generate four constant-index kernels at Cargo build time from the same independent geometric stencil factory used for optional GPU packing. Preserve exact f32 coefficient bits, gradient and sample contribution order, tie-inclusive threshold, selected directions, division and native-sample assignment. Select row phase outside the pixel loop. Load interior 5x5 neighborhoods directly, reflect y once per row and retain the exact reflected x border path at the first/last two columns. Preserve minimum/odd dimensions, CFA phases, ROI halos, validation and cancellation. No new runtime/build dependency, precision change, image cache/intermediate or scientific policy change.

Require the complete prior scalar VNG as an independent test-only bit oracle, all phase/size/field/bright-star gates and real source equality before/after cosmetics, exact original GPU coefficient words and locally required hardware parity. Preserve existing scientific gates and limitations. After default/all-feature lint/release verification, require entire DS-B master/map byte equality, then a fresh sequential full DS-A CPU pair against RegisteringTemplate200pct.fit without cropping, fixed 16 workers/512-square tiles/12288 MiB/cache 0, identical accepted inputs and source hashes. Record both binary hashes and separate full/reconstruction wall from overlapping task elapsed. Do not replace fresh controls with historical timings or claim a portable speedup. See ADR-027; M5 remains open.


VNG validation completed: exact 32/full-1140 master/map/source gates PASS, including owner-accepted CPU/M4 products. Keep the exact optimization with cache 0. Fresh full processing 510.694/438.435 s (14.149% shorter), reconstruction 334.964/260.558 s (22.213% shorter), peak RSS 3422.172/3404.977 MiB. Summed overlapping VNG task elapsed is 61.010% shorter, not additive wall/CPU time. Retain unmodified-phase variation, single-pair scope, existing OS caches/load and prior quality limitations; no universal or isolated causal speedup claim. Default/all-feature release tests, required local GPU and Windows/macOS CI pass. See [CPU VNG review](docs/M5_CPU_VNG_REVIEW.md).

### 7.6 Owner-Authorized Exact Rejection Data Layout Optimization (2026-10-22)

Keep FITS cache default/benchmark setting 0 and the existing registered frame-major output tile. For rejection, pack at most 32 neighboring output pixels into contiguous per-pixel RGB stacks in original source order. Reuse this bounded working block and existing clipping/value scratch across a band; never duplicate a full registered tile or retain source data between tiles. Limit concurrent reduction bands to at most 16 even in a larger Rayon pool. Account for 32 * accepted_sources * workers * 12 additional bytes in every candidate/manual tile estimate, retaining the system reserve and 12 GiB ceiling. Report layout, block size, band cap and packing capacity explicitly.

Preserve exact f32 sample bits, independent channel validity, geometric red-channel weights, source order, median/MAD/clipping, f64 Neumaier sums, map denominators, export bounds and all image geometry. Require differential exact master/map/statistics gates against the prior direct reducer, immutable inputs and cancellation plus all existing default/all-feature/local GPU gates. Verify entire DS-B master/map bytes before a fresh sequential full 1,140-frame CPU pair against RegisteringTemplate200pct.fit without cropping, 16 workers/512-square tiles/12288 MiB/cache 0. Allow only explicitly validated layout telemetry and its precise bounded scratch estimate delta, timing/RSS and available-RAM snapshots to differ; science, geometry, cache-off and work/I/O stay strict. Retain unfavorable evidence and single-pair limitations. See ADR-028; M5 remains open.


Layout validation completed: exact 32/full-1140 master/map/source gates PASS, including accepted CPU/M4 products. The full fresh CPU pair measured 437.471/430.546 s processing (1.583% shorter), rejection/maps 124.304/118.462 s (4.699% shorter), peak RSS 3405.273/3419.832 MiB. Retain this exact implementation with explicitly planned 5,775,360-byte full-set packing bound, cache 0 and unchanged scientific behavior. Keep the small-set 5.130% rejection slowdown, higher RSS, unmodified-phase variation and single-pair/OS-cache/load qualifications. Default/all-feature/local-GPU and Windows/macOS CI pass. See [rejection layout review](docs/M5_REJECTION_LAYOUT_REVIEW.md); M5 remains open.
