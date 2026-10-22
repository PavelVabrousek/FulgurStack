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
