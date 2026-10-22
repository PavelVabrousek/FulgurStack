# M5: Output Tile Geometry Optimization

Date: 2026-10-22. Scientific program revision: `7a65568bccb4c639fa32108f6b8465c99bbb6eaa`. M5 remains in progress. The owner authorized tile size/shape tuning, exact 32-frame master/map parity, then full DS-A measurement against RegisteringTemplate200pct.fit without cropping.

## Scope and gates

Expose `--tile-width` (1..1024) and `--tile-height` (1..512). The former height cap of 256 was extended to test larger square/tall shapes; the memory ceiling remains 12288 MiB. Existing planning may lower requested tile height before allocation, and Windows process-RSS enforcement remains active. JSON config records requested dimensions and integration stats record actual dimensions. Canvas dimensions/origin do not depend on tile geometry.

The verified default is 512x512, selected after 26 search runs covering 11 distinct shapes and retained after full parity/performance verification. The original 256x128 remains reproducible with explicit flags. CPU remains the default engine and `--engine gpu` remains optional with `--features gpu`.

No reconstruction, precision, source selection, photometry, interpolation, rejection or accumulation algorithm changed. Preserve the 8-pixel composed source halo, even CFA ROI origins, global f64 sampling coordinates, original sample order and f64 Neumaier sums. Existing reusable rejection buffers remain in place. No cached/debayered/registered image intermediates or dataset-wide RGB storage.

The quality gate requires exact entire-file SHA-256 equality for master, coverage and high-rejection FITS, plus exact scientific registration/quality/photometry, exposure, valid/rejected sample counts and export limiting metadata. The verifier's explicit tile-change option permits only tile configuration and geometry-dependent work counters to differ. Decoded/cosmetically corrected ROI counts include repeated halos, so lower counts are less repeated work, not fewer unique sensor defects. GPU timing/transfer/buffer counters may vary; adapter/backend/driver/numeric policy remain strict. No scientific tolerance is relaxed.

## Automated verification

- Formatting and default/all-feature Clippy with warnings denied: PASS.
- Default release suite 81/81 PASS; GPU-feature release suite 82/82 PASS, with local real Radeon 890M gates explicitly required via FULGURSTACK_TEST_GPU=1.
- [Windows MSVC and macOS Apple Silicon CI for the exact selected revision](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39777091953): PASS for default/GPU feature compilation, warnings and release tests. Real GPU parity is the separately required local gate.
- Scaled/rotated inverse-warp tests cover asymmetric/odd-edge tiles, shapes through 512x512, exact master and both map equality, and strict sensor masks. Six-frame weighted rejection tests verify channel-dependent high-rejection/coverage across shapes.
- Invalid CLI dimensions fail before source processing/output publication. A positive 512x512 CLI gate verifies the requested configuration and effective 288x288 tile on a smaller reference canvas.
- All 26 real DS-B search runs passed exact master/map equality against the accepted CPU baseline. Default selection was then verified on native and 200% grids for both CPU and GPU (four additional runs), each byte-identical to its own prior-engine products. All used 27 accepted of 32 originals, five quality rejections, 270 s accepted exposure and the complete reference canvas.

## DS-B search measurements

All timings are optimized release processing wall time, including final FITS writes. Fixed 16 CPU workers, reference canvas, same template/settings and no concurrent compiler/test/benchmark. Each search stage uses the same preserved binary for its candidates; the second binary extends only the height bound and tests, with no arithmetic changes. Orders were varied between two repetitions. Median of two runs is a descriptive midpoint, not a randomized statistical confidence estimate. OS/driver caches are retained; no cold-cache claim.

Initial search (eight shapes, two different orders):

| Shape | Median processing s | Median reconstruction s | Maximum peak MiB | Exact master/maps |
| --- | ---: | ---: | ---: | --- |
| 512 x 256 | 20.762 | 11.642 | 697.9 | PASS |
| 384 x 192 | 22.590 | 12.558 | 723.2 | PASS |
| 256 x 256 | 22.630 | 12.594 | 689.0 | PASS |
| 128 x 256 | 23.689 | 14.208 | 677.2 | PASS |
| 256 x 128 | 23.840 | 13.952 | 692.3 | PASS |
| 512 x 128 | 24.354 | 14.282 | 680.2 | PASS |
| 256 x 64 | 26.695 | 17.094 | 682.4 | PASS |
| 128 x 128 | 27.359 | 16.779 | 729.9 | PASS |

Extended search (three new shapes and two interleaved controls, two orders):

| Shape | Median processing s | Median reconstruction s | Maximum peak MiB | Decoded ROI pixels | Exact master/maps |
| --- | ---: | ---: | ---: | ---: | --- |
| 512 x 512 | 20.853 | 11.480 | 691.9 | 165,253,188 | PASS |
| 384 x 384 | 21.325 | 11.775 | 696.8 | 173,461,538 | PASS |
| 256 x 512 | 22.197 | 12.465 | 704.6 | 185,123,821 | PASS |
| 512 x 256 | 23.883 | 13.245 | 704.7 | 185,264,278 | PASS |
| 256 x 128 | 25.431 | 15.071 | 703.4 | 215,616,134 | PASS |

512x512 led the extended search: 20.853 s versus its 25.431 s control, with 165,253,188 decoded ROI pixels versus 215,616,134. This is DS-B evidence only. Large whole-set frame stacks require much more per-tile memory; full-run telemetry is the deciding measurement. Favorable and unfavorable shapes/runs are retained in the local JSON search reports.

Selected-default verification timings (single commands, not matched GPU speedup tests): CPU native 20.629 s, CPU external-200% 20.693 s, GPU native 19.884 s and GPU external-200% 19.813 s. Each passed exact byte equality with its prior engine output; comparing these with historical timings does not isolate a tile-caused performance effect. The historical native GPU command was faster overall, while its reconstruction phase was slower; do not discard that unfavorable total-time comparison.

## Full-run protocol

Run baseline 256x128, then candidate/default 512x512 sequentially using the same preserved release executable `target/m5-tile-selected.exe`. Both use CPU reconstruction, 16 workers, unchanged 3.0/2.5 rejection thresholds, 12288 MiB budget and the exact 4000x4000 external-reference canvas at (0,0). Baseline explicitly overrides the new tile defaults; the candidate command uses the defaults, verifying actual CLI selection. No builds/tests/other benchmark run concurrently. Binary SHA-256, start times and complete command times are recorded. Source/template hashes passed before both commands.

## Reproduction

From the repository root, use unused output prefixes:

```powershell
cargo run --release --features gpu --bin fulgurstack -- stack --engine cpu --threads 16 --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-width 512 --tile-height 512 --memory-budget-mib 12288 --output "outputs/M5_tile_owner_review.fits" --report "outputs/M5_tile_owner_review.json"
cargo run --release --features gpu --bin fulgurstack -- stack --engine cpu --threads 16 --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-width 256 --tile-height 128 --memory-budget-mib 12288 --output "outputs/M5_tile_owner_baseline.fits" --report "outputs/M5_tile_owner_baseline.json"
```

Each command writes the final master plus automatic coverage/high-rejection FITS and JSON only. All scientific source files remain immutable. Open FITS normally in PixInsight. Existing absolute CFA/VNG-photometry and unmatched PixInsight workload limitations remain unchanged.

## Full DS-A measurement and delivery

The paired release runs completed with all 1,140 inputs registered, the same 940 accepted and 200 quality rejected, 9,400 s accepted acquisition exposure and the exact 4000 x 4000 template grid at (0,0). No crop was applied. Both runs used the identical preserved release binary (SHA-256 `9e03a82caa213fa8bed6af459b103e19a49dcc00d7c64cb4d3d0f95e9c53260c`).

| Measurement | 256 x 128 control | 512 x 512 selected |
| --- | ---: | ---: |
| processing seconds | 676.741 | 558.198 |
| complete command seconds | 677.452 | 558.704 |
| reconstruction seconds | 481.696 | 367.423 |
| rejection/accumulation/maps seconds | 135.951 | 134.150 |
| inspection seconds | 18.240 | 16.136 |
| registration seconds | 28.448 | 29.146 |
| peak process RSS MiB | 993.746 | 3412.633 |
| output tiles | 512 | 64 |
| planned buffer bytes | 1,822,105,600 | 4,783,415,296 |
| decoded ROI pixels | 7,454,242,911 | 5,715,067,217 |
| corrected ROI events, including repeated halos | 2,106,272 | 1,751,132 |

**Exact scientific parity: PASS.** All three complete FITS files match byte-for-byte between the new paired runs and also match the owner-accepted M4 full products. All 1,140 DS-A source hashes/inventory, 32 DS-B source hashes, two PI references and the external-template hash remain unchanged. No `.tmp` or calibrated/debayered/registered intermediate images remain. Native PI import was not redundantly repeated: these entire FITS bytes already have the accepted M4 native-import proof.

**Measured performance:** total processing is 118.543 s shorter (17.517%, 1.212x speed ratio); reconstruction is 114.273 s shorter (23.723%). Decoded ROI work fell 23.331%, consistent with less repeated reconstruction. Most of the total improvement is in that intended phase. Other phases also vary; one sequential pair does not establish a portable speedup or global optimum. The control was slower than historical 256x128 measurements, so historical timing is not substituted for this pair.

Peak RSS increases from 993.746 to 3412.633 MiB because the current tile holds more frame samples. Planned buffers increase to 4,783,415,296 bytes. Both remain under the unchanged 12288 MiB ceiling. Larger tiles are not a reduction in memory consumption; explicit dimensions and the budget remain available for other machines. Retain 512x512 as the default on this measured evidence, with the existing height-reduction/failure policy enforcing bounded memory.

Scientific accounting remains 44,271,162,492 valid channel samples, 85,995,586 low rejections, 698,062,316 high rejections and 43,487,104,590 survivors. Export retains the same 170 upper sensor-range limits, separate from high rejection. Coverage/high maps retain the denominator of all 1,140 originals and their original scientific definitions.

Delivered products:

| Product | Local path | SHA-256 |
| --- | --- | --- |
| master | `outputs/M5_tile_DS-A_optimized_external200.fits` | `d61e1026722463d3f21e7d5781db02d0b22de67cd3fd9d8ca6bd7ed335c4f0cc` |
| coverage | `outputs/M5_tile_DS-A_optimized_external200_coverage.fits` | `50ec6eca6a86780f0b308553ed1b84a4829dbc64f5c12306975a45df6921b4d5` |
| high_rejection | `outputs/M5_tile_DS-A_optimized_external200_high_rejection.fits` | `fff5d0554a636258c21066d24104704235f11c9df65348453e9f7d4ae6114970` |

- `outputs/M5_tile_geometry_performance.json`: compact complete evidence, search rows/summaries, selected-mode checks, full timings/work/memory, source integrity and product hashes.
- `outputs/M5_tile_DS-A_baseline_external200.json` and `outputs/M5_tile_DS-A_optimized_external200.json`: detailed scientific plans and telemetry.
- `outputs/M5_tile_DS-A_equivalence.json`: exact full product/report comparison and all source hashes.
- `outputs/M5_tile_DS-A_command_timings.json`: same binary hash, fixed workers, starts, requested shapes and complete command times.
- `outputs/M5_tile_DS-B_sweep.json` / `_summary.json` and `outputs/M5_tile_large_DS-B_sweep.json` / `_summary.json`: all favorable/unfavorable repeated CPU measurements.
- `outputs/M5_tile_selected_DS-B_summary.json` and the corresponding `_equivalence.json` files: exact native/200% checks for both engines.

M5 remains open; this tile-geometry optimization is complete. GPU selection is retained and no full DS-A GPU speed claim is made from the CPU results.
