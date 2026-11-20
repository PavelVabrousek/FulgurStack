# DS-A speed of version 0.15, and DSPX — 2026-11-18

Owner request of 2026-11-18: add the new stacking program DSPX, which the owner ran on the same data set, to the speed comparison, and regenerate the English comparison for developers of similar software. FulgurStack 0.15 was re-timed the same day so that both new numbers come from one machine state.

## Method

- **Machine.** The laptop of the [2026-11-05 log](M5_PERFORMANCE_DSA_20261105.md): Windows 11, AMD Ryzen AI 9 HX 370 (12 cores, 24 threads), 31.1 GiB RAM, NVMe disk, mains power. DSPX ran on it too.
- **Data.** DS-A, `C:\Astrofoto\2026\10 October\SeeStar50P M33\M33 Subs`, 1,140 Seestar S50 frames of 10 s, 2160 × 3840.
- **FulgurStack.** The 0.15 Windows package (`3736674`), `fulgurstack.exe stack --input <DS-A> --geometric-correction --reference-strategy auto`, CPU engine, Auto rejection (ESD), Auto tiles 512 × 256, three runs in a row. Time = the report's wall time.
- **DSPX.** Run by the owner in Easy mode; times from its own log: step 1 (analysis) and step 2 (stacking).

## Results

| Run | Frames stacked | Wall time | Peak RAM |
| --- | --- | --- | --- |
| FulgurStack 0.15, run 1 | 941 | 100.3 s | 1.90 GiB |
| FulgurStack 0.15, run 2 | 941 | 91.5 s | 1.89 GiB |
| FulgurStack 0.15, run 3 (median) | 941 | **97.2 s** | 1.89 GiB |
| DSPX, Easy mode | about 1,100 (40 left out by its analysis; the log prints no final count) | **813.0 s** (149.6 + 663.4) | not logged |

Stages of the median FulgurStack run: inspection 8.0 s, registration 5.3 s, integration 82.8 s (reconstruction 48.9 s, ESD reduction 33.7 s), writing 0.6 s.

DSPX log: analysis pass 1 138.0 s, registration 5.5 s, metrics 0.7 s (total 149.6 s); stacking integration 650.1 s, autocrop 1.2 s, master statistics 0.5 s, plate solving 10.3 s (total 663.4 s). Its analysis left out 35 out-of-focus frames, 4 clouded frames and 1 frame with a satellite trail.

All three FulgurStack masters stacked 941/1,140 frames, as on 2026-11-05 (105.0 s median then; the difference is within the laptop's day-to-day spread, so no speed change is claimed for 0.15).

## Comparison

| Program | Frames stacked | Time | Per stacked frame | FulgurStack 0.15 is |
| --- | --- | --- | --- | --- |
| PixInsight FBPP 1.2.1 (log, earlier run) | 1,140 | 34 min 56 s | 1.84 s | 21.6× faster (17.8× per frame) |
| StackingWizard BETA (log, earlier run) | 1,025 | 29 min 52 s | 1.75 s | 18.4× faster (17.0× per frame) |
| Siril 1.4.5 (log, earlier run) | 1,129 | 18 min 52 s | 1.00 s | 11.6× faster (9.7× per frame) |
| DSPX, Easy mode (log, 2026-11-18) | about 1,100 | 13 min 33 s | 0.74 s | 8.4× faster (7.2× per frame) |
| **FulgurStack 0.15** (2026-11-18) | 941 | **1 min 37 s** | 0.103 s | — |

## Limits

PixInsight, StackingWizard and Siril were not rerun. DSPX also crops and plate-solves its master; FulgurStack's star registration run does not plate-solve. Frame selection, debayering, interpolation and weighting differ between the programs. One laptop, whose speed changes with temperature and power state.
