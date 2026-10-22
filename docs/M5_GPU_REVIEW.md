# M5 First Slice: GPU Cosmetics and VNG

Date: 2026-10-22. M4/G4 is owner-accepted. M5 is in progress; this slice introduces optional GPU reconstruction only. GUI and GPU warp/reduction remain later work.

## Scope and profile evidence

The full M4 external-200% run spent 442.753 s (70.09% of processing) in reconstruction batches and 132.635 s in rejection/accumulation/maps. The first hybrid engine moves the conservative CFA cosmetic filter and accepted VNG reconstruction to bounded GPU ROI batches. CPU inspection, registration, f64 inverse geometry/photometry, Keys sampling, rejection and Neumaier accumulation retain the accepted algorithms. No source or intermediate image files are written.

The approved CPU engine remains the default. Optional `--features gpu` and explicit `stack --engine gpu` select hardware acceleration. Unsupported hardware and software adapters fail clearly. CPU/GPU benchmark commands include initialization, transfers, dispatch/wait/readback and final FITS writes. Device telemetry identifies AMD Radeon(TM) 890M Graphics, Vulkan, AMD proprietary driver 26.8.1 (LLPC). Numerical hardware parity has been tested on this device only; Metal/DX12 compilation does not establish their numerical parity.

## Predeclared quality gates and verification

Gates were recorded in PROJECT_SPEC section 7 and ADR-021 before measurements: reconstruction maximum <=0.05 source ADU; exact native post-cosmetic values and correction counts; final normalized RMS <=1e-5 and maximum <=1e-3; isolated-star aperture flux differences <=0.1%; exact quality selection and scientific registration/photometry. Report map count changes explicitly.

- Formatting and both default/all-feature Clippy checks: PASS.
- Default release suite: 79/79 PASS. GPU-feature release suite: 80/80 PASS, including two explicitly required hardware gates with `FULGURSTACK_TEST_GPU=1`, rather than hardware skips.
- Static generic/generated WGSL validation runs without GPU hardware. CI compiles/tests default and GPU features on Windows MSVC and macOS Apple Silicon. Hardware gates explicitly report SKIP unless requested; requesting hardware may not silently skip or use a software adapter.
- Hardware reconstruction: four Bayer phases, fractional flat/color/noise fields, gain/offset, hot pixels, supported/saturated narrow stars, minimum/odd ROI sizes, cancellation and the real 2160 x 3840 CFA frame. Maximum synthetic difference 0.01171875 ADU; real-frame maximum 0.0078125 ADU and RMS 0.000008196 ADU. All native samples and correction counts match; 2,178 real-frame cosmetic corrections on both paths.
- Hardware integration: scale 0.33/1/2/3 with rotation and fractional translations, clipping, masks, coverage/high maps and flux. Largest synthetic normalized difference 1.49014e-8; maps/counts match exactly.
- Existing integration-relative photometry gates remain in place. The disclosed narrow colored-star analytic CFA/VNG counterexample is unchanged; these parity tests do not establish new absolute photometric accuracy or equivalent PixInsight processing workloads.

## Reproduction

Use new output prefixes; existing products are never overwritten. Run from the project root:

```powershell
cargo run --release --features gpu --bin fulgurstack -- stack --engine gpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --output "outputs/M5_owner_review.fits" --report "outputs/M5_owner_review.json"
cargo run --release --features gpu --bin fulgurstack -- stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --output "outputs/M5_owner_cpu.fits" --report "outputs/M5_owner_cpu.json"
```

Both generate final coverage and high-rejection FITS beside the master. Open normally in PixInsight; compare the same canvas/region with the CPU product using a shared display stretch. Inspect bright-star colors/cores, faint structure and edges. Maps remain fractions of all 32 original frames. Owner visual approval of the GPU products is separate from automated import/parity verification.

Hardware test invocation:

```powershell
$env:FULGURSTACK_TEST_GPU='1'
$env:FULGURSTACK_TEST_FITS_DIR=(Join-Path $PWD 'SeestarS50 Subs')
$env:FULGURSTACK_TEST_FITS=(Join-Path $env:FULGURSTACK_TEST_FITS_DIR 'Light_M 33_10.0s_IRCUT_20261006-204534.fit')
cargo test --workspace --all-features --release -- --nocapture
```

The development-only independent NumPy checker `scripts/tests/verify-gpu-stack.py` compares real CPU/GPU masters, isolated-star sky-subtracted RGB apertures, diagnostic-map counts, complete scientific registration reports, approved M4 CPU byte hashes and source/reference integrity. It writes JSON/PI probe manifests only; NumPy is not a product dependency.

## Real DS-B delivery and measured performance

Scientific program revision: `a21f38523c589061df2b34454c144e5573614782`. Final commands ran sequentially with the same release binary, 16 CPU workers and existing OS/driver caches, CPU followed by GPU for each geometry. No build or second benchmark ran concurrently. These are measured individual commands, not randomized repeated trials or cold-cache measurements. Processing includes GPU initialization and all three FITS writes; complete command time additionally includes Cargo launch and report serialization.

[Cross-platform CI for this exact scientific program revision](https://github.com/PavelVabrousek/FulgurStack/actions/runs/39764626633). Subsequent report/spec-layout documentation changes do not alter the measured program.

| Geometry | CPU processing | GPU processing | CPU/GPU speed ratio | Complete CPU/GPU command | CPU/GPU peak process RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| native 2160 x 3840 | 18.590 s | 17.518 s | 1.061x | 18.869 / 17.811 s | 710.7 / 806.2 MiB |
| external 200%, 4000 x 4000 | 22.514 s | 23.362 s | 0.964x | 22.829 / 23.746 s | 679.6 / 720.1 MiB |

**Quality parity: PASS. General performance improvement: NOT ESTABLISHED.** Native processing is 5.77% shorter in this final pair, while external-200% processing is 3.77% longer. This experimental GPU path is not promoted to the default. No full 1,140-frame GPU run or speed extrapolation is claimed in this slice. All runs selected the same 27/32 inputs, rejected five, preserved the complete reference canvas at (0,0), and retained 270 s accepted exposure.

| GPU telemetry | Native | External 200% |
| --- | ---: | ---: |
| initialization, included above | 0.499 s | 0.296 s |
| reconstruction wall, including raw reads/CPU warp | 12.417 s | 13.798 s |
| CPU reconstruction counterpart | 13.997 s | 13.079 s |
| upload/compute/wait/readback/host-copy batch total | 8.830 s | 7.769 s |
| synchronous GPU batches | 531 | 1,018 |
| reconstructed ROI pixels, including halos/overlap | 341,191,540 | 215,616,134 |
| uploaded/downloaded bytes | 1,364,908,464 / 4,094,332,464 | 862,748,168 / 2,587,458,760 |
| peak explicit device/readback buffers, includes growth overlap | 48.034 MiB | 8.034 MiB |
| CPU rejection/accumulation/maps wall | 2.055 s | 5.114 s |

The GPU batch timer is not a kernel-only profile. Large numbers of small synchronous batches and RGB round trips limit this slice, especially on the denser output grid. Next optimize batching/overlap and measure transfer versus dispatch costs before expanding GPU warp/reduction. Any GPU warp must separately pass coordinate/phase/mask/photometry gates; do not replace CPU f64 accumulation merely to chase throughput. GUI remains outside this slice.

The initial generic storage-indexed shader measured native CPU/GPU 19.327/23.606 s and external CPU/GPU 23.314/25.967 s. It was rejected as an acceleration candidate. Constant-index generated stencils and phase-uniform dispatch reduced overhead. Startup/cache/system variation also contributes; these nonrandomized revision measurements do not isolate a pure shader speedup. An intermediate native pair was 21.739/18.338 s; its external CPU command was interrupted while a new source revision was being built and is excluded from performance evidence.

## Real numerical and import checks

| Gate | Native | External 200% |
| --- | ---: | ---: |
| normalized master RMS difference | 1.10727e-10 | 7.98728e-9 |
| maximum absolute normalized difference | 5.96046e-8 | 5.53317e-5 |
| isolated unsaturated stars tested, all RGB apertures | 245 | 118 |
| maximum relative RGB aperture-flux difference | 4.95308e-8 | 2.87796e-8 |
| changed coverage / high-rejection channel-map samples | 0 / 0 | 1 / 1 |
| total surviving-sample / high-rejection count delta | 0 / 0 | -1 / +1 |

All predeclared gates pass. Native maps are byte-identical to CPU; the external grid has one additional high rejection at (195,3316), red channel, explaining the largest master difference (about 3.63 source ADU after the fixed divisor). Low-rejection totals and geometric valid sample counts agree. This is a threshold decision from tiny f32 differences, not rescaling or a changed rejection policy. Registration/quality/photometry reports agree exactly after removing timing/working-set fields. All six CPU FITS products match the accepted M4 products byte-for-byte, proving the CPU refactor preserves the baseline.

All 32 sources, two immutable PI references and the external template retain SHA-256 hashes. No image intermediates or `.tmp` files remain; GPS stays out of default reports and logs. Ordinary PixInsight import of all six GPU master/map files: PASS, exact probes/orientation/range, zero out-of-range samples. This checks import, not owner visual acceptance.

Delivered local prefixes:

- `outputs/M5_DS-B_gpu_native_final`: native master, coverage, high-rejection and detailed run JSON.
- `outputs/M5_DS-B_gpu_external200_final`: 200% master, coverage, high-rejection and detailed run JSON.
- `outputs/M5_GPU_performance_report.json`: compact performance, parity, product hashes and PI import results.
- `outputs/M5_DS-B_native_validation.json` and `outputs/M5_DS-B_external200_validation.json`: all tested apertures and independent parity checks.
- `outputs/M5_DS-B_native_PI_validation.json` and `outputs/M5_DS-B_external200_PI_validation.json`: native ordinary-open proofs.
- `outputs/M5_DS-B_final_command_timings.json`: complete matched command timers.

M5 remains open. This first slice is ready for owner image review; consistent end-to-end acceleration, full DS-A GPU scaling, hardware parity on other backends and subsequent M5 slices remain unverified.
