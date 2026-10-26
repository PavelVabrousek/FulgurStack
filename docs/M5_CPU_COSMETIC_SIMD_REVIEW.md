# M5: Exact SIMD CPU Cosmetic Correction

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. All development and verification are local on Windows x86_64. No commits, pushes, PR updates or remote CI. GitHub Actions remain disabled; no new macOS execution is claimed. M5 remains open. FITS cache stays off.

Only the optimized version was stacked in this task. The owner explicitly requests reusing stored performance/quality controls instead of another reference run. Compared with the stored pixel-rejection result, full processing measured **322.559 -> 282.965 s** (12.275% shorter) and reconstruction **186.705 -> 142.527 s** (23.662% shorter). Exact entire master/coverage/high FITS match the stored outputs and accepted CPU/M4 products.

## Implementation and safety

Runtime-detect AVX2 once per source ROI. Process eight independent neighboring CFA pixels per vector; use the unchanged scalar arithmetic/decisions on other architectures or unsupported CPUs and at partial tails. Load eight same-phase neighbor vectors directly from immutable source data. Convert each float encoding to the reversible signed-integer ordering key used by f32::total_cmp, sort the eight ranks with integer min/max, and invert the two central keys. This preserves signed zero and the full float encoding order without float min/max ambiguities. Preserve the original f32 median sum/half, absolute deviations, MAD, multiplication by 1.4826, 1 ADU noise floor and separate unfused candidate-threshold operations.

First exclude groups with no sample above median+5 ADU, as in the accepted floor shortcut. Compute vector MAD/first candidate decisions for remaining groups. Confirm candidate lanes using the original scalar 24-neighbor noise and stellar-support checks, with identical thresholds and operation order. Keep four outer rows/columns unchanged, source samples immutable, border/tail handling and cancellation at every row. Validate exact backing and destination dimensions and the row border before unsafe dispatch; every eight-float neighbor load remains within its validated source row. AVX2/OS detection guards every target-feature call. Fixed stack lane arrays only; no extra image/cache buffer, allocation plan, dependency or schema. Same <=16 logical ROI workers, VNG, SIMD warp, rejection/Neumaier/map/export math. GPU reconstruction remains optional and independently verified.

The previously rejected scalar sorting-network candidate remains historical adverse evidence in M5_CPU_COSMETIC_REVIEW.md. This implementation sorts eight independent pixels concurrently using exact integer ordering; it does not replace the portable scalar sorting path.

## Local gates

- Format and default/all-feature Clippy with warnings denied: PASS.
- Release suites **109/109 default**, **111/111 all-feature** PASS. Actual local GPU and AVX2 explicitly required; real FITS fixtures configured and passed without silent skipping.
- Exhaustive 40,320 permutations prove all eight integer ranks of the vector network. 8,192 lane medians match original scalar total-order arithmetic across random finite float encodings, signed subnormals/zeros and extreme/infinite samples.
- Forced scalar/vector row bits and correction counts match across all four Bayer phases, 12 row widths including vector-boundary/tail cases, noise/defect/colored-star fields, signed zeros/subnormals/extremes/nonfinites, unchanged borders and immutable sources. Existing independent frozen full cosmetic oracle, fractional threshold boundaries, real CFA and cancellation/invalid-size gates PASS. Actual GPU cosmetic/VNG and end-to-end parity gates PASS.
- DS-B: 32 inputs, 27 accepted/5 rejected; 270 s accepted acquisition; entire master/coverage/high FITS identical to stored controls. All 32 sources, two PI references and external template hashes unchanged.
- DS-A: 1,140 inputs, 940 accepted/200 rejected; 9,400 s accepted acquisition. All original source hashes/inventory and external template verified before and after. Entire master/maps identical to stored accepted CPU/M4 files.
- Scientific reports, selection, registration, photometry, exposure, corrected/rejected/export counts, work/I/O, cache-off telemetry, geometry and memory plans remain exact. Only timings/RSS and explicitly permitted available-RAM snapshots/derived headroom vary. No layout/cache/geometry exception, image intermediate or temporary leftover. GPS remains only in scientific FITS and is redacted from default reports/logs.

## Stored-baseline performance comparison

No baseline executable was run. Stored controls are outputs/M5_CPU_rejection_performance.json and its DS-A/DS-B final report/products. Same locked compiler/profile/features (verified toolchain equality), thin LTO/one codegen unit, GPU-enabled executable with CPU engine, 16 threads, 512-square tiles, 12288 MiB ceiling, cache 0, sigma 3.0/2.5, RegisteringTemplate200pct.fit and full 4000-square reference grid at (0,0), without crop. No concurrent builds/tests during timed runs.

| Full-set measurement, seconds | Stored baseline | New optimized run |
|---|---:|---:|
| Processing through final FITS writes | 322.559 | 282.965 |
| Complete command including report publication | 323.140 | 283.510 |
| Inspection (unchanged) | 14.203 | 16.576 |
| Registration (unchanged) | 27.826 | 22.779 |
| Reconstruction including source reads | 186.705 | 142.527 |
| Rejection, accumulation and maps (unchanged) | 83.337 | 89.296 |
| FITS writes (unchanged) | 0.961 | 0.972 |

Peak RSS **3407.215 -> 3399.676 MiB**. Conservative planned allocation remains **4,789,190,656 bytes**, with available-RAM enforcement/system reserve intact. Throughput **4.029 original inputs/s**, including quality rejects. No general RAM-reduction claim.

Summed overlapping cosmetic task elapsed **1067.244 -> 148.582 s** (86.078% shorter). These timers overlap and include waiting/scheduling; they are not CPU execution time or additive wall phases. Primary measurements are processing/reconstruction wall. Unchanged rejection/maps saves -5.959 s; unchanged inspection/registration saves 2.674 s (negative means slower). Keep these variations separately from cosmetic work.

Small-set processing **12.576 -> 11.638 s**, reconstruction **5.207 -> 3.902 s**, unchanged rejection/maps **3.789 -> 3.972 s**, RSS **679.902 -> 662.465 MiB**. Retain the small-set rejection slowdown.

This is a historical observed comparison, not a fresh controlled pair. OS caches, background load, temperatures and RAM availability may differ; no isolated causal or portable speedup is established. No new baseline run is justified by this qualification. Preserve prior analytic CFA/VNG and unmatched PixInsight-workload limitations.

## Exactness and provenance

Full-set corrected ROI pixels remain 1,751,132 across 5,715,067,217 decoded ROI pixels. Valid channel samples 44,271,162,492; low rejected 85,995,586; high rejected 698,062,316; upper export limits 170/lower 0. Map denominator 1,140 original inputs. Application read work remains 11,430,134,434 bytes, 18,619,113 row reads and 59,983 opens/checks; these are not physical SSD traffic counters.

Git base `b836b93e3045164007efaf93d88354fc4edb0a8f` is the shared committed base only. Source fingerprints identify both actual uncommitted compiled states: outputs/M5_cosmetic_simd_baseline_source.json and outputs/M5_cosmetic_simd_optimized_source.json. Preserved baseline binary SHA256 `d3e46e54b18223ea84652828db8b6d58a3c5c5d30d74e4b9344b5f195bd74e01`; optimized `f0ce7a4bd4dd9b614fc2645e4ef60e360c99c28dcc2c21a9cc8532fc130bc43f`. Copying the baseline executable for provenance did not execute it. Measured binary is target/m5-cosmetic-simd-optimized.exe.

Full-file SHA256, equal to stored CPU/M4:

- master: `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc`.
- coverage: `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5`.
- high_rejection: `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970`.

## Artifacts and reproduction

- Full master FITS (private historical artifact: `outputs/M5_cosmetic_simd_DS-A_after_external200.fits`)
- Coverage FITS (private historical artifact: `outputs/M5_cosmetic_simd_DS-A_after_external200_coverage.fits`)
- High-rejection FITS (private historical artifact: `outputs/M5_cosmetic_simd_DS-A_after_external200_high_rejection.fits`)
- Compact performance evidence (private historical artifact: `outputs/M5_CPU_cosmetic_simd_performance.json`)
- Full exactness gate (private historical artifact: `outputs/M5_cosmetic_simd_DS-A_equivalence.json`)
- 32-frame exactness gate (private historical artifact: `outputs/M5_cosmetic_simd_DS-B_equivalence.json`)

Run from the project directory with new output paths. Open all three FITS directly in PixInsight. No loading script/intensity conversion is required. Exact file equality preserves previous owner visual review.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_cosmetic_simd_owner32.fits" --report "outputs/M5_cosmetic_simd_owner32.json"
```

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:\Astrofoto\2026\10 October\SeeStar50P M33\M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_cosmetic_simd_owner_full.fits" --report "outputs/M5_cosmetic_simd_owner_full.json"
```
