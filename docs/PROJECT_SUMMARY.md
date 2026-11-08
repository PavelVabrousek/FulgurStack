# FulgurStack — project summary (2026-11-08)

FulgurStack stacks deep-sky light frames into one linear 32-bit master for PixInsight. It is written in pure Rust, needs no C/C++ libraries and runs on Windows 11, with a desktop application and a command line. It is in alpha testing; macOS is a target but not yet verified.

## Principle

Most stacking tools work frame by frame and write every intermediate result to disk: calibrated, debayered and registered copies of each frame. FulgurStack turns this around:

1. **Measure first, on the raw data.** Star quality (HFR, FWHM, eccentricity, sky noise) and the geometry of every frame are measured directly on the raw Bayer data, without demosaicing.
2. **Register on stars or on the sky.** Frames are matched by star triangles to a reference. Alternatively, every frame is plate-solved against Gaia DR3 and registered through the sky, which also joins several cameras and optics in one stack. Each mapping must pass a whole-field accuracy check, or the frame is left out.
3. **Stack the output tile by tile.** For each small output tile, for example 1024 × 256 px, only the needed part of every frame is read. That part is calibrated, debayered, resampled, cleaned by outlier rejection (winsorized, generalized ESD, linear fit or median) and summed with weights in compensated double precision. Nothing is written to disk except the master, its maps and a report, and memory stays bounded whatever the frame count.

The master carries a plate solution (FITS WCS), so it is ready for colour calibration in PixInsight.

## Results

Test set: 1,140 frames of M 33 from a Seestar S50 (10 s each, 2160 × 3840, 17.6 GiB). FulgurStack, PixInsight and Siril ran on the same 24-thread laptop with 32 GB RAM; the StackingWizard log does not name its machine.

| Program | Frames stacked | Time |
| --- | --- | --- |
| PixInsight Fast Batch Preprocessing 1.2.1 | 1,140 | 34 min 56 s |
| StackingWizard (beta) | 1,025 | 29 min 52 s |
| Siril 1.4.5 | 1,129 | 18 min 52 s |
| **FulgurStack** (CPU, ESD rejection) | 941 | **1 min 45 s** (peak RAM 1.9 GiB) |

- **Speed:** FulgurStack is 11–20 times faster, or 9–16.5 times per stacked frame. The other programs wrote about 100–230 GB of intermediate files; FulgurStack writes none.
- **Quality:** on 337–369 matched stars, the FulgurStack master's stars were 0.9–5.5 % narrower than in all four reference masters, including a manual PixInsight master. Its sky noise matched what its frame count predicts. This comparison used an earlier build (932 frames, winsorized rejection).
- **Fewer frames:** FulgurStack stacks fewer frames because it leaves out frames that fail its accuracy check.

Limits: the other programs' times come from their own logs of earlier runs and were not rerun. Frame selection and interpolation differ between the programs. FulgurStack has been measured on this one machine only.
