# M5: Reused CPU Integration Buffers (2026-10-27)

Owner-requested local optimization, ADR-070, on top of ADR-069 (AVX2 VNG). M5 remains open. No commit, push or remote CI. Scientific operations, precision, thresholds, source order, tiles and outputs are unchanged; products are bit-identical.

## Change

CPU integration previously requested new memory for every source in every tile: the decoded CFA ROI, the cosmetic copy, the VNG RGB ROI and the resampled tile samples (about 10 MB per source and tile on the native grid). On Windows these large allocations go directly to the operating system, which provides freshly zeroed pages each time, a cost that shows up as kernel CPU time.

Now:

- Each active reconstruction slot takes one reusable ROI scratch (raw CFA, cosmetic CFA, RGB) from a small pool and returns it after its source. The pool never holds more entries than active slots.
- The previous tile's sample buffers become the next tile's output buffers. Each source index is claimed exactly once, so every buffer has one owner and at most one set of tile samples exists, as before.
- The tile reduction result buffer is reused across tiles.

Every reused buffer is fully reinitialized before use: ROI decoding resizes and overwrites every sample (same zero initialization as before in the cached path), the cosmetic buffer starts as an exact copy of the original CFA, VNG writes every pixel, and resampled samples are reset to NaN ("no sample") exactly as a fresh buffer was. Only capacity carries over. The public single-use functions remain and delegate to the new reusing variants. GPU reconstruction and the FITS block cache semantics are unchanged.

Memory: retained scratch is bounded by active slots × the largest ROI, already covered by the conservative per-worker ROI term in the tile plan, and the recycled samples replace (not add to) the per-tile sample set. Measured peak working set rises by about 155 MiB in the central region and 31 MiB in the complete-grid pair, far below the 12288 MiB ceiling.

## Verification gates

- New: `reused_roi_scratch_and_tile_buffers_match_fresh_allocations_bit_for_bit` reuses one scratch and recycled tile buffers across three sources (identity, rotated, 2x scaled; one with a trail) and seven tiles that grow, shrink, cross the sensor edge or are entirely uncovered, with the FITS cache off and on. All retained buffers are poisoned with distinct NaN payloads before each use. Output bits and decoded/corrected counts match fresh allocations; sources are unchanged.
- Extended: the AVX2 VNG differential test now reuses one poisoned RGB buffer across all differently sized frames.
- Existing whole-integration gates (tile-size invariance against direct full-frame reconstruction, 12/16/24-worker equality, real ROI vs full frame, rejection/map gates) PASS through the reused path.
- Format, default/all-feature Clippy with warnings denied: PASS. Release suites **163/163 default, 166/166 all-feature**, with real FITS, required AVX2 and actual GPU.

## Measurement

Same design as ADR-069: preserved control profiler (the ADR-069 AVX2 VNG build) versus the candidate, stored native plan `outputs/M5_native_walks_20261026/parallel_plan.json` (1140 original / 940 selected, native 2160 x 3840, no geometric correction), 24 threads, manual 512-square tiles, cache 0. One excluded warmup each, three rotated central-region rounds, then one complete-grid pair; sequential, no builds/tests in parallel. Machine conditions are uncontrolled.

Central region (four tiles at (512,1024), 1024-square, all 940 sources), medians of three rounds:

| Measurement | Control (ADR-069) | Reused buffers | Change |
|---|---:|---:|---:|
| Integration wall (s) | 14.6702947 | 13.6289897 | −7.098% |
| Reconstruction wall (s) | 9.5654078 | 9.0465202 | −5.425% |
| Process kernel CPU (s) | 26.171875 | 12.421875 | −52.537% |
| Process user CPU (s) | 237.265625 | 237.546875 | unchanged |

Individual integration observations: control 14.8100145 / 14.6164946 / 14.6702947, candidate 13.5399165 / 13.6289897 / 13.6515435.

Complete native grid, single pair:

| Measurement | Control (ADR-069) | Reused buffers |
|---|---:|---:|
| Integration wall (s) | 112.7421006 | 105.8856032 (−6.082%) |
| Reconstruction wall (s) | 74.1998770 | 72.3680864 |
| Rejection/maps wall (s) | 32.1998767 | 33.3365067 (unchanged code; run variation) |
| Process kernel CPU (s) | 257.171875 | 131.609375 (−48.824%) |
| Process user CPU (s) | 1700.796875 | 1717.46875 |
| Peak working set (MiB) | 3383.0 | 3413.7 |

End-to-end CLI, 32-frame DS-B, chronological-median: processing 6.2910175 / 5.7075395 s to 5.2639911 / 5.3319585 s; integration kernel CPU about 5.2–5.5 s to 2.9–3.0 s; 27 accepted / 5 rejected.

The ADR-069 control completed the full grid in 112.74 s today versus 130.01 s for the same build on 2026-10-26, so machine state differs between days. Only same-session pairs are compared; do not add savings across days. The time saved is smaller than the halved kernel CPU suggests because zeroing was spread across 24 workers and partly overlapped other work.

## Exactness and integrity

- All 10 profiling runs produce one identical set of raw-float hashes and whole master/coverage/high-rejection FITS per scope, and these equal the ADR-069 products (complete-grid master fc1af4a3…, coverage d65c87e4…, high rejection dff8eb3b…).
- DS-B: `verify-stack-equivalence.py` PASS (exact master/map FITS bytes and scientific report fields); all six runs share one hash per product, equal to the ADR-069 DS-B master.
- All 1140 DS-A sources match the stored SHA256/size/mtime fingerprints; all 32 DS-B sources match `ds-b-manifest.json`. No temporary or intermediate files remain.

## Limits

Windows 11 only. The benefit depends on the operating system allocator; macOS was not measured. No full reference stack, GUI timing or PixInsight review is claimed; products are bit-identical. Private logs and products: `outputs/buffer_reuse_20261027/`. Portable summary: [evidence](M5_CPU_BUFFER_REUSE_EVIDENCE.json).
