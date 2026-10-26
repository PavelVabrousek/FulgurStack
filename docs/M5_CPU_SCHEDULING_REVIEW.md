# M5: Continuous Bounded CPU Reconstruction Scheduling

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. Scientific revision: `54f6ea12cbfc8e80ed9dab0c578d29d84776b592`. M5 remains open. FITS cache stays **off**. Local scientific/release gates PASS. Remote CI: **BLOCKED**.

The fresh full 1,140-frame pair with RegisteringTemplate200pct.fit measured **408.271 -> 383.990 s** processing (5.947% shorter). Reconstruction including scheduling/I/O measured **246.191 -> 220.943 s** (10.255% shorter). Entire master/coverage/high FITS match the fresh control and accepted CPU/M4 products exactly. Preserve the slower small-set result below. These single-pair observations do not establish portable or isolated causal acceleration.

## Scheduling and bounded memory

Replace CPU barriers between groups of 16 sources with at most min(Rayon threads,16,source count) long-lived scoped tasks. A task atomically claims one source, completes its entire immutable ROI reconstruction and warp, writes its result exactly once to that source's slot, then claims another source. Nested Rayon work can run other logical workers but cannot create more logical ROI holders. Never use an unbounded source parallel iterator or acquire blocking permits from arbitrary nested tasks.

Join all tasks and gather in original source order. Retain the same frame-major registered samples for the current output tile; only small source/result metadata is additional. Never retain raw/cleaned/RGB source data after its ROI completes. Bound residency to <=16 regardless of source count, with the unchanged conservative allocation plan, system reserve and 12 GiB ceiling. Peak-RSS checks occur after each worker-count group of completions and the final partial group, retaining the previous check frequency and Windows peak guard.

Observed errors/cancellation stop new claims; join all live work and release buffers before returning. Original GPU dispatch/readback batches remain unchanged. No new dependency, image cache, extra image buffer, disk intermediate, schema or scientific-policy change. Preserve all f32/f64 arithmetic, cosmetics/VNG/photometry/geometry, source ordering, validity, clipping, Neumaier sums, maps and final saturation.

## Verification

- Format/default and all-feature Clippy, warnings denied: PASS.
- Release tests **101/101 default**, **103/103 all-feature**, PASS. Real source fixtures and actual local GPU gates explicitly enabled and PASS.
- Dedicated forward-progress gate holds the first source until a source beyond its original group starts. Continuous scheduling passes; waiting for that group's completion would fail. A bounded timeout prevents a hanging test.
- Exactly-once source/result bit ordering over populations 0/1/2/17/97 and logical limits 1/3/16 in an eight-thread pool with nested parallel work. Active ROI counter never exceeds the logical limit. Cancellation/error cleanup and invalid limits PASS.
- Existing flux/phase/star protection, mixed-exposure, scaled-reference, ROI-halo, tile/clipping and real GPU gates remain PASS.
- DS-B: 32 inputs, 27 accepted/5 quality rejected, 270 s exposure; entire master/maps equal fresh controls and previous accepted CPU files.
- DS-A: 1,140 inputs, 940 accepted/200 quality rejected, 9,400 s exposure; entire products equal fresh controls and accepted M4 hashes. Scientific/work/I/O, corrected/rejected/export counts and all memory-plan estimates remain exact.
- All 1,140 source hashes/inventory, 32 local sources, two PI references and external-template hash unchanged. No image intermediates/temp leftovers. GPS remains in scientific FITS only, redacted from default reports/logs.

Only --allow-ram-snapshot-change is enabled in the verifier: available RAM and derived effective budget/candidate-fit flags can vary. Total/reserved RAM, cache-off fields, selected tiles, allocation estimates and science/work stay strict. No scheduling/schema, layout, cache or geometry comparison exception.

Remote Windows/MSVC and macOS/Apple Silicon CI is BLOCKED: GitHub refused to start both jobs under the account payment/spending restriction. This is not a remote test pass; local Windows gates do not substitute for new macOS execution. Run: https://github.com/PavelVabrousek/FulgurStack/actions/runs/39819941521. Original annotations and job states are recorded locally in `outputs/M5_scheduling_CI_jobs.json`. No billing settings were changed.

## Fresh full-set measurements

Baseline `4563b372590990b41529c1300eabf1790ace02f4` versus measured revision above: same locked toolchain/profile/features, thin LTO/one codegen unit, GPU-enabled binaries with **CPU engine**, 16 threads, fixed 512 x 512 tiles, 12288 MiB ceiling, cache explicitly 0, sigma 3.0/2.5, same external 4000 x 4000 reference canvas at (0,0), no crop. Sequential before then after with no concurrent local builds/tests, source/reference hashes before/after. Existing OS caches/background load are uncontrolled; no confidence interval or isolated causal/universal speedup claim.

Baseline executable SHA256: `9005daef3f8b1c75ae457e075cf00d937b3834cc04c7aab60e8412a1bf06deb0`.

Optimized executable SHA256: `9a7546068aaad86ef569aae49675349d9b8a04b05fed1e530a9d0d7b11d1b9c5`.

| Measurement, seconds | Before | After |
|---|---:|---:|
| Processing through final FITS writes | 408.271 | 383.990 |
| Complete command including report publication | 408.967 | 384.464 |
| Inspection (unchanged) | 13.030 | 13.573 |
| Registration (unchanged) | 23.058 | 27.755 |
| Reconstruction including scheduling and I/O | 246.191 | 220.943 |
| Rejection/maps (unchanged) | 116.346 | 112.861 |
| Output copy (unchanged) | 0.286 | 0.266 |
| FITS writes (unchanged) | 0.949 | 0.878 |

Peak RSS **3416.082 -> 3398.227 MiB**. Planned allocation stays **4,789,190,656 bytes**. Throughput after change **2.969 original inputs/s**, including quality rejections. Keep actual RSS changes; no automatic memory-reduction claim.

Observed processing saving **24.280 s**; directly affected reconstruction saving **25.248 s**. Unchanged rejection/maps takes **3.485 s less**, while unchanged inspection/registration takes **5.240 s longer**. These are separate variations, not evidence of a scheduling-code speedup. Historical 420.343 s from the cosmetics report is context, not this task's control.

ROI task elapsed sums overlap and include scheduling/waiting; they are not additive wall phases or CPU utilization/execution measurements. Preserve every raw timer in the compact evidence rather than interpreting their sums as elapsed processing. Full processing and nonoverlapping reconstruction wall are the principal performance evidence.

Source I/O remains **11,430,134,434 application bytes**, **18,619,113 reads**, **59,983 opens/checks**, **5,715,067,217 decoded ROI pixels**. These are not physical SSD traffic. Maps retain denominator 1,140 originals. Valid channel samples 44,271,162,492; low rejected 85,995,586; high rejected 698,062,316; upper export limits 170/lower 0. Existing narrow analytic CFA/VNG and unmatched PixInsight workload limitations remain; no new absolute-photometry/PI quality-equivalent performance claim.

## Retained small-set result

DS-B processing **15.885 -> 16.558 s** (4.237% longer), reconstruction **7.192 -> 7.602 s** (5.713% longer), rejection/maps **4.964 -> 4.836 s**, RSS **646.270 -> 677.395 MiB**. Small-set performance does not establish an acceleration. Source read task elapsed **6.210 -> 13.900 s**, with exactly the same reads/bytes; elapsed changes alone do not identify physical storage or scheduler causality. Retain all unfavorable observations.

## Exact full-set hashes

| Product | SHA256: before = after = accepted M4 |
|---|---|
| master | `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc` |
| coverage | `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5` |
| high_rejection | `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970` |

## Evidence and owner commands

- `outputs/M5_CPU_scheduling_performance.json`: compact measurements, gates, phases/memory, scientific summaries, commands/hashes and qualifications.
- `outputs/M5_scheduling_DS-A_equivalence.json` / DS-B counterpart: exact whole master/map/report/work/input checks.
- `outputs/M5_scheduling_DS-A_after_external200.fits`, `_coverage.fits`, `_high_rejection.fits`: full final products, detailed report with same prefix and `.json`.
- Final DS-B prefix: `outputs/M5_scheduling_DS-B_after_external200`.
- `outputs/M5_scheduling_DS-A_command_timings.json` / DS-B counterpart: commands and binary hashes/complete-command timings.
- `outputs/M5_scheduling_automated_gates.json`, `M5_scheduling_CI_jobs.json`: local gates and remote CI evidence.

Run from the project root using new destination names; both maps are created beside the master. Open FITS normally in PixInsight; accepted grid/pixels/units remain identical. Optional GPU remains available with its original batching.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_scheduling_review32.fits" --report "outputs/M33_scheduling_review32.json"
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_scheduling_review1140.fits" --report "outputs/M33_scheduling_review1140.json"
```
