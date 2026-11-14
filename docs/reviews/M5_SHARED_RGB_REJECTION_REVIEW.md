# M5: Shared RGB Rejection Preparation

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. All development/tests/benchmarks are local on Windows x86_64. No commits, pushes, PR updates or remote CI. GitHub Actions remain disabled. No new macOS execution is claimed. M5 remains open; FITS cache stays off.

Only the new version was stacked. Compared with the stored SIMD-cosmetics result, full processing measured **282.965 -> 275.913 s** (2.492% shorter). RGB preparation/rejection/accumulation/maps measured **89.296 -> 82.613 s** (7.485% shorter). Entire master and both maps match stored accepted CPU/M4 products byte for byte. No new reference stack was run.

## Change and scientific invariants

The previous reducer gathered each channel separately into (f32 value, source index) tuples after another pass for geometric coverage. Now one source-order pass gathers all finite RGB samples into three contiguous f32 arrays, collects weights and accumulates geometric red-channel coverage. While RGB validity masks agree, all three channels share one contiguous f64 weight sequence. At the first independent channel mask, copy the already-gathered common weight prefix into two pre-reserved arrays and continue channel-specific finite filtering. Reset/reuse all storage per pixel, including after a previous pixel required independent weights. No source data is retained between tiles.

Each color independently computes the same exact certified median/MAD thresholds, clips the original samples and accumulates original survivors in their original source order with f64 Neumaier sums. Dense sample/weight iteration replaces source-index lookup; clipping starts with an exact contiguous copy. No changed precision, median/MAD/sigma floor/iteration policy, RGB coupling in rejection, map denominator, geometric footprint, photometry or final export limiting. Cosmetics, VNG, SIMD resampling, GPU option and source work/I/O remain unchanged.

All six preparation arrays and both clipping arrays reserve at most 44 bytes of payload per accepted source per band, within the existing conservative 64-byte source-scratch allowance. Fixed vector metadata stays within runtime headroom. Preserve the 32-pixel packing block and <=16 bands/ROIs. All allocation estimates, available-RAM reserve and 12 GiB ceiling remain unchanged. No new unsafe code, dependency, image/cache buffer, schema or intermediate file.

## Local verification

- Format and default/all-feature Clippy with warnings denied PASS; release tests **112/112 default**, **114/114 all-feature** PASS. Actual AVX2 and GPU required locally; real FITS fixtures configured and PASS without silent skips.
- Finite RGB sample bits and dense weight bits match independent old per-channel filters for empty/sparse/all-valid/all-invalid/common/independent masks, first/last-source mismatches, signed zeros, nonfinite omissions and populations up to 1,500. Compensated red coverage matches exactly. Pointer reuse verifies no reallocation across pixels/population changes.
- Dense combiner matches the prior tuple-based combiner for exact output/weight bits and low/high counts across signed/subnormal/extreme/faint/saturated/outlier fields, zero/unequal weights, varying populations and asymmetric clipping settings.
- Frozen previous complete band reducer independently verifies all master/coverage/high sample bits and every row-statistic bit over common and independent masks, 0/1/31/32/33/73 output lengths, offsets and 1..1,500 sources. Measured capacity test proves the 44-byte payload fits the unchanged 64-byte planner allowance. Existing cancellation, source immutability, flux/halo/quality and real GPU gates PASS.
- DS-B: 32 originals, 27 accepted/5 rejected, 270 s accepted acquisition. Whole master/maps exactly equal stored controls. All 32 source hashes, two PI references and external template unchanged.
- DS-A: 1,140 originals, 940 accepted/200 rejected, 9,400 s accepted acquisition. All 1,140 source hashes/inventory and external template verified before/after; entire master/maps also match accepted CPU/M4 hashes.
- Strict science/registration/photometry/exposure/correction/rejection/export fields, source work/I/O, cache-off telemetry, geometry and memory plans remain exact. Only timings/RSS and explicitly permitted available-RAM snapshots/derived headroom differ. No layout/cache/geometry exception. No intermediate images or temporary leftovers. GPS preserved only in scientific FITS and redacted from default reports/logs.

## Stored-baseline performance

Stored baseline: outputs/M5_CPU_cosmetic_simd_performance.json and its final DS-A/DS-B report/products. Baseline was not executed again. Same verified locked toolchain/profile/features, thin LTO/one codegen unit, GPU-enabled build with CPU engine, 16 threads, 512-square tiles, 12288 MiB ceiling, cache 0, sigma 3.0/2.5, RegisteringTemplate200pct.fit, full 4000-square reference at (0,0), no crop. No concurrent builds/tests during timed runs.

| Full-set measurement, seconds | Stored baseline | New optimized run |
|---|---:|---:|
| Processing through final FITS writes | 282.965 | 275.913 |
| Complete command including report publication | 283.510 | 276.600 |
| Inspection (unchanged) | 16.576 | 14.622 |
| Registration (unchanged) | 22.779 | 25.259 |
| Reconstruction (unchanged) | 142.527 | 141.122 |
| RGB preparation, rejection, accumulation and maps | 89.296 | 82.613 |
| FITS writes (unchanged) | 0.972 | 1.200 |

Peak RSS **3399.676 -> 3417.559 MiB**. Planned allocation remains **4,789,190,656 bytes**. Throughput after change **4.132 original inputs/s**, including quality rejects. No general RAM-reduction claim.

Small set: processing **11.638 -> 11.363 s**, preparation/rejection/maps **3.972 -> 3.720 s**, RSS **662.465 -> 662.426 MiB**.

Unchanged reconstruction saves 1.405 s; unchanged inspection/registration saves -0.526 s (negative means slower). These variations are not assigned to RGB preparation. Reduction timing includes preparation, clipping, accumulation and maps; it does not isolate preparation. Overlapping ROI task timers are not additive wall time or CPU execution time.

This is a historical observed comparison with OS caches, background load, temperatures and RAM availability uncontrolled, not a fresh controlled pair or isolated causal/portable speedup guarantee. No new baseline run is justified by that qualification. Retain unfavorable results and prior analytic CFA/VNG and unmatched PixInsight-workload limitations.

## Provenance and artifacts

Git base `b836b93e3045164007efaf93d88354fc4edb0a8f` identifies the shared committed base only. Both actual uncommitted source states are fingerprinted in outputs/M5_rgb_prepare_baseline_source.json and outputs/M5_rgb_prepare_optimized_source.json. Baseline binary SHA256 `f0ce7a4bd4dd9b614fc2645e4ef60e360c99c28dcc2c21a9cc8532fc130bc43f` (copied for provenance, never executed); optimized `88424780dbdd48661d6735b5fc48c17b8db9994504a8aebb28479ccbe3ff2793` in target/m5-rgb-prepare-optimized.exe.

Full scientific work remains 5,715,067,217 decoded ROI pixels, 1,751,132 corrected pixels, 44,271,162,492 valid channel samples, 85,995,586 low and 698,062,316 high rejections, 170 upper export limits/lower 0. Both maps keep denominator 1,140 original inputs. Direct application reads remain 11,430,134,434 bytes, 18,619,113 row reads, 59,983 opens/checks; these are not physical SSD traffic counters.

Full-file SHA256, equal to stored accepted CPU/M4:

- master: `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc`.
- coverage: `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5`.
- high_rejection: `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970`.

- Master FITS (private historical artifact: `outputs/M5_rgb_prepare_DS-A_after_external200.fits`)
- Coverage FITS (private historical artifact: `outputs/M5_rgb_prepare_DS-A_after_external200_coverage.fits`)
- High-rejection FITS (private historical artifact: `outputs/M5_rgb_prepare_DS-A_after_external200_high_rejection.fits`)
- Performance evidence (private historical artifact: `outputs/M5_CPU_rgb_prepare_performance.json`)
- 32-frame exactness gate (private historical artifact: `outputs/M5_rgb_prepare_DS-B_equivalence.json`)
- Full exactness gate (private historical artifact: `outputs/M5_rgb_prepare_DS-A_equivalence.json`)

## Owner reproduction

Run from the project directory using new output names. Open FITS directly in PixInsight; no loader script or manual intensity conversion. Whole-file equality preserves prior owner visual review.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_rgb_prepare_owner32.fits" --report "outputs/M5_rgb_prepare_owner32.json"
```

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:\Astrofoto\2026\10 October\SeeStar50P M33\M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_rgb_prepare_owner_full.fits" --report "outputs/M5_rgb_prepare_owner_full.json"
```
