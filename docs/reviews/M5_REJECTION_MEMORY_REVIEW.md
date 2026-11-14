# M5: Reuse Pixel-Rejection Working Memory

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. M5 follow-up authorized by the owner after retaining explicit CPU/GPU selection. Scientific program revision: `4a61c2f` (based on `d5546b7`). This change addresses working-memory overhead only; it does not change the reconstruction or rejection algorithm, reference selection, canvas, weights or numeric precision.

## Change and acceptance gates

The allocating baseline recreated the absolute-deviation buffer for every clipping iteration of every RGB output pixel. The new row-local ClippingScratch reserves winsorized/deviation vectors once for the maximum accepted-frame population and reuses them across channel/pixel/iteration calls. The existing values buffer remains row-local. Buffers are not shared between concurrently processed rows. Memory remains bounded by tile/worker/frame counts, with no dataset-wide RGB cache, new dependency or intermediate image files.

Median selection, f64 deviation arithmetic and f32 conversion, sigma floor, low/high thresholds, iteration count, original-sample rejection, accumulation order and f64 Neumaier sums remain identical. Existing conservative row-scratch budgeting covers the simultaneously live buffers. This removes repeated allocation/free operations; it is not a promise of lower peak RSS or an assumed speedup.

Predeclared acceptance requires exact byte equality of entire master, coverage and high-rejection FITS files, not image differences within a tolerance. Scientific selection/registration/photometry and integration/export counts must also agree. Both CPU and optional GPU reconstruction feed this same reduction path; the user-facing `--engine cpu|gpu` switch remains.

## Automated and DS-B verification

- Formatting, default and all-feature Clippy with warnings denied: PASS.
- Default release suite: 80/80 PASS. GPU-feature release suite: 81/81 PASS, including actual Radeon 890M hardware tests requested with `FULGURSTACK_TEST_GPU=1`.
- [Windows MSVC and macOS Apple Silicon CI for the exact scientific revision](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39770899140): PASS for both default/GPU feature builds, warnings and release tests. Hardware parity is the separate local gate; hosted CI does not claim a real-adapter pass.
- Added differential gate: 672 populations (14 population sizes/orderings x 12 field realizations x four threshold configurations), including 0/1/2/4/5/6/27/32/940/1140/1500 samples, sparse/empty populations after larger ones, fractional fields, bright plateaus and outliers. Output f32 bits, rejection counts and f64 surviving-weight bits match the retained allocating baseline. Both scratch pointers/capacities remain unchanged throughout.
- Real DS-B: both native 2160 x 3840 and external-200% 4000 x 4000 outputs pass. All six master/map FITS hashes are exactly identical before/after. Each run uses the same 27 accepted of 32 original frames, 270 s accepted exposure and the complete reference canvas at (0,0), without cropping.
- Sources/reference integrity: all 32 DS-B sources, two PI references and external template hashes unchanged. All 1,140 DS-A source hashes also passed before the full benchmark. GPS remains in scientific FITS only, with no raw coordinate leakage into default reports.

Independent checks: `scripts/tests/verify-stack-equivalence.py` uses Python standard-library SHA-256 and exact scientific report comparison. It writes JSON only, validates input/reference integrity and rejects remaining `.tmp` files. Development Python is not a product runtime dependency.

## Measurement protocol

Both binaries were built with the same installed Rust release profile and `--features gpu`, using `--engine cpu`; source code differs only in rejection working-memory reuse and its tests. The unmodified release executable was preserved under `target/m5-rejection-baseline.exe`, and the optimized release executable under `target/m5-rejection-reuse.exe`. Binary hashes and complete command times are recorded in the full-run timing JSON. Full commands run sequentially, baseline then optimized, with fixed 16 workers, 256 x 128 output tiles, 12288 MiB budget, the same external template, unchanged source files and no additional crop. No compiler/test suite or other FulgurStack benchmark runs concurrently. Existing OS caches are retained; neither command is asserted to be a cold-cache or randomized statistical benchmark.

The DS-B pair establishes exact parity. Its unoptimized/optimized processing times were native 17.052/20.983 s and external-200% 19.265/24.155 s. Unchanged reconstruction phases also slowed (12.720/15.572 s and 11.069/14.085 s), so these short runs do not establish an isolated rejection-optimization speedup. Report them rather than discarding unfavorable measurements. The full matched pair separates total processing time and the directly affected rejection/accumulation/map phase. Historical M4 processing (631.724 s, rejection phase 132.635 s) is context, not a substitute for the new paired baseline.

## Full DS-A results

Both full release commands completed successfully. The geometry-only reference was `RegisteringTemplate200pct.fit`, the output grid was exactly 4000 x 4000 at (0,0), and no additional crop was applied. All 1,140 frames registered, the same 940 frames were integrated, and the same 200 were quality rejected. Accepted acquisition exposure remains 9,400 s; master EXPTIME remains 10 s.

| Measurement | Allocating baseline | Reused buffers | Difference |
| --- | ---: | ---: | ---: |
| processing, including three FITS writes | 604.454 s | 578.304 s | 26.150 s shorter; 4.326% |
| complete command, including JSON publication | 605.030 s | 578.922 s | 26.107 s shorter |
| rejection/accumulation/map phase | 117.160 s | 108.467 s | 8.694 s shorter; 7.420% |
| reconstruction phase, unchanged algorithm | 427.833 s | 420.518 s | 7.316 s shorter |
| inspection | 20.255 s | 15.841 s | 4.414 s shorter |
| registration | 29.129 s | 23.363 s | 5.766 s shorter |
| peak process working set | 999.703 MiB | 987.039 MiB | 12.664 MiB lower |
| planned buffer budget | 1,822,105,600 bytes | 1,822,105,600 bytes | unchanged; below 12 GiB |

**Exact quality gate: PASS.** Master, coverage and high-rejection FITS are identical byte-for-byte before/after and also match the owner-accepted M4 full-run products. All 1,140 source hashes and inventory, 32 DS-B source hashes, two PI references and the template hash remain unchanged. Scientific reports, selection, normalization, valid samples, rejection counts and export limiting counts agree exactly. No intermediate images or `.tmp` files remain. A new native PI import was not repeated: the entire FITS bytes are identical to the previously native-PI-validated, owner-approved products.

**Measured performance:** the directly affected phase is 7.420% shorter in this pair (1.080x speed ratio), and total processing is 4.326% shorter (1.045x). Other unchanged phases varied as shown, so do not attribute all 26.150 s to buffer reuse. This is one sequential pair with existing caches, not an isolated causal timing or a portable speedup guarantee. The negative DS-B timings remain disclosed above. The optimization is retained with exact scientific parity and modest full-run improvement; the main remaining wall-time cost is reconstruction.

Full accounting remains: 44,271,162,492 valid channel samples; 85,995,586 low rejections; 698,062,316 high rejections; 43,487,104,590 surviving samples. All map pixels use the same denominator of 1,140 original frames. Final export has the same 170 upper sensor-range limits, distinct from high-rejection counts.

Delivered local products:

| Product | Path | SHA-256 |
| --- | --- | --- |
| optimized master | `outputs/M5_scratch_DS-A_after_external200.fits` | `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc` |
| coverage | `outputs/M5_scratch_DS-A_after_external200_coverage.fits` | `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5` |
| high rejection | `outputs/M5_scratch_DS-A_after_external200_high_rejection.fits` | `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970` |

- `outputs/M5_rejection_memory_performance.json`: compact complete benchmark, both DS-B comparisons, binary hashes/timers, exact-parity and integrity results.
- `outputs/M5_scratch_DS-A_before_external200.json` and `outputs/M5_scratch_DS-A_after_external200.json`: detailed full-run telemetry and scientific reports.
- `outputs/M5_scratch_DS-A_equivalence.json`: exact image/report equality and all input hash checks.
- `outputs/M5_scratch_DS-A_after_validation.json`: entire-file equality to the owner-accepted M4 products and provenance of the existing PI import proof.
- `outputs/M5_scratch_DS-A_command_timings.json`: preserved release binary hashes, fixed worker count, start times and complete command timers.
- `outputs/M5_scratch_DS-B_native_equivalence.json` and `outputs/M5_scratch_DS-B_external200_equivalence.json`: exact small-dataset before/after gates.

## Owner reproduction

From the repository root, choose an unused output prefix. Explicit tile overrides preserve this measurement geometry after later default tuning:

```powershell
cargo run --release --features gpu --bin fulgurstack -- stack --engine cpu --threads 16 --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-width 256 --tile-height 128 --memory-budget-mib 12288 --output "outputs/M5_scratch_owner_review.fits" --report "outputs/M5_scratch_owner_review.json"
```

Coverage and high-rejection FITS are generated automatically beside the master. Normal PixInsight File > Open applies; there is no intensity conversion or loading-script requirement. Exact before/after image-byte equality is the quality gate for this memory-only change. M5 remains in progress; existing absolute CFA/VNG-photometry and unmatched PixInsight-workload limitations are unchanged.
