# M5: Bounded Original FITS Block Cache

Date: 2026-10-22. Scientific program revision: `8524fdaf3c51735b4660bdf4c269983496815970`. M5 remains open. The owner requested a RAM-aware original-data cache up to 1 GiB, exact DS-B master/map parity and a full DS-A external-200% performance comparison without cropping.

## Implementation and memory policy

`--fits-cache-mib 0..1024` defaults to **0 (off)** after this negative performance result; 1024 opts into RAM-aware caching up to 1 GiB. The measured program used explicit 0/1024 flags, so the subsequent default-only policy change does not alter the benchmark settings or scientific algorithm. Keep original signed-16 big-endian bytes in on-demand 128x128 source blocks. Never preload whole frames or store decoded/calibrated/registered image caches. Independent source quotas and LRU locks retain each source's recent spatial blocks across the output-tile-major traversal. Adjacent missing blocks are read together by source row. All-hit requests avoid file open/read calls. Read-only size/mtime checks, including all-hit requests, reject changed inputs.

Select the tile first, then allocate only available headroom to the cache, including conservative metadata/entry overhead and additional bounded transient read buffers. Preserve max(2 GiB, 10% physical RAM) system reserve, configured ceiling <=12 GiB and Windows peak-RSS enforcement. Insufficient/unknown RAM disables or reduces caching without changing tile geometry, reference canvas or pixel math. The RAM snapshot does not reserve memory against later use by other applications.

Decode requested ROI samples with exactly the same i16+BZERO calculation. Original ROI shape, Bayer phase, 8-pixel halo, cosmetics, VNG, global f64 geometry, photometry, rejection, f64 Neumaier accumulation and final master/map export are unchanged. No source writes, precision reductions or image intermediates.

## Verification

- Formatting and default/all-feature Clippy with warnings denied: PASS.
- [Windows MSVC and macOS Apple Silicon CI for the measured implementation](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39788822124): PASS. The delivered default-only policy change is verified again locally; hardware tests explicitly opt into cached reads.
- Release tests: 90/90 default and 91/91 all-feature PASS; real GPU hardware gates explicitly required locally.
- New gates cover block/odd sensor boundaries, signed decode, exact ROI values, no-read all-hit behavior, eviction, per-source concurrent quotas, cancellation, invalid ROIs, source size/mtime mutation, available-RAM headroom/disable and CLI bounds/off control. Source bytes remain unchanged.
- DS-B: all 32 inputs, 27 accepted/5 rejected, exact 4000x4000 reference grid at (0,0). Entire master/coverage/high-rejection FITS byte equality: PASS against both cache-off and prior verified CPU products.
- DS-A: all 1,140 inputs, 940 accepted/200 rejected, same reference grid/origin. Entire master/maps match the cache-off control AND owner-accepted M4 hashes. Registration, quality, photometry, exposure, valid/low/high and export counts remain exact. All 1,140 source hashes/inventory, 32 local sources, two PI references and external-template hashes remain unchanged; no temporary leftovers or image intermediates.
- Ordinary PixInsight import is not redundantly repeated for identical accepted FITS bytes. Existing absolute-photometry/reference-workload limitations remain.

## Measurement protocol

Both modes use the same preserved release executable, CPU engine, 16 workers, explicit 512x512 tiles, 3.0/2.5 clipping and 12288 MiB ceiling, with RegisteringTemplate200pct.fit and no crop. Each dataset runs off then on, sequentially, without concurrent local builds/tests/benchmarks. Hash checks precede the full pair. Startup and JSON publication are recorded separately as complete command times. Existing OS caches/background load are not controlled, so this is one descriptive pair, not an isolated causal or portable speedup estimate.

## Full 1,140-frame results

| Measurement | Cache off | Cache on |
|---|---:|---:|
| Processing seconds through FITS writes | 512.633 | 604.148 |
| Peak process MiB | 3423.148 | 4610.973 |
| Effective cache capacity MiB | 0.000 | 1024.000 |
| Retained original payload MiB | 0.000 | 992.652 |
| Application ROI source-read bytes | 11430134434 | 14511738880 |
| Read calls | 18619113 | 25858816 |
| File opens | 59983 | 59622 |
| Metadata checks | 59983 | 119605 |

Processing seconds saved: **-91.515 s**; percent shorter **-17.852%** (negative means slower). Source-read byte change: **+26.960%**; read-call change: **+38.883%** (positive means more). Cache serves 35.629% of requested ROI bytes from retained blocks. These counters cover integration reads only, include block prefetch and OS-cache service, and do not measure physical SSD traffic. Cache metadata/lookup/copy work is part of the measured path. Do not attribute variation in unchanged phases to caching.

| Non-overlapping wall phase, seconds | Off | On |
|---|---:|---:|
| reconstruction | 340.214 | 431.621 |
| rejection_accumulation_maps | 118.969 | 120.432 |
| inspection | 15.436 | 14.245 |
| registration | 28.365 | 26.104 |
| FITS_writes | 0.932 | 0.870 |
| output_copy | 0.356 | 0.386 |
| other | 8.361 | 10.491 |

**No acceleration was established.** Cache-on is slower in the full pair, so the feature remains explicit opt-in and the delivered CLI/library defaults are 0. Source-read amplification from partial-block prefetch and quota evictions is measured; detailed CPU causality is not isolated by this one pair.

The 32-frame pair measured 17.955/18.387 s and 665.488/889.980 MiB peak. Its source-read bytes fell 24.495%, but total processing did not improve. Retain this unfavorable small-run result alongside the full pair.

## Local evidence and reproduction

- `outputs/M5_FITS_cache_performance.json`: complete compact evidence, scientific parity, phases, I/O, memory and command/binary hashes.
- `outputs/M5_cache_DS-A_equivalence.json` and `outputs/M5_cache_DS-B_equivalence.json`: exact products/reports and source-integrity checks.
- `outputs/M5_cache_DS-A_on_external200.fits`, `_coverage.fits`, `_high_rejection.fits`: final full-set products, byte-identical to owner-accepted M4.
- Full detailed reports use the same prefixes with `.json`; timing logs use `outputs/M5_cache_DS-A_command_timings.json`.

Run from the project root using new destinations. The cache is a maximum; actual capacity can be lower when RAM is occupied.

```powershell
cargo run --release --features gpu --bin fulgurstack -- stack --engine cpu --threads 16 --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --fits-cache-mib 1024 --output "outputs/M33_cache_review.fits" --report "outputs/M33_cache_review.json"
cargo run --release --features gpu --bin fulgurstack -- stack --engine cpu --threads 16 --input "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --fits-cache-mib 0 --output "outputs/M33_no_cache_review.fits" --report "outputs/M33_no_cache_review.json"
```

Both also generate coverage and high-rejection maps. Inspect integration.fits_cache and integration.memory_plan for actual cache/I/O/memory behavior. CPU remains the default engine; the optional GPU path uses the same cache and retains its existing numeric gates.
