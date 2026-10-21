# FulgurStack Architecture Decisions

## ADR-001: Product Identity and Language (2026-10-21)

**Status:** Accepted by owner instruction.

Use FulgurStack as the product name, `fulgurstack` as the executable, `fulgur-core`, `fulgur-pipeline`, and `fulgur-cli` as the initial workspace crates, and `FULGURSTACK_TEST_FITS` / `FULGURSTACK_TEST_FITS_DIR` for optional real-data test inputs. No compatibility aliases are required because no implementation exists yet. All artifacts are English; owner-facing communication is Czech.

## ADR-002: Mixed Exposure Durations (2026-10-21)

**Status:** Required by owner; implementation pending within M1-M4.

First-release stacks support one target and filter with different positive exposure durations and compatible camera settings. Keep output intensities on the reference anchor's scale. Matched unsaturated star fluxes determine normalization; derive atmospheric transparency from exposure-corrected flux ratios. Weight normalized samples using noise in those same units, with the specified HFR factor, without applying exposure or transparency twice.

M2 weights remain explicitly provisional until M3 provides star matches. Star-count rejection must account for exposure-duration groups. Synthetic tests cover mixed durations, invalid exposure, saturation, and normalized flux preservation; the available real dataset covers 10 s only. The output distinguishes reference exposure, accepted total acquisition exposure, and accepted frame count. Coverage and per-pixel clipping prevent interpreting the total acquisition exposure as uniform effective exposure.

## ADR-003: GPS Preservation and Provenance (2026-10-21)

**Status:** GPS retention requested by owner; reference-anchor selection is an engineering decision.

Preserve valid GPS cards from the chosen reference anchor in the final FITS, including single-frame exports. FITS history identifies them as reference-anchor coordinates; they do not imply that all sessions share a location. Do not average sites or invent coordinates. Coordinates remain excluded from console output, default logs, and default reports.

## ADR-004: Reference Dataset and Benchmark Evidence (2026-10-21)

**Status:** Header inventory verified; performance and image quality unverified.

The owner-provided DS-A directory contains 1,140 compatible 10 s M33/IRCUT FITS files, rather than the inherited 1,150-frame estimate. The owner-provided immutable XISF reference is RGB Float32 at the input sensor dimensions and declares 11,400 s acquisition exposure per channel, consistent with the input exposure sum. XISF is an external evaluation artifact; native XISF support is outside the first release.

The owner confirmed on 2026-10-21 that approximately three hours represents the entire PixInsight processing workflow. The exact reference input selection is not yet verified. Keep the <=18 min target and qualify speedup against this baseline as approximate. Acquisition exposure metadata must never be used as processing duration. Compare equivalent input selections and processing scopes. No performance or quality acceptance gate has passed from header inspection alone.

## ADR-005: Small Regression Dataset and PI Comparison (2026-10-21)

**Status:** Dataset header inventory and SHA-256 manifest recorded; timing/settings supplied by owner; image quality unverified.

Use project-local DS-B (32 source FITS frames) for quick milestone regressions and the supplied PI debayered, registered, and integrated images as immutable reference artifacts. Retain DS-A as the complete performance and quality gate. The sum of owner-reported DS-B stage timings is approximately 115 s; do not extrapolate this linearly to DS-A.

The PI integration uses ESD rejection and PSF Signal Weight, while FulgurStack retains the specified Winsorized Sigma Clipping and quality weighting. Compare image quality in compatible geometry and intensity units, document input-set differences, and do not require bitwise identity. Treat the provided PI memory allocations as separate diagnostics, not measured peak RSS. Exact paths, configuration, timing limitations, and milestone usage are recorded in `docs/BENCHMARKS.md`; the source and FITS reference fingerprint inventory is `docs/ds-b-manifest.json`.

## ADR-006: Isolated Development Environment Validation (2026-10-21)

**Status:** Windows environment verified; application implementation pending.

Validate the owner-installed Windows MSVC Rust toolchain using a dependency-free scratch package under the ignored `target/` directory. This checks formatting, lint tooling, release compilation, linking, and executable startup without introducing a placeholder product workspace. The scratch harness has no product tests and does not satisfy any M1 gate. Product workspace creation, meaningful FITS/image tests, and cross-platform CI remain part of M1. DS-B source/reference fingerprints were independently rechecked after the smoke run.

## ADR-007: M1 FITS I/O and Atomic Publication (2026-10-21)

**Status:** Implemented and verified on Windows; owner visual acceptance pending. The original ADU export scale is superseded by ADR-010.

Use a bounded pure-Rust primary-image FITS parser with immutable buffered reads and signed big-endian decoding. Retain original header cards privately; expose only an explicit metadata whitelist through reports and Debug. M1 supports 16-bit primary CFA images, BSCALE=1, BZERO=0 or 32768, valid positive EXPTIME, and four standard Bayer phases. Reject BLANK pixels, nonzero Bayer offsets, malformed/truncated arrays, and unsupported metadata explicitly rather than silently guessing. Limit raw payloads to 512 MiB for the M1 full-frame path.

Write planar RGB big-endian float FITS in source ADU without stretching, clipping, dark/BIAS subtraction, or copying stale CFA/scaling/WCS/checksum cards. Preserve observation metadata and valid reference GPS. Use `tempfile` to create exactly `<output>.tmp` exclusively, flush/sync it, and publish with no-clobber persistence. Existing destinations, hardlink aliases, and stale temp files are protected. On supported systems no-clobber persistence may use a link operation rather than a replacing rename; atomic visibility and source protection take precedence over replacing any existing file. Ctrl-C/SIGTERM cancellation unwinds and removes owned incomplete output.

References: [FITS data representation](https://fits.gsfc.nasa.gov/standard30/htmlfiles/fits_standard3000047.html), [FITS integer offsets and floating-point data](https://fits.gsfc.nasa.gov/standard30/htmlfiles/fits_standard3000048.html), and [`tempfile` no-clobber persistence](https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html#method.persist_noclobber).

## ADR-008: M1 Reconstruction and Conservative Cosmetics (2026-10-21)

**Status:** Implemented; synthetic gates passed and real export generated. The original ADU export/import advice is superseded by ADR-010; reconstruction remains in ADU.

Implement the published MHC equations independently in Rust with Rayon, preserving measured CFA samples exactly after optional cosmetic correction. Reflect sensor boundaries without changing CFA parity. Retain negative/overshoot interpolation values in f32 rather than clipping them; FITS output remains linear in physical ADU. PixInsight may require its floating-point range rescale option on import.

A five-sigma rule based on eight noisy neighbors alone altered 92,175 samples on the first real frame. Confirm candidates using a 24-neighbor same-phase MAD estimate and preserve spatially supported stars; the final real frame changes 5,140 samples (0.062%). The synthetic Gaussian-noise gate limits false corrections to less than 0.1%, and a separate undersampled-star test protects the peak. Cosmetics leaves the outer four sensor rows/columns unchanged. Combined cosmetic/MHC/bicubic support requires an eight-pixel raw halo in future integration, recorded in the specification without implementing M2-M4.

References: [original MHC paper](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/Demosaicing_ICASSP04.pdf) and [PixInsight floating-point FITS range handling](https://pixinsight.com/forum/index.php?threads/scaling-of-fits-on-open.626/). The implementation does not copy the separately distributed IPOL C source.

## ADR-009: Bounded Survey and Telemetry (2026-10-21)

**Status:** Implemented; DS-A and DS-B release I/O runs passed.

Use bounded per-worker read buffers, not decoded full images, for `survey`. CPU workers default to available parallelism capped at 16, with a configurable limit. Report effective full-file throughput with uncontrolled OS cache state; do not present cache-assisted throughput as isolated SSD hardware speed. On Windows, query the process peak working set through generated Rust Windows API bindings; report null on unsupported telemetry platforms rather than estimating RAM. Dependencies require no separately installed C/C++ image libraries.

## ADR-010: Fixed Linear Export and Explicit PI Import (2026-10-21)

**Status:** Implemented for the owner's direct-comparison request; numerical gates passed, native PI and owner visual verification pending.

Keep reconstruction in source ADU, but divide each exported sample by the fixed 65,535 sensor full scale. Use the same divisor across frames/channels, without pedestal, per-image extrema normalization, stretch, or clipping. The supplied PI single-frame reference already uses these units. `FLXDIV=65535`, `BUNIT='adu/65535'`, and history describe the representation; `FLXDIV` does not instruct PI to rescale. Preserve GPS and observation cards. Report schema 2 adds the divisor, zero offset, output extrema, preserved out-of-range counts, and zero clipped samples.

Simply dividing the old export is insufficient for ordinary PI File > Open: 66 negative samples and 48 samples above one remain in this MHC reconstruction. PI's default FITS policy can therefore rescale the entire image again. Clipping those samples or adding an image-dependent offset would violate the owner's requirements. FITS has no supported header that overrides PI's range policy. Supply `scripts/OpenFulgurStack.js`, using the documented `ImageWindow.open` format-hints argument with `ignore-out-of-range`, `top-down`, `verbosity 0`, and a fresh image copy. It performs no pixel arithmetic, changes no global preferences, and rejects legacy ADU exports without the new marker. The CLI remains independent of PixInsight; no native XISF feature or FulgurStack GUI is added.

Verified all 24,883,200 new output samples equal the old reconstruction divided by 65,535 at Float32 precision, including all 114 out-of-range samples. Reference-scale signed median residuals against PI VNG are zero in all three channels. Different interpolation/cosmetic methods still require quality review. Import-adapter tests use host API doubles; they do not claim native PI execution.

References: [official FITS module range-policy implementation](https://gitlab.com/pixinsight/PCL/-/raw/master/src/modules/file-formats/FITS/FITSInstance.cpp), and the installed PixInsight `doc/pjsr/objects/ImageWindow/ImageWindow.html` documentation for the `ImageWindow.open(url, imageId, formatHints, copy)` signature. The range-policy implementation ignores the fix mode only with `ignore-out-of-range`; it otherwise rescales or truncates even after fixed-scale export.

## ADR-011: Preserve Bright Stellar Cores During Cosmetics (2026-10-21)

**Status:** Owner rejected the prior normalized export because of dark stellar-core dots. Fix implemented; automatic and real-data numerical gates passed; renewed visual review pending. Supersedes the MAD-only neighbor support rule in ADR-008.

The former support guard interpreted a star's spatial intensity slope as neighbor noise. It therefore failed to protect some genuine CFA samples before MHC reconstruction. For example, source/reference 65,535 ADU at a saturated red core became 11,403.5 ADU; this is a destructive cosmetic error, not an expected MHC/VNG difference or an export-range issue.

Keep the original five-sigma candidate checks and the two-neighbor spatial support requirement, but cap each neighbor's support threshold at one eighth of the candidate's excess above its same-phase median. Use `min(3 * neighbor_noise, candidate_excess / 8)` above the neighbor's same-phase median. The cap is a conservative morphological safeguard; it is not a new noise estimate or a measured probability. It preserves coherent stellar profiles when local MAD is inflated by gradients or saturation. Flat-sky isolated defects still lack spatial support. Ambiguous defects may remain near stars; avoiding stellar flux loss takes priority over aggressive single-frame cleaning. CFA support/halo, MHC equations, export scale and unclipped output remain unchanged.

Regression coverage includes the anonymized 9x9 intensity neighborhood that reproduced the real failure, 60 analytic colored-star cases spanning five PSF widths, four amplitudes including saturation and three subpixel positions, the existing undersampled-star test, ordinary Gaussian noise, and isolated-hot-pixel suppression. The final real export changes 2,178 candidates instead of 5,140. It restores 2,962 native samples, including all 71 previously changed source samples above 10,000 ADU. All 42 saturated source samples survive. This threshold/count observation is specific to DS-B's selected frame, not a claim that every restored sample is a proven star.

Prepared review output: `outputs/M1_M33_star_safe.fits`. Independent evidence: `outputs/M1_star_safe_validation.json`. All source/reference fingerprints and previous exports are unchanged. M1 remains unapproved; no M2 work is authorized by these numerical checks.

## ADR-012: Replace the Rejected MHC Default with Unclipped VNG (2026-10-21)

**Status:** Engineering revision in response to the owner's second stellar-artifact rejection. VNG is the new M1 default/review candidate; owner visual approval remains pending. Supersedes the default-MHC choice in ADR-008 and specification v2.5. MHC remains explicitly selectable for reproducible diagnostics.

The second screenshot shows colored satellites after the core-erasure fix. Numerical evaluation reproduces them with MHC applied directly to untouched raw CFA: they arise from fixed cross-channel Laplacian corrections, independently of cosmetics or export scaling. A satellite near the reported red star has native R 4,858 ADU, but MHC estimates G -338.5 and B -3,162.625 ADU. Merely preserving bright native cores therefore did not establish acceptable reconstructed color. Do not conceal this failure by clamping samples, painting stars or applying a display stretch.

Implement threshold-based VNG independently in Rust/Rayon using geometric directional stencils rather than another implementation's lookup tables or code. Use four cardinal and four diagonal gradients on a 5x5 CFA window, threshold `min + max/2`, directional per-color means, and missing-channel color differences relative to the measured channel. Diagonal stencils are symmetric under quarter turns. Preserve the native sample exactly after unchanged conservative cosmetics. Remain in f32 and retain all negative/above-full-scale interpolation excursions. Border reflection keeps CFA parity and the existing eight-pixel composed support for future tiling.

Default `debayer-single` to `--demosaic vng`; keep `--demosaic mhc` as the rejected comparison baseline. The output carries the actual method in `DEBAYER`, FITS history and JSON `demosaic_method`. This adaptive reconstruction is not a linear operator on arbitrary images; its output is linear-light in the same fixed ADU/65,535 units, with no gamma or tone curve. Common gain/offset equivariance, unchanged native samples and unclipped excursions are tested explicitly. No new dependencies, intermediate images or later-milestone features are introduced.

The official PI source is useful for evaluating compatibility but is not the output contract: PI clamps reconstructed channels to 0..1 and its inspected non-green SE/SW diagonal formulas have unequal coefficients. FulgurStack uses symmetric coefficients and deliberately omits clipping. PI also uses a different border policy and the supplied reference has no equivalent cosmetic step. Bitwise identity is therefore neither promised nor required. The remaining 40 above-one samples in the real VNG export survive rather than being silently removed. Use the PI import adapter; its view IDs now include the output filename to avoid mixing review versions.

Validation: 40 Rust release tests and four adapter contract tests passed locally. New coverage includes all CFA phases, flat/fractional color, native samples, affine photometric equivariance, quarter-turn symmetry, a golden satellite neighborhood and overshoot from bounded sensor data. An independent Float64 prototype matches 1,024 sampled output positions within 0.000227 ADU; all 8,294,400 native samples match the prior conservative cleaning, including 42 saturated samples. In the reported red-star region, PI residual RMS falls from about 1,194 to 11.8 ADU. Its green aperture flux differs from PI by 0.024% and HFR by 0.039%; these are local diagnostics, not whole-field/stack quality approval. Whole-frame R/G/B residual RMS falls from 59.0/36.2/63.0 to 17.4/11.9/12.8 ADU. Residuals are method differences, not estimated additional noise.

Prepared output: `outputs/M1_M33_vng.fits`; evidence: `outputs/M1_vng_validation.json`. Release run: 0.463 s, 164.5 MiB peak working set. Source/reference fingerprints and all prior exports are unchanged. Numerical success does not override the owner's failed visual gates; the new candidate must be reviewed before M1 approval or M2.

References: [original MHC paper](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/Demosaicing_ICASSP04.pdf), [official PI VNG implementation and cited algorithm](https://gitlab.com/pixinsight/PCL/-/raw/master/src/modules/processes/contrib/spool/Debayer/DebayerInstance.cpp). Algorithm attribution: Edward Chang, Shiufun Cheung and Davis Y. Pan, *Color Filter Array Recovery Using a Threshold-Based Variable Number of Gradients* (1999). Directional arithmetic is formulated independently; no PI source tables/code or external C/C++ runtime are included.

## ADR-013: Prevent PixInsight Display Wrap Without Changing Scientific Samples (2026-10-21)

**Status: FAILED owner review; superseded by ADR-014.** The display STF did not remove cyan pixels. The following records the abandoned approach, not current instructions. Its transformed-copy test did not verify the actual viewport conversion path.

At x=1694, y=1409 the current VNG export and PI reference contain identical RGB values. Seven adjacent channel samples exceed one (six red, one blue). Their screenshot bytes exactly match rounded value times 255 modulo 256: for example R=1.04744029045 appears as 11 instead of 255. Permissive FITS import correctly preserves scientific data, but identity screen rendering can convert unbounded floats directly to bytes. Earlier API-double tests did not exercise this rendering path.

Enable a linked linear screen transfer through the native `ScreenTransferFunction` process: shadows 0, highlights `1 - 2^-20`, midtones 0.5, output range 0..1, and identity lightness. A highlight point slightly below one forces a non-identity clipping path before byte conversion; exactly one would retain the unsafe identity optimization. Display gain is less than one ppm. The process also enables STF when disabled and does not update pixel history. Do not apply a histogram process to the scientific view, clamp the export, introduce a pedestal, or normalize by frame extrema. Disabling/resetting STF can expose the upstream renderer behavior again; reopening through the adapter restores the guard.

Seven host/transfer-model tests cover the import contract, rejected formats, cancellation, process failure cleanup, transfer parameters and the seven measured overshoots. A separate native PJSR regression runs in a new PI automation instance without saved preference changes. All 24,883,200 loaded samples match a permissive baseline exactly, including all 40 above-one values and maximum 1.1716045141220093. Rendering a tiny in-memory display copy through PI's histogram/bitmap primitives reproduces all seven original wraps and gives corrected RGB bytes identical to the reference. This is native transfer verification, not automated capture of the actual viewport. Only JSON is saved; all created image windows are closed. The existing FITS SHA-256 remains `1298113d5582c9ff89a60ec69f3deac5e40347b73b7737a198115986c956b79a`.

References: [official STF process](https://gitlab.com/pixinsight/PCL/-/raw/master/src/modules/processes/IntensityTransformations/ScreenTransferFunctionInstance.cpp), installed `include/pcl/HistogramTransformation.h` and `include/pcl/PixelTraits.h`. The renderer's integer conversion and identity fast path are external PI behavior; this is a local interoperability workaround, not a modification of PixInsight itself.

## ADR-014: Owner-Approved Sensor Saturation in the FITS Output (2026-10-21)

**Status:** Implemented after explicit owner authorization. Supersedes ADR-010/012's unbounded final-export requirement and removes the failed ADR-013 workaround. M1/G1 owner visual acceptance remains pending.

The owner accepts limiting reconstructed colors to the normalized sensor range as compatible with the product's linearity/no-data-clipping requirement. This is a FulgurStack output contract, not a FITS standard limitation. PI VNG bounds interpolated channels to 0..1; the previous FulgurStack export deliberately retained 40 above-one channel values. A loading/display script did not eliminate the observed cyan pixels or satisfy ordinary opening.

Complete reconstruction at final export with `(source_adu / 65535).clamp(0, 1)` for both VNG and diagnostic MHC. Keep internal interpolation unbounded for diagnostics and never mutate the input RGB buffer. Supported Seestar native measurements are already within sensor range; only interpolated excursions change. Every in-range sample remains bit-identical. Fixed divisor, zero offset, common channel scale and the absence of a tone curve/extrema normalization remain unchanged. The operator has saturation at its limits: linear-light describes the approved product convention with sensor saturation, not global mathematical linearity through those limits.

Record `RGBBOUND='0:1'` and truthful FITS history. Report schema 3 distinguishes lower/upper limited reconstruction samples, their total `samples_clipped`, and final output out-of-range counts (zero). These count color channels, not rejected frames or cosmetic candidates. Preserve input files and valid GPS/observing metadata; keep coordinates private in diagnostics. Current FITS opens with File > Open; retain the importer only for legacy diagnostics and remove its failed STF guard.

Release: 0.503 s, 164.4 MiB peak working set, 16 threads. Candidate `outputs/M1_M33_vng_bounded.fits` has range 0.027237353846..1, 40 upper limits and no lower limits. All other 24,883,160 samples and all 8,294,400 native CFA samples match the previous export; all 42 saturated native samples survive. All manifest/reference/source hashes and GPS/date/exposure cards are unchanged; no temporary images remain. SHA-256: `d60369afde3915ebb3a46d1cd159e8e674f4ae351e7b06ed33ce09959118d411`.

Verification: 40 release Rust tests, warnings-denied Clippy/format checks and four legacy importer contracts pass locally. The new native PJSR regression uses ordinary import, identity STF and direct rendering of the actual bounded image, with no histogram process or pixel modification. All loaded samples match expected output; all seven reported stellar RGB triples and display bytes match PI. This avoids ADR-013's invalid inference from a transformed copy. The actual viewport still requires owner review. Only final FITS and JSON reports are saved; no M2 work is started.
