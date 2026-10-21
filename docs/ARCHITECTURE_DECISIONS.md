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

**Status:** Implemented and verified on Windows; owner visual acceptance pending.

Use a bounded pure-Rust primary-image FITS parser with immutable buffered reads and signed big-endian decoding. Retain original header cards privately; expose only an explicit metadata whitelist through reports and Debug. M1 supports 16-bit primary CFA images, BSCALE=1, BZERO=0 or 32768, valid positive EXPTIME, and four standard Bayer phases. Reject BLANK pixels, nonzero Bayer offsets, malformed/truncated arrays, and unsupported metadata explicitly rather than silently guessing. Limit raw payloads to 512 MiB for the M1 full-frame path.

Write planar RGB big-endian float FITS in source ADU without stretching, clipping, dark/BIAS subtraction, or copying stale CFA/scaling/WCS/checksum cards. Preserve observation metadata and valid reference GPS. Use `tempfile` to create exactly `<output>.tmp` exclusively, flush/sync it, and publish with no-clobber persistence. Existing destinations, hardlink aliases, and stale temp files are protected. On supported systems no-clobber persistence may use a link operation rather than a replacing rename; atomic visibility and source protection take precedence over replacing any existing file. Ctrl-C/SIGTERM cancellation unwinds and removes owned incomplete output.

References: [FITS data representation](https://fits.gsfc.nasa.gov/standard30/htmlfiles/fits_standard3000047.html), [FITS integer offsets and floating-point data](https://fits.gsfc.nasa.gov/standard30/htmlfiles/fits_standard3000048.html), and [`tempfile` no-clobber persistence](https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html#method.persist_noclobber).

## ADR-008: M1 Reconstruction and Conservative Cosmetics (2026-10-21)

**Status:** Implemented; synthetic gates passed and real export generated.

Implement the published MHC equations independently in Rust with Rayon, preserving measured CFA samples exactly after optional cosmetic correction. Reflect sensor boundaries without changing CFA parity. Retain negative/overshoot interpolation values in f32 rather than clipping them; FITS output remains linear in physical ADU. PixInsight may require its floating-point range rescale option on import.

A five-sigma rule based on eight noisy neighbors alone altered 92,175 samples on the first real frame. Confirm candidates using a 24-neighbor same-phase MAD estimate and preserve spatially supported stars; the final real frame changes 5,140 samples (0.062%). The synthetic Gaussian-noise gate limits false corrections to less than 0.1%, and a separate undersampled-star test protects the peak. Cosmetics leaves the outer four sensor rows/columns unchanged. Combined cosmetic/MHC/bicubic support requires an eight-pixel raw halo in future integration, recorded in the specification without implementing M2-M4.

References: [original MHC paper](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/Demosaicing_ICASSP04.pdf) and [PixInsight floating-point FITS range handling](https://pixinsight.com/forum/index.php?threads/scaling-of-fits-on-open.626/). The implementation does not copy the separately distributed IPOL C source.

## ADR-009: Bounded Survey and Telemetry (2026-10-21)

**Status:** Implemented; DS-A and DS-B release I/O runs passed.

Use bounded per-worker read buffers, not decoded full images, for `survey`. CPU workers default to available parallelism capped at 16, with a configurable limit. Report effective full-file throughput with uncontrolled OS cache state; do not present cache-assisted throughput as isolated SSD hardware speed. On Windows, query the process peak working set through generated Rust Windows API bindings; report null on unsupported telemetry platforms rather than estimating RAM. Dependencies require no separately installed C/C++ image libraries.
