# Documentation index — 2026-10-30

Start with [current delivery state](CURRENT_STATUS.md). M1–M4 are owner-accepted; M5 remains open. The latest completed batch was published to `main` with ADR-088; Actions remain disabled and future publication needs the owner's authorization. Old measurements and FAIL outcomes are retained, with their original scope. Local-only statements inside dated reviews describe their measurement-time policy, not the publication status of this snapshot.

## Current guides and contracts

| Document | Purpose |
| --- | --- |
| [Product overview](../README.md) | Launch, current defaults, CLI examples and evidence summary |
| [Current state](CURRENT_STATUS.md) | Current behavior, scientific gates, acceptance limits and local release |
| [Project specification](../PROJECT_SPEC.md) | Binding scientific requirements and later owner amendments |
| [Engineering contract](../AGENTS.md) | Verification, data protection and batch publishing policy |
| [Development](DEVELOPMENT.md) | Current workspace, build commands and local checks |
| [Windows distribution](WINDOWS_DISTRIBUTION.md) | Portable ZIP contents, build and check procedure, current package hashes |
| [Benchmarks](BENCHMARKS.md) | Distinct measured scopes, immutable datasets and owner PI references |
| [Native Windows review](M5_WINDOWS_APP_REVIEW.md) | Current review entry point followed by dated UI history |
| [Auto reference](M5_AUTO_REFERENCE_REVIEW.md) | Default single/multi-night reference policy, approximation and measured limits |
| [Geometry acceptance repair](M5_GEOMETRIC_ACCEPTANCE_REPAIR.md) | Current whole-field validity and safe recovery rules |
| [Architecture decisions](ARCHITECTURE_DECISIONS.md) | Decision log; later owner amendments supersede earlier choices |
| [UI design history](WINDOWS_UI_DESIGN.md) | Current Compact Desk summary followed by original proposals |
| [Icon assets](../crates/fulgur-gui/assets/README.md) | Approved artwork, exports and Windows identity |

## Dated milestone and optimization evidence

The reviews below are historical records, not current default/acceptance claims. In particular, the initial geometric correction and mixture diagnosis retain their failed product acceptance; the subsequent repair is required. Historical full pipeline timings, frozen integration timing and native catalog timing measure different work. Do not combine them into a new speedup or treat earlier test counts as current. Each review links back to current state.

- [M1: FITS Reconstruction and Single-Frame Export](M1_REVIEW.md)
- [M2: DS-B 32-Frame Manual Review](M2_DS-B_REVIEW.md)
- [M2: CPU Subframe Inspector and Quality Filtering](M2_REVIEW.md)
- [M3: DS-B Registration Review](M3_DS-B_REVIEW.md)
- [M3: Direct-to-Anchor Rigid Registration and Canvas Planning](M3_REVIEW.md)
- [M4 Full DS-A Reference-Canvas Run and Performance Profile](M4_FULL_RUN_PROFILE.md)
- [M4 Full DS-A Run with the 200% External Reference](M4_FULL_SCALED_RUN_REVIEW.md)
- [M4 Revision: Reference Geometry and Diagnostic Maps](M4_REFERENCE_MAPS_REVIEW.md)
- [M4: Tiled CPU Integration and PixInsight Master Review](M4_REVIEW.md)
- [M4 Revision: Scaled External Registration References](M4_SCALED_REFERENCE_REVIEW.md)
- [M4 Quality Comparison Plan](M4_TEST_PLAN.md)
- [M5: Exact CPU Cosmetic Correction Optimization](M5_CPU_COSMETIC_REVIEW.md)
- [M5: Exact SIMD CPU Cosmetic Correction](M5_CPU_COSMETIC_SIMD_REVIEW.md)
- [M5 CPU Optimization Batch](M5_CPU_OPTIMIZATION_SUMMARY.md)
- [M5: Exact Pixel-Rejection Optimization](M5_CPU_REJECTION_REVIEW.md)
- [M5: Exact CPU Resampling Optimization](M5_CPU_RESAMPLING_REVIEW.md)
- [M5: Continuous Bounded CPU Reconstruction Scheduling](M5_CPU_SCHEDULING_REVIEW.md)
- [M5: Exact SIMD CPU Resampling](M5_CPU_SIMD_REVIEW.md)
- [M5: Exact CPU VNG Optimization](M5_CPU_VNG_REVIEW.md)
- [M5: Exact AVX2 VNG Demosaicing (2026-10-26)](M5_CPU_VNG_SIMD_REVIEW.md)
- [M5: Reused CPU Integration Buffers (2026-10-27)](M5_CPU_BUFFER_REUSE_REVIEW.md)
- [M5: Light Sets, Calibration Sets and Declared Sensor Layout (2026-10-28)](M5_LIGHT_SETS_REVIEW.md)
- [M5: Hot-Pixel-Robust Star Measurement and Scale/Deformation Fallback (2026-10-28)](M5_HOT_PIXEL_AND_SCALE_REVIEW.md)
- [M5: Light-Frame Calibration — Bias, Darks, Flats (2026-10-29)](M5_CALIBRATION_REVIEW.md)
- [M5: Reconstruction of Missing Header Values and Camera Catalogue; dense-field registration fixes (2026-10-29)](M5_HEADER_RECONSTRUCTION_REVIEW.md)
- [M5: Master Quality Metrics and Pixel Rejection Methods (2026-10-29/30)](M5_MASTER_QUALITY_AND_REJECTION_REVIEW.md)
- [M5: GPU Pixel Rejection and Weighted Mean (2026-10-30)](M5_GPU_REDUCTION_REVIEW.md)
- [M5: Bounded Parallel External Registration](M5_EXTERNAL_REGISTRATION_REVIEW.md)
- [M5: Bounded Original FITS Block Cache](M5_FITS_CACHE_REVIEW.md)
- [M5: Optional Validated Geometric Correction (2026-10-26)](M5_GEOMETRIC_CORRECTION_REVIEW.md)
- [M5: Owner-observed geometric mixture regression (2026-10-26)](M5_GEOMETRIC_MIXTURE_DIAGNOSIS.md)
- [M5 First Slice: GPU Cosmetics and VNG](M5_GPU_REVIEW.md)
- [M5: Integration CPU and I/O Profile (2026-10-26)](M5_INTEGRATION_PROFILE_REVIEW.md)
- [M5: Independent Native Registration Walks (2026-10-26)](M5_NATIVE_REGISTRATION_WALKS_REVIEW.md)
- [M5: Available-RAM Tile Selection](M5_RAM_TILE_SELECTION_REVIEW.md)
- [M5 Exact Registration Fallback Optimization Review](M5_REGISTRATION_FALLBACK_REVIEW.md)
- [M5 Registration Phase Profile Review](M5_REGISTRATION_PROFILE_REVIEW.md)
- [M5: Exact Pixel-Rejection Data Layout Optimization](M5_REJECTION_LAYOUT_REVIEW.md)
- [M5: Reuse Pixel-Rejection Working Memory](M5_REJECTION_MEMORY_REVIEW.md)
- [M5: Shared RGB Rejection Preparation](M5_SHARED_RGB_REJECTION_REVIEW.md)
- [M5: Output Tile Geometry Optimization](M5_TILE_GEOMETRY_REVIEW.md)

## Machine-readable evidence

- [Auto reference](M5_AUTO_REFERENCE_EVIDENCE.json): current gate snapshot, artificial-date real-pixel comparison, exact external-reference control and release hashes.
- [Whole-field repair](M5_GEOMETRIC_ACCEPTANCE_REPAIR_EVIDENCE.json): stored catalog and bounded image acceptance results.
- [Initial geometric correction](M5_GEOMETRIC_CORRECTION_EVIDENCE.json): historical fit/fixture evidence; superseded acceptance behavior is unsafe without the repair.
- [Native walks](M5_NATIVE_REGISTRATION_WALKS_EVIDENCE.json): paired catalog-only scheduling measurements and exact serial comparison.
- [CPU optimization batch](M5_CPU_OPTIMIZATION_EVIDENCE.json): historical whole-pipeline evidence.
- [Header reconstruction](M5_HEADER_RECONSTRUCTION_EVIDENCE.json): derived values and registration counts of the ASI2600MC M42 set.
- [Master quality and rejection](M5_MASTER_QUALITY_AND_REJECTION_EVIDENCE.json): per-method integration time, rejection, master noise and star metrics on four real sets.
- [GPU pixel rejection](M5_GPU_REDUCTION_EVIDENCE.json): CPU/GPU integration times, parity and synthetic kernel timings.
- [DS-B fingerprint manifest](ds-b-manifest.json): immutable source/reference inventory, with no image pixels or GPS values.

Other dated evidence files under `docs/` accompany their corresponding reviews. Raw reports, generated FITS, preferences and executables stay local and are excluded from Git. Windows is locally exercised; real multi-night observations, latest owner visual acceptance and macOS remain unverified. No remote CI is claimed.
