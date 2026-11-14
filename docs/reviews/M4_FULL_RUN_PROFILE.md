# M4 Full DS-A Reference-Canvas Run and Performance Profile

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. Scientific code revision: `5a3d81deed4d8d106eb041058bfd57b0ac577abf`.

The owner approved the current 32-frame reference-sized master/maps visually and requested this full run. Full-run execution and automated product verification: **PASS**. Owner visual review of this new full master and remaining G4 quality evidence are still pending. No GPU implementation was introduced.

## Run and products

- Input: `C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs`, 1,140 immutable 10 s M33 IRCUT frames, 18,917,804,160 source bytes.
- CPU: AMD Ryzen AI 9 HX 370, 12 physical / 24 logical processors; 16 Rayon workers. Detected GPU: AMD Radeon 890M, driver 32.0.31041.1004; GPU unused.
- Automatic reference: `Light_M 33_10.0s_IRCUT_20261006-232530.fit`, FITS DATE-OBS `2026-10-06T21:25:09.047658`. This is the earlier central frame of the 940 quality-accepted acquisition timestamps, not a quality-ranked anchor.
- Accepted: **940/1,140**; rejected: **200**. Registration succeeded for all 1,140 measurable frames; rejected-quality frames contribute no integrated samples. Reason counts (may overlap): `{'excessive_eccentricity': 193, 'excessive_hfr': 86, 'low_star_count_in_exposure_group': 2}`.
- Accepted exposure: **9,400 s**; master EXPTIME remains **10 s**. Full reference canvas **2160 x 3840**, origin **(0,0)**, no crop.
- All outputs: planar RGB Float32 FITS (`BITPIX=-32`), ordinary PixInsight File > Open. Scientific samples use fixed linear reference ADU / 65,535; maps use direct fractions. GPS provenance is preserved privately in the master and excluded from reports/maps.

| Product | Local path | SHA-256 |
| --- | --- | --- |
| master | `outputs/M4_DS-A_reference_median_profiled.fits` | `02c3faf5c1b50a7a6581f7e7f047569a12f86c19a2ef43bcba8ef22b8d4117fb` |
| coverage | `outputs/M4_DS-A_reference_median_profiled_coverage.fits` | `d65c87e4f166da072b90477a029010e448c26b83e25bd0c55ce6a195bf4cd08e` |
| high_rejection | `outputs/M4_DS-A_reference_median_profiled_high_rejection.fits` | `dff8eb3b84be6a3ad84edcf8f50d5c362a0e25744b9fc1351c622903722b444b` |

Main report: `outputs/M4_DS-A_reference_median_profiled.json`. Compact independent validation/profile: `outputs/M4_DS-A_reference_median_profiled_validation.json`. Complete command timer: `outputs/M4_DS-A_reference_median_profiled_timing.json`.

## Measured release performance

Complete prebuilt `cargo run --release` command: **561.284 s = 9.35 min**, including startup, the three FITS products and JSON serialization. Internal processing through FITS publication: **560.677 s**, excluding JSON serialization. Throughput: **2.033 original frames/s**. Process peak working set sampled through FITS publication: **773.6 MiB (0.755 GiB)**, below the 12 GiB ceiling. Planned integration buffers/reserve: 1473.1 MiB. This is a single run; input hashing preceded it and OS cache/background load were uncontrolled. Compilation, source hashing and post-run checks are excluded.

The <=18 minute full-run performance target passes. The raw ratio to the owner's approximate three-hour PI full-process time is 19.2x; it is **not an accepted equivalent-quality speedup**: PI's exact accepted inputs and reference are unknown, and its integration/rejection differs. The older cropped full run is also a different workload and is not a regression comparison.

### Non-overlapping wall phases

Percentages use the 560.677 s processing interval. Reconstruction batches include raw ROI I/O/decoding, cosmetics, VNG, inverse warp, normalization, task scheduling and waits. Reduction includes sample gathering, median/MAD clipping, compensated weighted means and both maps.

| Phase | Wall seconds | Processing share |
| --- | ---: | ---: |
| inspection | 15.417 | 2.75% |
| registration | 13.580 | 2.42% |
| reconstruction_batches | 475.299 | 84.77% |
| rejection_accumulation_maps | 52.024 | 9.28% |
| output_copy | 0.204 | 0.04% |
| three_fits_writes | 0.428 | 0.08% |
| other_processing | 3.724 | 0.66% |

Integration spans 531.030 s over 270 output tiles (256 x 128 nominal). The additional integration overhead includes allocation and tile-buffer lifecycle; it is included in other_processing above. The wall profile identifies reconstruction batches (**84.77%**) as the primary bottleneck. Writing three final FITS is only 0.428 s.

### Overlapping ROI task timers — not CPU samples

| ROI task | Sum of elapsed task seconds |
| --- | ---: |
| read_seconds | 1185.894 |
| cosmetic_seconds | 2167.147 |
| demosaic_seconds | 2968.759 |
| resampling_seconds | 573.111 |

These values overlap across workers. Nested Rayon scheduling can include waits inside cosmetics/VNG timers. They cannot be added to end-to-end wall time or read as CPU percentages. VNG has the largest elapsed task total, followed by cosmetics; this supports investigating reconstruction first, but does not establish an exact function-level CPU attribution. This is instrumented phase profiling, not a sampled call-stack trace.

Decoded ROI area: **11,733,137,192 pixels**, about 1.505 times the total full-sensor area of accepted inputs. Rotated bounding rectangles, overlapping support halos and tile-edge work contribute repeated reconstruction. This ratio measures decoded work, not physical SSD traffic; OS caching matters.

## Diagnostic maps and data integrity

Coverage counts surviving channel samples after whole-frame rejection, geometry, low rejection and high rejection, divided by **all 1,140 originals**. Maximum 82.456142% (940/1,140), full-image/channel mean 72.234455%, minimum 0.614035%. High-rejection map maximum 19.912280%, mean 1.153952%. Values 0..1 mean 0..100%; maps are not weights or exposure times. Display STF may be applied without changing samples.

Exact integer count conservation reconstructed independently from every map sample:

- Used: **20,490,637,904**.
- Low rejected: **39,482,460**.
- High rejected: **327,339,814**.
- Sum = **20,857,460,178** geometrically valid channel samples.

No zero-geometry-coverage output pixels in this run; geometrically valid frame counts range from 10 to 940, with rejection further reducing used samples. Export sensor bounding affected 78 channel samples above 1; these are reported separately and are not high-rejection events.

- All 1,140 source SHA-256 hashes match the pre-run manifest; no originals changed.
- No intermediate calibrated/debayered/registered images; no `.tmp` remains. Only final products and requested reports were generated.
- Full dimensions, fractions, finite 0..1 range, exposure provenance, reference selection, private GPS preservation and budget accounting: PASS.
- Native ordinary PixInsight import of all three outputs: PASS, zero probe mismatches or out-of-range samples. See `outputs/M4_DS-A_reference_median_profiled_PI_validation.json`. The separate hidden automation instance was closed; the owner's session was not modified.
- Profiling preserves the approved DS-B master and both maps byte-for-byte: PASS, `outputs/M4_profile_science_parity.json`.
- fmt / Clippy warnings denied / 71 release Rust tests / four legacy adapter tests: PASS. Real FITS gates enabled locally. [Windows and macOS CI for the scientific code revision](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39727308691): PASS.

## Optimization proposals, in priority order

These are proposals from the measured phase profile and code inspection, not implemented or benchmarked speedups.

1. **Optimize reconstruction on CPU first.** Benchmark representative high-rotation tiles with bounded tile sizes, 12/16/24 workers and flatter task scheduling. Reduce unused halo/rectangle reconstruction, add an interior VNG path avoiding repeated boundary reflection, and reuse immutable CFA direction plans/buffers where safe. Track decoded area and true wall time; larger tiles or more threads are not automatically faster. Preserve byte-identical scientific results wherever arithmetic order is unchanged; use existing numeric/image gates for intentional arithmetic changes. No full-frame RGB cache across all inputs or disk intermediates.
2. **Prototype GPU cosmetics + VNG + warp as a future isolated candidate.** Keep the bounded output-tile interface and CPU inspection/registration/rejection/Neumaier accumulation initially. This targets the measured dominant phase while retaining the validated CPU clipping/accumulation path. Batch ROIs and minimize round trips. Measure uploads, execution, readbacks, peak shared memory and complete stack time on the actual Radeon 890M. Integrated graphics does not make copies/synchronization free. A smaller GPU tile or channel splitting may be needed; 940 x 256 x 128 RGB f32 samples alone require about 352.5 MiB, exceeding the portable default single-storage-binding size.
3. **Remove repeated clipping allocations and improve gather locality.** Reuse the deviations scratch per row/worker instead of allocating it per pixel/channel/pass; test tile-layout/cache changes. This targets a measured 52.0 s (9.28%) phase. Even eliminating the entire phase would cap total speedup near 1.10x, so it is a useful smaller optimization rather than the main GPU target.
4. **Change I/O only after separating real device traffic from waits.** Raw task elapsed read time includes ROI file opening, row seeks, decoding and possible contention. Use sampled CPU/OS I/O profiling before selecting read pooling, buffer reuse or contiguous reads. Do not infer SSD saturation from summed overlapping read timers. Final FITS export is not a priority.

For portability, [wgpu documents Direct3D 12, Vulkan and Metal backends](https://wgpu.rs/doc/wgpu/enum.Backend.html). Its [device limits](https://docs.rs/wgpu/latest/wgpu/struct.Limits.html) must be queried rather than assuming one large storage buffer. [WGSL scalar types](https://gpuweb.github.io/gpuweb/wgsl/#scalar-types) do not provide portable f64; a later GPU reduction requires an explicitly tested f32 compensated/tree approach or retained CPU f64. Never introduce f16. Device support and end-to-end benefit must be measured; these sources establish feasibility constraints, not a speed claim.

### Illustrative ceilings, not GPU predictions

If the **entire** reconstruction-batch wall phase, including I/O/waits, were accelerated while all other work remained unchanged:

| Assumed phase speedup | Hypothetical processing time | Overall speedup |
| --- | ---: | ---: |
| 2x | 323.0 s | 1.74x |
| 5x | 180.4 s | 3.11x |

Perfect removal of that whole phase would leave 85.4 s, an ideal upper bound of 6.57x. A real GPU accelerates only part of the phase and adds dispatch/transfer/synchronization, so these are deliberately optimistic Amdahl bounds. No exact GPU speedup is promised.

## Owner review and reproduction

Open the three local FITS normally in PixInsight; inspect bright-star colors/cores, faint structure, sensor edges and low-coverage corners. This full master has not yet received the owner's visual approval. Previous full-reference quantitative parity and the disclosed narrow colored-star analytic photometry limitation remain open; automated success does not close G4 by itself.

The delivered files already exist and cannot be overwritten. To repeat the run, use a new prefix from the project directory:

```powershell
cargo run --release --bin fulgurstack -- stack --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --canvas reference --memory-budget-mib 12288 --output "outputs/M4_DS-A_review_repeat.fits" --report "outputs/M4_DS-A_review_repeat.json"
```

Both diagnostic map names are derived automatically. No `--reference` means automatic accepted chronological median. The full checker is `scripts/tests/verify-full-stack.py`; NumPy is a development-only verification dependency, not part of the product runtime.
