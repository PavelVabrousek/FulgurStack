# DS-A performance log and ten optimization rounds — 2026-11-05

Owner request of 2026-11-05: take the original 1,140-frame Seestar S50 set (DS-A) as the test set for the speed of every stage of the current run, from reading through measurement, registration on stars and on the Gaia catalogue, tile stacking on CPU and GPU, to the final export. Log the run in detail, find the stages worth optimizing, and repeat ten times: propose an optimization, test it on the same set for speed, stability and the resulting master, document it, and apply it if it passes. This run of DS-A was authorized by the owner. Decisions: ADR-144 to ADR-152.

## Method

- **Machine.** Windows 11, AMD Ryzen AI 9 HX 370 (12 cores, 24 threads), Radeon 890M integrated GPU, 31.1 GiB RAM, NVMe disk, on mains power.
- **Data.** `C:\Astrofoto\2026\10 October\SeeStar50P M33\M33 Subs`, 1,140 frames of 2160 × 3840, 10 s; 941 pass the quality limits.
- **Command.** `fulgurstack stack --input <DS-A> --geometric-correction --reference-strategy auto`, with `--engine cpu`, `--engine gpu`, or `--registration-engine sky --gaia-db "C:\PixInsightData\Gaia DR3"`. Auto rejection is ESD on this set; Auto tiles are 512 × 256.
- **Log.** Every run is sampled every 0.5 s (process CPU time, working set, private bytes, bytes read). Stage times come from the report, and from an opt-in stage timeline (`FULGURSTACK_TIMELINE=1`, ADR-144). Each master, coverage and high-rejection map is hashed.
- **Integration alone.** `profile_integration` replays the stored registration plan of the baseline run, its cubic models included, with Auto rejection (ADR-144). On the four central 512-pixel tiles one replay takes about 12 s, and repeated runs agree within about 1 %.
- **Noise.** The same program on the same data took between 90 and 140 s over the day: the laptop's clock speed changes with temperature and power state. Every comparison therefore alternates the old and the new program, and only pairs measured side by side are compared.
- **Acceptance.** A round passes only when every master stays byte-identical and the stage it targets is measurably faster in alternating runs. Failed rounds are reverted and recorded.

## Baseline: version 5c4c801

DS-A, CPU engine, first run: **121.5 s**, master `81024B85B3397F91`, byte-identical to the owner's stored `master_v54.fits`; 941/1140 frames; peak working set 1.9 GiB.

| Stage | Wall s | Share | Average CPU | Notes |
| --- | --- | --- | --- | --- |
| Inspection: reading and measuring | 11.7 | 10 % | 47 % | 16 frames at a time; read 57.9 s and measurement 127.7 s summed over threads |
| Registration on stars | 8.9 | 7 % | 11 % | two sequential walks; 337 frames need the triangle fallback (12.0 s summed), 603 the predicted match (1.9 s) |
| Integration: reconstruction | 53.7 | 44 % | | summed over threads: resampling 459 s, demosaicing 369 s, reading 225 s, cosmetics 220 s |
| Integration: ESD reduction | 42.8 | 35 % | | 757 CPU seconds |
| Writing master and maps | 0.6 | 1 % | | |
| Quality, report (158 MB JSON) and rest | 3.6 | 3 % | | stage timeline later showed the report takes 0.4 s |

Integration read 92.7 GB of application bytes for 941 frames of 16.6 MB, because each tile row is read once per column of tiles; physical reads stay far lower because the operating system's cache holds the files (ADR-117).

The ESD reduction, timed by step on the central tiles: sorting the columns 29 %, choosing the surviving range 21 %, the weighted mean of the survivors 24 %, rearranging the samples per pixel 16 %, preparing the samples 10 %.

**GPU engine** (`--engine gpu`): 113.0 s, master `47ECC0D63197520D`. Reconstruction stays on the CPU (65.8 s with the GPU's 512 × 512 tiles); the reduction on the Radeon 890M overlaps it but leaves 27.0 s of waiting. Its telemetry: 93.7 GB uploaded in 29.3 s, 52.8 s waiting for results.

**Sky registration** (`--registration-engine sky`): 135.0 s, master `7476DFC38C20E029`. Catalogue 0.4 s, plate solutions 5.2 s (one blind solve of about 2.4 s, then 940 seeded solves in parallel), reconstruction 74.0 s: all 940 frames registered through the sky carry a cubic model, and such frames reconstructed their whole region of interest.

## The ten rounds

| Round | Proposal | Test result | Decision |
| --- | --- | --- | --- |
| 1 | Reconstruct only the part of a distorted frame's region of interest that the tile samples, from the solved position of every sample | byte-identical; DS-A reconstruction CPU −1.8 % (only 16 frames have a cubic). The final check found LBN 437 reconstruction +24 % and sky mode +12 %: solving every sample position first cost more than it saved | replaced by 1b |
| 1b | The same, from the tile's edge samples only (about 1,500 instead of 131,000 positions); resampling unchanged | byte-identical everywhere; sky mode reconstruction CPU −19.5 % with 24 % less area scanned, LBN 437 −9 % | **applied** (ADR-144) |
| 2 | Measure up to 24 frames at once instead of 16, within a quarter of free RAM | 10.5 s against 10.3 s, 550 MB more memory: the physical cores are the limit, not the worker count | reverted (ADR-145) |
| 3 | ESD weighted mean of the survivors, four columns at once with AVX2, same additions in the same order | byte-identical; reduction −11 %, its CPU −13 % | **applied** (ADR-146) |
| 4 | ESD choice of the surviving range, four columns at once with AVX2 | byte-identical; reduction −8 % more, CPU −6 % | **applied** (ADR-146) |
| 5 | Read a region of interest with one read of its row span instead of row by row through a 64 KB buffer | 8.64 s against 8.65 s; CPU slightly higher | reverted (ADR-147) |
| 6 | Star registration: count the triangle votes in parallel chunks merged in order, and fit the 24 candidates in parallel | byte-identical; registration 9.2/7.8 s → 5.0/4.4 s (−45 %) | **applied** (ADR-148) |
| 7 | Sky registration: try all field sizes and parities of a blind search at once and read them in the sequential order | identical solutions for all 941 frames and identical master; plate solving 5.5 → 2.0 s | **applied** (ADR-149) |
| 8 | GPU: overlap upload and computation | already double-buffered; the Radeon 890M is the limit (about 82 s of ESD against about 35 s on the CPU after rounds 3–4). A faster shader would change the GPU master | analysis only (ADR-151) |
| 9 | Four reduction bands per thread instead of one, for balance | full frame: 35.4 s against 35.1–39.8 s for the old one, CPU higher | reverted (ADR-150) |
| 10 | 64-pixel rejection blocks instead of 32, for fewer memory streams | full frame: 29.05/26.63 s against 28.96/27.43 s | reverted (ADR-152) |

## Result

Alternating runs on the same machine state, all masters byte-identical to the baseline:

| Run | Before | After | Change |
| --- | --- | --- | --- |
| DS-A, CPU | 90.6 s | **76.5 s** | −16 % |
| DS-A, CPU (earlier pairs, before round 1b) | 119.1 / 114.2 s | 99.5 / 99.3 s | −15 % |
| DS-A, sky registration | 108.5 s | **83.7 s** | −23 % |
| DS-A, GPU engine (before round 1b) | 113.5 s | 104.6 s | −8 % |
| LBN 437, 166 frames of 9576 × 6388 | 110.8 s | **91.3 s** | −18 % |

DS-A CPU stages, the 90.6/76.5 s pair: inspection 6.5/6.4 s, registration 8.0/5.1 s, reconstruction 41.2/38.8 s, ESD reduction 33.8/25.0 s.

Masters: DS-A CPU `81024B85B3397F91` (also the stored `master_v54.fits`), GPU `47ECC0D63197520D`, sky `7476DFC38C20E029`; reference gates Seestar `3F752C45EFDB46DF`, LBN 437 `F22D99BF062BC003`, M42 `D2330D5E10FA77B0`.

## Against the alpha release and other programs

Four alternating pairs on 2026-11-05 afternoon, CPU engine, the alpha package 0.12 (`4d62d72`) against the current build; both masters `81024B85B3397F91`. The other programs were not rerun: their times come from their own logs of earlier runs on the same data (ADR-089 report; Siril on this PC on 2026-10-31).

| Program | Frames stacked | Time | Per stacked frame | Current FulgurStack is |
| --- | --- | --- | --- | --- |
| PixInsight FBPP 1.2.1 (log) | 1,140 | 34 min 56 s | 1.84 s | 20.0× faster (16.5× per frame) |
| StackingWizard BETA (log) | 1,025 | 29 min 52 s | 1.75 s | 17.1× faster (15.7× per frame) |
| Siril 1.4.5 (log, this PC) | 1,129 | 18 min 52 s | 1.00 s | 10.8× faster (9.0× per frame) |
| FulgurStack alpha 0.12 | 941 | 1 min 56 s (108.6–120.9 s, median 116.0 s) | 0.123 s | 9.5 % faster |
| **FulgurStack current** | 941 | **1 min 45 s** (103.3–109.8 s, median 105.0 s) | 0.112 s | — |

Peak working set: alpha 0.12 3.37 GiB, current 1.88 GiB (512 × 256 tiles, ADR-118). Stage medians, alpha → current: inspection 8.1 → 7.8 s, registration 9.0 → 4.7 s, integration 97.9 → 91.8 s.

## What is left

- **Reconstruction is now about half of a CPU run.** Resampling (Keys 4 × 4, already AVX2) and VNG demosaicing (already AVX2) take most of it; reading was ruled out twice (ADR-117, ADR-147).
- **ESD sorting** (about 29 % of the reduction) and the rearranging of samples per pixel (about 26 %); a larger block did not help (ADR-152).
- **Inspection** is limited by the physical cores; only less work per frame would help.
- **GPU.** On this PC the CPU engine is faster for ESD. A faster GPU needs a new shader, and its master would differ from today's GPU master.

## Limits

Measured on one Windows laptop whose speed changes during a session; the percentages come from side-by-side pairs, not from one long series. The GPU result was measured before round 1b. The AVX2 paths keep the scalar paths' results bit for bit, and a CPU without AVX2 runs the unchanged scalar code. macOS was not tried.
