# M5: Exact CPU Resampling Optimization

Date: 2026-10-22. Scientific revision: `29bbdaedca9cdb2849c2ea32ccf5d3517251ae9e`. M5 remains open. FITS cache stays **off** by default and was explicitly disabled in every measured run.

The final paired full-set run processed all 1,140 inputs against RegisteringTemplate200pct.fit on its complete 4000 x 4000 grid at (0,0), without cropping. Processing measured **523.466 -> 501.305 s**, a **22.160 s / 4.233%** reduction in this pair. Reconstruction wall measured **344.081 -> 329.472 s** (4.246% shorter). Master, coverage and high-rejection FITS are byte-identical to the control and owner-accepted M4 products.

## What changed

Reuse exact inverse-rotation products per output column/row. Validate the same footprint, use positive-coordinate integer truncation equivalent to floor, choose the unchanged Keys polynomial for its known footprint position, and access four contiguous source pixels per row. Keep the original coefficient expressions, all sixteen row-major f32 additions and f64 geometry/photometric normalization. No coordinate stepping, reassociated sums, approximation, reduced precision, changed masks or new scientific policy. Temporary geometry is at most 16 KiB per worker, covered by the existing runtime reserve; no image cache or intermediate image is added. The optional GPU engine retains its existing policy and shares the CPU warp.

## Quality and automated gates

- Format and default/all-feature Clippy, warnings denied: PASS.
- Release suites: **92/92 CPU/default**, **93/93 all-feature**, PASS. Real GPU hardware gates were explicitly required locally and passed.
- [Windows MSVC and macOS Apple Silicon CI for the measured revision](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39800116113): PASS, including default/all-feature builds, release tests and import-adapter checks. CI hardware GPU gates remain explicit local checks.
- Differential bit/mask comparison against the complete prior scalar sampler/transform: PASS over scales 0.33/0.5/1/2/3, five angles, negative/positive origins, full/cropped ROIs, high-dynamic-range/negative/sub-ADU RGB and nontrivial photometry. Independent coefficient tests cover integer boundaries and their adjacent representable values. Cancellation, existing flux/halo/tile/mixed-exposure/rejection gates pass.
- DS-B: 32 inputs, 27 accepted/5 rejected; entire master and both maps byte-identical to the before control AND prior verified CPU products. All scientific report fields, rejection/export counts and work counters remain unchanged.
- DS-A: 1,140 inputs, 940 accepted/200 quality rejected, 9,400 s accepted exposure; all three whole FITS hashes match the control AND owner-accepted M4. All 1,140 source hashes/inventory, 32 local sources, two PI references and external-template hash remain unchanged.
- Exact comparison permits only timing/RSS and explicitly requested OS available-RAM snapshots/derived headroom to differ. Configuration, total/reserved RAM, selected tiles, allocation estimates, cache-off counters, registration, photometry and scientific counts remain strict.
- No image intermediates or temporary leftovers; source bytes unchanged. GPS is retained in scientific FITS and redacted from default reports/logs.

The final grid is 4000 x 4000, origin (0,0), reference canvas, no crop. Coverage/high-rejection maps retain denominator 1,140 original inputs. Valid channel samples: 44,271,162,492; low rejected: 85,995,586; high rejected: 698,062,316. Export limits 170 upper samples, as before. Existing narrow analytic CFA/VNG limitation and unmatched PixInsight-workload qualification remain. Byte equality preserves the previously reviewed visual result; no new absolute-photometry claim is made.

## Measurement protocol

Preserve the unchanged `8267d7c240d0809f7b049bf1bc00171aa6649b1e` release executable and compare it with the measured revision above. Same locked toolchain/profile/features, GPU-enabled build with **CPU engine** in both binaries, thin LTO, one codegen unit. Explicit 16 workers, 512 x 512 tiles, 12288 MiB ceiling, cache 0, 3.0/2.5 rejection and identical reference canvas. Run before then after, sequentially, with no concurrent local builds/tests. Hash sources before and after. Do not substitute historical cache/tile runs for this new control.

Baseline executable SHA256: `6b96a4a0277a996a47e48d04ca4b0fd3ec153ce92ccd02b5623932030c730c91`.

Optimized executable SHA256: `8fc1df11c4ce7803868b707edc0b18a5b1e6741111a7d58673ff1c15a58d4cae`.

Physical RAM is 31865.9 MiB, system reserve 3186.6 MiB. The static allocation estimate is unchanged at 4561.8 MiB for the full set; measured peaks are **3409.789 / 3416.391 MiB**, below the 12 GiB ceiling. Cache accounted capacity is exactly 0 in both runs. Direct ROI read workload is unchanged: 11,430,134,434 bytes, 18,619,113 reads, 59,983 opens/metadata checks. Decoded ROI pixels remain 5,715,067,217. These are application counters, not physical SSD traffic.

## Full-set wall times

| Measurement, seconds | Before | After |
|---|---:|---:|
| Processing through final FITS writes | 523.466 | 501.305 |
| Complete command including report publication | 524.045 | 501.922 |
| Inspection (unchanged) | 16.272 | 14.620 |
| Registration (unchanged) | 31.841 | 27.399 |
| Reconstruction wall | 344.081 | 329.472 |
| Rejection / accumulation / maps (unchanged) | 120.342 | 120.050 |
| Output copy | 0.362 | 0.343 |
| FITS writes | 0.906 | 1.008 |

Full-input throughput after the change is 2.274 frames/s (includes quality-rejected inputs). The observed end-to-end speed ratio is 1.0442. Existing OS caches/background load and run order are uncontrolled; this is one descriptive pair on this computer, without confidence intervals or a portable speedup guarantee. **6.094 s of the total difference is in unchanged inspection/registration**, so do not attribute the entire 22.160 s to resampling. Reconstruction is the relevant nonoverlapping wall phase for the modified code.

## Overlapping ROI task timers

| Summed task elapsed, seconds | Before | After |
|---|---:|---:|
| read_seconds | 596.303 | 570.800 |
| cosmetic_seconds | 1040.464 | 1038.252 |
| demosaic_seconds | 1689.138 | 1719.838 |
| resampling_seconds | 1596.192 | 1498.974 |

Resampling's summed task elapsed is 6.091% shorter. These intervals overlap across workers and can include scheduling/wait effects; they are not CPU execution time, additive wall phases or CPU/GPU utilization percentages. Do not sum this table or infer an isolated 97.218 s wall-clock saving from the resampling difference.

## Smaller run and retained unfavorable measurements

Final DS-B processing: **18.614 / 18.205 s** (2.199% shorter); reconstruction **10.280 / 10.023 s**; peak RSS **663.391 / 682.723 MiB**. Whole FITS equality and source/report gates pass.

A preliminary candidate using column/row reuse and contiguous footprint access, before direct polynomial dispatch/floor removal, passed exact FITS gates but measured **20.330 / 20.789 s** (slower). Preserve that result; it was not promoted as a speedup. An additional baseline-only run measured approximately 19.594 s before a timing-collector field-name error; the helper was fixed and fresh complete pairs were run. That interrupted collection is not part of the final paired comparison. Report and processing times are distinguished in the JSON evidence.

## Full-set product hashes

| Product | Exact SHA256, before = after = accepted M4 |
|---|---|
| master | `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc` |
| coverage | `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5` |
| high_rejection | `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970` |

## Evidence and owner commands

- `outputs/M5_CPU_resampling_performance.json`: compact complete evidence, protocol, binary hashes, phase timing, both exact gates and retained unfavorable result.
- `outputs/M5_resampling_DS-A_equivalence.json`, `outputs/M5_resampling_DS-B_equivalence.json`: exact reports/products and immutable-input checks.
- `outputs/M5_resampling_DS-A_after_external200.fits`, `_coverage.fits`, `_high_rejection.fits`: full final products.
- Detailed scientific report: `outputs/M5_resampling_DS-A_after_external200.json`.
- DS-B final prefix: `outputs/M5_resampling_DS-B_after_external200_v3`.
- `outputs/M5_resampling_DS-A_command_timings.json` and DS-B counterpart record exact arguments, complete-command times and executable hashes.

Run from the project root using new destinations. Both commands also export the coverage and high-rejection maps. Open FITS normally in PixInsight; the reference grid and accepted pixel values are unchanged.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_cpu_resampling_review32.fits" --report "outputs/M33_cpu_resampling_review32.json"
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_cpu_resampling_review1140.fits" --report "outputs/M33_cpu_resampling_review1140.json"
```
