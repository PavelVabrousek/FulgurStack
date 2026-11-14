# M5: Exact Pixel-Rejection Data Layout Optimization

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. Scientific revision: `45b58a5dec476d2f1ec90f6cecd25543996efeed`. M5 remains open. FITS cache stays **off** in defaults and both performance controls.

The fresh full 1,140-frame pair against RegisteringTemplate200pct.fit measured **437.471 -> 430.546 s** processing (**6.924 s / 1.583% shorter**). Rejection/accumulation/map generation measured **124.304 -> 118.462 s** (**5.841 s / 4.699% shorter**). All three entire FITS products match both the fresh control and accepted CPU/M4 hashes. Retain the implementation for the observed full-set benefit, while preserving the small-set rejection slowdown and higher RSS below.

## Layout and bounded memory

Keep the existing frame-major registered tile. Copy only 32 neighboring output pixels into a small pixel-major RGB block per active reduction band, retaining exact f32 sample bits and original source indices/order. Short source runs are read contiguously; each destination pixel's frame samples are adjacent. Gather red-channel geometric validity/weights and independent per-channel finite samples from those stacks. Median/MAD/clipping thresholds/iterations, original-sample rejection, f64 Neumaier order, maps, photometry, VNG, geometry and final saturation are unchanged.

Reuse packing, value and clipping buffers across every row/pixel of a band. At most min(Rayon workers,16) bands are resident, even if a caller uses a larger Rayon pool. Check cancellation every block. Never duplicate a complete registered tile, cache source FITS blocks or write image intermediates.

Account for a worst-case additional packing payload of 32 * accepted_sources * workers * 12 bytes in every candidate/manual tile estimate. Full bound is **5,775,360 bytes / 5.508 MiB**; DS-B bound is **165,888 bytes / 0.158 MiB**. Retain the system reserve, RAM-aware tile rules and 12 GiB ceiling. Telemetry records reduction_layout=pixel-major-blocks, rejection_block_pixels=32, max_concurrent_reduction_bands and rejection_packing_buffer_bytes. These are transient working copies from the current tile; FITS cache capacity remains 0.

## Verification

- Format, default/all-feature Clippy with warnings denied: PASS.
- Release suites: **95/95 default**, **97/97 all-feature**, PASS; real local GPU hardware gates explicitly required and PASS.
- [Windows MSVC and macOS Apple Silicon CI for the measured code](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39810470614): PASS, including default/all-feature builds/tests and adapter checks. Hardware GPU parity is verified separately locally.
- Differential gate retains the prior direct frame-major reducer: exact master/map bits, valid/low/high/zero/min/max counts and f64 minimum geometric/surviving-weight bits over populations 1/2/4/5/27/32/940/1500, lengths 1/31/32/33/73, nonzero offsets and four clipping settings. Fields include signed/fractional/saturated values, empty coverage, independent channel NaNs/infinities and sparse populations. Input buffers remain bitwise unchanged; cancellation passes.
- Existing clipping/flux/mixed-exposure/scaled-reference/ROI-halo/tile/border/GPU gates remain PASS.
- DS-B: 32 original inputs, 27 accepted/5 rejected, 270 s accepted exposure. Entire master/coverage/high-rejection FITS match the fresh before control and prior CPU products exactly.
- DS-A: 1,140 original inputs, 940 accepted/200 quality rejected, 9,400 s accepted exposure. Entire master/maps match the fresh control AND owner-accepted M4 hashes. Scientific reports, geometry, photometry, valid/low/high/export and work/I/O counters remain exact.
- All 1,140 source hashes/inventory, 32 local sources, two PI references and external-template hash remain unchanged. No image intermediates or temporary leftovers. GPS retained only in scientific FITS and redacted from default reports/logs.

The output uses the complete external 4000 x 4000 reference canvas at (0,0), without cropping. Maps retain denominator 1,140 original inputs. Valid channel samples 44,271,162,492; low rejected 85,995,586; high rejected 698,062,316; final upper export limits 170, all unchanged. Exact accepted FITS bytes preserve previous owner visual acceptance. Existing narrow analytic CFA/VNG and unmatched PixInsight-workload limitations remain; no new absolute photometry or PI quality-equivalent speedup claim.

The explicit verifier option --allow-rejection-layout-change validates the four new telemetry fields and the precisely calculated extra packing allocation. Only that allocation delta is normalized in planned_buffer_bytes, memory_plan estimates and all three candidate estimates. Timing/RSS and explicitly allowed available-RAM snapshots/derived headroom may differ. Scientific settings, selected dimensions, geometry, total/reserved RAM, cache-off settings/counters and all work/I/O remain strict. No whole allocation plan or scientific block is silently omitted.

## Measurement protocol

Preserve unchanged `392bcf6dcb7ee5eee8231934c2c4ee677ec7941c` and compare with the measured revision above using the same locked toolchain/profile/features, GPU-enabled builds with **CPU engine**, thin LTO and one codegen unit. Both controls explicitly use 16 workers, 512 x 512 tiles, 12288 MiB ceiling, cache 0, 3.0/2.5 clipping and the same full external reference. Run before then after sequentially, without concurrent local builds/tests. Check source/reference hashes before and after. Historical 438.435 s from the VNG review is context, not this task's control.

Baseline executable SHA256: `9ba3a65db6ce32db57667028ce61249f4fc997c1b328e85f2e409b34f9a20740`.

Optimized executable SHA256: `55ea118cc8766f5a1e1360a4fabab45b6b85b3baf8bc25145659c0106c98a74b`.

| Full-set measurement, seconds | Before | After |
|---|---:|---:|
| Processing through final FITS writes | 437.471 | 430.546 |
| Complete command including report publication | 438.033 | 431.308 |
| Inspection (unchanged) | 14.482 | 13.669 |
| Registration (unchanged) | 29.662 | 27.721 |
| Reconstruction (unchanged) | 259.010 | 261.071 |
| Rejection / accumulation / maps | 124.304 | 118.462 |
| Output copy | 0.357 | 0.300 |
| FITS writes | 0.981 | 0.996 |

The observed full processing speed ratio is 1.0161. Full-input throughput after the change is **2.648 frames/s**, including quality-rejected inputs. The directly affected rejection/accumulation/maps phase is 5.841 s shorter. Unchanged inspection/registration save 2.754 s, while reconstruction takes 2.060 s longer. Retain these variations without attributing them to layout. This is one descriptive sequential pair with existing OS caches/background load and no confidence interval, isolated causal or portable speedup guarantee. The gain is modest.

Peak RSS **3405.273 -> 3419.832 MiB** is slightly higher. Conservative planned bytes **4,783,415,296 -> 4,789,190,656**, exactly the 5,775,360-byte packing delta, below the same 12 GiB ceiling with system reserve 3186.6 MiB on 31865.9 MiB physical RAM. Direct ROI workload is unchanged: 11,430,134,434 application bytes, 18,619,113 row reads, 59,983 opens/metadata checks and 5,715,067,217 decoded ROI pixels. These counters are not physical SSD traffic. ROI task timers overlap; they are not additive wall phases or CPU execution time.

## Retained small-set performance

DS-B processing **15.739 / 15.580 s** (1.011% shorter), reconstruction **7.397 / 7.333 s**, rejection/maps **4.577 / 4.811 s** (**5.130% slower**). Peak RSS **654.086 / 681.215 MiB**. The total-time change does not establish a small-set rejection speedup; other unchanged phases also vary. Preserve this unfavorable affected-phase result alongside the positive full-set measurement. No universal acceleration or memory-reduction claim is made.

## Full-set exact product hashes

| Product | SHA256: before = after = accepted M4 |
|---|---|
| master | `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc` |
| coverage | `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5` |
| high_rejection | `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970` |

## Evidence and owner commands

- `outputs/M5_CPU_rejection_layout_performance.json`: full compact evidence, both gates, actual phases/memory, protocol and limitations.
- `outputs/M5_layout_DS-A_equivalence.json` / `M5_layout_DS-B_equivalence.json`: exact entire-product/scientific/source checks and validated allocation delta.
- `outputs/M5_layout_DS-A_after_external200.fits`, `_coverage.fits`, `_high_rejection.fits`: final full-set products.
- `outputs/M5_layout_DS-A_after_external200.json`: detailed scientific report.
- DS-B final prefix: `outputs/M5_layout_DS-B_after_external200`.
- `outputs/M5_layout_DS-A_command_timings.json` and DS-B counterpart: exact commands, preserved binary hashes and complete-command times.
- `outputs/M5_layout_automated_gates.json`, `M5_layout_CI_jobs.json`: local gates and measured-code CI.

Run from the project root with new destination names. Both commands generate coverage/high-rejection maps beside the master. Open FITS normally in PixInsight; accepted pixels/grid/units are unchanged. GPU remains an explicit optional engine, using the same reduction path.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_layout_review32.fits" --report "outputs/M33_layout_review32.json"
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_layout_review1140.fits" --report "outputs/M33_layout_review1140.json"
```
