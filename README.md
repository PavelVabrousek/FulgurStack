# FulgurStack

A CPU-first astrophotography stacking project for ZWO Seestar S50 Pro exposures, targeting a linear 32-bit floating-point RGB FITS master for further processing in PixInsight.

## Project Status

M1 implements immutable 16-bit CFA FITS input, conservative isolated hot-pixel correction, multithreaded VNG debayering, atomic linear-light float RGB FITS export, and a dataset I/O survey. Earlier MHC outputs failed owner review because of stellar artifacts; Sensor-bounded VNG passed owner visual inspection in PixInsight on 2026-10-21: **M1 / G1 PASS**. MHC remains available for diagnostics. M2/G2, M3/G3 and M4/G4 were owner-accepted on 2026-10-22. M5 starts with optional GPU cosmetics/VNG acceleration; the approved CPU engine remains the default. See [M1 review instructions and evidence](docs/M1_REVIEW.md).

## Latest CPU Optimization Results

The owner-authorized [CPU optimization batch](docs/M5_CPU_OPTIMIZATION_SUMMARY.md) preserves byte-identical master, coverage and high-rejection FITS on 32 and 1,140 inputs. The latest full external-200% CPU run took **247.673 s**, with **18.754 s** registration and **3397.9 MiB** peak RSS. The earlier accepted M4 run took 632.991 s; this historical endpoint comparison is not a controlled attribution of all speed changes. Cache remains off, the full reference grid is preserved, and CPU/GPU selection remains available. Local release gates pass (118 default / 120 all-feature tests, including explicitly required GPU and AVX2). GitHub Actions stay disabled; M5 remains open.

## Native Windows Application (M5 Slice)

The connected native Workspace combines navigation/settings on the left, a continuously scrollable Quality table without pagination, and a selected subexposure preview on the right. It supports real stacking, optional review before registration, cancellation and master/map previews. CPU remains the default; GPU is an explicit option in GPU-enabled builds. M5 remains open. See [current Windows progress, manual review steps and historical verification](docs/M5_WINDOWS_APP_REVIEW.md).

```powershell
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui -- "SeestarS50 Subs"
```

Add inputs, choose an existing output folder and a master filename, then select Stack images. Existing output names receive a new version suffix. Coverage and high-rejection file saving are separate Output options, off by default. Normal launches remember selected inputs, output, reference and stack preferences in local application data; launch without input arguments to restore the previous selection. Open exported FITS normally in PixInsight. The first native 32-frame CPU outputs were byte-identical to stored accepted CLI products; later UI-only iterations use the owner's manual review policy without new baseline/full-set benchmarks.

Inspect quality retains the complete assessment for the current application session. Stack images reuses it after checking that the source list, sizes and modification times are unchanged. Quality's Use checkboxes control actual frame inclusion; Accepted/Rejected tabs only filter the view. Use automatic selection restores the original recommendations. Processing shows a compact modal without scrolling, with STOP, phase times and progress; reused inspection is explicitly labeled Reused. Restarting restores preferences but requires a fresh quality assessment.

## M2 Quality Inspection

M2 adds CPU inspection of original CFA lights with sky/noise estimates, detected star counts, HFR, moment eccentricity, explicit rejection reasons and provisional weights. M1 and M2/G2 are owner-approved. See [M2 validation and manual-review table](docs/M2_REVIEW.md).

```powershell
cargo run --release --bin fulgurstack -- inspect --input "SeestarS50 Subs" --report "outputs/M2_review_new.json" --csv "outputs/M2_review_new.csv"
```

Choose unused report names. The CSV uses UTF-8 with BOM, semicolons and decimal points; the JSON includes up to 250 measured stars per frame with zero-based source coordinates. CSV star counts are uncapped detections, distinct from measured stars. Weights are provisional until M3 matches stars and measures transparency. Inspection preserves every input and writes no intermediate images.

## M3 Registration Plan

M3 directly registers original-CFA star catalogs to an automatic chronological-median anchor, including arbitrary field rotation and session-anchor composition. It measures matched-star photometry, final weights and safe common/80%-weight canvas rectangles. All 32 DS-B frames registered; M2 retains 27 and excludes five. See [M3 gates and telemetry](docs/M3_REVIEW.md) and [32-frame manual table](docs/M3_DS-B_REVIEW.md).

```powershell
cargo run --release --bin fulgurstack -- register --input "SeestarS50 Subs" --report "outputs/M3_owner_review.json" --csv "outputs/M3_owner_review.csv"
```

Reports use zero-based original sensor coordinates and source-to-master rigid transforms. Rejected measurable frames are registered for diagnostics only and retain zero final weight. No image intermediates are written; resampling and the integrated FITS master belong to M4 after owner G3 approval.

## M4 Tiled Integration

M4 processes raw CFA ROIs in bounded spatial tiles and integrates registered, normalized, pixel-rejected samples into one atomic linear 32-bit RGB FITS. It preserves accepted M1 VNG reconstruction, M2 selection and M3 registration/photometry. See [M4 review and performance evidence](docs/M4_REVIEW.md) and [predeclared quality gates](docs/M4_TEST_PLAN.md).

```powershell
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --output "outputs/M4_reference_review.fits" --report "outputs/M4_reference_review.json"
```

Open the FITS normally in PixInsight and apply a display STF as desired. No loading script or manual intensity conversion is required. The JSON distinguishes frame rejection, channel-sample rejection, geometric coverage and surviving weight. The default canvas preserves the reference FITS dimensions at origin (0,0), without cropping. The reference is the chronological median of quality-accepted inputs; `--reference` selects an accepted source manually. Both `register` and `stack` support it. Every stack also writes RGB coverage and high-rejection FITS maps beside the master (custom paths: `--coverage-output`, `--high-rejection-output`). Map values 0..1 mean 0..100% of all original input frames; coverage counts actual surviving samples after all rejection. See [current reference/map review](docs/M4_REFERENCE_MAPS_REVIEW.md). An existing external FITS path is also accepted as `--reference`, including cropped templates at 33..300% scale. It defines geometry only; native source photometry, exposure and GPS provenance remain unchanged. See [scaled reference tests and commands](docs/M4_SCALED_REFERENCE_REVIEW.md). `--canvas coverage-optimized` or `--canvas common-intersection` explicitly requests a crop. Repeat `--input` for additional folders. Existing outputs are never overwritten. The owner accepted G4 after the full-set visual review; documented analytic-photometry and unmatched PI-workload limitations remain explicit. GPU acceleration starts in M5 below.

## M5 GPU Reconstruction

Build with `--features gpu` and select `stack --engine gpu` to reconstruct bounded CFA ROI batches on a hardware GPU. Inspection, registration, geometric resampling, rejection and f64 accumulation retain the CPU implementation. GPU selection fails clearly if unsupported; it never silently substitutes a CPU benchmark. The report identifies the adapter/backend, initialization, transferred bytes, reconstruction batch time and reserved GPU/readback buffers. Readback time includes CPU copies and waiting; it is not a GPU-only kernel timer. The first native GUI is available above; GPU warp/reduction remain later work.

```powershell
cargo run --release --features gpu --bin fulgurstack -- stack --engine gpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --output "outputs/M5_owner_review.fits" --report "outputs/M5_owner_review.json"
```

Use unused output names. Open the master and its automatically generated coverage/high-rejection maps normally in PixInsight. Compare with the accepted CPU image on the same reference grid. Hardware numeric parity is tested locally with `FULGURSTACK_TEST_GPU=1`; The historical CI workflow is retained, but GitHub Actions are disabled by owner policy; current gates run locally and do not claim new macOS verification. See [M5 GPU validation and performance](docs/M5_GPU_REVIEW.md).

The owner retained explicit CPU/GPU selection and authorized a CPU-side follow-up: reuse working buffers during pixel rejection while requiring exact before/after master/map byte equality. See [rejection-memory optimization and matched full-dataset measurement](docs/M5_REJECTION_MEMORY_REVIEW.md). The same reduction path supports both reconstruction engines.

Tile selection now defaults to **automatic**: before integration, reserve at least 2 GiB or 10% of physical RAM for the system, then select a previously verified **512 x 512**, **256 x 256** or **128 x 128** square within available RAM and the configured memory ceiling. The full reference canvas is preserved. Exact master/map equality passed on DS-B and all 1,140 DS-A inputs; the matched full CPU pair measured 676.741/558.198 s with 993.7/3412.6 MiB peak RSS. Use `--tile-size 128|256|512` for a manual square, or advanced `--tile-width` and `--tile-height` overrides for rectangles, including the original 256 x 128. Manual requests fail if they cannot fit; they are never silently resized. The report includes the RAM snapshot, reserve, estimates and effective geometry. These controls cannot be combined. An omitted rectangular companion dimension defaults to 512. Automatic selection needs an available-RAM measurement; manual mode explicitly reports if only the configured ceiling can be enforced. This is a starting snapshot; tile dimensions stay fixed during the run. See [tile geometry selection, quality gates and performance](docs/M5_TILE_GEOMETRY_REVIEW.md). GPU remains an explicit optional engine.

## M1 Commands

Run from the project root. Output filenames must be new; existing files are never overwritten.

```powershell
cargo run --release --bin fulgurstack -- debayer-single --input "SeestarS50 Subs/Light_M 33_10.0s_IRCUT_20261006-204534.fit" --output "outputs/M33_vng_review.fits" --report "outputs/M33_vng_review.json" --demosaic vng
cargo run --release --bin fulgurstack -- survey --input "SeestarS50 Subs" --report "outputs/DS-B_review.json"
```

Float FITS samples use a fixed linear scale: source ADU / 65,535, with zero offset and the same divisor for every image and channel. Reconstruction is completed at export by limiting interpolation excursions to the normalized sensor range 0..1, as approved by the owner. In-range values remain unchanged; reports count lower/upper limits. GPS is retained in the FITS and excluded from default reports and console output.

In PixInsight, open the new FITS normally with **File > Open**. No loading script, special STF or manual intensity conversion is required. Exports retain the supplied single-frame reference's intensity units and saturation bounds. Different diagonal coefficients, borders and cosmetics mean bitwise equality is not promised. Historical unbounded exports and the legacy loading script are not the current review candidate. Use `--demosaic mhc` only to reproduce the earlier reconstruction baseline (its final export uses the same 0..1 bounds). See [M1 review](docs/M1_REVIEW.md).

## First-Release Goals

- Process hundreds to thousands of exposures of one target and filter, across one or more sessions, including different exposure durations.
- Preserve source images and avoid writing intermediate calibrated, debayered, or registered images.
- Handle field rotation and keep memory bounded during integration.
- Preserve linear photometry and retain reference-anchor observing coordinates in the final master while redacting them from default logs and reports.
- Validate output quality against supplied PixInsight references before accepting speed improvements.

## Documentation

- [Windows UI design proposal](docs/WINDOWS_UI_DESIGN.md): quick stacking with optional quality review, layout alternatives and native implementation acceptance.
- [Native Windows application](docs/M5_WINDOWS_APP_REVIEW.md): launch commands, continuous Quality review, subexposure preview and local verification.
- [Full 1,140-frame 200% reference run](docs/M4_FULL_SCALED_RUN_REVIEW.md): 4000 x 4000 master/maps, measured performance and verification.
- [Full 1,140-frame native-reference run and performance profile](docs/M4_FULL_RUN_PROFILE.md): no-crop master/maps, measured bottlenecks and CPU/GPU optimization proposals.
- [Project specification](PROJECT_SPEC.md): authoritative scope and milestone acceptance gates.
- [Engineering instructions](AGENTS.md): implementation and verification rules.
- [Architecture decisions](docs/ARCHITECTURE_DECISIONS.md): recorded engineering choices.
- [Benchmark references](docs/BENCHMARKS.md): datasets, PixInsight settings, and owner-reported timings.
- [Development setup](docs/DEVELOPMENT.md): repository setup and Windows prerequisites.

## Local Data

Real FITS/XISF images, the `SeestarS50 Subs/` and `PixInsight Files/` directories, build artifacts, and generated outputs are excluded from Git. Keep these files locally; GitHub is not a backup of the observing data. The DS-B JSON manifest records relative filenames, sizes, and hashes without image pixels or GPS values.

## Language and Platforms

Project artifacts are English; communication with the project owner is Czech. Primary validation targets Windows 11 x86_64, with macOS Apple Silicon build and test support required by the project specification.

## Automatic and Manual Tile Selection

Run from the project root with new output names. Automatic is the default; no repeated geometry benchmarking is performed. The 12 GiB ceiling still applies. Close other applications if even the 128-pixel option cannot fit after the system reserve. The GPU engine remains optional and uses the same selection policy with its own conservative memory estimate.

```powershell
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --tile-size auto --output "outputs/M33_auto_tiles.fits" --report "outputs/M33_auto_tiles.json"
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --tile-size 256 --output "outputs/M33_manual_tiles.fits" --report "outputs/M33_manual_tiles.json"
```

Both commands also export coverage and high-rejection FITS on the unchanged reference grid. Inspect `integration.memory_plan` in the JSON report for the allocation decision.

See [RAM selection policy, verification and observed telemetry](docs/M5_RAM_TILE_SELECTION_REVIEW.md).

## Bounded FITS Block Cache

`stack` may retain original FITS byte blocks in RAM, up to `--fits-cache-mib 1024` (explicit opt-in; default 0). The available-RAM planner first chooses the output tile, then assigns only the remaining headroom to the cache, retaining the system reserve and configured ceiling. It reduces or disables caching if needed; unknown physical RAM disables it. `--fits-cache-mib 0` uses the direct reader for comparison. No decoded/calibrated/registered frame cache or image intermediates are created. Pixel arithmetic and final products remain unchanged.

The report's `integration.fits_cache` includes accounted capacity/retained bytes, hits/misses/evictions, source-read bytes and read/open counts. These describe application reads during integration, including OS-cache service and block prefetch; they are not physical SSD traffic. `integration.memory_plan` includes cache capacity and conservative transient buffers. Performance depends on the workload and must be measured against the uncached mode with the same tile geometry.


The matched full CPU pair measured **512.633 s without / 604.148 s with cache**, with exact master/map equality. Source-read bytes and read operations increased, so the cache remains optional and defaults to **off**. A smaller 32-frame pair also showed no speedup. See the [cache review](docs/M5_FITS_CACHE_REVIEW.md) for the retained measurements; do not infer a speedup from cache hits alone.


## CPU Resampling Optimization

CPU inverse resampling now reuses exact coordinate products and contiguous footprint access, with unchanged Keys arithmetic, precision and scientific outputs. Cache remains off by default. Entire master/coverage/high-rejection FITS byte equality passed on 32 and 1,140 inputs and against accepted products. The fresh full CPU pair measured **523.466 / 501.305 s** (4.233% shorter), reconstruction **344.081 / 329.472 s**, peak RSS **3409.789 / 3416.391 MiB**, using the full 200% reference without cropping. This is one measured pair; unchanged inspection/registration also varied. See [CPU resampling verification, performance and commands](docs/M5_CPU_RESAMPLING_REVIEW.md). Optional GPU selection is retained; M5 remains open.


## Exact CPU VNG Optimization

CPU VNG now uses generated constant-index kernels and direct interior-window access, preserving exact coefficients, arithmetic and reflected borders. Cache remains off. Entire master/coverage/high-rejection FITS equality passed on 32 and 1,140 inputs and against accepted CPU/M4 products. The fresh full pair measured **510.694 / 438.435 s** (14.149% shorter), reconstruction **334.964 / 260.558 s** (22.213% shorter), peak RSS **3422.172 / 3404.977 MiB**, with the complete 200% reference and no crop. This is one measured pair; overlapping task timers are not wall phases. See [VNG verification, performance and release commands](docs/M5_CPU_VNG_REVIEW.md). No new dependency; optional GPU is retained and M5 remains open.


## Exact Pixel-Rejection Layout

Rejection now packs 32 neighboring output pixels into bounded pixel-major RGB stacks, retaining exact source order, clipping and f64 accumulation. At most 16 bands reuse scratch; every tile estimate includes the extra packing bound. Cache remains off. Entire master/maps match accepted products on 32 and 1,140 inputs. The fresh full pair measured **437.471 / 430.546 s** processing (1.583% shorter), rejection/maps **124.304 / 118.462 s** (4.699% shorter), RSS **3405.273 / 3419.832 MiB**. The small-set rejection phase was 5.130% slower; preserve this modest, workload-dependent result. See [rejection layout verification, performance and commands](docs/M5_REJECTION_LAYOUT_REVIEW.md). M5 remains open.


## Exact CPU Cosmetic Correction

Cosmetic verification completed: local format/default-all-feature lint and release gates PASS (98/98 default, 100/100 all-feature), required real local GPU gates PASS. Entire 32/full-1140 master/maps equal fresh controls and accepted CPU/M4 products; all sources/reference hashes, scientific/work/I/O and memory plans remain exact. Fresh full CPU processing 447.335/420.343 s (6.034% shorter), reconstruction 271.522/252.006 s, peak RSS 3411.090/3405.703 MiB. Summed overlapping cosmetic task elapsed is 18.258% shorter, not additive wall/CPU time. Retain the preliminary slower sorting-network evidence and all unchanged-phase variation; one pair with uncontrolled OS caches/load is not an isolated causal/portable guarantee. FITS cache stays 0, no crop and no quality-policy change. Remote Windows/macOS CI for this revision is BLOCKED by GitHub account billing/spending restrictions and is not claimed as passed. See [cosmetic review](docs/M5_CPU_COSMETIC_REVIEW.md); M5 remains open.


## Continuous Bounded CPU Scheduling

Continuous CPU scheduling verification: local format/default-all-feature lint and release tests PASS (101/101 default, 103/103 all-feature), required real GPU gates PASS. Exact whole DS-B and full DS-A master/maps match fresh controls and accepted CPU/M4 products; all input/reference hashes, science/work/I/O and memory-plan fields unchanged. At most 16 logical ROI workers continuously refill; nested work cannot exceed that bound and source-order publication is exact. GPU dispatch retains its original batches. Fresh full processing 408.271/383.990 s (5.947% shorter), reconstruction 246.191/220.943 s (10.255% shorter), peak RSS 3416.082/3398.227 MiB. Retain the slower small-set processing 15.885/16.558 s and all unchanged-phase variations; a single sequential pair with OS caches/background load uncontrolled is not an isolated causal/portable guarantee. Cache remains 0, full 4000-square reference without crop. Remote measured-code Windows/macOS CI is BLOCKED by GitHub account payment/spending restrictions and is not claimed as passed. See [continuous scheduling review](docs/M5_CPU_SCHEDULING_REVIEW.md). M5 remains open.


## Exact SIMD CPU Resampling

SIMD CPU resampling local verification PASS: 104/104 default and 106/106 all-feature release tests, format/lint, explicitly required AVX2 and real GPU hardware gates. Whole DS-B/full DS-A master/maps equal fresh controls and accepted CPU/M4; all source/reference hashes, scientific/work/I/O and memory-plan fields remain exact. Runtime AVX2 computes four coefficient sets and paired RGB accumulations with original operation order and scalar fallback. Fresh full processing 387.618/350.600 s (9.550% shorter), reconstruction 222.171/184.033 s (17.166% shorter), peak RSS 3399.652/3391.395 MiB. Summed overlapping resampling task elapsed is 46.892% shorter, not additive wall/CPU time. Keep the slower preliminary gather candidate and unchanged-phase variations. One pair with OS caches/load uncontrolled is not an isolated causal/portable guarantee. Cache stays 0, full 4000-square reference without crop. All work/tests remain local and uncommitted; no pushes, PR updates or remote CI. Source fingerprints and binary hashes identify the measured local code. See [SIMD CPU review](docs/M5_CPU_SIMD_REVIEW.md); M5 remains open.


Pixel-rejection optimization locally verified: format/default-all-feature lint, release tests 106/106 and 108/108, explicitly required GPU/AVX2 PASS. Exact entire 32/full-1140 master/maps equal fresh controls and accepted CPU/M4; all input/reference hashes, scientific/work/I/O/memory fields preserved. Certified median/MAD fixed point skips only redundant scratch winsorization; original iterations remain the fallback. Full processing 366.351/322.559 s (11.954% shorter), rejection/maps 123.116/83.337 s (32.310% shorter), peak RSS 3400.324/3407.215 MiB. Keep higher small-set RSS and unchanged-phase variations; one pair with OS caches/load uncontrolled is not an isolated causal/portable guarantee. Cache 0, full 4000-square reference without crop. All work remains local/uncommitted; no pushes, PR updates or remote CI. See [pixel-rejection review](docs/M5_CPU_REJECTION_REVIEW.md); M5 remains open.


SIMD CPU cosmetic correction locally verified: format/default-all-feature lint, release suites 109/109 and 111/111, explicitly required real GPU/AVX2 PASS. Eight independent pixels use reversible f32 total-order integer keys and exact median/MAD arithmetic; scalar confirmation, borders, tails and portable fallback remain unchanged. Exact whole DS-B/full DS-A master/maps match stored accepted CPU/M4, source/reference hashes and all scientific/work/I/O/memory fields preserved. Reused stored baseline without reexecution: full processing 322.559/282.965 s (12.275% shorter), reconstruction 186.705/142.527 s (23.662% shorter), RSS 3407.215/3399.676 MiB. Cosmetic overlapping task elapsed is 86.078% shorter, not additive wall/CPU time. Retain unchanged-phase/small-set adverse variations; historical comparison with OS caches/load/temperatures uncontrolled is not isolated causal/portable acceleration. Cache 0, no crop, only new-version stacks, all local/uncommitted, no pushes or remote CI. See [SIMD cosmetics review](docs/M5_CPU_COSMETIC_SIMD_REVIEW.md); M5 remains open.


Shared RGB rejection preparation locally verified: format/default-all-feature lint, release suites 112/112 and 114/114, required real GPU/AVX2 PASS. One source-order RGB/coverage gather, shared weights with exact independent-mask fallback, dense per-channel clipping/Neumaier and unchanged scientific rules. Reserved payload 44 bytes/source/band fits existing 64-byte scratch allowance; plans unchanged. Entire 32/full master/maps match stored accepted CPU/M4, all source/reference hashes and science/work/I/O/memory fields exact. Reused stored baseline without reexecution: full processing 282.965/275.913 s (2.492% shorter), preparation/rejection/maps 89.296/82.613 s (7.485% shorter), RSS 3399.676/3417.559 MiB. Historical comparison with OS conditions uncontrolled; preserve unchanged-phase/adverse variation, no isolated causal/portable claim. Cache 0, no crop, only new-version stacks, all local/uncommitted, no pushes or remote CI. See [shared RGB review](docs/M5_SHARED_RGB_REJECTION_REVIEW.md); M5 remains open.


Parallel external registration locally verified: format/default-all-feature lint, release suites 114/114 and 116/116, actual GPU/AVX2 required PASS. Disjoint Rayon source-row chunks cap concurrent external fits at sixteen; original solver/prediction/row arithmetic and all serial source-ordered planning/reductions retained. Exact whole 32/full master/maps match stored accepted CPU/M4; all input/reference hashes, scientific/work/I/O/memory fields preserved. No baseline rerun: full processing 275.913/300.919 s (9.063% longer), registration 25.259/26.244 s (3.899% longer), RSS 3417.559/3391.301 MiB. Keep unfavorable small-set and unchanged-phase variations. Historical comparison with OS conditions uncontrolled is not isolated causal/portable acceleration; registration timer includes native/external/catalog/canvas work. Cache 0, full reference/no crop, only new-version stacks, all local/uncommitted, no pushes or remote CI. See [external registration review](docs/M5_EXTERNAL_REGISTRATION_REVIEW.md); M5 remains open.


The [M5 registration profile review](docs/M5_REGISTRATION_PROFILE_REVIEW.md) separates native fitting, external reference work and canvas planning on 32 sources. Native triangle fallback dominates this sparse subset; the existing parallel external fit loop is a small remaining cost. Exact master/map and local hardware/test gates pass. Cache remains off; M5 remains open.


The [M5 registration fallback review](docs/M5_REGISTRATION_FALLBACK_REVIEW.md) records lazy reference-star preparation and bounded consensus-buffer reuse, exact 32/full master-map parity, and local-only release gates. Against the latest stored full control, registration measured 26.244 -> 18.754 s and processing 300.919 -> 247.673 s; substantial variation in unchanged phases prevents assigning the entire total reduction to this optimization. FITS cache stays off; M5 remains open.
