# Changelog

Each version lists what changed for users, with the architecture decisions (ADR) that record the details in
[docs/ARCHITECTURE_DECISIONS.md](docs/ARCHITECTURE_DECISIONS.md). The application's About page carries the
release notes since the previous alpha package; this file keeps the whole history. Windows packages are built
locally (see [docs/WINDOWS_DISTRIBUTION.md](docs/WINDOWS_DISTRIBUTION.md)).

## 0.16 — 2026-11-20 (ADR-187 to ADR-190)

- **Trail mask for ESD (ADR-187).** Optional, in Settings › Stacking and as `--trail-mask hysteresis|grow`: around paired upper ESD rejections, the faint edges of asteroid and satellite trails in the same frame are rejected too, so they leave no trace in the master. Off by default.
- **Drizzle 2× (ADR-188), first version.** Optional, in Settings › Stacking and as `--drizzle`: integration onto a grid with half-size pixels, Bayer drizzle from raw CFA samples for colour cameras, in the same single pass and without intermediate files. CPU engine only.
- **Drizzle without a size limit (ADR-189).** A drizzle master and its maps are written tile by tile as they are made, so the sensor size is not limited by memory (M 101, 104 Mpx drizzle master: peak 3.4 GB RAM).
- **Speed comparison (2026-11-18).** DS-A re-timed with 0.15 (97.2 s median) and DSPX added to the comparison; the English summary for developers is in `docs/FulgurStack_speed_comparison_2026-11-18.pdf` and the log in `docs/reviews/PERFORMANCE_DSA_20261118.md`.

## 0.15 — 2026-11-17 (ADR-173 to ADR-186)

- **Results view (ADR-183).** Zooming into an XISF master, coverage map or high-rejection map now shows sharp native pixels instead of the upscaled 2048 px overview. When the full-resolution detail cannot be read, a red banner says so.
- **Plate solution in the maps (ADR-184).** The coverage and high-rejection maps now carry the same WCS astrometry as the master, in FITS and XISF, so they line up with the master in PixInsight and other tools.
- **Median capture time (ADR-185).** The master and both maps carry the median capture time of the stacked frames as DATE-OBS and DATE-AVG, instead of the start time of the reference frame.

Also the work from the [code review of 2026-11-13](docs/CODE_REVIEW_2026-11-13.md). Master and map pixels from 16-bit cameras are identical to 0.14 (DS-B checked; the headers gain the median time and the maps the WCS); only sensors below 16 bits change slightly, see ADR-177.

- **Application (ADR-173).**
  - A light set set to "None" calibration stays uncalibrated after rescans, new calibration sets and restarts.
  - Opening files with the program no longer replaces the saved light sets.
  - Only the newest scan of a light set counts.
  - Background tasks (Gaia check, target lookup, index test and build, Auto-solve) can no longer stay "running" after an internal error.
  - Abandoned calibration previews and in-memory masters stop.
  - Closing the application warns about an index build or master in progress.
- **Robustness (ADR-174).**
  - A damaged blind-solve index or XISF file is refused instead of ending the program.
  - Index files are saved without clashing partial files.
  - Master headers are read safely.
  - Hot-pixel maps use the shared no-overwrite writer.
  - Calibration masters from many frames stay within about 2 GB.
  - Several internal crash points became error messages.
  - A cancel during geometric correction stops sooner.
- **`fulgurstack solve` (ADR-181).** Blind plate solving moved into the main command line: `fulgurstack solve-index build|info` and `fulgurstack solve`, with the options of the former `fulgur-solve` program, which is removed. A solve report must now be a new `.json` file.
- **Clearer errors and CLI exposure per set (ADR-180).** A file that cannot be opened is named in the error. With `--input-sets`, `--exposure` takes one value per set. A failed Gaia or field-centre setup reports its cause. The experiment tools moved to `tools/archive/`.
- **Code structure (ADR-179).** One file per CLI command, two registration entry points, the integration function split into named steps, and the application's stacking settings in one group. Masters, options and reports are unchanged.
- **One stacking path (ADR-178).** The application and the CLI make masters through the same code after registration. Masters are unchanged. A sky registration without its own grid solution now gets its WCS by solving the reference in the application too.
- **Registration (ADR-177).** Stars clipped by a sensor below 16 bits (12-bit Dwarf3, 14-bit DSLRs) no longer enter the brightness scale. A missing sky level refuses the frame on every path. Joined cameras always get absolute weights. Excluded frames can no longer join two nights into one session. The CLI `register` command runs the same registration as `stack`.
- **Engine clean-up (ADR-176).** About 3,400 lines of tests moved out of production files, repeated numbers named, and four copies of the registration finish shared; no stacked value changed.
- **Documentation (ADR-175).**
  - Milestone reviews moved to `docs/reviews/`, finished plans to `docs/archive/`.
  - New: an architecture overview, this changelog and an open-items register.
  - The specification has an amendments index.

## 0.14 — 2026-11-09 (ADR-165 to ADR-172)

- **Registration.**
  - Blind plate solving with the new module `fulgur-solve`: it places a frame of 0.5 to 12 degrees on the sky with no coordinates or target name, from a quad index built once from the Gaia DR3 files (ADR-168). The application builds, tests and deletes the index in Settings, and Auto-solve in the Field centre row finds the field of the first frames (ADR-169).
  - A second camera that shares only saturated stars with the reference registers through their positions (ADR-165).
  - A target is suggested from the object name, file names and folder names when the headers name none (ADR-167).
  - A failed plate solve says where it searched and offers the star registration.
- **Stacking and output.**
  - Exposure written in milliseconds (Vaonis Stellina) is read correctly (ADR-166).
  - Positions are shown as hours, minutes and seconds.
- **Application.**
  - Warnings that can be put right carry a red fix button (ADR-170), and those that can be left alone a "Keep as is" button (ADR-171).
  - The status message sits in the status bar.
  - Settings use drop-down lists, and the smallest text is larger.
- **Command line.** New program `fulgur-solve` to build and inspect the index and place frames on the sky.
- **Fix.** Calibration masters were built again in every phase when a run needed more than four of them.

## 0.13 — 2026-11-08 (ADR-154 to ADR-164)

- Masters carry their plate solution as FITS WCS (TAN with SIP), written only when the master's own stars match Gaia through it (ADR-155, ADR-158).
- Plate solving adds its star lists to an existing quality measurement (ADR-154). It is the default for mixed light sets and takes the field centre from the reference header (ADR-162).
- A light set without stars in common with the reference (broadband L against narrowband OIII) keeps a brightness link of its own (ADR-156).
- Calibration sets filled kind by kind are offered to light sets, and a stack without calibration is flagged (ADR-157).
- A folder dropped on the start screen gives one light set per sub-folder with frames (ADR-159).
- A mono and a colour camera never form one light set (ADR-161).
- Hot pixel maps, a fifth calibration file type made from any dark or bias (ADR-163).
- Quick and full test suites with `docs/TESTING.md`. Dead code was removed (ADR-160).
- The master's FITS `REJECT` and `CREATOR` cards are truthful.

## 0.12 — 2026-11-01 (ADR-101 to ADR-115)

- Separate masters per filter or light set on one pixel grid (ADR-101), and sky offset per colour channel (ADR-102).
- Geometric correction is on by default in the application (ADR-103).
- XISF input, bottom-up FITS and sensors below 16 bits (ADR-104 to ADR-106).
- Dense-field registration with flux-ranked lists, and an Auto reference that continues (ADR-107, ADR-108).
- Experimental partial frames (ADR-109), and one stack from several cameras on an external reference (ADR-110).
- Faster ESD, inspection, reconstruction and median, with byte-identical masters (ADR-111 to ADR-114).
- Version number and About page (ADR-115).

### Published after 0.12, without a package (ADR-116 to ADR-153)

- Registration of several cameras and optics in both directions, mirrored cameras and adjustable accuracy (ADR-116 to ADR-130).
- Sky registration: every frame is plate-solved against Gaia DR3 and registered through the sky, from the CLI or the application (ADR-131 to ADR-138).
- Splitting a light set by camera, gain and filter, and light folders that read only their own files (ADR-139, ADR-140).
- One-column Settings page with info markers (ADR-141). Masters and maps can be written as FITS or XISF (ADR-142).
- DS-A speed rounds: full 1,140-frame run in 105 s median, masters byte-identical (ADR-144 to ADR-153).

## First alpha package — 2026-10-31 (ADR-084 to ADR-100)

- Reconstruction of missing header values with a camera catalogue and owner confirmation (ADR-084).
- Dense-field registration fixes (ADR-085), and quality metrics of the stacked master (ADR-086).
- Pixel rejection methods: Winsorized, ESD, linear fit and median, with Auto by frame count (ADR-087).
- GPU pixel rejection and a weight plot (ADR-090).
- Windows portable package with a Czech guide (ADR-089, ADR-100).

## Before the alpha (M1 to M5, 2026-10-21 to 2026-10-30)

- **M1–M4, accepted by the owner:**
  - FITS reconstruction (VNG) and single-frame export;
  - subframe inspection and quality filtering;
  - rigid registration to an anchor;
  - tiled CPU integration with Winsorized clipping and Neumaier accumulation;
  - coverage and rejection maps.
- **M5 (open):**
  - Native Windows application (Compact Desk);
  - CPU optimizations with byte-identical masters;
  - validated geometric correction;
  - Auto reference;
  - light and calibration sets;
  - full calibration with bias, darks and flats (ADR-071 to ADR-083).
- Evidence for this period is in [docs/reviews/](docs/reviews/).
