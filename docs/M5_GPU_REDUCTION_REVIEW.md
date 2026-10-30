# M5: GPU Pixel Rejection and Weighted Mean (2026-10-30)

> **Documentation context — 2026-10-30:** dated review of ADR-090, published to `main` with the owner's authorization of 2026-10-30. See [current state](CURRENT_STATUS.md) for authoritative behavior. M5 remains open.

## Starting point

The first M5 GPU slice (ADR-021 onwards) moved cosmetic correction and VNG demosaicing to the GPU. Since then the CPU path gained AVX2 VNG, AVX2 cosmetics and buffer reuse. Measured on the reference machine (Radeon 890M integrated GPU, Vulkan) before this work:

| Stack | CPU integration | GPU-reconstruction integration |
| --- | --- | --- |
| Seestar DS-B, 27 frames | 5.6 s | 14.2 s |
| ASI2600MC Flaming Star, 30 frames | 21.3 s | 42.4 s |

The old GPU path was two to three times slower than the CPU. It read about 3× more data back from the device than it sent (RGB vs CFA) and kept the CPU idle while the device worked. Integration time is dominated by pixel rejection and the weighted mean, not by reconstruction. On Dwarf3 (856 frames) that part took 94.8 s of 173.3 s, and it ran only on the CPU.

## What was built

- **GPU pixel rejection (`fulgur-gpu/src/reduce.rs`, `reduce.wgsl`).** One workgroup owns one output pixel channel. Each workgroup:
  - bitonic-sorts that channel's samples from all frames in workgroup memory;
  - applies the selected method: winsorized sigma clipping, ESD, linear fit clipping or median;
  - sums the survivors' weights.

  Details:
  - **Methods.** All four use the CPU definitions, thresholds and minimum-sample rules (ADR-087).
  - **ESD critical values** are computed on the CPU in f64 for every sample count, then uploaded.
  - **Median and MAD** for winsorized clipping come from binary searches on the sorted samples.
  - **Precision.** Sums are f32 Kahan, taken around the sample median. No f16 is used.
- **Upload.** Each pixel chunk goes to the device as one contiguous run of all frames per pixel channel:
  - four CPU threads write each chunk;
  - on an integrated GPU the data is written straight into mapped device memory, so there is no staging copy;
  - two upload slots let one chunk be written while the device reduces the other.
- **Overlap.** The device reduces tile k while the CPU reconstructs tile k + 1. The tile memory plan therefore counts two tiles of samples plus the device buffers, which are system memory on an integrated GPU.
- **Stages.**
  - `--engine gpu` now means CPU reconstruction with GPU pixel rejection, the fastest measured split.
  - `--gpu-stages reconstruction|all` keeps the old GPU reconstruction, alone or together with GPU rejection.
  - GPU rejection also supports monochrome stacks. GPU reconstruction remains colour-only.
  - The CPU engine is unchanged, byte for byte.
- **GUI.** Settings › Advanced › Processing engine › GPU uses the default split. CPU threads can now be set for the GPU engine too, and the old mono warning is gone.
- **Reports.** `integration.backend` names the split (for example `cpu-reconstruction-gpu-reduction`). `integration.gpu_reduction` holds:
  - adapter, sort size and threads;
  - chunk count and uploaded/downloaded bytes;
  - upload time (overlapped) and GPU wait time.

  With GPU rejection, `reduction_wall_seconds` counts only the wait that is not hidden behind reconstruction.

## Real data (Windows 11, release, Radeon 890M; ESD as chosen by Auto)

| Stack | Engine | Integration s | Reconstruction s | Rejection wait s | Total s | Peak RAM GiB |
| --- | --- | --- | --- | --- | --- | --- |
| Flaming Star, 30 × 300 s, calibrated | CPU | 19.7 | 7.2 | 11.9 | 25.8 | 3.58 |
| | **GPU** | **12.9** | 10.7 | 1.3 | **20.5** | 3.76 |
| M42, 235 frames 30–300 s | CPU | 153.9 | 68.2 | 84.9 | 182.9 | 2.99 |
| | **GPU** | **103.8** | 100.5 | 2.4 | **133.8** | 2.91 |
| Dwarf3, 856 × 60 s, darks | CPU | 173.3 | 78.4 | 94.8 | 230.2 | 3.05 |
| | **GPU** | **115.7** | 103.2 | 11.9 | **180.9** | 5.90 |
| Seestar DS-B, 27 frames | CPU | 5.8 | 1.8 | 3.8 | 7.5 | 0.97 |
| | **GPU** | **3.7** | 2.0 | 1.5 | **5.5** | 1.59 |

The CPU rows of the three large sets are the stored ADR-087 runs; both DS-B rows were measured on 2026-10-30. The GPU rows were measured on 2026-10-30. On the large sets integration is about one third shorter.

Almost all of the rejection now hides behind reconstruction, so reconstruction is the new limit. It became slower in parallel, from 78 to 103 s on Dwarf3: the integrated GPU, the upload threads and the reconstruction workers share one memory bus. This is a measured trade-off, not a free gain.

Dwarf3 needs 2.85 GiB more RAM, because two tiles of 856 frames are resident at once. Peak RAM stays well under the 12 GiB ceiling.

**Agreement with the CPU masters (0..1 FITS units):**

| Stack | Master RMS / max | Coverage map: differing pixels (max) | Rejected samples CPU / GPU |
| --- | --- | --- | --- |
| DS-B | 9.7e-8 / 3.8e-4 | 8 of 24.9 M (0.094) | 4 225 401 / 4 225 397 |
| Flaming Star | 5.5e-8 / 4.3e-4 | 11 of 78.3 M (0.30) | 10 408 876 / 10 408 880 |
| M42 | 1.7e-9 / 1.1e-5 | 41 of 78.3 M (0.014) | 44 270 786 / 44 270 762 |
| Dwarf3 | 5.7e-10 / 1.2e-6 | 36 of 25.2 M (0.0027) | 111 801 996 / 111 801 977 |

**Agreement criteria:**
- **Master.** The pre-declared T-NUM-01 limits are normalized RMS ≤ 1e-5 and maximum ≤ 1e-3. Every set passes.
- **Master quality.** Green sky noise is identical to three decimals in every set (DS-B: identical in all three channels).
- **Maps.** They differ only where a sample lies exactly at a rejection threshold, in f32 against f64. ESD then removes a different number of samples in that pixel. The largest such case is 9 of 30 frames in one Flaming Star pixel. These differences are reported, as T-NUM-01 requires; byte identity was never the requirement.

## Synthetic benchmark (developer tool)

`cargo run --release -p fulgur-gpu --example bench_reduce -- <frames> <pixels>`: GPU time per pixel channel, 65 536 pixels, median method:

| Frames | 32 | 128 | 512 | 856 |
| --- | --- | --- | --- | --- |
| ns per pixel channel | 64 | 191 | 866 | 1 535 |

The bitonic sort dominates and scales as P log² P. ESD adds about 25% and winsorized clipping about 15%. Tried and kept or rejected:
- **Kept:** sorting keys only, without frame indices (−30%), and CPU packing through a cache-resident block (writing scattered samples into mapped memory directly was 7× slower).
- **No measurable effect:** 128 or 512 threads instead of 256, and replacing divisions in the sort index.

## Verification

- `cargo fmt --all -- --check` and clippy (default and all features): PASS. `cargo test --workspace --release`: 213/213 default, 217/217 with all features, run with the real FITS, SIMD and GPU variables.
- Hardware gate `hardware_gpu_pixel_rejection_matches_cpu_for_every_method_and_stack_size` (FULGURSTACK_TEST_GPU=1):
  - random stacks with noise, outliers, invalid samples and unequal weights;
  - 1, 4, 5, 27, 40, 300 and 856 frames, with all four methods;
  - checks the T-NUM-01 limits, identical sample counts and coverage statistics, and rejected samples within 0.2%;
  - result: winsorized, ESD and median match to RMS ≤ 3.8e-10 with identical rejection counts; linear fit (f32 line fit) to RMS ≤ 9.7e-8 with rejection counts within 5 of 123 758.
- The two existing GPU reconstruction gates now run explicitly with `--gpu-stages reconstruction` and keep their exact expectations.
- Unit tests:
  - the shader validates for every method and sort size;
  - workgroup memory plans;
  - run packing.
- The CPU engine's DS-B master and maps are byte-identical to the run before this change.

## Owner action steps

```powershell
cargo run --release --bin fulgurstack --features gpu -- stack --engine gpu --input "SeestarS50 Subs" --output "outputs/review_gpu_new.fits" --report "outputs/review_gpu_new.json"
cargo run --release --bin fulgurstack --features gpu -- stack --engine cpu --input "SeestarS50 Subs" --output "outputs/review_cpu_new.fits" --report "outputs/review_cpu_new.json"
```

The two masters should be indistinguishable in PixInsight. In the GUI choose Settings › Advanced › Processing engine › GPU and compare the Processing times of a large set.

## Not verified / limits

- Only one GPU was tested: the integrated Radeon 890M. A discrete GPU uploads through a staging copy. Its speed, and the Metal backend on macOS, are untested.
- Reconstruction is now the limit. It slows down while the GPU works, because the memory bus is shared.
- Stacks larger than the workgroup sort size fail clearly with GPU rejection and must use the CPU. On this adapter the largest power of two that fits is reported by `max_frames`.
- The DS-A full 1,140-frame reference run was not repeated.

Sanitized numbers: [evidence](M5_GPU_REDUCTION_EVIDENCE.json).
