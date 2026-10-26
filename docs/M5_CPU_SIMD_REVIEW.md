# M5: Exact SIMD CPU Resampling

Date: 2026-10-22. Work remains **local and uncommitted** on Git base `b836b93e3045164007efaf93d88354fc4edb0a8f`. No commits, pushes, PR updates or remote CI. GitHub Actions are disabled by owner policy and are not a delivery blocker. All gates below were executed locally on Windows x86_64; no new macOS execution is claimed. M5 remains open. FITS cache stays **off**.

The fresh full 1,140-frame pair with RegisteringTemplate200pct.fit measured **387.618 -> 350.600 s** processing (9.550% shorter). Reconstruction measured **222.171 -> 184.033 s** (17.166% shorter). Entire master/coverage/high FITS match fresh controls and accepted CPU/M4 products exactly. Single-pair observations with existing OS caches/background load do not establish isolated causal or portable acceleration.

## Implementation and safety

Runtime-detect AVX2 once per ROI; keep the original scalar path on unsupported CPUs/architectures. Compute four independent output coordinates and Keys coefficient sets in f64 SIMD lanes. Accumulate two RGB triplets plus ignored register-only padding in each f32 vector. Preserve exact inverse-coordinate and Keys polynomial expressions, f32 weight conversion, separate multiplication/addition without FMA, all sixteen row-major contributions per channel and f64 photometric normalization before f32 output. No approximate weights, interpolation change, precision reduction or regrouped sums. Mixed-validity groups and partial tails use the scalar sampler. VNG/cosmetics, geometry, scientific units, clipping, Neumaier order, maps and final limiting remain unchanged.

Use standard-library intrinsics with runtime CPU/OS checks. Validate each complete 4x4 footprint against sensor, ROI and the actual backing Vec. Use contiguous four-float RGB loads only when a following pixel proves the fourth float exists; assemble the last triplet from exactly three checked values. Ignore padding lanes throughout output/finiteness checks. Validate exact integer-to-f64 coordinate range. Fixed-size lane/weight arrays are stack-only, covered by conservative runtime headroom. No whole-image padded RGB copy, extra image buffer, image cache/intermediate or dependency. Existing continuous <=16 logical ROI scheduling, all conservative allocation estimates and system reserve remain unchanged. The optional GPU engine keeps its reconstruction math and shares this exact CPU warp.

## Local verification

- Format and default/all-feature Clippy with warnings denied: PASS.
- Release suites **104/104 default**, **106/106 all-feature**, PASS. SIMD hardware explicitly required with FULGURSTACK_TEST_SIMD=1; actual GPU hardware explicitly required with FULGURSTACK_TEST_GPU=1. Both PASS without silent skipping. Real source fixtures explicitly configured.
- Exact scalar/SIMD RGB bits and invalid masks across scales 0.33/0.5/1/2/3, five rotations, three positive/negative origins, full/cropped ROIs, empty/short/odd/partial-tail rows and signed/fractional/subnormal/saturated fields. Source arrays remain bitwise unchanged.
- Keys weights match every scalar bit at integer and adjacent-representable boundaries and fractional sweeps. The final backing RGB triplet loads safely and matches scalar bits; invalid lengths/nonfinite results are rejected. Existing independent full-warp/cancellation/halo/flux/star/rejection/GPU gates PASS.
- DS-B: 32 originals, 27 accepted/5 quality rejected, 270 s accepted exposure. Entire master/maps equal fresh controls and previous accepted CPU products.
- DS-A: 1,140 originals, 940 accepted/200 quality rejected, 9,400 s accepted exposure. Entire products equal fresh controls and accepted M4 hashes. All scientific reports, work/I/O, corrected/rejected/export counters and allocation plans remain exact.
- All 1,140 source hashes/inventory, 32 local originals, two PI references and external-template hash unchanged. No image intermediates or temporary leftovers. GPS stays in scientific FITS, redacted from default reports/logs.

Only --allow-ram-snapshot-change is enabled in the verifier. Available RAM and derived effective budget/candidate-fit flags can differ. Total/reserved RAM, selected tiles, allocation estimates, cache-off telemetry and every scientific/work field stay strict. No schema/layout/cache/geometry exception.

## Fresh paired performance

Both preserved binaries use the same locked release toolchain/profile/features, thin LTO/one codegen unit, GPU-enabled build with **CPU engine**, 16 threads, 512-square tiles, 12288 MiB ceiling, explicit cache 0, sigma 3.0/2.5 and the same complete 4000-square reference at (0,0), without crop. Sequential before then after, no concurrent local builds/tests, source/reference hashes before/after. No OS-cache flushing or background-load control. Historical 383.990 s from the scheduling review is context, not this task's control.

Baseline executable SHA256: `9a7546068aaad86ef569aae49675349d9b8a04b05fed1e530a9d0d7b11d1b9c5`.

Optimized executable SHA256: `278828f47ca95926c0a679918ed8f265959074aa993a270b06773f901e968ccb`.

| Measurement, seconds | Before | After |
|---|---:|---:|
| Processing through final FITS writes | 387.618 | 350.600 |
| Complete command including report publication | 388.309 | 351.183 |
| Inspection (unchanged) | 16.681 | 13.990 |
| Registration (unchanged) | 21.047 | 27.323 |
| Reconstruction including scheduling/I/O | 222.171 | 184.033 |
| Rejection/maps (unchanged) | 116.356 | 115.202 |
| FITS writes (unchanged) | 1.029 | 1.013 |

Peak RSS **3399.652 -> 3391.395 MiB**; planned allocation remains **4,789,190,656 bytes**. Throughput after change **3.252 original inputs/s**, including quality rejections. No general memory-reduction claim.

Summed resampling task elapsed **1456.360 -> 773.448 s** (46.892% shorter). These timers overlap and include scheduling/waiting: they are not additive wall phases or CPU execution/utilization measurements. Full processing and nonoverlapping reconstruction wall are the principal evidence. Unchanged rejection/maps takes **1.155 s less**, while unchanged inspection/registration takes **3.584 s longer**. Keep these variations separately from the affected reconstruction phase. Do not credit all phase variation to SIMD.

Direct ROI I/O/work remains 11,430,134,434 application bytes, 18,619,113 reads, 59,983 opens/checks and 5,715,067,217 decoded ROI pixels. These counters are not physical SSD traffic. Coverage/high maps keep denominator 1,140 original inputs. Valid channel samples 44,271,162,492; low rejected 85,995,586; high rejected 698,062,316; upper export limits 170/lower 0. Prior narrow analytic CFA/VNG and unmatched PixInsight workload limitations remain; no new absolute-photometry or PI quality-equivalent speedup claim.

## Small set and adverse preliminary candidate

Final DS-B processing **15.316 -> 15.118 s** (1.292% shorter), reconstruction **6.890 -> 5.780 s** (16.103% shorter), rejection/maps **4.843 -> 5.126 s**, RSS **707.855 -> 688.355 MiB**. Keep unfavorable unmodified-phase observations and avoid a universal speedup claim.

The first four-output indexed-gather candidate passed exactness gates but processing regressed **15.309 -> 16.088 s**, reconstruction **6.728 -> 6.937 s**, while summed resampling task elapsed was **42.625 -> 41.513 s**. It is not retained. Local preliminary binary/source/fingerprints/products/timings/gate remain under M5_SIMD_DS-B and target/m5-simd-preliminary prefixes, separate from final M5_SIMD_final products. This demonstrates why an affected task timer alone cannot establish end-to-end acceleration.

## Exact full-set hashes

| Product | SHA256: before = after = accepted M4 |
|---|---|
| master | `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc` |
| coverage | `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5` |
| high_rejection | `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970` |

## Evidence and owner commands

- `outputs/M5_CPU_SIMD_performance.json`: complete compact local evidence and both final gates, phases/memory/commands/hashes, preliminary adverse result and source fingerprints.
- `outputs/M5_SIMD_baseline_source.json`, `M5_SIMD_optimized_source.json`, `M5_SIMD_preliminary_source.json`: exact source hashes and Git base; optimized code is explicitly uncommitted, not mislabeled as the base revision.
- `outputs/M5_SIMD_final_DS-A_equivalence.json` / DS-B counterpart: entire FITS/scientific/work/input checks.
- `outputs/M5_SIMD_final_DS-A_after_external200.fits`, `_coverage.fits`, `_high_rejection.fits`: final full products; detailed report uses the same prefix and `.json`.
- DS-B final prefix: `outputs/M5_SIMD_final_DS-B_after_external200`.
- `outputs/M5_SIMD_final_DS-A_command_timings.json` / DS-B counterpart and `M5_SIMD_automated_gates.json`: exact commands/binary hashes and local gates.

Run locally from the project root using new destination names; both maps are generated beside the master. Open FITS normally in PixInsight; accepted pixels/grid remain identical.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_SIMD_review32.fits" --report "outputs/M33_SIMD_review32.json"
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_SIMD_review1140.fits" --report "outputs/M33_SIMD_review1140.json"
```
