# M5: Exact Pixel-Rejection Optimization

Date: 2026-10-22. All development, tests and benchmarks are local on Windows x86_64. No commits, pushes, PR updates or remote CI. GitHub Actions remain disabled by the owner. No new macOS execution is claimed. M5 remains open; FITS cache remains off.

The fresh full 1,140-frame pair measured **366.351 -> 322.559 s** processing (11.954% shorter). Rejection/accumulation/maps measured **123.116 -> 83.337 s** (32.310% shorter). Entire master and both maps match fresh controls and accepted CPU/M4 products byte for byte.

## Change and exactness

The previous implementation repeatedly selected the median and MAD after winsorizing a scratch copy. Retain the existing central order statistics and certify a fixed point: the f32-rounded low/high endpoints must enclose both central sample ranks, and their absolute deviations, computed with the original f64-to-f32 expression, must stay at or above the upper central MAD rank. Monotone clamping then preserves both central ranks of both distributions; the next center, sigma and thresholds have identical bits. The rounded clamp is idempotent. Remaining scratch-copy iterations cannot alter the thresholds used to reject original samples and can be skipped. Check finite endpoints conservatively; if any condition fails, execute the original loop, up to three iterations with its original early stop.

This is an exact shortcut, not an approximate rejection estimator or a changed clipping policy. Preserve the unweighted median/MAD, 1e-6 sigma floor, configured low/high sigma, finite-channel filtering, original-source weighted accumulation order and f64 Neumaier sums. Original samples, rejection counts, map denominators, photometry, VNG/cosmetics, SIMD resampling, geometry and final sensor saturation are unchanged. No extra allocation, dependency or schema; existing <=16 ROI workers, source scratch, 32-pixel packing and all conservative memory estimates remain unchanged. Optional GPU reconstruction remains available and shares this CPU reduction.

## Local verification

- Format and default/all-feature Clippy with warnings denied: PASS.
- Release tests: **106/106 default**, **108/108 all-feature** PASS. Real FITS fixtures configured; actual GPU and AVX2 explicitly required and PASS without silent skipping.
- Frozen previous production median and clipping oracle, independent of the new median helper/certificate. 9,408 exact comparisons across populations 0/1/2/4/5/6/7/8/27/32/33/940/1140/1500, seven symmetric/asymmetric clipping settings, random finite float exponents, subnormals, signed zero, near-saturated faint fractions, constant/outlier and extreme positive/negative samples. Compare master bits, both rejection counts and surviving weight bits. Separate endpoint/rank tests exercise certificate acceptance/refusal and rounding. Existing channel-mask/map/statistics, cancellation, halo, flux and GPU gates PASS.
- DS-B: 32 originals, 27 accepted/5 rejected, 270 s accepted acquisition. Entire master/coverage/high FITS exact; immutable 32 sources, two PI references and external template verified.
- DS-A: 1,140 originals, 940 accepted/200 rejected, 9,400 s accepted acquisition. All source hashes/inventory and template unchanged before/after; exact master/maps also match accepted CPU/M4 hashes.
- Strict reports preserve registration, weights, photometry, exposure, all science/rejection/export counters, geometry, work/I/O and memory plans. Only timings/RSS and explicitly permitted available-RAM snapshots/derived headroom may differ; no layout/cache/geometry exception. No image intermediates or temporary leftovers. GPS retained only in scientific FITS, redacted from default logs/reports.

## Fresh paired performance

Same locked release compiler/profile/features, thin LTO/one codegen unit, GPU-enabled binaries using CPU engine, 16 threads, 512-square tiles, 12288 MiB ceiling, cache explicitly 0, sigma 3.0/2.5, RegisteringTemplate200pct.fit and complete 4000-square canvas at (0,0), without crop. Fresh sequential before then after, preserved binaries and uncommitted source fingerprints; no concurrent builds/tests. OS caches/background load uncontrolled. One descriptive pair does not establish isolated causal or portable speedup. Historical SIMD results are context, not the performance control.

| Full-set measurement, seconds | Before | After |
|---|---:|---:|
| Processing through final FITS writes | 366.351 | 322.559 |
| Complete command including report publication | 366.793 | 323.140 |
| Inspection (unchanged) | 14.327 | 14.203 |
| Registration (unchanged) | 22.463 | 27.826 |
| Reconstruction (unchanged) | 195.251 | 186.705 |
| Rejection, weighted accumulation and maps | 123.116 | 83.337 |
| FITS writes (unchanged) | 0.705 | 0.961 |

Peak RSS **3400.324 -> 3407.215 MiB**. Planned allocation **4,789,190,656 bytes**, unchanged and below the effective budget/system reserve. Throughput after change **3.534 original inputs/s**, including quality rejections. No general RAM-reduction claim.

Small set: processing **13.577 -> 12.576 s**, rejection/maps **4.759 -> 3.789 s**, peak RSS **652.453 -> 679.902 MiB**. Preserve higher small-set RSS and all unchanged-phase variations. Overlapping ROI task timers are not additive wall time or CPU utilization. Principal evidence is measured processing and nonoverlapping rejection/maps wall time.

All full-set valid channel samples (44,271,162,492), low rejections (85,995,586), high rejections (698,062,316) and upper export limits (170; lower 0) remain exact. Coverage/high denominator stays 1,140 original inputs. Prior analytic CFA/VNG and unmatched PixInsight workload limitations remain; no new absolute-photometry or equivalent-workload PI speed claim.

## Provenance and final products

Git base `b836b93e3045164007efaf93d88354fc4edb0a8f` identifies the shared committed base only. Both measured source states include prior uncommitted SIMD work; source fingerprint manifests identify exact compiled code. Baseline binary SHA256 `278828f47ca95926c0a679918ed8f265959074aa993a270b06773f901e968ccb`; optimized `d3e46e54b18223ea84652828db8b6d58a3c5c5d30d74e4b9344b5f195bd74e01`. Preserved as target/m5-rejection-baseline.exe and target/m5-rejection-optimized.exe. Source manifests: outputs/M5_rejection_baseline_source.json and outputs/M5_rejection_optimized_source.json.

Full final products:

- [Master](../outputs/M5_rejection_DS-A_after_external200.fits)
- [Coverage](../outputs/M5_rejection_DS-A_after_external200_coverage.fits)
- [High rejections](../outputs/M5_rejection_DS-A_after_external200_high_rejection.fits)
- [Performance and gates](../outputs/M5_CPU_rejection_performance.json)
- [Full exactness gate](../outputs/M5_rejection_DS-A_equivalence.json)
- [32-frame exactness gate](../outputs/M5_rejection_DS-B_equivalence.json)

Exact full-file SHA256:

- master: `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc`.
- coverage: `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5`.
- high_rejection: `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970`.

## Owner reproduction

Run from the project directory in PowerShell. Outputs must be new paths. The geometry reference and all source images remain read-only. Open the three FITS directly in PixInsight; no loading script or manual intensity conversion. Existing visual quality is preserved by exact file equality.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_rejection_owner_32.fits" --report "outputs/M5_rejection_owner_32.json"
```

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:\Astrofoto\2026\10 October\SeeStar50P M33\M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_rejection_owner_full.fits" --report "outputs/M5_rejection_owner_full.json"
```
