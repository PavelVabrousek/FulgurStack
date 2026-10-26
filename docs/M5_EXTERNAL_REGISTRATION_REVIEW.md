# M5: Bounded Parallel External Registration

Date: 2026-10-22. All development/tests/benchmarks local on Windows x86_64. No commits, pushes, PR updates or remote CI. GitHub Actions remain disabled; no new macOS execution is claimed. M5 remains open; FITS cache stays off.

Only the new version was stacked. Against the stored shared-RGB result, full processing measured **275.913 -> 300.919 s** (9.063% longer). Registration measured **25.259 -> 26.244 s** (3.899% longer). Whole master and both maps match stored accepted CPU/M4 products byte for byte. No new reference stack was executed.

## Implementation

The external catalog and native-anchor-to-external bridge are still computed once with the unchanged serial code. Only independent source-to-external refinements are dispatched concurrently. Zip native report rows with their source catalogs in disjoint contiguous Rayon chunks, capped at min(current pool threads,16) chunks even in a larger caller pool. Each row stays at its original source position; one fit runs per chunk at a time. Share catalogs immutably; preserve original prediction composition, solver/arithmetic/match ordering, accepted/rejected decisions, native photometric provenance, row metadata and footprint operations. Rows without a native transform remain untouched, exactly as in the prior loop.

After joining every chunk, retain original source-ordered serial final weight normalization, polygon intersections, coverage/canvas planning, exposure totals and report reductions. Native session predictions remain serial; scientific photometry and all integration/CPU/GPU math are unchanged. No dependency, unsafe code, schema, image/cache allocation or result queue. At most sixteen catalog fits are active; existing runtime headroom and all integration allocation estimates/system reserve remain unchanged. Check cancellation before dispatch and on each row, join work on error/cancellation and reject mismatched row/catalog lengths before mutation. Failed commands publish no final image/report.

## Local gates and exactness

- Format and default/all-feature Clippy with warnings denied PASS; release suites **114/114 default**, **116/116 all-feature** PASS. Actual local GPU/AVX2 required and PASS; real FITS fixtures configured, no silent skipping.
- Frozen complete prior serial loop independently compares all-field row snapshots and explicit transform/native-provenance/RMSE/match-residual bits against pools of 1/3/16/32 threads. Scales 0.33/1/2/3, varied native rotations/translations, missing/outlier stars, absent native transforms, quality-rejected but measurable rows and insufficient-star external failures preserve order, decisions, footprints, photometry and immutable catalogs. Cancellation and malformed lengths reject without publication; empty row dispatch is safe.
- DS-B: 32 originals, 27 accepted/5 rejected, 270 s accepted acquisition. Entire master/coverage/high FITS equal stored controls. Source hashes, two PI references and external template unchanged.
- DS-A: 1,140 originals, 940 accepted/200 rejected, 9,400 s accepted acquisition. All source hashes/inventory and template verified before/after. Whole master/maps also match accepted CPU/M4 hashes.
- Scientific registration/quality/geometry/photometry/exposure/rejection/export fields, integration work/I/O, cache-off telemetry and memory plans remain strict. Only timing/RSS and explicitly permitted available-RAM snapshots/derived headroom differ; no layout/cache/geometry exception. No image intermediates or temporary leftovers; GPS retained in scientific FITS and redacted from default reports/logs.

## Stored-baseline performance

Control is outputs/M5_CPU_rgb_prepare_performance.json and its final DS-A/DS-B products/reports; no reference stack rerun. Same verified locked toolchain/profile/features, thin LTO/one codegen unit, GPU-enabled executable with CPU engine, 16 threads, 512-square tiles, 12288 MiB ceiling, cache 0, sigma 3.0/2.5 and full 4000-square RegisteringTemplate200pct.fit reference grid at (0,0), no crop. No concurrent builds/tests during timed runs.

| Full-set measurement, seconds | Stored baseline | New run |
|---|---:|---:|
| Processing through final FITS writes | 275.913 | 300.919 |
| Complete command including report publication | 276.600 | 301.415 |
| Inspection (unchanged) | 14.622 | 16.367 |
| Registration including native/external fits and canvas planning | 25.259 | 26.244 |
| Reconstruction (unchanged) | 141.122 | 156.202 |
| Rejection/accumulation/maps (unchanged) | 82.613 | 90.325 |
| FITS writes (unchanged) | 1.200 | 0.974 |

Peak RSS **3417.559 -> 3391.301 MiB**; conservative planned integration allocation remains **4,789,190,656 bytes**. Throughput **3.788 original inputs/s**, including quality rejects. No general RAM-reduction claim.

Small-set processing **11.363 -> 11.948 s** (5.153% longer); registration **1.613 -> 1.629 s** (1.011% longer); RSS **662.426 -> 654.582 MiB**. Retain the unfavorable small-set result, including variations in unchanged reconstruction/rejection.

Unchanged full reconstruction saves -15.080 s; unchanged rejection/maps saves -7.713 s; unchanged inspection saves -1.745 s (negative means slower). These variations are not assigned to parallel registration. Registration timing includes native registration, template read/catalog/bridge/refinements and canvas planning; no isolated external-fit or CPU-utilization timing is claimed. Overlapping ROI task elapsed is not additive wall/CPU time.

This is a historical observed comparison, with OS caches/background load/temperatures/RAM availability uncontrolled, not a fresh controlled pair or isolated causal/portable acceleration guarantee. No new baseline is justified by that qualification. Preserve prior analytic CFA/VNG and unmatched PixInsight-workload limitations.

## Provenance and artifacts

Git base `b836b93e3045164007efaf93d88354fc4edb0a8f` is the shared committed base only. Both actual uncommitted states are fingerprinted in outputs/M5_external_parallel_baseline_source.json and outputs/M5_external_parallel_optimized_source.json. Preserved baseline SHA256 `88424780dbdd48661d6735b5fc48c17b8db9994504a8aebb28479ccbe3ff2793` was copied for provenance, never executed. Optimized binary target/m5-external-parallel-optimized.exe SHA256 `35584ee8f4a9a269d9b1fc177884b6df9ed7ccbd3568ff40ef62dd230580c296`.

Scientific work unchanged: 5,715,067,217 decoded ROI pixels, 1,751,132 cosmetic corrections, 44,271,162,492 valid channel samples, 85,995,586 low/698,062,316 high rejections, 170 upper export limits/lower 0. Both maps keep 1,140-input denominator. Direct application I/O remains 11,430,134,434 bytes, 18,619,113 row reads and 59,983 opens/checks, not physical SSD traffic counters.

Whole-file SHA256 equal to stored accepted CPU/M4:

- master: `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc`.
- coverage: `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5`.
- high_rejection: `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970`.

- [Full master FITS](../outputs/M5_external_parallel_DS-A_after_external200.fits)
- [Coverage FITS](../outputs/M5_external_parallel_DS-A_after_external200_coverage.fits)
- [High-rejection FITS](../outputs/M5_external_parallel_DS-A_after_external200_high_rejection.fits)
- [Performance evidence](../outputs/M5_CPU_external_parallel_performance.json)
- [32-frame exactness gate](../outputs/M5_external_parallel_DS-B_equivalence.json)
- [Full exactness gate](../outputs/M5_external_parallel_DS-A_equivalence.json)

## Owner reproduction

Run from the project directory with new output names. Open FITS directly in PixInsight; no loader script/intensity conversion. Exact file equality preserves prior owner visual review.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_external_parallel_owner32.fits" --report "outputs/M5_external_parallel_owner32.json"
```

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:\Astrofoto\2026\10 October\SeeStar50P M33\M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_external_parallel_owner_full.fits" --report "outputs/M5_external_parallel_owner_full.json"
```


Interpretation: functional/exactness gates PASS; acceleration is NOT ESTABLISHED. Both stored-baseline comparisons were slower, including total registration. The full run spent 22.792 s more in unchanged reconstruction/rejection phases, so the 25.006 s total increase cannot be assigned to the external-registration change. Existing timing does not isolate the external fit loop; retain that limitation and the requested parallel implementation without claiming a measured speedup. No extra baseline/new-version reruns were performed to seek a favorable result.
