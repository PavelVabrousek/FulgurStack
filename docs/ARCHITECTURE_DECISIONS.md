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
