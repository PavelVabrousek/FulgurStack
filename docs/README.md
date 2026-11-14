# Documentation index — 2026-11-13

Start with [current delivery state](CURRENT_STATUS.md) for what is published, what is local and what awaits the owner, and with the [architecture overview](ARCHITECTURE.md) for how the code is organized. M1–M4 are owner-accepted; M5 remains open. Old measurements and FAIL outcomes are kept with their original scope; "local" statements inside dated reviews describe their measurement-time state.

Layout: current documents are in `docs/`, dated milestone reviews with their evidence are in `docs/reviews/`, and finished plans and old status logs are in `docs/archive/`.

## Current guides and contracts

| Document | Purpose |
| --- | --- |
| [Product overview](../README.md) | What FulgurStack does, launch, CLI examples and evidence summary |
| [Changelog](../CHANGELOG.md) | What changed in each version, with the decisions behind it |
| [Current state](CURRENT_STATUS.md) | Publication state, current behaviour, scientific gates, acceptance limits |
| [Open items](OPEN_ITEMS.md) | Owner decisions, pending reviews, unverified requirements, test gaps, planned clean-up |
| [Architecture overview](ARCHITECTURE.md) | Crates, data flow of a stack, where each decision family lives in the code |
| [Project summary](PROJECT_SUMMARY.md) | One-page English overview for other developers: principle and results against other programs |
| [Project specification](../PROJECT_SPEC.md) | Binding scientific requirements, with the owner amendments index at the top |
| [Engineering contract](../AGENTS.md) | Verification, data protection and batch publishing policy |
| [Architecture decisions](ARCHITECTURE_DECISIONS.md) | Decision log with an index; later decisions supersede earlier ones |
| [Development](DEVELOPMENT.md) | Workspace, prerequisites, build commands and local checks |
| [Automated testing](TESTING.md) | Quick and full test suites, inventory, requirement traceability, known defects, proposed tests |
| [Code review 2026-11-13](CODE_REVIEW_2026-11-13.md) | Findings and work packages for clean-up, refactoring and fixes |
| [Windows distribution](WINDOWS_DISTRIBUTION.md) | Portable ZIP contents, build and check procedure, current package hashes |
| [Benchmarks](BENCHMARKS.md) | Distinct measured scopes, immutable datasets and owner PI references |
| [Native Windows review](reviews/M5_WINDOWS_APP_REVIEW.md) | UI review entry point followed by dated UI history |
| [Auto reference](reviews/M5_AUTO_REFERENCE_REVIEW.md) | Default single/multi-night reference policy, approximation and measured limits |
| [Geometry acceptance repair](reviews/M5_GEOMETRIC_ACCEPTANCE_REPAIR.md) | Current whole-field validity and safe recovery rules |
| [Built-in deep-sky table](../crates/fulgur-catalog/data/README.md) | Sources, licence and rebuild of the object table used for field centres |
| [Icon assets](../crates/fulgur-gui/assets/README.md) | Approved artwork, exports and Windows identity |

## Experiment tools

The feasibility studies `skyreg` (ADR-131), `synthref` (ADR-127) and `blindspike` (ADR-168) are archived; see [tools/archive](../tools/archive/README.md).

## Archive (`archive/`)

| Document | Purpose |
| --- | --- |
| [Sky registration plan](archive/SKY_REGISTRATION_PLAN.md) | The plan of 2026-11-04 with the owner's decisions; completed as ADR-133–138 |
| [UI design history](archive/WINDOWS_UI_DESIGN.md) | Compact Desk summary and the original proposals |
| [Status history 2026-10-26 to 2026-11-09](archive/STATUS_HISTORY_2026-10-26_to_2026-11-09.md) | The dated delivery log and "Since ADR-…" list formerly in the status page |

## Dated milestone and optimization evidence (`reviews/`)

The reviews below are historical records, not current default/acceptance claims. In particular, the initial geometric correction and mixture diagnosis retain their failed product acceptance; the subsequent repair is required. Historical full pipeline timings, frozen integration timing and native catalog timing measure different work. Do not combine them into a new speedup or treat earlier test counts as current. Each review links back to current state.

- [M1: FITS Reconstruction and Single-Frame Export](reviews/M1_REVIEW.md)
- [M2: DS-B 32-Frame Manual Review](reviews/M2_DS-B_REVIEW.md)
- [M2: CPU Subframe Inspector and Quality Filtering](reviews/M2_REVIEW.md)
- [M3: DS-B Registration Review](reviews/M3_DS-B_REVIEW.md)
- [M3: Direct-to-Anchor Rigid Registration and Canvas Planning](reviews/M3_REVIEW.md)
- [M4 Full DS-A Reference-Canvas Run and Performance Profile](reviews/M4_FULL_RUN_PROFILE.md)
- [M4 Full DS-A Run with the 200% External Reference](reviews/M4_FULL_SCALED_RUN_REVIEW.md)
- [M4 Revision: Reference Geometry and Diagnostic Maps](reviews/M4_REFERENCE_MAPS_REVIEW.md)
- [M4: Tiled CPU Integration and PixInsight Master Review](reviews/M4_REVIEW.md)
- [M4 Revision: Scaled External Registration References](reviews/M4_SCALED_REFERENCE_REVIEW.md)
- [M4 Quality Comparison Plan](reviews/M4_TEST_PLAN.md)
- [M5: Exact CPU Cosmetic Correction Optimization](reviews/M5_CPU_COSMETIC_REVIEW.md)
- [M5: Exact SIMD CPU Cosmetic Correction](reviews/M5_CPU_COSMETIC_SIMD_REVIEW.md)
- [M5 CPU Optimization Batch](reviews/M5_CPU_OPTIMIZATION_SUMMARY.md)
- [DS-A performance log and ten optimization rounds, 2026-11-05](reviews/M5_PERFORMANCE_DSA_20261105.md)
- [M5: Exact Pixel-Rejection Optimization](reviews/M5_CPU_REJECTION_REVIEW.md)
- [M5: Exact CPU Resampling Optimization](reviews/M5_CPU_RESAMPLING_REVIEW.md)
- [M5: Continuous Bounded CPU Reconstruction Scheduling](reviews/M5_CPU_SCHEDULING_REVIEW.md)
- [M5: Exact SIMD CPU Resampling](reviews/M5_CPU_SIMD_REVIEW.md)
- [M5: Exact CPU VNG Optimization](reviews/M5_CPU_VNG_REVIEW.md)
- [M5: Exact AVX2 VNG Demosaicing (2026-10-26)](reviews/M5_CPU_VNG_SIMD_REVIEW.md)
- [M5: Reused CPU Integration Buffers (2026-10-27)](reviews/M5_CPU_BUFFER_REUSE_REVIEW.md)
- [M5: Light Sets, Calibration Sets and Declared Sensor Layout (2026-10-28)](reviews/M5_LIGHT_SETS_REVIEW.md)
- [M5: Hot-Pixel-Robust Star Measurement and Scale/Deformation Fallback (2026-10-28)](reviews/M5_HOT_PIXEL_AND_SCALE_REVIEW.md)
- [M5: Light-Frame Calibration — Bias, Darks, Flats (2026-10-29)](reviews/M5_CALIBRATION_REVIEW.md)
- [M5: Reconstruction of Missing Header Values and Camera Catalogue; dense-field registration fixes (2026-10-29)](reviews/M5_HEADER_RECONSTRUCTION_REVIEW.md)
- [M5: Master Quality Metrics and Pixel Rejection Methods (2026-10-29/30)](reviews/M5_MASTER_QUALITY_AND_REJECTION_REVIEW.md)
- [M5: GPU Pixel Rejection and Weighted Mean (2026-10-30)](reviews/M5_GPU_REDUCTION_REVIEW.md)
- [M5: Bounded Parallel External Registration](reviews/M5_EXTERNAL_REGISTRATION_REVIEW.md)
- [M5: Bounded Original FITS Block Cache](reviews/M5_FITS_CACHE_REVIEW.md)
- [M5: Optional Validated Geometric Correction (2026-10-26)](reviews/M5_GEOMETRIC_CORRECTION_REVIEW.md)
- [M5: Owner-observed geometric mixture regression (2026-10-26)](reviews/M5_GEOMETRIC_MIXTURE_DIAGNOSIS.md)
- [M5 First Slice: GPU Cosmetics and VNG](reviews/M5_GPU_REVIEW.md)
- [M5: Integration CPU and I/O Profile (2026-10-26)](reviews/M5_INTEGRATION_PROFILE_REVIEW.md)
- [M5: Independent Native Registration Walks (2026-10-26)](reviews/M5_NATIVE_REGISTRATION_WALKS_REVIEW.md)
- [M5: Available-RAM Tile Selection](reviews/M5_RAM_TILE_SELECTION_REVIEW.md)
- [M5 Exact Registration Fallback Optimization Review](reviews/M5_REGISTRATION_FALLBACK_REVIEW.md)
- [M5 Registration Phase Profile Review](reviews/M5_REGISTRATION_PROFILE_REVIEW.md)
- [M5: Exact Pixel-Rejection Data Layout Optimization](reviews/M5_REJECTION_LAYOUT_REVIEW.md)
- [M5: Reuse Pixel-Rejection Working Memory](reviews/M5_REJECTION_MEMORY_REVIEW.md)
- [M5: Shared RGB Rejection Preparation](reviews/M5_SHARED_RGB_REJECTION_REVIEW.md)
- [M5: Output Tile Geometry Optimization](reviews/M5_TILE_GEOMETRY_REVIEW.md)

## Machine-readable evidence

- [Auto reference](reviews/M5_AUTO_REFERENCE_EVIDENCE.json): current gate snapshot, artificial-date real-pixel comparison, exact external-reference control and release hashes.
- [Whole-field repair](reviews/M5_GEOMETRIC_ACCEPTANCE_REPAIR_EVIDENCE.json): stored catalog and bounded image acceptance results.
- [Initial geometric correction](reviews/M5_GEOMETRIC_CORRECTION_EVIDENCE.json): historical fit/fixture evidence; superseded acceptance behavior is unsafe without the repair.
- [Native walks](reviews/M5_NATIVE_REGISTRATION_WALKS_EVIDENCE.json): paired catalog-only scheduling measurements and exact serial comparison.
- [CPU optimization batch](reviews/M5_CPU_OPTIMIZATION_EVIDENCE.json): historical whole-pipeline evidence.
- [Calibration](reviews/M5_CALIBRATION_EVIDENCE.json), [buffer reuse](reviews/M5_CPU_BUFFER_REUSE_EVIDENCE.json), [AVX2 VNG](reviews/M5_CPU_VNG_SIMD_EVIDENCE.json), [hot pixels and scale fallback](reviews/M5_HOT_PIXEL_AND_SCALE_EVIDENCE.json), [integration profile](reviews/M5_INTEGRATION_PROFILE_EVIDENCE.json) and [light sets](reviews/M5_LIGHT_SETS_EVIDENCE.json): sanitized numbers behind the reviews of the same name.
- [Header reconstruction](reviews/M5_HEADER_RECONSTRUCTION_EVIDENCE.json): derived values and registration counts of the ASI2600MC M42 set.
- [Master quality and rejection](reviews/M5_MASTER_QUALITY_AND_REJECTION_EVIDENCE.json): per-method integration time, rejection, master noise and star metrics on four real sets.
- [GPU pixel rejection](reviews/M5_GPU_REDUCTION_EVIDENCE.json): CPU/GPU integration times, parity and synthetic kernel timings.
- [DS-B fingerprint manifest](ds-b-manifest.json): immutable source/reference inventory, with no image pixels or GPS values.

Every evidence file sits in `reviews/` next to its review. Raw reports, generated FITS, preferences and executables stay local and are excluded from Git. Windows is locally exercised; real multi-night observations, latest owner visual acceptance and macOS remain unverified. No remote CI is claimed.
