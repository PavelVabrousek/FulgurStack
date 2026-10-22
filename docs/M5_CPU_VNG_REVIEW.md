# M5: Exact CPU VNG Optimization

Date: 2026-10-22. Scientific revision: `5455c5a24190b5966b1db7191a366c89bd190eaa`. M5 remains open. FITS cache stays **off**, including both fresh performance controls.

All 1,140 inputs were processed against RegisteringTemplate200pct.fit on its complete 4000 x 4000 reference grid at (0,0), without cropping. Full processing measured **510.694 -> 438.435 s**, a **72.259 s / 14.149%** reduction in this pair. Reconstruction wall measured **334.964 -> 260.558 s** (22.213% shorter). Entire master, coverage and high-rejection FITS match the control and previously accepted CPU/M4 products byte-for-byte.

## Implementation

Generate four constant-index CPU pixel kernels from the existing independent geometric stencil factory, shared with optional GPU coefficient packing. The four kernels represent the four possible centered 2x2 Bayer channel cells. Serialize coefficient f32 bits exactly; preserve the original gradient contribution order/Iterator::sum, threshold min + 0.5*max including ties, selected-direction sample order, channel sums/count division, reconstruction and native-sample assignment. No approximation, precision reduction, coefficient duplication or reassociated/fused sums.

Select the alternating kernels once per row. Reflect the five source rows once per output row, load contiguous interior 5x5 windows directly, and retain exact reflected x access for the first/last two columns. Minimum/odd sizes, Bayer phase, ROI halos, input validation and cancellation remain unchanged. Remove per-ROI CPU stencil construction/allocation. Existing original GPU coefficient words/API and hardware numerical policy are unchanged.

The build script is standard Rust and emits Rust only into Cargo OUT_DIR; no new dependency or external build/runtime tool is required. No image/source cache, additional frame buffer, source write or image intermediate. Allocation planning/system reserve/12 GiB ceiling are unchanged. The complete prior scalar VNG remains a test-only independent oracle and is excluded from release products.

## Automated and scientific verification

- Format, default/all-feature Clippy with warnings denied: PASS.
- Release suites: **94/94 default**, **96/96 all-feature**, PASS. Local real GPU hardware gates explicitly required and PASS.
- [Windows MSVC and macOS Apple Silicon CI for the measured code](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39805648942): PASS, default/all-feature builds, tests, warnings and adapter checks. Real GPU hardware validation is the separate local gate.
- Exact pre-export bit comparisons cover all four Bayer patterns, five minimum/odd/asymmetric sizes, six fields (flat RGB, signed noise, sub-ADU signal, saturated plateaus, colored bright stars and gradients), unchanged input values and cancellation/invalid buffers. The actual 2160 x 3840 source also matches every RGB bit before AND after cosmetics. Every original GPU coefficient word across all patterns/phases/directions matches independently.
- Existing common gain/offset, native-sample, rotation, phase, stars/flux, halo/tile, mixed-exposure and rejection gates remain PASS.
- DS-B: 32 inputs, 27 accepted/5 rejected, 270 s accepted exposure. All three entire FITS files exactly match both the fresh control and prior verified CPU products. Scientific reports, rejection/export counts and work counters remain exact.
- DS-A: 1,140 inputs, 940 accepted/200 quality rejected, 9,400 s accepted exposure. All three entire FITS files exactly match both the fresh control and owner-accepted M4 hashes. All scientific selection, geometry, photometry, exposure, valid/rejected/export and work/I/O counters remain exact.
- All 1,140 source hashes/inventory, 32 local sources, two PI references and external-template hash remain unchanged. No image intermediates or temporary leftovers; GPS retained in scientific FITS only and redacted from default reports/logs.
- Comparison ignores timing/RSS and explicitly permitted available-RAM snapshots/derived headroom only. Tile geometry, allocation estimates, total/reserved RAM, cache-off settings/counters and science remain strict.

The output grid is 4000 x 4000 at (0,0), reference canvas, no crop; both maps use the original 1,140-input denominator. Valid channel samples 44,271,162,492; low rejected 85,995,586; high rejected 698,062,316. Final upper limits 170 samples, as before. Exact accepted FITS bytes preserve prior owner visual review. Existing narrow analytic CFA/VNG limitation and unmatched PixInsight-workload qualification remain; no new absolute photometry or PI quality-equivalent speedup claim is made.

## Fresh paired measurement protocol

Preserve unchanged `d39ab46d16f84c7990ab528566d6f9c820e3ac62` and compare against the measured revision above. Same locked Rust toolchain/profile/features: GPU-enabled build with the CPU engine, thin LTO and one codegen unit. Both commands explicitly select 16 workers, 512 x 512 tiles, 12288 MiB ceiling, cache 0, 3.0/2.5 clipping and the same full reference. Run before then after sequentially with no concurrent local builds/tests. Source hashes checked before and after. Historical 501.305 s from the resampling review is context, not this task's control.

Baseline executable SHA256: `8fc1df11c4ce7803868b707edc0b18a5b1e6741111a7d58673ff1c15a58d4cae`.

Optimized executable SHA256: `9ba3a65db6ce32db57667028ce61249f4fc997c1b328e85f2e409b34f9a20740`.

| Full-set measurement, seconds | Before | After |
|---|---:|---:|
| Processing through final FITS writes | 510.694 | 438.435 |
| Complete command including report publication | 511.258 | 439.038 |
| Inspection (unchanged) | 15.729 | 14.318 |
| Registration (unchanged) | 28.372 | 27.601 |
| Reconstruction wall | 334.964 | 260.558 |
| Rejection / accumulation / maps (unchanged) | 121.559 | 125.990 |
| Output copy | 0.349 | 0.340 |
| FITS writes | 0.915 | 0.899 |

The observed processing speed ratio is 1.1648; full-input throughput after the change is **2.600 frames/s**, including quality-rejected inputs. Reconstruction is **74.406 s shorter**; unchanged inspection/registration are 2.182 s shorter, while unchanged rejection/accumulation/maps are 4.431 s longer. Retain these variations; do not attribute changes in unmodified phases to VNG. This is one descriptive sequential pair with existing OS caches/background load, without confidence intervals or a portable/isolated causal speedup guarantee.

Peak RSS is **3422.172 / 3404.977 MiB**. Both retain the same conservative 4561.8 MiB allocation estimate, 3186.6 MiB system reserve on 31865.9 MiB physical RAM and cache accounted capacity 0. Direct ROI workload stays 11,430,134,434 application bytes, 18,619,113 row reads, 59,983 opens/metadata checks and 5,715,067,217 decoded ROI pixels. Counters are not physical SSD traffic.

## Overlapping ROI task elapsed

| Summed task elapsed, seconds | Before | After |
|---|---:|---:|
| read_seconds | 600.576 | 578.713 |
| cosmetic_seconds | 1043.631 | 1056.260 |
| demosaic_seconds | 1696.479 | 661.450 |
| resampling_seconds | 1491.353 | 1491.302 |

Summed demosaic task elapsed is **61.010% shorter**. These task intervals overlap across workers and may include scheduling/wait effects; they are not additive wall phases, CPU execution time or utilization percentages. Do not interpret the roughly 1,035 s difference as an end-to-end wall saving. Reconstruction/full processing wall are the performance evidence above.

## 32-frame result

Processing **18.043 / 16.145 s** (10.519% shorter), reconstruction **9.831 / 7.574 s** (22.951% shorter), peak RSS **653.645 / 674.102 MiB**. Complete command **18.487 / 16.627 s**. Summed overlapping VNG task elapsed **41.700 / 16.067 s**. Exact entire master/map/source/scientific gates PASS. Retain the slightly higher small-run RSS; no general memory-reduction claim is made.

## Full-set exact product hashes

| Product | SHA256: before = after = accepted M4 |
|---|---|
| master | `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc` |
| coverage | `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5` |
| high_rejection | `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970` |

## Evidence and reproduction

- `outputs/M5_CPU_VNG_performance.json`: compact full evidence, all phases, settings, binary hashes, gates and limitations.
- `outputs/M5_vng_DS-A_equivalence.json` / `M5_vng_DS-B_equivalence.json`: entire-file/scientific/source integrity gates.
- `outputs/M5_vng_DS-A_after_external200.fits`, `_coverage.fits`, `_high_rejection.fits`: final full-set products.
- `outputs/M5_vng_DS-A_after_external200.json`: detailed scientific report.
- DS-B final prefix: `outputs/M5_vng_DS-B_after_external200`.
- `outputs/M5_vng_DS-A_command_timings.json` and DS-B counterpart: exact commands, preserved executable hashes and complete-command times.
- `outputs/M5_VNG_automated_gates.json`, `M5_VNG_CI_jobs.json`: local and measured-code CI checks.

Run from the project root using new destinations. Both commands also generate coverage/high-rejection maps. Open FITS normally in PixInsight; accepted pixel values, reference grid and units are unchanged. GPU remains optional and explicitly selected; these measurements use CPU.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_vng_review32.fits" --report "outputs/M33_vng_review32.json"
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_vng_review1140.fits" --report "outputs/M33_vng_review1140.json"
```
