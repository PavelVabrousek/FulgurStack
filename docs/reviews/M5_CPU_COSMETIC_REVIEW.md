# M5: Exact CPU Cosmetic Correction Optimization

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. Scientific revision: `d9949a05810c9248af4d31a95e77d807d8c31f83`. M5 remains open. Cache stays **off**. Local scientific/release gates PASS; remote Windows/macOS CI is **BLOCKED**, not passed: GitHub refuses to start jobs under the account billing/spending restriction.

The fresh full 1,140-frame pair with RegisteringTemplate200pct.fit measured **447.335 -> 420.343 s** processing (26.991 s / 6.034% shorter). Reconstruction measured **271.522 -> 252.006 s** (7.188% shorter). All three entire FITS products match the fresh control and accepted CPU/M4 hashes exactly. Single-pair observations on this computer do not establish an isolated causal or portable speedup.

## Behavior and verification

Keep the original eight-value total-order sort, median expression and noise floor of at least 1 ADU. A sample <= median + 5 ADU cannot exceed the original median + 5 * noise threshold. Skip only its unnecessary eight-neighbor MAD. Other pixels retain the original MAD, thresholds and 24-neighbor confirmation. Load the same eight samples through three row centers. Copy each immutable source row once, process its interior and retain the four-pixel border. Stop stellar support after two qualifying neighbors establish the same boolean result. Decisions always read the original CFA. No new buffer/cache/dependency/intermediate image; no scientific or allocation policy change. Optional GPU remains explicit and its cosmetics arithmetic is unchanged.

- Format and default/all-feature Clippy with warnings denied: PASS.
- Release tests **98/98 default**, **100/100 all-feature**, PASS; real source fixtures and actual GPU hardware gates explicitly enabled and PASS.
- Complete previous scalar cosmetics retained as an independent test-only oracle and verified against baseline source. Exact corrected CFA bits/counts and unchanged inputs across Bayer phases, minimum/odd/asymmetric dimensions, fractional/noisy/gradient fields, isolated/border defects, saturated colored stars, real input and finite extreme/fractional noise-floor boundaries. Cancellation/invalid buffers PASS.
- Existing flux/star protection, mixed-exposure, scaled-reference, ROI-halo, tile/rejection and GPU quality gates remain PASS.
- DS-B: 32 inputs, 27 accepted/5 quality rejected, 270 s accepted exposure; entire master/coverage/high FITS bytes match fresh controls and prior CPU products.
- DS-A: 1,140 inputs, 940 accepted/200 quality rejected, 9,400 s accepted exposure; entire products match fresh controls and accepted M4. Scientific reports, geometry, photometry, all rejection/export/work/I/O and memory-plan fields remain exact.
- All 1,140 input hashes/inventory, 32 local inputs, two PI references and external-template hash unchanged. No image intermediates/temp leftovers. GPS retained only in scientific FITS, redacted from default reports/logs.

The verifier uses **only --allow-ram-snapshot-change**: available RAM and derived effective headroom/candidate-fit flags can vary. Total/reserved RAM, allocations, selected tile geometry, cache-off telemetry and all scientific/work fields remain strict. No layout/cache/geometry exception.

Remote CI could not start. GitHub annotations report failed recent payments or a spending limit requiring attention; macOS additionally reports runner capacity constraints. Run URLs, job states and original annotations are in `outputs/M5_cosmetic_final_CI_jobs.json`. Local gates do not substitute for new macOS validation. No billing settings changed and no remote pass claimed. Prior measured-code CI is historical evidence only.

## Fresh full-set measurements

Baseline `bc3271d44aa0208a59a4043a740de4db20df1724` versus measured revision above: same locked toolchain/profile/features, thin LTO/one codegen unit, GPU-enabled binaries running the **CPU** engine, 16 workers, fixed 512 x 512 tiles, 12288 MiB ceiling, explicit cache 0, unchanged 3.0/2.5 clipping and complete external 4000 x 4000 canvas at (0,0), without crop. Sequential before then after, no concurrent local builds/tests, source hashes before/after. OS caches and background load uncontrolled.

Baseline binary SHA256: `55ea118cc8766f5a1e1360a4fabab45b6b85b3baf8bc25145659c0106c98a74b`.

Optimized binary SHA256: `9005daef3f8b1c75ae457e075cf00d937b3834cc04c7aab60e8412a1bf06deb0`.

| Measurement, seconds | Before | After |
|---|---:|---:|
| Processing through final FITS writes | 447.335 | 420.343 |
| Complete command including report publication | 447.858 | 420.948 |
| Inspection (unchanged) | 13.132 | 13.592 |
| Registration (unchanged) | 26.733 | 28.992 |
| Reconstruction | 271.522 | 252.006 |
| Rejection/maps (unchanged) | 125.002 | 115.999 |
| FITS writes (unchanged) | 1.025 | 1.046 |

Peak RSS **3411.090 -> 3405.703 MiB**. Planned allocation remains **4,789,190,656 bytes**, retaining the system reserve and 12 GiB ceiling. Throughput after optimization: **2.712 input frames/s**, including rejected frames.

Summed cosmetic task elapsed **1138.230 -> 930.411 s** (18.258% shorter). Task timers overlap and include scheduling/waiting; they are not additive wall phases or CPU execution time. Processing and nonoverlapping reconstruction wall are the primary evidence. Unchanged inspection/registration takes 2.718 s longer, while unchanged rejection/maps takes 9.003 s less. The latter is part of the 26.991 s total saving but is not evidence of a cosmetic-code speedup. Keep these variations without attributing them all to cosmetics.

Application ROI I/O counters are unchanged and are not physical SSD traffic. Map denominator remains 1,140 original inputs. Valid channel samples 44,271,162,492; low rejected 85,995,586; high rejected 698,062,316; upper export limits 170/lower 0. Existing narrow analytic CFA/VNG and unmatched PixInsight workload limitations remain; no new absolute photometry or PI quality-equivalent speedup claim.

## Small set and retained adverse candidate

Final DS-B processing **16.161 -> 16.030 s** (0.814% shorter), reconstruction **7.649 -> 7.322 s**, rejection/maps **4.883 -> 4.916 s**, peak RSS **685.395 -> 662.258 MiB**. Summed cosmetic task elapsed is 11.976% shorter. Keep all unchanged-phase variation and unfavorable observations.

A preliminary 19-comparator sorting-network candidate passed exact product gates but regressed DS-B processing **15.523 -> 16.646 s** and summed cosmetic task elapsed **25.755 -> 37.068 s**. It is **not retained**. Its binary/products/timings/exactness gate remain under the original M5_cosmetic_DS-B prefix, separately from final M5_cosmetic_final products. No overwrite of that evidence.

## Exact full-set product hashes

| Product | SHA256: before = after = accepted M4 |
|---|---|
| master | `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc` |
| coverage | `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5` |
| high_rejection | `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970` |

## Evidence and owner commands

- `outputs/M5_CPU_cosmetic_performance.json`: compact evidence, both final gates, protocols, adverse candidate and remote CI block.
- `outputs/M5_cosmetic_final_DS-A_equivalence.json` / DS-B counterpart: full exact product/scientific/work/input checks.
- `outputs/M5_cosmetic_final_DS-A_after_external200.fits`, `_coverage.fits`, `_high_rejection.fits`: final full products; detailed report uses the same prefix and `.json`.
- Final DS-B prefix: `outputs/M5_cosmetic_final_DS-B_after_external200`.
- `outputs/M5_cosmetic_final_DS-A_command_timings.json` / DS-B counterpart: commands, binary hashes and complete-command times.
- `outputs/M5_cosmetic_final_automated_gates.json` / `M5_cosmetic_final_CI_jobs.json`: local gates / remote restriction.

Run from the project root with unused output names. Both maps are generated beside the master. Open FITS normally in PixInsight; accepted pixels/grid are unchanged.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_cosmetic_review32.fits" --report "outputs/M33_cosmetic_review32.json"
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M33_cosmetic_review1140.fits" --report "outputs/M33_cosmetic_review1140.json"
```
