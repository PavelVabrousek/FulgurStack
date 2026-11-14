# M5: Exact AVX2 VNG Demosaicing (2026-10-26)

Owner-requested local optimization, ADR-069. M5 remains open. No commit, push or remote CI. Scientific operations, f32/f64 precision, thresholds, stencils, source order, tiles, memory policy and outputs are unchanged; the result is bit-identical by construction and by measurement.

## Change

The CPU VNG kernel previously reconstructed one pixel at a time. The build script now also generates an eight-lane AVX2 copy of each of the four phase kernels from the same shared stencils (`vng_stencil.rs`). Each lane performs the scalar kernel's IEEE single-precision operations in the same order: weighted absolute gradient terms are left-folded as before; min/max/threshold are the same folds; each selected direction adds its samples in the original order using a per-lane blend instead of a branch; the final difference/division by the selected-direction count is unchanged. No fused multiply-add, reassociation, reduced precision or approximation is used.

Interior pixel pairs of a row are processed in groups of eight same-phase pixels from even/odd column planes of the five input rows; the reflected first/last two columns and any remaining pairs use the unchanged scalar kernel. AVX2 is runtime-detected; the scalar path remains for other CPUs and for macOS/Apple Silicon. A row uses the vector path only when all five input rows are finite and within ±1e30, so NaN, infinity and overflow never reach the min/max/compare steps, where vector and scalar semantics could differ. Real Seestar data (decoded 16-bit ADU) is always inside this domain. The per-row scratch planes are small (about two input rows per worker) and are not images; no intermediate file, cache or full-frame buffer is added.

## Verification gates

- New: forced-scalar vs AVX2 output bits across all four Bayer patterns, widths 6–44 plus 47/48/49/63/64/65/127/530 (every group/tail boundary), six heights, and 13 fields: constant, pseudo-random, faint, saturated stripes, star, ramp, signed zeros, subnormals, bounded ±1e30 extremes, ±f32::MAX, ±infinity/NaN mixtures, a single NaN row in ordinary data, and full 16-bit ADU ranges. Finite fields also match the frozen pre-generation reference implementation. Source buffers are verified unchanged.
- New: real Seestar frame before and after cosmetic correction, forced scalar vs AVX2, bit-identical.
- Existing frozen-reference VNG, GPU stencil-word and all integration/resampling/rejection gates PASS with the vector path active.
- Format, default/all-feature workspace Clippy with warnings denied: PASS. Release suites **162/162 default, 165/165 all-feature**, with real FITS, required AVX2 (`FULGURSTACK_TEST_SIMD=1`) and actual GPU (`FULGURSTACK_TEST_GPU=1`).

## Measurement design

The stored ADR-062 plan (`master_v11_report.json`) is no longer present locally. The stored native registration plan `outputs/M5_native_walks_20261026/parallel_plan.json` is used instead (1140 original / 940 selected sources, chronological anchor, native 2160 x 3840 grid, no geometric correction). The profiling example now also accepts a stored `register` report with this top-level plan; the pipeline is untouched by that change.

A control profiler executable was built before the engine change and preserved; the candidate differs only in the VNG change. Both use 24 threads, CPU engine, manual 512-square tiles, cache 0, sigma 3.0/2.5. Central region: four complete tiles at origin (512,1024), 1024-square, all 940 sources; one excluded warmup per binary, then three rounds in rotated order. Then one complete-grid pair. Runs were sequential with no builds/tests in parallel. Other applications, OS file cache, power and thermal state are not controlled.

## Results

Central region, medians of three rounds (seconds):

| Measurement | Control | AVX2 VNG | Change |
|---|---:|---:|---:|
| Integration wall | 21.8704137 | 15.8733048 | −27.421% |
| Reconstruction wall | 16.1154370 | 10.2864817 | −36.170% |
| Summed demosaic task time | 234.360 | 94.904 | −59.505% |
| Process user CPU | 350.797 | 245.734 | −29.950% |

Individual integration observations: control 22.715637 / 21.1852978 / 21.8704137, candidate 16.0484807 / 15.8399244 / 15.8733048.

Complete native grid, single pair:

| Measurement | Control | AVX2 VNG |
|---|---:|---:|
| Integration wall (s) | 162.2608958 | 130.0071238 (−19.878%) |
| Reconstruction wall (s) | 119.7366403 | 84.9744411 (−29.032%) |
| Rejection/maps wall (s) | 35.2534623 | 37.4227259 (unchanged code; run variation) |
| Summed demosaic task (s) | 1633.930 | 731.285 (−55.244%) |
| Process user / kernel CPU (s) | 2488.516 / 307.531 | 1907.531 / 285.813 |
| Peak working set (MiB) | 3339.7 | 3512.6 |

Today's control is slower than the stored ADR-062 observation (135.0441689 s), under different, uncontrolled machine conditions. Only the paired same-session comparison is used here; do not subtract today's saving from the historical figure or claim a historical full-pipeline time. Peak working set varies between runs (historically 3267.8–3501.2 MiB on this workload); the candidate is higher in this one pair and remains far below the 12288 MiB ceiling and the conservative plan.

End-to-end CLI, 32-frame DS-B, chronological-median reference, native grid (control CLI built from commit 5fbd867): processing 6.654397 / 6.6038844 s to 5.6494159 / 5.4470505 s; reconstruction 3.1488894 / 3.1178829 s to 2.1349757 / 2.0122656 s. 27 accepted / 5 rejected in all runs.

## Exactness and integrity

- All 10 profiling runs (warmups included) produce one identical set of raw-float sample hashes and whole master/coverage/high-rejection FITS per scope.
- Complete-grid coverage and high-rejection FITS are byte-identical to the stored ADR-062 products (d65c87e4…, dff8eb3b…). The master differs from the stored a288ee5c… only because a different stored decimal plan is replayed; control and candidate masters are identical (fc1af4a3d92f9de77c4b6f6c9c52f3b8087a338ad9818d0946b54c4a6476ecb2).
- DS-B: `verify-stack-equivalence.py` PASS: exact entire master/map FITS byte equality and exact scientific report fields; only timing and RAM-snapshot telemetry differ. All six DS-B runs share one hash per product.
- All 1140 DS-A sources match the stored SHA256/size/mtime fingerprints; all 32 DS-B sources match `ds-b-manifest.json`. No source, intermediate or temporary image is written; outputs contain only final FITS and JSON.

## Limits

Windows 11 / AVX2 only (local Ryzen AI 9 HX 370). The scalar fallback is covered by the forced-scalar tests but no non-AVX2 or macOS machine was run. GPU reconstruction is unaffected. No full reference stack, GUI timing or PixInsight review is claimed; products are bit-identical, so no new visual review is required by this change. Private logs and products: `outputs/vng_simd_20261026/`. Portable summary: [evidence](M5_CPU_VNG_SIMD_EVIDENCE.json).

## Reproduce

```powershell
cargo run --release -p fulgur-pipeline --example profile_integration -- "outputs/M5_native_walks_20261026/parallel_plan.json" "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" "outputs/vng_simd_new_center" 24 center
```

Use a new output folder each time; `full` instead of `center` runs the complete grid.
