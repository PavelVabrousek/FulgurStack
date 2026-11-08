# FulgurStack Architecture Decisions

## ADR-164: Version 0.13, release notes since 0.12, consolidated publication and package (2026-11-08)

**Status:** Owner authorization of 2026-11-08: prepare the alpha distribution 0.13, add release notes since 0.12, commit and push.

**Decision.** The workspace version goes from 0.12.0 to 0.13.0. The About page lists release notes since the alpha package 0.12. One commented commit with `[skip ci]` publishes ADR-154–ADR-163 (master WCS, set photometry, calibration sets, drop behaviour, mono/colour grouping, plate solving by default for mixed sets, hot pixel maps, test suites and dead-code removal). Actions configuration is unchanged; local images, outputs, preferences, raw telemetry and binaries are excluded. The Windows package `FulgurStack-0.13.0-<commit>-windows-x64.zip` stays local; publishing it is not part of this authorization. This authorization applies to this batch only.

**Verification.** See docs/CURRENT_STATUS.md for the suites run and the package checks.

## ADR-163: Hot pixel maps, a calibration file type made from any dark or bias (2026-11-08, local)

**Status:** Owner proposal of 2026-11-08, implemented in the engine, the CLI and the application. Local, not published; the application screen awaits the owner's manual check. Nothing changes for runs without a map: the reference masters are not affected.

**Decision.**
- The owner names the share of the sensor to repair (default 0.01 %, at most 5 %). From the per-pixel median of any dark or bias frames (any exposure) the program takes that share of pixels with the highest excess over the median of their own 2 x 2 Bayer phase. Ranking by excess keeps the colour with the higher offset from filling the map alone.
- In every light, after calibration and before cosmetics and demosaicing, each map pixel is replaced by the median of its same-colour neighbours (two pixels away on a Bayer mask, one on a mono sensor); neighbours that are hot themselves are left out. The step also runs for the quality measurement, so the star lists are free of the mapped pixels. A map works alone or together with bias, dark and flat.
- A map is a calibration file of its own: an 8-bit FITS image with `IMAGETYP = 'Hot Pixel Map'`, `HOTCOUNT` and `HOTPCT`, written atomically and never over an existing file. CLI: `--hotpixel-map` (map file, or darks and bias as files or folders), `--hotpixel-percent`, `--hotpixel-map-save`. The report holds `calibration.hot_pixels` and the formula ends with "hot pixel map". Application: a fifth card "Hot pixels" in a calibration set with the share (Settings value `hot_pixel_percent`), a saved map or a master dark counted as a master file; the stack and save buttons of the other cards are not shown for it.

**Not done.** The map has no preview of its own pixels, and the application does not yet write a map file (use the CLI option). Only a mono or Bayer light of the sensor size of the map is accepted.

**Verification.** Format and Clippy PASS; quick suite PASS. New tests: the engine test (brightest share, replacement from same-colour neighbours, ROI equal to the full frame, save and load, no overwrite, invalid share, calibration formula and report) and a CLI test (built from darks, saved, reused, combined with a dark, refused share, darks byte-identical). The real folders were not run.

## ADR-162: Plate solving is no longer experimental: it is chosen for mixed light sets and takes the field centre from the reference header (2026-11-08, local)

**Status:** Owner decision of 2026-11-08, after the run in which the ASI2600MM L set could not be registered to the ASI2600MC reference (the star lists of the two cameras share about nine stars, far below the 50 the whole-field check needs; the Gaia plate solve of the reference put both fields at one place). Application change only; awaiting the owner's manual check. Local, not published.

**Decision.**
- When the included light sets differ in camera (INSTRUME), header colour layout or sensor size, the registration method switches to Plate solving as soon as the headers are read, once per session and only when a Gaia folder or downloading is set. The owner's own choice in Settings wins for the session; the method row names the reason. The labels read "Stars · one camera" and "Plate solving · any camera"; "experimental" is gone from the method, About and CLI help (partial frames stay experimental).
- The pointing in the header of the reference becomes the field centre: the chosen reference, else the first frame of the first included set. `fulgur_core::fits::read_sky_position` reads, in this order, the centre of the WCS (inverse TAN from CRVAL, CRPIX and CD or CDELT), the RA and DEC cards (degrees or sexagesimal) and OBJCTRA and OBJCTDEC, from FITS or XISF. It never reads the site location. The Settings field switches to Coordinates and shows "From the header of …". Coordinates the owner typed are kept; when they are more than 0.5° from the header, a button "Use the header position" offers the header's.

**Not done.** The auto switch is not stored: after a restart mixed sets switch again unless chosen in that session. The Auto reference may still pick another frame than the one whose header gave the centre; the solver searches around the centre, and cameras of one pointing differ by a fraction of a degree.

**Verification.** Format and Clippy PASS; core test of the header pointing (WCS, shifted reference pixel, RA/DEC, sexagesimal, none, invalid); GUI test of the automatic switch. The run with the real MM and MC folders was not repeated.

## ADR-161: A mono and a colour camera never form one light set (2026-11-08, local)

**Status:** Owner report of 2026-11-08: two sets of lights (ASI2600MC colour, ASI2600MM mono) were dropped one after the other and the program neither told the cameras apart nor debayered the colour frames. Local, not published; awaiting the owner's manual check of the application.

**Cause (from the last run's report and settings).** Both folders ended in one light set whose layout had been declared Mono; all 46 frames (20 MM, 26 MC) were stacked as mono. A declared layout overwrote each frame's header pattern before the frames were compared, so a mono and a colour camera of one sensor size looked identical: one group, no "several cameras" warning, no split offered. Dropping onto a set also added any folder to it without looking at the headers. Splitting a set copied the declared layout to every new set.

**Decision.**
- Inspection compares the header colour layout before the owner's declared layout is applied (`FrameGroup.header_pattern`); a mono and a colour camera are always two groups, and the group label shows the true colour or mono.
- Splitting a set whose groups differ in header layout gives each new set Auto, so each reads its own headers.
- A folder dropped on a light set whose first frame shows another camera (INSTRUME) or Bayer pattern becomes a light set of its own; same-camera folders still extend the set. The message says so.

**Verification.** Format and Clippy PASS; quick suite PASS (adds one pipeline assertion and the GUI test `a_folder_of_another_camera_dropped_on_a_set_becomes_its_own_set`). The real folders were not re-run through the application. The release GUI (with GPU) is built in `targetelease`.

## ADR-160: Two automated test suites, a CLI smoke test and removal of dead code (2026-11-08, local)

**Status:** Owner request of 2026-11-08: review requirements and results, test the CLI, sort the automated tests into a quick regression suite and a full slower suite, document automated testing and remove unused source. Local, not published; no scientific output changed.

**Decision.**
- `scripts/test-quick.ps1` runs format, Clippy and every test except the 19 slow ones in `scripts/tests/slow-tests.txt` (297 tests, 7 s of test time on a warm build). `scripts/test-full.ps1` runs everything with the all-feature build, real DS-B frames, AVX2 and GPU gates, the PixInsight adapter tests and `scripts/test-cli-smoke.ps1`. It refuses to start without the data, so real-data tests cannot pass vacuously; a gate that cannot run gives INCOMPLETE, never PASS.
- `scripts/test-cli-smoke.ps1` is new: every CLI command on DS-B with exit codes, RAM, privacy, immutability, no-intermediates and the PixInsight quality comparison.
- Dead code removed: the wrappers `inspect_inputs`, `inspect_inputs_with_progress`, `register_inputs`, `register_inputs_with_reference`, `register_inputs_with_options`, `register_inspected`, `register_inspected_with_progress`, `accepted_monochrome_frames`, the duplicate `calibration::mismatch_warnings` and the unused GUI dev-dependency `sha2`. Two tests that called wrappers now call the functions the application uses; the gain-differs warning of ADR-080 gained the direct assertion it only had through the duplicate.
- Defects found and then fixed on request: FITS `REJECT` card always `WINSOR-MAD` (D-01) and `CREATOR` version `0.1.0` (D-02); `MasterMetadata` now carries the applied rejection, tested for all four methods. See [Testing](TESTING.md) for the full list, the proposed new tests and the retirement proposals.

**Verification.** Before and after the removal: format PASS, Clippy `-D warnings` default and all features PASS, 316 passed (default) and 320 passed, 3 ignored (all features with real frames, AVX2 and GPU), 0 failed. CLI smoke PASS (stack of 27/32 frames in 4.5 s, 1.17 GB peak RAM, quality versus the PixInsight master PASS, inputs byte-identical). Windows only.

## ADR-159: A dropped folder without frames of its own gives one light set per sub-folder, and says so before the drop (2026-11-08, local)

**Status:** Owner report of 2026-11-08: a folder dragged onto the start screen showed "New light set", but releasing it created nothing. The owner tests the change manually (UI only). Local, not published.

**Cause.** The drop reached the application: a simulated drop of a folder with frames creates its light set. A light folder gives only the files directly in it (ADR-140). A folder that holds frames only in sub-folders (such as "20261105 Olyhen", one sub-folder per filter) or none at all (the new folders "olyhen RGB" and "M 45_sub Seestart30p EQ 30s" are empty) added nothing, with a note in the status line only, while the hint during the drag promised a new light set.

**Decision.**
- A folder added as lights, by drop or by Add light folder, gives itself when it holds frames directly, as before. Otherwise, every folder below it (up to four levels) that holds frames directly becomes its own light set. Frames are still never gathered from sub-folders into one set (ADR-140). Folders whose names mark calibration (bias, dark, flat, dark flat) are left out and named.
- While dragging outside every set, the hint says what the drop gives: "New light set", "3 new light sets, one per folder", the calibration folders left out, or, in red, "No FITS or XISF files here or in its sub-folders".
- When nothing is added, the reason is also shown on the start screen card, not only in the status line.

**Verification.** fmt and clippy: PASS. Application tests 28 passed, 3 ignored, with a new test of drops on the start screen (a folder with frames, a target folder with three filter folders and a flat folder, an empty folder: hint, result and notice). The earlier test of ADR-140 now expects the sub-folder as its own set. The release application is built for the owner's manual review.

## ADR-158: The master's plate solve falls back to the frames' own field, and a failed solve is recorded (2026-11-07, local)

**Status:** Owner report of 2026-11-07: PixInsight SpectrophotometricColorCalibration refused the Seestar S30 Pleiades master `master_v110` with "The image has no valid astrometric solution". Local, not published.

**Cause.** `master_v110` carries no WCS: its header says "No plate solution", and its report has no astrometry entry, so no grid solution was made. Since `master_v105` (M 33) no master of a star registration has a WCS (Olyhen, Phi Tauri, M 45). The master's plate solve searched only around the field chosen in Settings (Object or coordinates), which overrides the frames' OBJECT card. A field left over from an earlier target finds no solution: the same M 45 frames with the object "M 33" end with "the reference frame could not be plate-solved: blind search found no unique match". The failure reached only the Results line; the master and report did not say a solve was tried. With the object "m45", the current application writes the WCS on these frames (fresh inspection and the reused one with the owner's selection of `master_v110`: 295/300 stars on Gaia, RMS 0.36 px), so the field chosen at the time of `master_v110` most likely named another target.

**Decision.**
- When the chosen object or coordinates name another field than the frames' most common OBJECT (names compared without case, spaces or punctuation; centres more than 0.25° apart), the frames' centre becomes a fallback. The master's plate solve tries the chosen centre first and the frames' centre after it. Sky registration itself still searches the chosen centre only.
- A chosen object that gives no centre at all falls back to the frames' OBJECT at once, with a line saying so, in both registration methods; before, the run stopped.
- A plate solve that was tried and gave no WCS is recorded: the report gets `astrometry: {written: false, reason}`, and the master's history says "Plate solve tried, no WCS written; reason in the JSON report." in FITS and XISF.

**Verification.**
- fmt and clippy: PASS. Workspace tests: 315 passed, 0 failed, 3 ignored, with a new test of the centre choice: same name written differently, no object, an object or coordinates of another field, coordinates within the field, an unknown name, and nothing resolvable.
- M 45 (503 frames) through the CLI: object "M 33" and coordinates 0°/0° both give the WCS from the frames' centre (294/300, RMS 0.37 px). An empty catalogue folder without downloads records the reason in the report and the two history lines.
- New ignored application test (`real_star_run_writes_the_master_wcs`) runs the application's stack on a real folder twice, fresh and with a reused inspection and selection: with "M 33" chosen both write the WCS.
- Masters without a catalogue byte-identical: Seestar `3F752C45EFDB46DF`, LBN 437 `F22D99BF062BC003`, M42 `D2330D5E10FA77B0`. The release application is rebuilt.

## ADR-157: A calibration set filled kind by kind is offered, and a raw stack is flagged (2026-11-06, local)

**Status:** Owner report of 2026-11-06: masters `master_v107` (flat and dark-flat switched on) and `master_v108` (both switched off) came out the same, "as if the flat was not applied even the first time". Local, not published.

**Cause.** The pixels of both masters are byte-identical; only the XISF creation time differs. Both reports show the light set "Phi Tauri" with no calibration set (`calibration_set: null`, `calibrated_frames: 0`): neither run calibrated anything, not even bias and dark. "Calibration set 1" was made with **+ Empty** and filled kind by kind (bias, dark, flat, dark-flat). The application offered a calibration set to light sets without one only after scanning a whole calibration folder, never after one kind was added, and nothing on screen said that the stacked set would stay uncalibrated. The headers fit: same camera (ZWO ASI2600MM Air), 6248 × 4176, gain 100, rotator 1°.

**Decision.**
- After a kind is added to a calibration set, the set is offered, as after a scanned folder, to every light set without one that it can calibrate (not judged incompatible, at least one file).
- Set overview warns for every light set of the stack that has no calibration set although a fitting one with files switched on exists, and says where to choose it.
- Calibration itself is unchanged: with the set assigned, the same Phi Tauri frames calibrate with `(light − bias − (dark − bias)·t_light/t_dark) / normalized flat + pedestal`, and the master differs from the one without flats (noise 17.86 against 18.32 ADU, sky 2001.3 against 1998.3 ADU).
- A light set the owner left without calibration may be offered a set again when files are added to it; the warning shows the state before every stack.

**Verification.** fmt and clippy: PASS. Application tests 27/27 (2 ignored) with a new test: an empty set is offered to nobody; after darks are added on their own it is flagged, then offered and assigned, and the warning clears. The release application is rebuilt. No engine code changed.

## ADR-156: A light set without stars in common with the reference keeps or links its own brightness (2026-11-06, local)

**Status:** Owner report of 2026-11-06: stacking three sets of one camera (SQA60P with ASI2600MM; Ha, OIII and L), every L frame was refused as `insufficient_photometric_matches`, with star registration and with sky registration. The owner chose the recommended fix ("Ano, implementuj"). Local, not published.

**Cause.** The L frames registered (149 inliers with stars, plate-solved through the sky). The brightness step then needs at least 10 stars that both the frame and the run's photometric anchor, an OIII frame, measured unsaturated. The L frames are dense broadband fields (about 53,000 detections): their brightest stars are saturated and kept for position only, and brightness is measured on fainter stars (from rank 250 of each cell, ADR-108), which the OIII anchor's 250 brightest do not hold. L frames shared 0 usable stars with the OIII anchor and 111–120 with each other; Ha frames shared 156–165 with the anchor.

**Decision** (`register::set_photometry`, both registration methods).
- After registration, a frame that registered but lacks photometric matches with the run anchor, in a light set other than the anchor's, is measured against an anchor of its own light set: the set's chronological median (its sharpest without dates). The pairs come from both frames' grid transforms as a prediction, refitted by the star matcher on the two lists, so an approximate registration still pairs stars within 1.5 px.
- **Link.** The set anchor and the run anchor are read once more and measured to 5,000 stars each; their median flux ratio over at least 10 unsaturated pairs is the set's scale onto the run anchor. With a link the frames join every kind of master at the run's brightness (`light_set_anchor_linked`).
- **Without a link** the set keeps its own brightness (`light_set_anchor_unlinked`). A master of that set alone is unaffected, because each group is rebased onto its own anchor (ADR-101); a master that mixes it with other sets is refused with a message to stack one master per set or per filter.
- Frames that already have photometric matches, and frames of the anchor's own light set, are never touched: every existing result stays the same.
- The report lists such sets (`set_photometry`); the console and Results show a Brightness line.

**Results (Olyhen, 139 frames).** Star registration, one master per set: L 10/12 (two refused by the existing whole-field accuracy check against the other night's OIII grid), OIII 69/76 and Ha 48/51 unchanged; L frame scales 0.98–1.02 on their set anchor; link 23 stars, scale 0.0147. One combined master: 127/139 frames. Sky registration, one master per set: L 12/12, link 888 stars, scale 0.0153; WCS written on all three masters. With the owner's 22 darks (ASI2600MM 300 s −20 °C) applied: star registration L 10/12, link 22 stars, scale 0.0146; sky registration L 12/12, link 861 stars, scale 0.0154.

**Verification.**
- fmt and clippy (default and all features): PASS. `cargo test --workspace --release`: 313/313 default, 317/317 all features.
- New tests: a set without shared stars is brought onto its own chronological-median anchor (a frame twice as bright gets scale 0.5), marked unlinked, accepted with weight and footprint; a frame of the anchor's own set is never rescued; a master mixing an unlinked set with others is refused, one per set is not, linked sets may mix.
- Single-set masters byte-identical: Seestar `3F752C45EFDB46DF`, LBN 437 `F22D99BF062BC003`, M42 `D2330D5E10FA77B0`.
- **Open:** the owner's review of the L master in PixInsight. The L master's WCS check gave 249 of 300 stars at RMS 0.92 px, against 0.15 px for OIII and Ha; its brightest stars are saturated, which likely moves their centres.

## ADR-155: The master carries its plate solution as FITS WCS (2026-11-05, local)

**Status:** Owner request of 2026-11-05: "plate-solve and store the solution in the XISF and FITS metadata, for both versions" (sky and star registration); the owner tests the masters in PixInsight. Local, not published. Until now every master said in its history that it has no plate solution.

**Decision.**
- **Which solution.** The master lies on the pixel grid of one frame (or of an external reference), so that frame's plate solution describes the master. Sky registration keeps the solution of its grid frame (`SkyReport::grid_solution`). A star registration plate-solves only its reference frame, when a Gaia catalogue is given: the frame is read and calibrated as inspection read it, and solved blind around the field centre (`astrometry::solve_grid`). An external reference is solved from its own stars.
- **Checked on the master.** The 300 brightest stars of the finished master's green plane are paired with Gaia DR3 through the solution within 2 px; the WCS is written only when at least 20 pair with an RMS distance of at most 1 px (`astrometry::check_master`). Otherwise the master is written without a WCS, and the console, Results and the report say why.
- **Cards** (`sky::fits_wcs`). Gnomonic projection about the solution's tangent point: `CTYPE1/2 = 'RA---TAN-SIP'/'DEC--TAN-SIP'` (plain TAN for a linear solution), `CRVAL1/2` the tangent point, `CRPIX1/2` its FITS pixel, the `CD` matrix, `RADESYS = 'ICRS'`, and the solver's quadratic and cubic terms as SIP `A_p_q`, `B_p_q`. The conversion is exact (a millionth of an arcsecond): the polynomial is re-expanded about the tangent pixel. The inverse terms `AP_p_q`, `BP_p_q` are fitted over the frame and invert within 0.02 px. Pixels follow the master as stored: FITS pixel (i, j) is master pixel (i - 1, j - 1), rows top-down (ROWORDER `'TOP-DOWN'`), shifted by the crop offset. Four HISTORY lines give the solved frame, the solution's accuracy and epoch, and the check on the master; they replace "No plate solution".
- **Both formats.** The XISF master carries the same cards as FITSKeywords. The coverage and high-rejection maps get no WCS.
- **Positions.** Gaia DR3 positions brought to the frame's epoch by proper motion, ICRS.
- **Settings and CLI.** The Gaia folder and field centre now also show under Stars, marked optional, for the master's WCS. In the CLI, `stack --gaia-db FOLDER` (or `--gaia-download`) gives a star registration a WCS; sky registration always has one. A missing catalogue or a failed solve never stops a stack.

**Results.** DS-B (27/32 frames): star registration, 298 of 300 master stars on Gaia with RMS 0.19 px; sky registration 299 of 300, RMS 0.18 px; 2.246″/px, centre RA 23.5154° Dec +30.6607°. DS-A (941/1140 frames): star registration 298 of 300, RMS 0.23 px; sky registration 298 of 300, RMS 0.16 px; centre RA 23.5548° Dec +30.6890°. The pixels of every WCS master are those of the master without one (DS-A CPU: identical to `master_v54.fits`); only the header differs. FITS and XISF masters for the owner's check are in `outputs/wcs_test/` (local).

**Verification.**
- fmt and clippy (default and all features): PASS. `cargo test --workspace --release`: 311/311 default, 315/315 all features.
- New tests: the TAN-SIP cards of solutions of degree 1–3, with a crop offset and an off-centre tangent point, give the solution's sky positions within a millionth of an arcsecond by the FITS rules, and their inverse terms return the pixel within 0.02 px; a solution without a linear part gives no cards; on a rendered star field a fitting solution is written and one 1.5 px off is refused; a crop shifts CRPIX by the crop offset; FITS and XISF headers carry the cards with their comments and the WCS history instead of "No plate solution".
- Masters without a catalogue are unchanged: Seestar `3F752C45EFDB46DF`, LBN 437 `F22D99BF062BC003`, M42 `D2330D5E10FA77B0`.
- **Open:** the owner's check in PixInsight (Image Solver's solution, Annotate Image or the coordinates under the cursor), especially whether PixInsight reads the SIP terms and the top-down row order of FITS and XISF.

## ADR-154: Sky registration adds its star lists instead of measuring again (2026-11-05, local)

**Status:** Owner question of 2026-11-05: why does registration through the catalogue run the quality inspection again, and is it necessary? The owner chose the recommended fix ("Oprav to podle vlastniho doporuceni"). Local, not published.

**Context.** Sky registration solves each frame from a list of its bright stars (ADR-136). Measure Quality keeps these lists only when plate solving is selected at that moment, because they cost about a third more inspection time. A stack with plate solving on a measurement made for star registration, or made before the Gaia settings were valid, ran the whole inspection again. The measurement came out the same, and the owner's Use choices in Frames were discarded.

**Decision.**
- `inspect::add_sky_stars` adds the lists to an existing inspection: every frame inspection measured is read and calibrated again exactly as inspection did, and only its bright-star list is computed, at most 16 frames at a time as in inspection. Measurements, classification and everything else in the report stay as they are; frames inspection did not measure get no list.
- The application's stack reuses a measurement that matches its inputs and adds the lists when plate solving needs them, under the Inspect phase with its own progress. The owner's Use choices and reference stay; the time counts as inspection time. A measurement whose inputs changed is measured again, as before.

**Verification.**
- fmt and clippy (default and all features): PASS. `cargo test --workspace --release`: 307/307 default, 311/311 all features.
- New tests: lists added later are bit for bit those of an inspection that kept them, and the rest of the report is unchanged, on synthetic starry frames and on four real DS-B Seestar frames; mismatched light sets are refused; in the application the session with added lists keeps its measurement, matches its inputs and equals a sky inspection's lists.
- The time of the added pass on DS-A was not measured; it skips the star measurement of a full inspection.

## ADR-153: Publication of the DS-A speed rounds ADR-144–ADR-152 (2026-11-05)

**Status:** Owner-authorized on 2026-11-05 for this batch only ("Ano, publikuj").

**Scope.** One commit to `main` with `[skip ci]`: the code of ADR-144, ADR-146, ADR-148 and ADR-149, the records of the reverted rounds ADR-145, ADR-147, ADR-150 and ADR-152 and of the GPU analysis ADR-151, the [DS-A performance log](M5_PERFORMANCE_DSA_20261105.md), and the owner's manual review PASS of 2026-11-05 recorded in ADR-143 and the status page. Actions configuration is unchanged. Images, outputs, preferences, raw telemetry, run logs and binaries are excluded. No package is built; the version stays 0.12.0.

**Verification before publication (Windows 11, release, local).** fmt and clippy (default and all features): PASS. `cargo test --workspace --release`: 304/304 default, 308/308 all features. Masters byte-identical: Seestar `3F752C45EFDB46DF`, LBN 437 `F22D99BF062BC003`, M42 `D2330D5E10FA77B0`, DS-A CPU `81024B85B3397F91`, DS-A GPU `47ECC0D63197520D`, DS-A sky `7476DFC38C20E029`.

## ADR-152: Larger rejection blocks give nothing (2026-11-05, local, reverted)

**Status:** Round 10 of the owner's DS-A speed work of 2026-11-05 ([DS-A performance log](M5_PERFORMANCE_DSA_20261105.md)). Reverted.

**Proposal.** The ESD reduction rearranges each block of 32 output pixels from 941 frames; 64-pixel blocks halve the number of short memory streams. Results cannot change: each pixel is reduced on its own.

**Result.** Central tiles: −6 % on average, but the machine was speeding up during the series. Full frame, alternating: 29.05 and 26.63 s against 28.96 and 27.43 s for 32-pixel blocks. No gain; the constant stays 32.

## ADR-151: GPU reduction on the Radeon 890M is the limit of the GPU engine (2026-11-05, local, analysis)

**Status:** Round 8 of the owner's DS-A speed work ([DS-A performance log](M5_PERFORMANCE_DSA_20261105.md)). No code change.

**Finding.** The GPU reduction already overlaps the CPU reconstruction, and the upload of each chunk overlaps the computation of the previous one (two buffer slots). On DS-A the Radeon 890M needs about 82 s for the ESD reduction (29.3 s writing 93.7 GB into mapped memory, 52.8 s waiting for results), against about 35 s for the CPU after ADR-146. The CPU waits 21–27 s for the GPU in a GPU run.

**Decision.** No change. Sharing tiles between CPU and GPU would make the master depend on timing, because the two engines' results differ in the last bits. A faster GPU needs a new shader, which would also change today's GPU master; that is a separate decision. On this PC the CPU engine is the faster choice.

## ADR-150: More reduction bands per thread give nothing (2026-11-05, local, reverted)

**Status:** Round 9 ([DS-A performance log](M5_PERFORMANCE_DSA_20261105.md)). Reverted.

**Proposal.** Four reduction bands per worker thread instead of one, so a band of costly pixels does not hold the others up. Results cannot change.

**Result.** Central tiles −3 %, within noise; full frame 35.29 and 35.50 s against 39.84 and 35.13 s for the current banding, with more CPU time. Not adopted.

## ADR-149: A blind plate solve tries its configurations at once (2026-11-05, local)

**Status:** Round 7 of the owner's DS-A speed work ([DS-A performance log](M5_PERFORMANCE_DSA_20261105.md)).

**Context.** Sky registration solves one frame per camera blind, then the others from its solution in parallel. The blind search tries field sizes from 0.06° to 3.2° and both parities in turn, stopping at the first strong pose; on DS-A that one frame took about 2.4 s of 5.2 s of plate solving.

**Decision.** `sky::solve::blind`: without a prior, every field size and parity of a hint is evaluated at once, and the results are then read in the sequential order with the same stopping rules, so the chosen pose, its runner-up and the reported counts of configurations and poses are those of the sequential search. The work of one configuration is now `Search::config`. A seeded search, which tries one configuration, stays serial.

**Result (DS-A, sky registration).** Plate solving 5.5 → 2.0 s, registration 16.8 → 13.4 s from start. The solutions of all 941 frames and the master `7476DFC38C20E029` are identical. Configurations beyond the stopping point are computed and discarded, so the blind search uses more CPU time for less wall time.

**Verification.** Sky unit tests pass; the per-frame sky results of both runs compare equal.

## ADR-148: The triangle fallback of star registration runs in parallel (2026-11-05, local)

**Status:** Round 6 of the owner's DS-A speed work ([DS-A performance log](M5_PERFORMANCE_DSA_20261105.md)).

**Context.** Star registration walks forward and backward from the anchor, each walk serial because every frame's prediction uses the solutions before it. On DS-A 337 of 941 frames miss the prediction and fall back to triangle matching, 34.5 ms each (12.0 s of 14 s of fitting); the registration ran at 11 % of the CPU.

**Decision.** `geometry::Matcher`: the votes of the source triangles are counted in parallel chunks of 256 and merged in chunk order, so every bin keeps the transform of its first vote in triangle order, as the sequential count did; the 24 best candidates are fitted in parallel and the best is chosen in rank order with the same rule. The walks themselves are unchanged.

**Result (DS-A, CPU).** Registration 9.2/7.8 s → 5.0/4.4 s in two alternating pairs; LBN 437 15.1 → 6.1 s. All masters byte-identical.

## ADR-147: Reading a region of interest in one call gives nothing (2026-11-05, local, reverted)

**Status:** Round 5 ([DS-A performance log](M5_PERFORMANCE_DSA_20261105.md)). Reverted.

**Proposal.** Read a region's rows with one read of their whole span into per-thread storage, instead of row by row through a 64 KB buffer.

**Result.** Central tiles 8.64 s against 8.65 s, reconstruction CPU slightly higher. Reading is not the limit, as ADR-117 found for tile shapes.

## ADR-146: ESD tests and sums four columns at once (2026-11-05, local)

**Status:** Rounds 3 and 4 of the owner's DS-A speed work ([DS-A performance log](M5_PERFORMANCE_DSA_20261105.md)).

**Context.** On DS-A the ESD reduction took 35 % of a run. Timed by step: sorting 29 %, choosing the surviving range 21 %, the weighted mean of the survivors 24 %, rearranging 16 %, preparing 10 %. The two middle steps were scalar, one column at a time.

**Decision.** `esd_columns`, AVX2 kernel only:
- **Survivor range** (`vector::survivor_windows`): the running sums and the test steps of four sorted columns run in the four lanes of a vector, each lane repeating the scalar arithmetic in the same order and stopping where the scalar test stops.
- **Weighted mean** (`vector::survivor_sums`): the compensated sums of four columns' survivors run in four lanes, each the arithmetic of `Sum::add` in frame order; samples outside the range leave a lane unchanged.
- Ties at the rejection limit still go through the scalar choice. The scalar kernel is unchanged and remains the reference.

**Result (DS-A).** Central tiles: reduction −11 % (round 3) and −8 % more (round 4); CPU time −18 % together. Full DS-A runs: 33.8 → 25.0 s of reduction. Masters byte-identical; the existing test compares every kernel with the scalar one bit for bit on stacks of 1 to 941 frames.

## ADR-145: More inspection workers give nothing (2026-11-05, local, reverted)

**Status:** Round 2 ([DS-A performance log](M5_PERFORMANCE_DSA_20261105.md)). Reverted.

**Proposal.** Inspection measures at most 16 frames at a time. Let small frames use every thread while their working memory stays within a quarter of the free RAM; large frames keep 16.

**Result.** DS-A inspection 10.5 s with 24 workers against 10.3 s with 16, peak RAM 1.72 against 1.17 GiB. The physical cores and memory are the limit, not the number of workers.

## ADR-144: Spans for distorted frames, a stage timeline and a replay profiler (2026-11-05, local)

**Status:** Round 1 of the owner's DS-A speed work of 2026-11-05, revised as round 1b ([DS-A performance log](M5_PERFORMANCE_DSA_20261105.md)). Owner request: optimize every stage of a run on the 1,140-frame Seestar set, ten rounds of proposal, test, documentation and application.

**Decision.**
- **Spans for distorted frames.** ADR-113 reconstructs only the columns of each region-of-interest row that a rotated tile can reach, but a frame with a cubic model kept its whole region. Now its spans come from the solved source positions of the tile's edge samples: the first and last rows and the first and last sample of every row, solved by the resampler's own function. The tile's source region is bounded by its edge, so in every row band its samples lie between the outermost edge samples of that band; one pixel more on every side covers the edge between samples. If any edge sample cannot be solved, the whole region is reconstructed as before. Resampling is unchanged.
- The first version (round 1) solved every sample of the tile first. It was byte-identical, but on LBN 437 and in sky mode, where most frames carry a cubic, reconstruction became 12–24 % slower; round 1b replaced it.
- **Stage timeline.** With `FULGURSTACK_TIMELINE=1`, `stack` prints to standard error the seconds since its start at each stage: inspection, registration (and the blind and seeded plate solves), integration sources, integration, master and maps written, report. Without the variable nothing is printed or measured.
- **Replay profiler.** `examples/profile_integration` now replays a stored plan's cubic models and uses Auto rejection and Auto tiles, as `stack` does; `PROFILE_TILES=WxH` chooses a tile size.

**Result.** Byte-identical everywhere. Reconstruction CPU time: sky registration on DS-A −19.5 % (scanned area −24 %), LBN 437 −9 %, star registration on DS-A −2 % (16 frames with a cubic).

**Verification (rounds 1b–7 together).** fmt and clippy (default and all features): PASS. `cargo test --workspace --release`: 304/304 default, 308/308 all features. New tests: the spans of a curved band of samples cover every footprint and window pixel; a rotated frame with a cubic model reconstructs bit for bit as with its whole region, on reused and poisoned buffers. Masters byte-identical: Seestar `3F752C45EFDB46DF`, LBN 437 `F22D99BF062BC003`, M42 `D2330D5E10FA77B0`, DS-A CPU `81024B85B3397F91` (the stored `master_v54.fits`), DS-A GPU `47ECC0D63197520D`, DS-A sky `7476DFC38C20E029`.

**Overall (alternating runs, same machine state).** DS-A CPU 90.6 → 76.5 s (−16 %), DS-A sky registration 108.5 → 83.7 s (−23 %), LBN 437 110.8 → 91.3 s (−18 %), DS-A GPU 113.5 → 104.6 s (−8 %, measured before round 1b).

## ADR-143: Consolidated main publication of ADR-116–ADR-142 (2026-11-05)

**Status:** Owner-authorized on 2026-11-05 for this batch only: "update the project documentation and make a commented commit and push".

**Scope.** One commit to `main` with `[skip ci]`, holding ADR-116–ADR-142 with their code, tests and documentation:
- registration for several cameras and optics: partial-frame report, tile choices, Auto reference by plate scale, cameras without an external reference and mirrored cameras, adjustable accuracy, wider cameras on a narrower grid, stranded-set second passes and the robustness fixes of ADR-124–ADR-130;
- sky registration: the standalone experiments `tools/skyreg` and `tools/synthref` (ADR-127, ADR-131), the owner decisions (ADR-132), the core in `fulgur_pipeline::sky`, the new crate `fulgur-catalog` with the built-in deep-sky table, the Gaia folder check, the engine behind `--registration-engine sky` and the application settings (ADR-133–ADR-138);
- the light-set split by camera, gain and filter, and light folders without sub-folders (ADR-139, ADR-140);
- the one-column Settings page with info markers (ADR-141) and XISF output of masters and maps (ADR-142).

Actions configuration is unchanged. Images, outputs, preferences, raw telemetry, Gaia data and binaries are excluded; `tools/*/target/` is ignored. The built-in deep-sky table (`crates/fulgur-catalog/data/deep_sky.csv`) is source data derived from OpenNGC (CC BY-SA 4.0) and SIMBAD, credited in its README and on the About page. No Windows package is built and the version stays 0.12.0.

**Verification before publication (Windows 11, release, local).** `cargo fmt --check` and clippy, default and all features: PASS. `cargo test --workspace --release`: 303/303 default and 307/307 with all features. Single-camera FITS masters byte-identical to their stored references: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.

**Limits.** Sky registration is experimental and off by default; its masters on the five-camera M33 set await the owner's review in PixInsight. The application screens of ADR-138, ADR-139 and ADR-141 and the XISF masters of ADR-142 await manual review. Multi-camera results rest on the M33, LBN 437 and M42 sets named in their ADRs. Only one Windows 11 computer was tried; macOS was not. [Distribution](WINDOWS_DISTRIBUTION.md) still describes package 0.12.

**Owner review 2026-11-05:** the owner checked every item left for manual review in this batch and accepted it: PASS for the sky-registration masters of the M33 set in PixInsight, the application screens of ADR-138, ADR-139 and ADR-141, and the XISF masters of ADR-142 in PixInsight. Sky registration stays experimental and off by default until the owner decides otherwise.

## ADR-142: Masters and diagnostic maps can be written as XISF (2026-11-05, local)

**Status:** Owner request of 2026-11-05: "add an Output format parameter to Settings and allow a choice between FITS and XISF (for the master and the diagnostic maps)". Local, not published.

**Decision.**
- **One rule: the extension.** `output::OutputFormat::of(path)`: .fit, .fits or .fts write FITS, .xisf writes XISF; anything else is refused before processing, and nothing is written. Diagnostic maps take the master's format (`_coverage.xisf`, `_high_rejection.xisf`); the JSON report stays JSON and records `output_format`.
- **The XISF unit** (`xisf_write`): one monolithic XISF 1.0 file per image, 32-bit float, planar, little-endian, `bounds="0:1"`, `colorSpace` RGB or Gray (one plane for monochrome masters, as in FITS), data block aligned to 4096 bytes, with `XISF:CreationTime` and `XISF:CreatorApplication`.
- **Same samples, same provenance.** Both formats are written from the same cards and the same sample function: the samples are the FITS samples byte-swapped, with the same 0..1 limiting and the same export counts. Every FITS card except the storage cards (SIMPLE, BITPIX, NAXIS…, EXTEND, ROWORDER, END) becomes a FITSKeyword; strings keep their quotes, HISTORY lines keep their text. Valid reference-anchor GPS is kept as in FITS; nothing else changes.
- **FITS output is unchanged**, byte for byte: the FITS path only moved into a shared writer.
- **The application:** Output has an "Output format" list (FITS, XISF). The choice is the extension of the master file name, so it is saved with the settings and typing .xisf chooses it as well. The diagnostic-maps option names the format.
- **The CLI:** `stack --output master.xisf` writes the XISF master and maps; explicit map destinations may be FITS or XISF.
- Owner amendment to the AGENTS.md output rule: the final master may be 32-bit float FITS or XISF.

**Verification.**
- fmt and clippy (default and all features): PASS.
- `cargo test --workspace --release`: 303/303 default, 307/307 with all features. New tests:
  - core: an RGB and a monochrome master written as FITS and XISF hold byte-swapped identical samples, the same export counts, the provenance keywords (NCOMBINE, OBJECT, CROPX, NDISTCOR, SITELAT, HISTORY) and no storage keywords, and read back alike; XISF maps; an unknown extension writes nothing;
  - CLI: a three-frame stack to .fits and to .xisf gives identical samples in the master and both maps, and `output_format` in the report;
  - application: .xisf names are valid file names and give XISF map names.
- Real data: the DS-A Seestar set (27/32 frames) stacked to FITS in 4.8 s and to XISF in 5.0 s. The XISF master (2160×3840×3, 35 keywords) and both XISF maps hold exactly the FITS samples.
- FITS masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.
- Owner review 2026-11-05: the XISF master and maps opened in PixInsight, PASS.

## ADR-141: Settings in one column, explanations behind an info marker (2026-11-04, local)

**Status:** Owner request of 2026-11-04: Settings were "terribly complicated and overloaded, with a lot of unused space and explanations". The owner proposed hiding explanations in tooltips, one Registration panel and moving partial frames to Stacking; the refined layout was accepted ("Ano, předělej"). UI only; local, not published.

**Decision.**
- **Layout.** One column of panels, at most 900 points wide, each row a label on the left and its control on the right; below 520 points the label goes above. The paired cards, whose heights were levelled to the taller one, are gone.
- **Explanations.** A drawn circled "i" beside a label carries its explanation: hovering shows it as a tooltip, and a click keeps it open until a click outside or Escape. It works in disabled panels too, because the owner reported that clicking showed nothing (egui hides tooltips after a click, and a disabled widget reports no clicks). Each option of a list carries its own tooltip. The page keeps only warnings and live lines: the next stack's rejection method, the masters it makes, the Gaia check and the field-centre result.
- **Registration**, one panel:
  - reference frame as a list, with the frame or file picker beside it when one is needed;
  - method Stars | Plate solving; plate solving opens a sub-panel with the Gaia DR3 folder (Browse, Test, result), the download choice and the field centre (target name with Look up, or RA and Dec). The cache location is named in the tooltip only;
  - geometric distortion; with plate solving it is shown on and locked, because the plate-solution map always corrects distortion (ADR-137);
  - registration accuracy folded under a one-line summary, e.g. "Maximum accuracy · 1 px RMS · 80 % of stars · at least 50 stars". With plate solving it notes that the limits apply to frames falling back to Stars.
- **Stacking:** rejection method as a list with the next-stack line, normalization as one line, and "Keep accurate parts of refused frames", disabled with the reason when distortion correction is off or plate solving is chosen.
- **Output:** folder, file name, masters with their preview, diagnostic maps.
- **Advanced:** folded by default, with a summary line (engine, threads, memory, tiles).
- Settings, their saved form and every value sent to a run are unchanged.

**Verification.**
- UI only, under the owner's UI policy: fmt and clippy of the application (default and all features) PASS; the application's tests 25/25 PASS, among them a simulated click that opens and closes the info marker's explanation in an enabled and a disabled panel; the release application with GPU is rebuilt.
- No scientific regression stacks: the engine is unchanged.
- Owner manual review 2026-11-05: PASS. The info marker opens its explanation on a click.

## ADR-140: A light folder gives only the files directly in it (2026-11-04, local)

**Status:** Owner decision of 2026-11-04, after ADR-139 showed that the M33 folder's sub-folder of 16 templates had joined the light set: "change the behaviour when loading folders, include only files directly in the folder, not from sub-folders". Local, not published.

**Decision.**
- **Light frames.** `survey::collect_folder_inputs` reads only the files directly in a folder; a file given by name is always read. It is used everywhere light frames come from:
  - inspection (`inspect`, `set_files`, `derived_metadata`), so the CLI `stack`, `register`, `inspect` and `survey` commands follow it;
  - the application's header scan of a light set, and drops onto the light sets.
  - A sub-folder (another night, camera, registered or template frames) is added as a light set of its own.
- **The application.** A light folder with no frames directly in it is not added. The message names it and says to add its sub-folders as their own light sets.
- **Calibration stays recursive.** `collect_inputs` still includes sub-folders for calibration frames and calibration folders, which often keep bias, darks and flats in sub-folders that the sorting by name and folder relies on.
- **Masters.** The DS-A, LBN 437 and M42 reference folders hold their frames directly, so their masters do not change.

**Verification.**
- fmt and clippy (default and all features): PASS.
- `cargo test --workspace --release`: 297/297 default, 301/301 with all features. The `survey` CLI test and an application test now expect the new rule, and a unit test covers both folder readers.
- Masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.
- The release application is rebuilt.

## ADR-139: A light set that mixes cameras, gains or filters can be split in one click (2026-11-04, local)

**Status:** Owner report of 2026-11-04: on the "M33 výběr z hroznů" test folder both registration methods lost most frames at Measure Quality, as `incompatible_target_filter_or_sensor`. The owner chose automatic splitting ("Yes, implement it"). Local, not published.

**Context.**
- Inspection keeps, in each light set, only the largest group of mutually compatible frames: same sensor size, colour layout, BZERO, gain and filter. The rule stays, because quality limits, exposure entries and calibration all belong to one light set.
- The M33 folder, added as one set, holds five such groups: 101, 97, 80, 60 and 50 files, the 16 templates in its sub-folder included. Only the first group stayed.

**Decision.**
- `inspect::compatibility_groups(files, layout)` groups files by exactly the inspection rule, reading headers only, largest group first. Unreadable files stay with the largest group, where inspection reports them. `FrameGroup::label` names a group, e.g. "4656×3520 mono · gain 100 · No filter".
- **Header scan.** The application's scan of a light set also computes its groups, and again after a sensor-layout change, because a declared layout can join colour and mono frames.
- **Frames.**
  - A set with several groups shows: "This set holds frames of N cameras, gains or filters. Measure Quality keeps only the largest group (k of n frames) …", with the groups and their counts.
  - A button, **Split into N light sets**, replaces the set with one set per group, named "<set> · <group>". Each new set lists its own files; the originals stay where they are.
  - The new sets keep the set's layout choice, entered exposure and Include. The calibration stays with the largest group, the only one it can have been made for. Derived values are confirmed again, and measurements are invalidated.
  - The older "different sizes" warning gives way to this message.
- **Rejection reason.** In the Frames table the code reads "another camera, gain or filter than most of its light set (split the set in Frames)"; reports keep the code.
- Inspection, registration and stacking are unchanged, so masters of unchanged inputs are byte-identical.

**Verification.**
- On the M33 folder the grouping finds the five groups in 1.2 s.
- A synthetic unit test checks groups, labels, the unreadable file and a declared layout.
- fmt and clippy (default and all features): PASS.
- `cargo test --workspace --release`: 296/296 default, 300/300 with all features.
- The release application is rebuilt. Owner manual review 2026-11-05: PASS.

## ADR-138: Sky registration in the application (2026-11-04, local)

**Status:** Phase 4 of docs/SKY_REGISTRATION_PLAN.md, on the owner's word of 2026-11-04 ("Yes, start phase 4"). UI work under the manual-review policy; the release application is built for the owner's review. Local, not published.

**Decision.**
- **Settings › Registration method** (new card pair below Registration geometry and accuracy):
  - **Stars** (recommended, default) or **Sky · Gaia plate solving** (experimental).
  - **Gaia DR3 catalogue folder.** A text field with Browse… and **Test**. Test runs `fulgur_catalog::xpsd::check_installation` (ADR-135) on its own thread. It reports Complete (files, magnitude range, stars, seconds) or Incomplete, Damaged or "No Gaia catalogue here", with up to six problems and notes. A changed path asks for a new test.
  - **Download Gaia stars the folder does not hold** (off by default). The cache sits next to the settings, in `%LOCALAPPDATA%\FulgurStack\gaia_cache`.
- **Field centre** (in the same pair):
  - **From the target name.** The name entered, or else the OBJECT card most frames carry, shown as a hint. **Look up** resolves the name on its own thread and shows RA, Dec and the source: the built-in table, or online when allowed.
  - **Look names up online** (NED, SIMBAD), off by default.
  - **Coordinates:** RA in degrees or hours (01:33:50.9) and Dec in degrees (+30:39:36), checked as typed.
- **Persistence.** All choices are saved with the other settings. The Gaia folder is a setting and never a fixed path (ADR-135).
- **Start checks.** A Sky run without a Gaia folder or download, or with invalid coordinates, does not start; the message says what is missing.
- **Job.**
  - With Sky, inspection also keeps the bright-star lists (ADR-136).
  - A cached star-run inspection without them is measured again.
  - Registration calls `register_sky` with `fulgur_catalog::sky::sky_options`, which the CLI now shares.
  - Results show a "Registration" row: frames solved and the time, how many registered through the sky and how many by stars, the field centre and its source, and the catalogue source.
- **Frames table.** Three columns, filled after a Sky stack and sortable and filterable like the others:
  - **Scale ″/px:** the plate scale;
  - **Solve px:** the held-out RMS of the plate solution;
  - **Map px:** the worst 3 × 3 cell offset of the stacking map.
- **About.**
  - A "Data and credits" section: Gaia (ESA/DPAC acknowledgement), OpenNGC (CC BY-SA 4.0), SIMBAD (CDS) and NED.
  - A feature line and a release note on Sky registration.
- **CLI.** `--field-ra` and `--field-dec` also accept sexagesimal input. `fulgur_catalog::sky::{parse_ra, parse_dec}` has a unit test.

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 295/295 default, 299/299 with all features (1 new). The pipeline is unchanged since ADR-137, whose star-engine masters were byte-identical. The release application (`target/release/fulgurstack-gui.exe`, GPU feature) is built. Owner manual review 2026-11-05: PASS.

## ADR-137: Sky registration engine, behind a CLI switch (2026-11-04, local)

**Status:** Phase 3 of docs/SKY_REGISTRATION_PLAN.md, steps B–F, on the owner's word of 2026-11-04 ("Yes, continue"). Experimental and off by default; the application switch follows in phase 4. Local, not published.

**Decision.**
- **Entry points.**
  - `register::register_sky` takes an inspection with sky star lists (ADR-136), the field centre and a catalogue provider (`SkyOptions`), and returns an ordinary `RegistrationReport`, which integration accepts unchanged.
  - `register::register_sets_sky` inspects light sets with the lists switched on, then registers.
  - The pipeline never touches the network.
- **Catalogue.** A cone of 6.5° to G 13.5 for wide fields and offset framing, and one of 1.7° to G 17.5 for the search. Local Gaia files carry no source ids, so overlapping stars are merged by position and brightness.
- **Solving.** `sky::batch::solve_all` runs over the quality-accepted frames, grouped by sensor size and colour layout. Epochs come from DATE-OBS; frames without it take the median epoch.
- **Reference and photometric anchor.**
  - **Selected frame:** that frame is both.
  - **External file:** read as a plane, detected and plate-solved; the anchor is then the median frame of the camera with the most solved frames.
  - **Auto:** that median frame. With several nights of one camera, the star engine's coverage choice runs on the sky run's own footprints (ADR-085 rule kept).
- **Geometry.**
  - The plate solutions pair the frame's point sources with the reference's own. At least half of the pairs must lie within 1 px of the coarser frame, or the frame is refused.
  - The stacking map is the exact composition of the two plate solutions, carried as a cubic `distortion::Warp`, when the cubic stays within 0.05 px of it (risk R1) and the stars carried through it pass the check: mean displacement ≤ 0.15 px and each 3 × 3 cell ≤ 0.3 px of the coarser frame.
  - Otherwise a cubic is fitted on the star pairs. It is judged by five-fold cross-validation, on pairs left out of each fit. Judging it on its own fitting pairs showed a mean displacement of 0.000 by construction, which proves nothing.
  - Mirrored frames get reversed rows, as in the camera code (ADR-120).
- **Why the composition comes first.** On the held-out check, a cubic on the pairs often lies closer to the reference's centroids. Yet with the pairs chosen wherever closer, the masters were less sharp and less round on DS-A, M33 and M42 than with the composition first.
- **Photometry and weights.**
  - Brightness is measured per anchor pixel: a = median(anchor flux / source flux) / area, with area = (source arcsec per px / anchor arcsec per px)².
  - Pairs come first from the inspection catalogues matched through the sky. When two cameras share fewer than 10 of them, they come from the bright-star lists, which now carry flux and raw peak (`SkyStar`); stars above 95 % of saturation are left out.
  - Weight = 1 / (noise² · HFR²), with HFR in anchor pixels.
- **Fallback.** Quality-accepted frames the sky could not register go to the star engine in the same run, on the same grid (the anchor frame or the external file). Their brightness is brought onto the sky run's anchor through the anchor's own row in that run.
- **Report.**
  - `RegistrationReport::sky` holds the catalogue, solve times, solved and registered counts, the fallback note and the check rule.
  - `RegisteredFrame::sky` holds the solution, both checks, the carrier error and mirroring.
  - Both fields are left out of star-engine reports.
- **CLI.** `stack --registration-engine sky --gaia-db FOLDER` (or `--gaia-download [--gaia-cache FOLDER]`), with the field from `--field-ra/--field-dec`, `--object` or the most common OBJECT card. `--online-lookup` allows NED/SIMBAD. Geometric correction is always on in sky runs.

**Results (CLI, 24 threads, PixInsight Gaia DR3 files; star engine on the same inputs).**

| Set | Frames stars → sky | Registration s | HFR | FWHM | Eccentricity | Noise ADU |
|---|---|---|---|---|---|---|
| DS-A Seestar (M 33) | 27 → 27 | 0.7–0.8 → 3.5–3.7 | 1.717 → 1.716 | 3.632 → 3.631 | 0.451 → 0.449 | 8.730 → 8.692 |
| LBN 437 (ASI6200, 9576 × 6388) | 166 → 166 | 16.3–19.2 → 3.2–3.5 | 1.441 → 1.433 | 3.321 → 3.313 | 0.410 → 0.408 | 5.661 → 5.681 |
| M42 | 245 → 255 | 3.9 → 5.3–6.9 | 1.851 → 1.864 | 3.962 → 3.988 | 0.227 → 0.253 | 0.720 → 0.690 |
| M42, the same 244 frames | 244 → 244 | 3.4 → 4.1 | 1.798 → 1.760 | 3.869 → 3.828 | 0.2214 → 0.2215 | 0.722 → 0.701 |
| M33 test set, five cameras, fixed reference | 213 → 330 | 42.6 → 7.7–9.8 | 1.608 → 1.598 | 3.578 → 3.554 | 0.267 → 0.289 | 1.244 → 1.195 |
| M33, the same 213 frames | 213 → 213 | 40.7 → 8.4 | 1.604 → 1.589 | 3.581 → 3.544 | 0.266 → 0.282 | 1.239 → 1.250 |

- **LBN 437.** The cubic cannot carry the composition of the two plate polynomials on this large sensor (0.46 px), so every frame stacks with the cross-validated cubic on the star pairs (worst cell 0.06–0.08 px). Brightness scales agree with the star engine within ±0.14 %, weights within 0.4 %. The 0.3 % noise rise goes with the sharper stars.
- **M42.** The ten extra frames are ones the star engine refused for `native_insufficient_field_evidence`. They pass quality inspection, but their stars are elongated (eccentricity 0.35–0.58), and they raise the master's eccentricity. On the same frames the sky master is sharper and less noisy at equal eccentricity.
- **M33.**
  - On the same frames the sky master is sharper, but its eccentricity is higher.
  - About 60 % of that gap is brightness, not geometry. The 2016 G3 optics share the reference's sensor size at another plate scale, and the star engine does not correct brightness for pixel area within one sensor size. The 2016 frames have the most elongated stars (0.46), and the sky run gives them their full brightness.
  - Without the area factor within one sensor size the sky master reaches eccentricity 0.272 and noise 1.245. The factor stays, because it is the right normalization after resampling.
  - The owner's PixInsight review of the M33 masters is open.
- **Time gate.** M33 sky registration takes 7.7–9.8 s after inspection (plate solving 5.5–8.5 s), under the 10 s gate. Inspection of the five camera sets costs about 4 s more with the star lists.
- **Verification.**
  - fmt, clippy (default and all features): PASS.
  - `cargo test --workspace --release`: 294/294 default, 298/298 with all features (2 new: two cameras, one mirrored, through the sky; cross-validation that exposes a local error a cubic smooths over).
  - Single-camera star-engine masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.
  - The M33 masters of both engines are in `outputs/sky_registration/` (not in Git) for the owner's review.

## ADR-136: Inspection can keep a bright-star list for sky registration (2026-11-04, local)

**Status:** Phase 3 of docs/SKY_REGISTRATION_PLAN.md, step A, started on the owner's word of 2026-11-04 ("start fázi 3"). Local, not published.

**Context.** Spike S2 (ADR-133) showed that inspection's registration catalogue cannot drive the plate solver: it leaves saturated stars out and picks stars per cell, and the blind search solved only 45 of the 372 M33 frames with it.

**Decision.**
- `fulgur_pipeline::sky::detect::bright_stars` keeps the 1 500 brightest detections of a frame, saturated ones included, as (x, y, FWHM, eccentricity), brightest first.
  - Local background from a 64 px mesh (sparse median of 256 samples per tile).
  - Maxima above 5 sigma with at least four neighbours above 3 sigma, so hot pixels drop out.
  - Intensity-weighted centroids.
  - A declared colour mosaic, or one recognised by the alternation of neighbouring pixel levels, is measured on 2 × 2 superpixels.
- `InspectConfig::sky_stars` (default off) fills `QualityFrame::sky_stars` after the field analysis. The list is not serialized, so reports are unchanged.
- The detection is a port of tools/skyreg's, made faster in two ways:
  - a per-row lower bound of the background rejects nearly every pixel before interpolation;
  - the tile medians and the mosaic test use sparser samples.
- tools/skyreg `--fs-sky-stars` runs FulgurStack's inspection with the switch on and solves with its lists.

**Results (M33 test set, 388 files, 372 accepted).**
- With inspection's lists the set solves 372/372 (6.2 s) and every one of the 16 templates registers all 372 frames: 5 952/5 952 PASS.
- Negative controls: every reference shifted by 0.5 px fails all 372 frames; a 0.2 px shift lies near the check's detection limit, as in phase 1.
- Cost: inspection takes 12.8–13.4 s instead of 10.8–10.9 s with the switch on (alternating runs), and nothing with it off. Single-thread detection takes 32–45 ms per frame.

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 292/292 default, 296/296 with all features (1 new). Single-camera masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.

## ADR-135: The local Gaia folder is a setting, and the system checks the installation (2026-11-04, local)

**Status:** Owner instruction on 2026-11-04: the path to the local Gaia database must be a configuration parameter, not hard-coded, and after setting it the system must be able to test that the installation is correct and complete. Local, not published.

**Decision.**
- **No path in code.** The folder of the PixInsight Gaia files is a parameter everywhere: `fulgur_catalog::GaiaSources::pixinsight`, and the CLI option `--gaia-db`. No source file names a folder; the application setting with a test button follows in phase 4 of the plan.
- **`fulgur_catalog::xpsd::check_installation(folder, deep)`** gives one of four verdicts:
  - **complete:** readable files of one database and version, magnitude ranges that join without gaps or overlaps from the brightest stars on, every file consistent with its header, and stars found at five probe positions over the whole sky;
  - **incomplete:** readable, but a magnitude gap or a patch of sky without stars;
  - **damaged:** an unreadable file, an index that disagrees with its header, a data block beyond the end of the file (truncated download), or a mixed database or version;
  - **missing:** no Gaia star catalogue files.
- **Per-file checks:**
  - node, leaf and source counts against the header statistics;
  - child indices inside the tree;
  - data blocks inside the file.
  - With `deep`, about 200 blocks per file are decompressed and their records decoded and range-checked.
- Other PixInsight databases, such as Gaia DR3SP (spectra), get a plain note telling the user to choose the Gaia DR3 folder.
- **CLI:** `fulgurstack gaia-check --gaia-db FOLDER [--deep] [--report FILE.json]` prints the files, probes, notes and problems. It exits with PASS only for a complete catalogue.

**Evidence.**
- **The owner's installation** (`C:\PixInsightData\Gaia DR3`) is complete: GaiaDR3 1.0.0, 16 files, G −2.00…25.59, 1 806 254 432 sources (Gaia DR3's published count of sources with a G magnitude). The quick check takes 1.0 s; the deep one decodes 201 blocks per file.
- **Broken installations, built from hard links and a partial copy; the originals are untouched:**
  - one file missing: "magnitudes G 17.61..18.24 are missing", verdict incomplete;
  - a file cut to 100 MB: "data block runs past the end of the file (file truncated?)", verdict damaged;
  - an empty or absent folder: verdict missing;
  - the DR3SP folder: the note to choose the Gaia DR3 folder.
- **Unit test:** synthetic files that are complete-but-partial-sky, have a magnitude gap, are truncated, or are absent.

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 291/291 default, 295/295 with all features. The stacking code is unchanged.

## ADR-134: Sky registration phase 2: catalogue and position sources (2026-11-04, local)

**Status:** Owner-approved on 2026-11-04 ("Ano zahaj fazi 2"), per `docs/SKY_REGISTRATION_PLAN.md` and ADR-132. No stacking behaviour changes: nothing calls the new crate yet. Local, not published.

**Decision.**
- New workspace crate `fulgur-catalog`, kept apart so that `fulgur-pipeline` stays free of network code:
  - **`xpsd`:** reads the PixInsight Gaia DR3 files (XPSD 1.0) read-only. The layout comes from the public PCL headers `StarDatabaseFile.h`, `GaiaDatabaseFile.h` and `Compression.h`: XML header, six quadtrees of 48-byte nodes, LZ4 blocks with byte shuffling, 32-byte records.
  - **`download`:** Gaia DR3 cones from the ESA archive with GAVO `gaia.dr3lite` as fallback. Each query has a magnitude limit and at most 1.75° radius; wider fields are tiled and de-duplicated. Answers are cached as CSV.
  - **`dso`:** a built-in deep-sky table of 15 739 entries (`data/deep_sky.csv`).
    - 14 027 entries come from OpenNGC (commit `be3cd6c`, CC BY-SA 4.0), with Messier and common names plus Caldwell, LDN, Melotte, Collinder, vdB, Ced and Abell aliases.
    - At the owner's request, the complete LBN, Sharpless (Sh2) and Barnard catalogues come from SIMBAD (CfA mirror, attribution "SIMBAD database, CDS, Strasbourg"): 1 029 LBN entries added and 95 already OpenNGC aliases (1 124 found in all), 313 Sh2 and 370 Barnard.
    - Names are normalised, so "Messier 33", "M033" and "Triangulum Galaxy" all find NGC 598, and "Sharpless 155" or "Sh 2-155" find Sh2-155.
  - **`lookup`:** NED, then SIMBAD at CfA, then CDS Sesame. Every failure is recorded.
  - **`gaia_field`:** PixInsight files when they reach the requested magnitude, else the cache, else a download (only when allowed). The source used is reported.
  - **`position`:** the owner's precedence (ADR-132): manual centre, header RA/Dec, deep-sky table, online lookup. The source and the reasons earlier sources gave nothing are reported.
- New dependency: `ureq` 2 with rustls. It builds bundled C code with the platform compiler, with no external C libraries.

**Gates.**
- **Read speed:** a 2° cone to G 17.5 from the PixInsight files takes 0.04 s at M33 (18 305 stars) and 0.39 s in the Milky Way at NGC 7000 (187 747 stars).
- **Against the ESA archive:**

  | Field | Matched | Worst position | Worst G |
  |---|---|---|---|
  | M33, 0.8°, G < 17 | 2374/2374 | 0.001 mas | 0.0005 |
  | NGC 7000, 0.4°, G < 15.5 | 1915/1915 | 0.001 mas | 0.0005 |
  | IC 1396, 0.5°, G < 16 | 3437/3437 | 0.001 mas | 0.0005 |

  The PixInsight cone holds one extra star in two fields, at the edge of the cone or the magnitude limit. XPSD keeps no Gaia source identifiers, so stars are matched by position.
- **No network:** with the PixInsight files covering the request, a cone at M42 (12 884 stars) used no network; a unit test checks the same with a synthetic XPSD file.
- **Deep-sky table against NED:**
  - M33, NGC 7000, IC 1396, M42, M27 and NGC 884 within 0.005′.
  - NGC 869, which NED did not resolve, is 1.0′ from SIMBAD: the two use different cluster centres, irrelevant for a field hint.
- **Online lookup:** LBN 437 (before the table held LBN) came from SIMBAD after NED had no position; the table now gives the same position (338.050, +40.583). An unknown name reports all three failures; CDS was unreachable, as before.
- **Unit tests (7):** LBN, Sh2 and Barnard names (with "Barnard 33" and "Horsehead Nebula" finding B 33); a synthetic XPSD file read with no download; deeper requests fall to the archive; ESA failure, GAVO answer, then cache, with download refused when not allowed; tiling with de-duplication; deep-sky names and aliases; parsing of NED, SIMBAD and Sesame and their order; the position precedence.

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 290/290 default, 294/294 with all features (7 new). Single-camera masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0; the stacking code is unchanged.

## ADR-133: Sky registration phase 1: core library in fulgur-pipeline (2026-11-04, local)

**Status:** Owner-approved on 2026-11-04 ("Yes, start phase 1"), per `docs/SKY_REGISTRATION_PLAN.md`. No behaviour change: nothing in the stacking pipeline calls the new module. Local, not published.

**Decision.**
- New public module `fulgur_pipeline::sky`, ported from `tools/skyreg`:
  - `catalogue`: Gaia star with proper motion, gnomonic projection.
  - `solve`: blind and prior-seeded search with early exit, refinement with degree chosen by held-out stars.
  - `batch`: one blind search per camera group, the rest seeded.
  - `check`: end-to-end check by systematic displacement.
  - `warp`: the composed map as the existing cubic `distortion::Warp`, with measured fit error and row reversal for the other parity.
  - `point_sources`.
- The check orders pairs by target star (`BTreeMap`), so sums run in the same order every time.
- `tools/skyreg` now calls the library; its own solver copy is gone.

**Synthetic gates (new unit tests, all pass).**
- Recovery at 1.5″/px, 37°, direct; 1.0″/px, −120°, mirrored, 3 px distortion; 0.5″/px; 5″/px, 200°, mirrored, 4 px distortion. Each with 0.1 px noise, 10 % missing and 20 % false stars. Scale within 1 %, parity right, RMS < 0.05 px over the true star positions.
- Two camera groups of four frames: 2 blind searches, 6 seeded.
- The check passes a true registration and fails one displaced by 0.5 px.
- The carrier reproduces the composition within 0.02 px, also with reversed rows.
- Point sources leave out wide and elongated detections.

**Real data (M33 mixed set through the library, 24 threads).**
- 372/372 frames solved in 7.0 s; 6324/6324 registrations onto the 16 templates and the synthetic reference PASS.
- Negative control at 0.5 px: 6324 FAIL.

**Spike S1: cubic carrier.** Error of the cubic `Warp` against the exact composition, in reference pixels:
- Median per target 0.000–0.018 px.
- Worst ≤ 0.05 px onto 10 of 17 targets; 0.050–0.063 onto Ha_004, m33_023 and Q205.
- 0.08–0.13 onto the G3 targets. The worst cases are G3 frames of 2016 (1.336″/px) carried onto G3 frames of 2017 (1.526″/px) and the reverse: two different distortion fields composed.
- 277 of 372 sources need reversed rows onto the G3 targets, 95 onto the others; this works through the carrier's row reversal.
- Consequence for phase 3: a carrier of higher degree, or a sampled correction, where the cubic exceeds 0.05 px (plan risk R1).

**Spike S2: FulgurStack's inspection stars.** Inspection keeps 250 stars per frame.
- Their positions agree with skyreg's within 0.02 px.
- Solving with only these stars solves 45 of 372 frames (no ASI2600 frame). The catalogue leaves saturated stars out and picks stars per cell, while the blind search builds triangles from the brightest catalogue stars, which are saturated on 300 s frames.
- A trial with fainter catalogue windows reached 86 of 372 and was reverted.
- Consequence for phase 3: the sky engine needs its own list of the brightest detections, saturated ones included, with proper centroids. Inspection reads the pixels anyway; skyreg's detector costs about 0.35 thread-seconds per frame (about 5 s for 372 frames on 24 threads).

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 283/283 default, 287/287 with all features (5 new). Single-camera masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.

## ADR-132: Owner decisions for sky-based registration in FulgurStack (2026-11-04, local)

**Status:** Owner decisions on the integration plan `docs/SKY_REGISTRATION_PLAN.md` (sections 4 and 5). Implementation not started; FulgurStack unchanged.

**Decisions.**
- **Catalogue.** Both sources: the PixInsight Gaia DR3 files when present (offline), otherwise a one-time cached download per target (ESA, then GAVO).
- **Target position.** Precedence, first found wins:
  1. A field centre entered by hand in the registration settings.
  2. Header RA/Dec.
  3. A built-in database of the main deep-sky catalogues (Messier, NGC, IC from OpenNGC, CC BY-SA 4.0).
  4. Internet lookup: NED, then SIMBAD at CfA, then CDS Sesame. On 2026-11-04 NED and SIMBAD-CfA answered from the reference PC and CDS Strasbourg did not.
- **Rollout.** The sky method is optional, chosen by a switch in the registration settings. The star registration stays the default and the fallback.
- **Ideal TAN grid.** Postponed.

**Consequences.**
- Phase 2 of the plan covers the XPSD reader, the download cache, the deep-sky table and the lookup chain.
- The About page gains the Gaia (ESA/Gaia/DPAC) and OpenNGC attributions.

## ADR-131: Experiment: universal sky-based registration in a standalone program (2026-11-04, local)

**Status:** Owner-requested on 2026-11-04. The task: register the whole mixed set `C:\Astrofoto\M33 Test Sada vyber z hroznu` onto any of its frames or onto an external reference (real or synthetic). The method may disregard FulgurStack's current approach; registration happens in memory, success is verified, and the result goes to a detailed log. The owner limited the targets to the folder `Registratiom templates`. The program is `tools/skyreg`, with a workspace of its own; FulgurStack is unchanged. Local, not published.

**Method.**
- Every frame is plate-solved on its own against Gaia DR3.
  - Blind triangle search near a hint over a ladder of field sizes and both parities.
  - Refinement with polynomials of degree 1–3, chosen by held-out stars; proper motion is moved to each frame's date.
- A registration source → target is source pixel → sky → target pixel. Cameras, optics, filters, mosaics, rotation and mirroring therefore do not matter.
- Colour mosaics are recognised by a header card, or without one by the alternation of neighbouring pixels (ASI071MC and calibrated ASI2600MC frames have no BAYERPAT). Their stars are found on 2 × 2 superpixels.
- Every registration is verified end to end with the point sources both frames detected. The criterion is the systematic displacement (whole frame and 3 × 3 cells), not the scatter of single centroids.
- The catalogue comes from the ESA archive with GAVO `gaia.dr3lite` as fallback; ESA answered 503 on 2026-11-04.

**Evidence (372 frames: ASI1600MM 77, ASI2600MM 94, ASI2600MC 57, ASI071MC 49, G3-16200 XISF 95).**
- **Solving.** All 372 frames solve, plus all 16 templates and the synthetic Gaia reference (ADR-127). Held-out RMS against Gaia: median 0.29 px, 95 % 0.47 px, worst 0.73 px. The fits use degree 3 except the synthetic reference (degree 1).
- **Registration.** All 17 targets × 372 sources = 6324 registrations PASS.
  - Systematic offset, in pixels of the coarser frame: whole frame median 0.028, max 0.100; worst cell median 0.086, max 0.25.
  - Single-star scatter: median 0.22 coarser px.
- **Negative control** (every target displaced on purpose):
  - 0.5 px: all 6324 FAIL.
  - 0.2 px: 5130 of 6324 FAIL (pairs whose coarser pixel is larger still pass).
  - 0.1 px: 29 FAIL.
  - 0.05 px: none FAIL.
- **Telemetry.**
  - Cached catalogue: 280 s wall (read 27 s, solve 160 s on 8 threads), peak RAM 1.46 GB.
  - The first catalogue download took 681 s (25 cones from GAVO).

**Time against FulgurStack (2026-11-04, same 372 frames, catalogue cached, application limits).** FulgurStack registered the frames as six light sets, one per camera.

| | FulgurStack (one reference) | skyreg, 8 threads | skyreg, 24 threads |
|---|---|---|---|
| Read and stars | 11 s (inspection) | 26 s | 21 s |
| Registration | 75–87 s | 184 s solve | 128 s solve |
| Total wall | 87–99 s | 220 s | 162 s |
| Each further reference | whole run again | 3–8 s (target solve) + 0.04 s | same |
| Frames registered | 245–249 of 330 quality-accepted | 372 of 372, verified | same |

- The blind search takes 99.4 % of skyreg's solve time: 2898 of 2916 thread-seconds, 25.5 million poses voted.
- Refinement takes 18 thread-seconds and the checks 0.04 s per target.
- Detection costs 0.4 s per frame; FulgurStack's inspection already measures these stars.

**Speed-up (owner: "Yes, speed it up", 2026-11-04).**
- **Prior-seeded search.** In each camera group (instrument, sensor size, mosaic) one frame is searched blind. The others start from a solved sibling's scale (±5 %), parity and field centre: one field size, one parity. A frame no sibling fits gets its own blind search, which then seeds the rest of its group: the 2016 G3 frames have another focal length than the 2017 ones.
- **Early exit.** A pose with 40 votes ends a configuration and the search; wrong poses gather about ten.
- **Detection.** Tile medians from every second pixel of every second row; the background is computed row by row instead of as a full background-free image.
- **Reading.** FulgurStack's light-frame reader first (16-bit FITS, XISF), the reference reader only for other files such as float templates. Each solution lives in its own reader's pixel order.
- Every solution still passes the same refinement and held-out check.

| Same 372 frames, cached catalogue, 24 threads | Before | After |
|---|---|---|
| Read and detect (wall) | 21 s | 10–11 s |
| Solve (wall) | 128 s | 6.1 s |
| Blind search work | 25.5 M poses, 4938 configurations | 0.44 M poses, 465 configurations |
| Total with two targets | 162 s | 19.0 s |
| Each further target | 3–8 s | 0.01–0.1 s solve + 0.05 s check |
| Peak RAM | 2.80 GB | 1.99 GB |

- **Against FulgurStack** (87–99 s per reference): 4.6–5.2× faster end to end. The solve alone, which is what would replace FulgurStack's registration, is 12–14× faster than its 75–87 s.
- **Accuracy after the speed-up** (17 targets with the negative control):
  - 6324/6324 PASS.
  - Systematic offset median 0.032 coarser px (max 0.132); worst cell median 0.088 (max 0.243).
  - Held-out RMS against Gaia median 0.29 px.
  - Negative control: at 0.5 px all 6324 FAIL; at 0.2 px 5225 FAIL.
- 380 of 389 solutions came from a seeded search, 9 from a blind one.

**Development findings, fixed before the final run.**
- The blind-index cache was keyed by the hint's list position; frames with header RA/Dec then searched with the wrong hint, and 90 colour frames failed.
- The same pose found at two field sizes counted as its own rival (32 ASI1600 frames).
- A per-star RMS criterion failed correct Hα registrations: their faint stars scatter by 0.6 px.

**Limits.**
- Geometry only: no frames were resampled or stacked, and no master was inspected in PixInsight.
- The second-pass hint and the colour-mosaic test were tested on this set only.
- Sesame is unreachable from this PC, so the position was given by hand.
- PixInsight's local Gaia DR3 (`C:\PixInsightData`, XPSD format) is not read. It would make the tool work offline but would not speed up cached runs.

## ADR-130: An undated single camera on an outside grid takes its sharpest frame as anchor (2026-11-04, local)

**Status:** Owner-approved on 2026-11-04 ("Yes, fix it") after GUI run v86 stopped. Local, not published.

**Cause.**
- Run v86 used one light set: the 95 XISF frames of the 2017 camera from `M33 Test Sada vyber z hroznu`, with the synthetic external reference. 45 of them carry neither `DATE-OBS` nor an XISF observation time.
- With an outside grid the native anchor is the median by capture time. Among several cameras, a camera without capture times already takes its sharpest frame (`undated_anchor`, ADR-120). A single camera had no such fallback and stopped: "Automatic reference selection requires valid DATE-OBS on every accepted frame".

**Decision.** On an outside grid, a stack whose usable frames lack capture times takes its sharpest usable frame as native anchor (`outside_grid_anchor`), in both single-camera entry points. A chosen frame, a stack on its own grid and a fully dated stack keep their rules. The case is covered in the anchor-selection unit test.

**Evidence.**
- **The 95 XISF frames,** synthetic reference, application limits (1, 3, 80 %, 50, partial frames).
  - Before, the run stopped with the same message as v86. After, 81 of 81 quality-accepted frames stack; inspection refuses 14 (12 for low star count, 2 for eccentricity).
  - The anchor is Green_0021, the same frame the 2017 camera uses in multi-camera runs. Its bridge has 97 pairs at 0.11 px.
  - Held-out RMS: median 0.10 px, worst 0.71 px; largest single error 1.21 px.
- **All 12 M33 sets with the synthetic reference** are unchanged: 925 under the application limits, 956 at Maximum coverage.

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 278/278 default, 282/282 with all features. Single-camera masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0. The release application was rebuilt.

**Limits.**
- The report's `reference_selection` text still names the accepted median.
- Registration only; no master was stacked.

## ADR-129: A blind anchor bridge tries the brighter part of a rich template (2026-11-04, local)

**Status:** Owner-approved on 2026-11-04 ("Yes, fix it") after the ADR-127 density test. Local, not published.

**Cause.** The blind similarity pairs triangles of each star's eight nearest neighbours, so both lists need a like density where they overlap.
- The ASI2600 anchor's 250-star list covers a field 1.6× the grid. On the grid it is about 16× sparser than a 1000-star synthetic template, and sparser still against denser ones.
- Its bridge succeeded on 1000 stars only by chance (SQA_0006). With 2000 stars and more the whole camera (406 frames) was lost, and with 5000 the 2017 camera too.

**Decision.** When the whole template list finds no blind similarity for a camera anchor (`similarity_bridge`), its brighter parts try in turn: 32, 64, 128 … stars. Each part is a fixed number of stars per cell, because the catalog lists cell by cell, rank by rank. A pose found there is refined on the whole list. The seeded path and every bridge that already succeeded are unchanged. A unit test covers a sparse wide anchor that the whole list of a rich template cannot place.

**Evidence (all 12 M33 sets, synthetic Gaia references).**

| Synthetic stars | Application limits (1, 3, 80 %, 50), before → after | Maximum coverage, after |
|---|---|---|
| 1000 | 925 → 925 | 956 |
| 2000 | 586 → 861 | – |
| 5000 | 431 → 865 | 954 |

- No camera is lost any more.
- A denser synthetic field still gives fewer frames under the application's limits.
  - The rows' witnesses are the anchor's bridge labels and the template's 64-per-cell list. A richer field does not raise their number, and its longer witness list (256 per cell) adds chance pairs: MC Quad goes 20 → 7–9/20.
  - MM L stays at 12–18/42 because it has too few witnesses.
- 1000 stars remain the best synthetic density.

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 278/278 including the new unit test (281/281 with all features before it). Single-camera masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.

**Limits.**
- Native references take the seeded path and were not rerun.
- The application binary was not rebuilt because the application was open.
- Registration only; no master was stacked.

## ADR-128: A second pass on an outside grid keeps the first photometric anchor (2026-11-04, local)

**Status:** Owner-approved on 2026-11-04 ("Yes, fix it") after the ADR-127 experiment. Local, not published.

**Cause.** With an external template every camera picks its own anchor by chronological median, and the camera with the most frames gives the run its photometric anchor.
- On M33 the MM L set (other optics on the ASI2600 sensor) failed in the first pass and got its second pass as a camera of its own (ADR-120). That pass registered it, 42/44.
- Leaving the ASI2600 camera moved that camera's median from SQA_0006 to another frame (20251219_0001). The second pass then had another photometric anchor, so its rows could not join the first pass (`taken_sets`). That second pass alone stacked fewer frames (909 < 914), and the run kept the first pass without MM L.

**Decision.** On an outside grid, the camera that holds the first pass's photometric anchor keeps it in the second pass. A camera that leads its own grid (a chosen or automatic reference) still chooses as before. `cameras::kept_anchor` has a unit test.

**Evidence (all 12 M33 sets, Maximum coverage).**

| Reference | Before | After |
|---|---|---|
| Synthetic Gaia reference (ADR-127) | 914 | **956** / 959 |
| Lb_093 copied outside the inputs | 914 | **956** / 959 |
| m33_004 / m33B_003 / Lb_093 / MM L 0016 as input frames | 946 / 951 / 950 / 875 | unchanged |

- With both external references MM L stacks 42/42. The other sets keep their first-pass rows exactly.
- The 3 remaining refusals are trailed Hα frames.
- Held-out RMS of MM L, median: 0.63 px on the Lb_093 copy and 0.34 px on the synthetic reference.

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 277/277 default, 281/281 with all features. Single-camera masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.

**Limits.** Registration only; no M33 master was stacked, and the application was not run.

## ADR-127: Experiment: a synthetic Gaia DR3 star reference instead of a registration frame (2026-11-04, local)

**Status:** Owner-requested on 2026-11-04: plate-solve a chosen registration frame, build a synthetic reference of 1000 catalogue stars, register with it, and save it as JSON and a monochrome FITS. The catalogue download, plate solving and generation live in a separate Rust program, `tools/synthref`, with a Cargo workspace of its own. FulgurStack is unchanged and used as a library. Local, not published.

**Method.**
- **Plate solving.** It is blind near a target position, because the M33 headers carry no RA/Dec and no focal length.
  - Triangle asterisms of the 40 brightest detected and 70 brightest Gaia DR3 stars are tried over a ladder of field sizes and both parities.
  - The scale must fit the field size tried, and each detected star counts once.
  - A linear TAN solution is then fitted with 3-sigma clipping.
- **Synthetic reference.**
  - It holds the 1000 brightest Gaia DR3 stars inside the frame, moved to DATE-OBS by proper motion, on the frame's pixel grid: an ideal TAN projection without the frame's own distortion.
  - Stars are Gaussians at the frame's median FWHM, with peaks of 0.8·10^(−0.2(G−Gmin)), on a 0.001 pedestal with 0.0005 seeded noise. The image is BITPIX −32 with a WCS.
  - The output is reproducible byte for byte. The catalogue answers are cached as CSV.

**Evidence (frame Lb_093, ASI1600MM, 4656 × 3520).**
- **Solution.** 0.9608″/px, field 1.24° × 0.94°, centre RA 23.4882° Dec +30.6688°.
  - The correct candidate agreed on 119 stars; every other candidate agreed on at most 7.
  - 461 of 500 detected stars confirm it, with linear RMS 0.80 px and maximum 2.4 px. A cubic would give 0.38 px: the optics bend the field by about 0.7 px RMS.
  - Solving the synthetic FITS itself returns the same centre, scale and rotation at 0.15 px RMS.
- **Registration of all 12 M33 sets (Maximum coverage).**

| Reference | Frames |
|---|---|
| Lb_093 as an input frame | 950 / 959 |
| Lb_093 copied outside the inputs (external) | 914 / 959 |
| Synthetic Gaia reference (external) | 914 / 959 |

  - Both external references lose all 42 MM L frames. With an external template the ASI2600 SQA frame leads the photometric anchor, and the MM L set (other optics on the same sensor) gets no camera of its own. This is a property of external templates, not of the synthetic one.
  - Held-out RMS per set, median in px, external Lb_093 → synthetic:
    - MM quad 1.73 → 0.29
    - OIII 1.16 → 0.26
    - Hα 0.50 → 0.30
    - Korčula 0.24 → 0.10
    - 2017 0.19 → 0.14
    - Q223 0.20 → 0.09
    - L 0.08 → 0.12
    - 2020 ASI1600 B set 0.18 → 1.02, maximum 4.0 px
  - On the synthetic grid the B set is held by a map without its own cubic, so the camera's ~0.8 px distortion remains.
  - The synthetic witnesses are exact catalogue positions without centroid noise, so part of the gain is in the measure itself.

**Verification.** synthref: fmt and clippy (`-D warnings`) PASS. FulgurStack sources, workspace, lockfile and tests are untouched. The registration used the first synthetic FITS. The final, reproducible build differs from it only by floating-point rounding in the fit.

**Limits.**
- Registration only: no master was stacked or inspected in PixInsight.
- CDS Sesame could not be reached from this PC; the M33 position was given by hand.
- One frame and one target.
- The validation sets are small (15–20 stars for most sets).

**Follow-up under the application's limits (1 px RMS, 3 px maximum, 80 %, 50 stars, partial frames; 2026-11-04, after ADR-128).** `synthref register --limits` reproduces GUI run v85 exactly.

| Synthetic stars | Frames |
|---|---|
| 1000 | 925 / 959 |
| 2000 | 586 / 959 |
| 5000 | 431 / 959 |

- With 1000 stars, 24 MM L frames lack witnesses: 11–20 against the 50 required.
- With 2000 or more stars the ASI2600 camera (406 frames) cannot bridge its anchor to the reference ("could not be matched with at least ten stars"); with 5000 the 2017 camera fails too.
- Even with 1000 stars, a single ASI2600 set registered alone fails the same bridge. Only the SQA_0006 anchor of the full run found one.
- The blind anchor-to-template similarity for these cameras is fragile against a synthetic star field, and a denser field makes it worse.

## ADR-126: A wider frame without a map of its own starts from its anchor's bridge (2026-11-04, local)

**Status:** Owner-approved on 2026-11-04 ("Yes, fix it") after the analysis of GUI runs v82 and v83. Local, not published.

**Cause.** In v82 (reference m33_004, 898/959) the anchor of the 2017 XISF camera, Green_0019, joined the grid by a similarity only, without a cubic. Only 18 of its stars were labelled in both frames. The refused frames of that camera held 0–5 of those labels, so they fitted no similarity, and no map was carried over from the anchor. The own cubic on the longer list (ADR-125) then had no starting point and never ran. 52 frames of 2017 and 5 Korčula frames failed.

**Decision.** When neither a carried map nor a refused map exists, a refused wider frame's own cubic starts from its native pose composed with its anchor's bridge to the grid (`bridge ∘ native`). This is the same pose that already places its shared field. The first search radius stays 12 px; held-out stars (every fifth) fit nothing; the run's limits judge the result unchanged.

**Evidence (all 12 M33 sets, Maximum coverage, `register_inspected_with_options`).**

| Reference | Before | After |
|---|---|---|
| m33_004 (v82) | 898 / 959 | 946 / 959 |
| m33B_003 | 951 | 951 |
| Lb_093 | 950 | 950 |
| MM L 0016 (v83) | 875 | 875 |

- On m33_004: 2017 goes from 23/75 to 66/75 and Korčula from 75/80 to 80/80. No frame is lost.
- All 48 new frames use `external_longer_cubic_validated`, with held-out RMS 0.64–1.45 px (median 0.97 px), at least 31 validation stars and a maximum error of at most 8.76 px.
- The 9 remaining 2017 refusals are Hα frames with trailed stars (eccentricity 0.44–0.57).
- v83 is not affected: its Hα and OIII sets fail when matching the MM L grid itself.

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 276/276 default, 280/280 with all features. Single-camera masters byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.

**Limits.** Registration only; no M33 master was stacked, and the application was not run. Registration time on m33_004 went from 80.7 s to 98.2 s, partly because 57 more frames are re-measured from their files (in RAM).

## ADR-125: A refused frame is judged again on longer star lists (2026-11-03, local)

**Status:** Owner-requested on 2026-11-03 after run v81 ("reduce registration failures, ideally to zero; implement the most promising recommendation, then the second"). This is the second. Local, not published.

**Cause.** A frame's registration list holds 250 stars over its whole sensor. Two cases leave too few witnesses or a wrong map:
- **A wider camera's frame** has only about 60 of its stars in the grid's part. The anchor's carried cubic does not fit every filter. With the L reference the OIII frames of the ASI2600MM reached 1.9 px RMS and more than 3 px in an outer quadrant. A filter bends the field otherwise than the colour anchor.
- **A narrowband frame on a broadband anchor of its own camera** detects emission knots. The anchor's 1000 precisely measured stars named only 9–12 of them. Example: the 2020 Hα frames in the L folder on the B-filter anchor m33B_003.

**Decision.**
- **Longer lists.** A star list can now hold more than 16 cells × 64 stars. Requests of up to 1024 stars stay byte-identical. The native field checks get the anchor's longer precisely measured list (up to 5000).
- **Native frame refused for too few witnesses.** It is measured once more from its file on its own longer list, while its coverage holds. Its mapping is then judged again under the run's limits: first against the anchor's longer precise list, then against the anchor's detections. The detections are about 1 px accurate, and their error only adds to the measured one. The mapping itself is not refitted.
- **Wider frame refused on the grid.** It fits a cubic of its own on its longer list against the grid's longer list. It starts from the carried map within 12 px. Its held-out stars (every fifth) fit nothing. The run's limits judge it, without the cubic's separate 1 px improvement rule (ADR-066).

**Masters (command line, the ten sets without the two 180 s sets, Maximum coverage, ADR-124 and ADR-125 together).**

| Reference | Frames | HFR / FWHM / eccentricity / noise / stars | Registration |
|---|---|---|---|
| Lb_093 | 779 → 795 | 2.016 / 4.246 / 0.262 / 4.39 / 7887 → 2.015 / 4.241 / 0.262 / 4.39 / 7881 | 26.7 → 42.2 s |
| m33B_003 | 740 → 797 | 1.829 / 4.013 / 0.285 / 12.79 / 7950 → 1.847 / 4.056 / 0.266 / 12.58 / 7745 | 51.8 → 57.9 s |

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 276/276 default, 280/280 with all features. A new unit test covers a list longer than 1024 stars; the 250- and 1000-star lists keep their size. Single-camera masters are byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.

**Limits.**
- Re-measurements happen in RAM and only for refused frames.
- On m33B_003 the master gains 57 frames, and its HFR grows by 1 %: the joined narrowband frames are less accurate than the broadband ones.
- One registration took 2592 s instead of 81 s with identical results; a repeat took 80.8 s. The cause lay outside FulgurStack (less free RAM at the time); it is not explained.
- Verified from the command line only; the application was not run.

## ADR-124: A wider anchor's cubic starts within 12 px of its similarity (2026-11-03, local)

**Status:** Owner-requested on 2026-11-03, the first and most promising recommendation after run v81. Local, not published.

**Cause.** With the B-filter reference m33B_003, the anchor of the ASI2600 camera (MC Quad) got no cubic: `insufficient_or_degenerate_training_field`. Its cubic was fitted against the grid's dense longer list (3729 detections), with a first search radius of 4 % of the field's diagonal (about 190 px). That radius pairs many stars by chance, and the fit fell apart. No frame of the camera could then carry the anchor's map. The similarity alone failed the outer quadrants of 50 frames, scattered over every night.

**Decision.** When a wider anchor's cubic fails and its similarity bridge is validated, the cubic is fitted again with a first radius of 12 px. The optics bend a star by a few pixels at most.

**Evidence (Windows 11, release, all 12 M33 sets with 180 s for the two sets without exposure, Maximum coverage, registration only).**

| Reference | v81 code (ADR-123) | + ADR-124 | + ADR-125 | Registration time before → after |
|---|---|---|---|---|
| m33B_003 (owner) | 894 / 959 | 939 | **951** | 80 s → 81 s |
| Lb_093 (L) | 930 / 959 | 930 | **950** | 48 s → 37 s |

- The MC Quad anchor's cubic: RMS 1.17 → 0.45 px on 55 held-out stars.
- The ASI2600 2025 sets and MM quad go in full on both references.
- OIII on Lb_093: 22 → 38/38, own cubic about 0.85 px RMS.
- The 2017 camera on Lb_093: 71 → 75/75.
- The 2020 Hα frames in the L folder on m33B_003: 9 of 10 now pass at 1.84–1.92 px RMS (with detection error), largest error under 3 px.

Remaining refusals:
- **m33B_003, 8:** 3 ASI2600MM Hα frames that do not register even on their own night's neighbours (eccentricity 0.5 against 0.27, likely trailed); 1 Hα frame of 99 stars without a match; 1 Hα frame without outer-quadrant witnesses; 3 Hα frames with one star 11–12 px off, above the 9 px limit.
- **Lb_093, 9:** the same 3 + 1 no-match frames, and 5 ASI2600MM Hα frames with one star about 11.9 px off.

## ADR-123: The registration progress bar plans every pass; a stranded set keeps its second pass (2026-11-03, local)

**Status:** Owner-reported on 2026-11-03 after run v80 (M33, 12 light sets): the registration bar reached 100 % after about 30 s and registration went on for about 30 s more, and 106 frames failed. The owner approved the per-set second pass the same day. Local, not published.

**Cause.** A light set that gets no frame into the stack on a sensor shared with other optics registers a second time as its own camera (ADR-120). That pass was not in the plan: its steps were capped at the old total, so the bar stayed at 100 %. Within the plan, a further camera was counted one step too high, and the further tries of a camera that missed the grid (sharpest frames, mirrored) were not counted at all.

**Decision.** The plan has one more step: the decision whether a second pass is needed. The bar reaches 100 % only when registration is over. A second pass is planned when it starts. The finished pass counts as it ran, so the bar moves back to the real share once and then rises to 100 %. A further camera counts its native rows, its grid rows and four steps (read, catalogue, bridge, finalization). Each further try of a camera adds its work when it starts. A try that ends early counts the rest of its work.

**Verification.** fmt, clippy (default and all features): PASS. `cargo test --workspace --release`: 274/274 default, 278/278 with all features. A new end-to-end test in `crates/fulgur-cli/tests/commands.rs` covers two light sets of other optics on one sensor. The bar never decreases its count, shows 100 % only at its last report and ends exactly there. Two existing exact-count tests gained the decision step. Masters are unaffected: only the reported steps changed.

**Run v80 analysis (same session, command line, release, ten of the twelve sets without the two 180 s sets, Maximum coverage).** The owner's reference m33B_003 reproduces v80 exactly (698 of 804 quality-accepted frames, the same 106 refusals). The grid's Lb_093 (L) instead of m33B_003 (B filter, eccentricity 0.52) brings 737. The 2025 ASI2600 sets go 64 → 75 and 93 → 105, MM quad 30 → 45, ASI1600 L 220 → 230, Ha 36 → 38, OIII 33 → 22. MM L (42 frames, other optics on the ASI2600MM sensor) fails in both runs. It registers alone (42/42) and joins as its own camera with L alone or with MC Quad (42/42). In the full stack its second pass joins all 42, but the other ASI2600 sets then fall to Ha 0, MM quad 2 and OIII 0 (661 < 737 frames), so the first pass stands and MM L is lost.

**Decision, per-set second pass.** The second pass keeps only what it found for the stranded sets. A stranded set whose own camera registered takes its rows, frame headers and sessions from the second pass. Every other set keeps its first-pass rows, and the first pass's cameras hand the taken frames to the new camera. Weights, canvases and counts are then computed once over the merged rows, as the join of cameras does. Rows of both passes stand on one grid only through the same leading anchor and size, so the merge runs only then. The whole second pass is taken only when it stacks more than the merge. A set refused again keeps its first rows.

**Evidence (the same ten sets, Maximum coverage).**

| Reference | Frames before → after | MM L | Master HFR / FWHM / eccentricity / noise / stars, before → after |
|---|---|---|---|
| Lb_093 | 737 → 779 | 0 → 42 (RMS median 0.21 px, largest error 5.4 px) | 2.11 / 4.44 / 0.299 / 4.62 / 6500 → 2.02 / 4.25 / 0.262 / 4.39 / 7887 |
| m33B_003 | 698 → 740 | 0 → 42 | 2.06 / 4.36 / 0.329 / 13.86 / 6485 → 1.83 / 4.01 / 0.285 / 12.79 / 7950 |

All other sets keep their counts. With Lb_093, the sharp MM L frames carry 45.5 % of the weight. A new unit test merges a second pass in which the other set fell apart; the CLI test of two optics on one sensor now runs through the merge. Final gates: fmt, clippy (default and all features) PASS; tests 275/275 and 279/279. Single-camera masters stay byte-identical: Seestar 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0.

**Limits.** The two 180 s sets were left out of the comparison (one exposure per command line). The application run was not repeated. Mono frames still feed all three colour channels of a combined master.

## ADR-122: A wider camera joins a narrower grid on the shared field (2026-11-03, local)

**Status:** Owner-requested on 2026-11-03 after run v78: the ASI2600 colour set (SQA106, 81 frames) did not join an M33 stack whose grid came from the ASI1600, and the owner wants registration to work in both directions, because the galaxy fills only a small part of the colour camera's field. Local, not published. Awaits the owner's review in PixInsight.

**Cause.** The ASI2600 pixel covers 1.6 ASI1600 pixels, so the ASI1600 grid shows about a quarter of its sensor. Its registration list holds 250 stars over the whole sensor and about 65 inside the grid. Too few to fit the cubic model (40 fitting stars needed), so the anchor stayed on a similarity with 0.9–1.5 px RMS and failed the 1 px check. Its frames had about 10 stars labelled through the anchor, too few for any check.

**Decision.**

- **Shared field.** A frame is fitted and judged on the part of its sensor that the grid shows: the bounding box of the grid mapped back through the frame's similarity, with 2 % of the longer side as margin. A frame whose part spans at least 90 % of both sides keeps the whole sensor, as before. The cubic model is normalized on that part and defined only there; pixels outside it never reach the stack. The 35 % extent rule, the reference stars inside the footprint and the search radius follow the part. Integration accepts a model whose part lies on the sensor.
- **Anchor.** A wider camera's anchor fits and validates its cubic on its longer list of precisely measured stars (up to 1000, read once more from its file). The labels of its frames stay on its registration list, matched through the validated model within 1.5 px.
- **Frames.** A frame of a wider camera carries the anchor's validated model through its own position on the anchor (from its camera's registration). The grid's stars judge the result, with the usual spatial split of held-out witnesses, because no star of the frame fitted that map. Where the grid's short list shows few of the frame's stars, the share counts the frame's stars that have a detection of the grid's longer list within 3 px, as in ADR-110. When both this map and the frame's own similarity pass, the more accurate one joins.
- **Narrowband frames of a wider camera.** Their stars are often emission knots that the grid's short list of bright stars lacks. Their direct check may name the grid's longer list as witnesses, and a map fitted on some of their stars keeps those stars out of the held-out split.
- The limits of ADR-121 apply unchanged. A narrower camera, or a frame inside the grid, is not affected.

**Evidence (Windows 11, release, command line, Auto reference, lead ASI1600 Lb_093, geometric correction, partial frames on, one exposure of 60 s for all frames).** M33: ASI1600 260 frames, the 2017 camera (1.59× coarser) 182, ASI2600 81 (75 pass quality). Before → after:

| Level | Frames | ASI2600 | ASI2600 RMS median / largest | 2017 camera (RMS median) | Master HFR / FWHM / eccentricity | Sky noise R / G / B |
|---|---|---|---|---|---|---|
| Maximum accuracy | 350 → 422 | 0 → 72 | – → 0.39 / 0.47 px | 129 → 129 (0.39 px) | 2.29 / 4.75 / 0.305 → 2.28 / 4.72 / 0.300 | 5.41 (no colour) → 5.01 / 5.07 / 4.81 |
| Owner's 1 px, 65 %, 30 stars | 379 → 453 | 0 → 74 | – → 0.32 / 0.45 px | 158 → 158 (0.39 px) | 2.34 / 4.80 / 0.301 → 2.28 / 4.72 / 0.297 | 5.37 (no colour) → 5.02 / 5.08 / 4.89 |
| Compromise | 425 → 450 | 46 → 75 | 0.70 / 0.94 → 0.39 / 1.11 px | 158 → 154 (0.40 → 0.27 px) | 2.32 / 4.77 / 0.301 → 2.29 / 4.72 / 0.292 | 5.34 / 5.36 / 5.12 → 4.99 / 5.04 / 4.87 |

The ASI2600 anchor's cubic on the shared field: 217 of 254 stars, 0.27 px RMS, 0.17 px held-out. At Compromise the 2017 camera loses four Ha frames of 0.02–0.04 % weight each, accepted before at 1.14–1.28 px or only as partial frames; its other frames improve from 0.40 to 0.27 px median through the new anchor cubic. With the ASI2600 frame as reference, the reverse direction, the master is byte-identical to before (454 frames). Default single-camera masters stay byte-identical: the 32-frame Seestar set 3F752C45EFDB46DF, LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0. Time 107–114 s against 83–113 s; the extra read of the wider anchor and the longer witness lists account for it.

**Verification.** `cargo fmt --check` and clippy, default and all features: PASS. `cargo test --workspace --release`: 274/274 default, 278/278 with all features, including real FITS, AVX2 and GPU. New unit tests: a 3 px cubic on the shared part of a 1.6× wider sensor is recovered exactly and never maps pixels outside the part; the field checks pass on the grid's part where the whole sensor would refuse.

**Limits.** Tested on one three-camera set from the command line, without the owner's Use choices and with one exposure for all frames; the application was not run. Mono frames still feed all three colour channels of a combined master. Eight ASI2600 frames stay out at 1 px: poorer seeing or one outer quadrant above the limit. The reasons recorded in reports keep their earlier names.

## ADR-121: Adjustable registration accuracy (2026-11-03, local)

**Status:** Owner-requested on 2026-11-03: a Registration accuracy card in Settings with three levels, then the limits wired into registration and compared on M33. Local, not published. Awaits the owner's review in PixInsight.

**Decision.**

- One set of limits governs every whole-field check: frames of one camera, cameras joining a reference, external references and the tiles of partial frames. Reports record it under `registration.accuracy`; the command line takes `--accuracy maximum-accuracy|compromise|maximum-coverage` or `RMS,PERCENT,MIN_STARS`.

| Level | RMS (all, held-out, each outer quadrant) | Largest single error | Stars that must match | Minimum matched stars |
|---|---|---|---|---|
| Maximum accuracy (default, earlier limits) | 1 px | 3 px | 80 % | 50 |
| Compromise | 2 px | 6 px | 60 % | 30 |
| Maximum coverage | 3 px | 9 px | 40 % | 20 |

- The share counts the weaker side: the stars that the frame or the reference shows in their shared field, whichever is fewer. Reference stars are counted inside the frame's mapped footprint. A frame needs at least the larger of the minimum and that share.
- Held-out witnesses scale with the minimum: a fifth of it, at least 4 (10 at 50); fitting witnesses three fifths (30 at 50); held-out cells of the field at most 6. The other structural checks stay: 35 % of the frame's width and height, 10 of 16 cells, a witness in each outer quadrant.
- Above a 3 px largest error, the 12 px witness window would count a star paired by chance with a neighbour, typically in a double star, as a true error. Such lone pairs are dropped there, as in dense fields. Up to 3 px they still fail the check and the 3 px catalog decides, as before.
- The cubic model's own fit criterion and the matching tolerances are unchanged; registration without geometric correction has no whole-field check and is not affected.
- Settings: the Registration geometry and Registration accuracy cards share one height. The card shows that the limits apply to every whole-field check.

**Evidence (Windows 11, release, command line, geometric correction, ESD).** Default limits keep single-camera masters byte-identical: LBN 437 F22D99BF062BC003, M42 D2330D5E10FA77B0, the 32-frame Seestar set 3F752C45EFDB46DF. M33, three sets from two cameras, reference Lb_091 for all three runs:

| Level | Frames | Master HFR / FWHM | Eccentricity | Sky noise measured in the master | Frame RMS median / 90th percentile | Largest single error | Time |
|---|---|---|---|---|---|---|---|
| Maximum accuracy | 338 of 442 | 2.198 / 4.639 px | 0.296 | 3.79 ADU | 0.16 / 0.51 px | 2.50 px | 80 s |
| Compromise | 341 | 2.217 / 4.676 px | 0.302 | 3.71 ADU | 0.16 / 0.36 px | 1.30 px | 79 s |
| Maximum coverage | 349 | 2.276 / 4.741 px | 0.304 | 3.68 ADU | 0.18 / 0.40 px | 1.31 px | 79 s |

Every frame added by a looser level fits within 0.08–0.44 px RMS. They had been refused for too few matched stars or too small a share (33–46 of 66–67, 162 of 250), never for accuracy between 1 and 3 px. Compromise adds ALL.xisf, an integrated master in the Korčula folder, with 12 % of the weight; Maximum coverage adds it with B3.xisf, six Blue frames, a Ha master, one Blue and one Lb frame, the rest under 0.2 % weight each. The 30 Ha frames of the Light set stay out at every level: 19–26 of 67 stars match. A first run with Auto chose another anchor per level (Lb_091, Lb_093, Lb_091), so levels must be compared on one reference. Before the chance-pair rule, Maximum coverage accepted 39 frames with an "error" of 8.8–9.0 px, all chance pairs on an unchanged map, and refused four frames that Maximum accuracy accepts.

**Verification.** `cargo fmt --check` and clippy, default and all features: PASS. `cargo test --workspace --release`: 272/272 default and 276/276 with all features, including real FITS, AVX2 and GPU. A new unit test covers the minimum, the weaker side and the RMS limit. A test that expected 60 of 90 stars to fail the 80 % rule now expects them to pass, because the reference shows only those 60.

**Limits.** The command line cannot apply the owner's Use choices: the integrated masters in the folders, switched off in the application, dominate the HFR and noise differences above, and the effective frame count falls from 39.7 to about 30 for that reason. On this set the looser levels change the master little: about 1–3 % less sky noise, about 1–4 % wider stars, mostly from those masters. The three levels were compared on one two-camera set only. The share now follows the weaker side at every level, but the default masters above stay byte-identical. Report reasons keep the names `rms_above_1px` and `star_above_3px` for partial-frame tiles whatever the limits.

## ADR-120: Several cameras without an external reference; mirrored cameras (2026-11-03, local)

**Status:** Owner-requested on 2026-11-03 after an M33 run stopped with "Requested reference did not pass input quality checks": frame 0222 is from a smaller light set of another camera, and the largest set had refused that whole set for its sensor size. Local, not published. Supersedes the size refusal of ADR-110 and the sentence on sensor dimensions in the light-set rule of PROJECT_SPEC. Awaits the owner's review.

**Decision.**

- Quality inspection no longer refuses another sensor size. The reason `different_sensor_size_unsupported` is gone.
- A stack with several cameras needs no external file. A chosen frame's camera leads, and Auto or the median gives the lead to the camera with most usable frames. The leading camera registers as in any other stack and stays on its anchor's grid, with its transforms and brightness unchanged. Other cameras reach the anchor as they reach an external reference, under the same accuracy limits.
- The anchor gives its own measured stars. Rereading its file would miss its calibration, and the reader of external files treats a missing `ROWORDER` as bottom-up while frames are read as stored, so a frame came back mirrored. A first match runs on the registration lists of both anchors, because they hold the same kind of bright stars, and the long lists then decide with that seed. External files keep their previous path.
- A later camera without capture times has no median frame, so its sharpest frame is its anchor.
- A camera that matches nothing as it stands tries again as its mirror image: star positions and the reader's row order reverse, and a Bayer pattern on an even height swaps its rows. It joins only under the same limits. Reports give `mirrored` per camera. A camera with calibration masters is not tried, because masters keep the stored row order.
- A camera that still has no match tries other bridges, because two cameras share only part of a field and one frame pair may hold too few common stars for the whole-field check. Up to five of its sharpest frames are tried besides its anchor, in both orientations. A frame is registered in full only when its registration list matches the leading anchor or one of the leading camera's five sharpest frames, mapped onto the grid through their validated maps. That list only starts the match: the frame is still validated against the leading anchor's long list. At most six full registrations per camera. Reports give `bridge_frame` when a frame other than the leading anchor started the match. The order of these tries is fixed by a synthetic test.
- A light set that lost every usable frame beside a set that stacked gets one more try as a camera of its own. This covers other optics on an equal sensor. The first result stands unless the second brings more frames, so a set of another sky field costs only time.
- A reference that failed quality still fails, as PROJECT_SPEC requires, and the message now separates a frame switched off in Use from a frame refused by quality.
- Frame HFR and FWHM in master quality use each frame's own scale onto the grid when several cameras stack. The "This run" card shows a stopped run's sets, not the previous run's. The status bar no longer says CFA for every set.

**Evidence (Windows 11, release, CLI, geometric correction, ESD).** M33 from a 4524 × 3624 camera (183 files in two sets, no DATE-OBS) and a ZWO ASI1600MM (259 files). Between the anchors: scale 0.720, rotation 102.8°, mirrored. A full search of the star lists found this, and on 250 stars 77 pair at 0.20 px RMS against 2–3 pairs for the control hypotheses.

| Run | Grid | Frames | First camera | Second camera | HFR | Sky noise, measured in the master | Effective frames | Time |
|---|---|---|---|---|---|---|---|---|
| Two sets, reference 0222 | 4524 × 3624 | 151 of 183 | 151 | none | 1.92 | 3.52 ADU | 15.8 | 33 s |
| Three sets, reference 0222 | 4524 × 3624 | 370 of 442 | 151 | 219 of 260, mirrored | 1.96 | 2.84 ADU | 44.0 | 72 s |
| Three sets, Auto | 4656 × 3520 | 338 of 442 | 220 | 118 of 182, mirrored | 2.20 | 3.79 ADU on a sky about 9 times brighter | not computed | 70 s |

The joined camera reaches 0.42 px RMS and at most 1.06 px, against limits of 1 and 3 px; 466 and 360 stars set its brightness. All 183 rows of the first two sets keep their transform, brightness and acceptance in the three-set run. Noise is the 20th-percentile block of 165 and 154 sky blocks, because the program's own estimate fell only from 3.80 to 3.71 ADU. Peak RAM stayed near 3.2 GB.

**Owner run `master_v74` (application, Auto).** Auto chose Lb_097 of the ASI1600 to lead, and the other camera was refused as "camera not registered": 182 frames out, 229 stacked. Its sharpest frame matched as a mirror image, with 52 stars, but the joint field was too small for a model of its distortion, and the similarity map missed by 1.62 px at the edge. Only one frame was tried. Seven stacks of the same three sets, each with another reference, command line:

| Reference | Before the bridge rule | With the bridge rule | Frame that joined the second camera |
|---|---|---|---|
| Lb_097 (owner's run) | 219 | 321 | Red_0000, the fourth try |
| Lb_091 | 338 | 338 | Green_0021 |
| Lb_050 | 330 | 330 | Green_0021 |
| Lb_150 | 219 | 317 | Red_0000 |
| Lb_010 | 330 | 330 | Green_0021 |
| Lb_200 | 220 | 319 | Green_0018 |
| 0222 of the other camera | 370 | 370 | Lb_091 |

Stacks that joined before are unchanged. Registration took about 10 s. One stack spent 925 s in inspection while the owner's application ran beside it; registration (10.5 s) and integration (35 s) were normal, and the cause was not examined.

Single-camera stacks are byte-identical to ADR-119: LBN 437 F22D99BF062BC003, M42 ASI2600 D2330D5E10FA77B0, the 32-frame Seestar set 3F752C45EFDB46DF.

**Verification.** `cargo fmt --check` and clippy, default and all features: PASS. `cargo test --workspace --release`: 271/271 default and 275/275 with all features, including real FITS, AVX2 and GPU. The synthetic camera gate now covers Auto, a reference of the smaller camera, other optics on an equal sensor and a mirrored camera; a reader test covers reversed XISF rows.

**Limits.** The M33 runs used the command line, which cannot set exposure per set, so every frame counted as 60 s, including 120 s and 300 s ones; accepted exposure in those reports is wrong and weights were not checked against true exposures. They also lack the owner's Use choices, so they contain files the owner switched off, such as masters in the Light folder, and frame counts will differ in the application, which was not run. The third folder holds 12 Ha frames with `No filter` in their headers; 11 stack with the L and Lb frames. The first camera's frames keep their maps, but their weights now compare in grid pixels, so the master is not the two-set master: when a second camera is refused, HFR moved from 1.92 to 1.96. With the ASI1600 leading, 98–118 of the other camera's 182 frames join, against 151 when that camera leads with 0222, because its frames reach the grid through one bridge frame from the Light set. Six full tries may not be enough for a harder pair, and a camera that runs out of them stays refused with its reason. Mirror evidence is this one camera pair, and whether optics or capture software causes it is unknown. A mirrored camera with calibration stays refused. One camera against an external file is not tried mirrored. Quality previews show frames as stored.

## ADR-119: Auto reference weighs plate scale and quality; coverage becomes a limit (2026-11-02, local)

**Status:** Owner-requested on 2026-11-02 after three LBN 437 stacks differed only in their reference: Auto 160/166, two manual references 166/166. Local, not published. Supersedes the selection order of ADR-067; the ADR-085 safeguard and the chronological median for one night stay. Awaits the owner's review.

**Finding.** Multi-night Auto ordered candidates by shared rectangle, mean coverage and centrality, with HFR and star count only as tie-breakers and eccentricity unused. It chose frame 2186 for 2 % more rectangle. That frame has the smallest plate scale of the set and eccentricity rank 165 of 166. The refused frames' 1.3–1.8 px rigid errors match the scale offset of about 530 ppm over the frame's 3323 px RMS radius. A frame with poor HFR (2255) reached 166/166, so frame quality alone does not explain the loss.

**Decision.** The time median still gives the preliminary registration. Each mapped frame's plate scale is then fitted from its own stars against the preliminary anchor, because rigid rows always report scale 1. Candidates narrow in this order, and a step that would leave none is skipped: within 3 % of the best shared rectangle; on the majority side of a meridian flip; not in the worst quartile of HFR, eccentricity or star count; closest to the median plate scale, within 50 ppm of the closest. The lowest rank sum of the three quality metrics wins, then centrality, mean coverage and file name. The final check of ADR-085 stays: a reference that maps fewer frames than the preliminary one is not kept. Reports add `selection_rule`, `plate_scale_spread_ppm`, `selected_scale_offset_ppm` and `candidates_after_filters`.

**Evidence (Windows 11, release, CLI, geometric correction, ESD).** Accuracy limits are unchanged.

| Set | Run | Reference | Frames | HFR | Eccentricity | Noise ÷ median photometric scale | Effective frames |
|---|---|---|---|---|---|---|---|
| LBN 437, 3 nights | old Auto (`master_v63`) | 2186 | 160/166 | 1.510 | 0.432 | 7.32 | 144.9 |
| LBN 437 | new Auto | 2274 | 166/166 | 1.441 | 0.410 | 7.16 | 150.0 |
| M42 ASI2600, 4 nights | old Auto | first preliminary | 235 of 255 | 1.831 | 0.228 | 0.970 | 94.1 |
| M42 ASI2600 | new Auto | same night, later frame | 245 of 255 | 1.851 | 0.227 | 0.912 | 99.3 |

On LBN 437 the plate-scale spread is 950 ppm and the chosen frame is 35 ppm from the median. Two runs gave a byte-identical master, with integration 118.4 s straight after the test suite and 102.5 s in the repeat. On M42 the old Auto had fallen back to its preliminary reference, because its chosen frame mapped only 231 frames. M42 ran with `--accept-derived`, and 96 frames are refused by quality in both runs. The 32-frame single-night set is byte-identical to the 0.12 build. Selection added 0.06–0.08 s.

**Verification.** `cargo fmt --check` and clippy, default and all features: PASS. `cargo test --workspace --release`: 270/270 default and 274/274 with all features, including real FITS, AVX2 and GPU, a new synthetic gate for scale and shape, and the unit tests of ADR-116 to ADR-118.

**Limits.** Two multi-night sets were run. On LBN 437 the shared rectangle is 2 % smaller than with the old choice (59.3 against 60.5 Mpx, estimated), and on M42 HFR and FWHM are about 1 % worse, within one run. Candidates still come from the frames the time median can map: 151 of 166 on LBN 437. A first-night reference with good eccentricity was not tried, so the scale explanation rests on the agreement above. Thresholds of 3 %, a quarter and 50 ppm are design choices, not measured optima.

## ADR-118: Low tiles by default; 1024 × 1024 removed (2026-11-02, local)

**Status:** Owner-requested on 2026-11-02 after the tile shape measurements in ADR-117. Local, not published. Supersedes the 1024 × 1024 tile of ADR-117 and the square-only automatic order of PROJECT_SPEC 7.2. The test suite was not run on this change by the owner's instruction; it ran with ADR-119 and passed.

**Decision: automatic order.** Auto takes the first shape that fits the memory budget. On CPU stacks with an image wider than 4000 px: 1024 × 256, 512 × 256, 256 × 256, 128 × 128. On other CPU stacks: 512 × 256, 256 × 256, 128 × 128. On any GPU stack it keeps 512 × 512, 256 × 256, 128 × 128, because lower tiles mean more device dispatches, which were not measured. Tile width sets how much is read from wide frames, and height only adds memory.

**Decision: manual choices.** Settings > Advanced offers Automatic, 1024 × 256, 512 × 256, 512 × 512, 256 × 256 and 128 × 128. 512 × 512 stays for comparison with earlier runs. The CLI keeps `--tile-size auto|128|256|512` and `--tile-width`, `--tile-height`, so the two low shapes are `--tile-width 1024 --tile-height 256` and `--tile-width 512 --tile-height 256`. Saved settings gain a tile height; a saved tile without one is read as a square.

**Decision: 1024 × 1024 removed.** It gained nothing over 1024 × 256 and needed 1.8 GiB more on the tester's set. The option, its 4000 px rule and its exception from the memory ceiling are gone, so every tile respects the 12288 MiB ceiling again. Reports list candidates with `requested_width_px` and `requested_height_px` instead of `requested_size_px`.

**Evidence (Windows 11, release, CLI, 32-frame Seestar set DS-B, 27 accepted, one run each).** Master, coverage map and high-rejection map are byte-identical between the 0.12 build with Auto 512 × 512, the new Auto 512 × 256, and manual 1024 × 256 and 512 × 512. Integration took 3.1–3.3 s in all four, which this small set cannot separate. The larger evidence is in ADR-117: tester's set 99.5 s to 93.8 s at the same 2.7 GiB, Seestar 941 frames 100.2 s to 98.8 s at 1.88 GiB instead of 3.37 GiB.

**Verification.** `cargo fmt` and clippy with all features: PASS, including the edited and new unit tests, which compile but were not run. Only the set above was stacked.

**Limits.** Auto reached 1024 × 256 on real data only with ADR-119 (LBN 437 at 9576 px, M42 at 6248 px). The 4000 px threshold is the owner's earlier limit, not a measured point. With partial frames (ADR-109) the master depends on the tile, so those masters change with the new Auto. Dwarf frames, compressed XISF and the GPU engine were not tried with the new shapes. The application's setting was not checked on screen.

## ADR-117: Manual 1024 px tile for machines with more memory (2026-11-02, local)

**Status:** Owner-requested on 2026-11-02, so that colleagues with 64 GB of RAM can try whether a larger tile is faster. Local, not published. The 1024 × 1024 tile and its exception from the memory ceiling are removed by ADR-118; the measurements below stay valid. The owner ran one comparison; the test suite was not run on this change, by the owner's instruction.

**Decision.** Output tiles of 1024 × 1024 px are a manual choice in Settings > Advanced and `--tile-size 1024`. Auto never picks them, and its candidates stay 512, 256 and 128. The choice needs both image sides above 4000 px, read as width and height each, so a 6000 × 4000 px sensor does not qualify. It also needs the estimated memory to fit free RAM minus the system reserve. The application greys the entry out with the reason, blocks the start if the frames change after the choice, and falls back to Automatic after a restart. The engine repeats the checks on the real canvas and stops with a message instead of resizing the tile. Only this exact square may exceed 512 px in height.

**Memory ceiling.** With this tile the configured ceiling of at most 12288 MiB does not apply, in the plan or in the runtime check of measured process memory; free RAM minus the reserve decides, and the runtime check never drops below the ceiling. By the memory estimate, a fixed 12 GiB would refuse a stack of 61 Mpx frames from roughly 600 frames on, which is the case this option is for. All other tiles keep the ceiling. On a 32 GB machine this option can therefore exceed the 12 GB target of the project contract; that is accepted as an owner-started manual choice.

**Evidence (owner's runs, Windows 11, release, application, the tester's 160/166 frames, ESD, CPU, 24 workers, one run each).** The master is byte-identical to the 512 px run. Integration 99.5 s against 108.0 s (−7.8 %), whole run 115.9 s against 122.2 s (−5.2 %). Reconstruction 72.7 s against 80.6 s; pixel rejection is unchanged at about 25 s. The gain comes from reading: 170 GB against 329 GB of source bytes, 11200 against 39520 region requests. Resampling and cosmetic work do not change. Registration differed by 2.2 s with identical settings, so about that much of the difference can be noise.

**Tile shapes and reading (Windows 11, release, CLI, same set, 12 runs, 3 per shape in rotated order, medians).** All 12 masters are byte-identical. 512 × 512: integration 99.5 s, 2.68 GiB during integration, 306.5 GiB read by the application. 1024 × 256: 93.8 s, 2.69 GiB, 167.8 GiB. 1024 × 512: 93.3 s, 3.29 GiB, 161.5 GiB. 1024 × 1024: 93.9 s, 4.49 GiB, 158.4 GiB. Tile width sets the speed and height only adds memory, so 1024 × 256 keeps the whole gain with no extra RAM. Its 40000 file opens cost the same as 11200 for 1024 × 1024, so file opens are not a cost worth removing. Physical disk reads during integration are 14–15 GiB, about 5 % of the application's 306.5 GiB at 512 px; the rest comes from the system file cache. Read time, as worker time over 24 workers, follows about 7 s plus 0.04 s per GiB. Reading to the master is therefore not a bottleneck on this machine, and background prefetch has at most about 7 s to gain. Only kernel CPU time falls (290 s to 177 s); user time stays at about 1500 s.

**Seestar S50 Pro (same day, CLI, 941 of 1140 frames, 2160 × 3840, ESD, one run per shape).** All masters are byte-identical to the owner's stored `master_v54.fits`. Integration and peak RAM: 512 × 512 100.2 s, 3.37 GiB (first run, more physical reads); 512 × 256 98.8 s, 1.88 GiB; 1024 × 256 98.9 s, 3.50 GiB; 1024 × 512 97.6 s, 6.43 GiB. The time differences are within what one run per shape can resolve, and reading is only 7–9 s here. On this sensor, shape changes memory and not speed, and 512 × 256 uses 44 % less RAM than 512 × 512. The 1024 × 1024 tile stopped after 20.5 s at the 4000 px rule, as designed; the memory estimate of about 14 GiB would not have fitted either.

**Verification.** `cargo fmt` and clippy with all features: PASS. A unit test was added and compiles, but was not run. No test suite was run on this change.

**Limits.** Two sets, on one machine with a fast NVMe disk, both smaller than RAM. A set larger than free RAM or a slower disk may behave differently. Whole-run times vary by several seconds with inspection and registration, so the comparison uses integration time. Peak RAM in reports stays at about 8.3 GiB for every shape because inspection sets it; the integration figures above are the sampled working set. The memory plan estimates 3.9 to 7.1 GiB, above what was measured. The master depends on tile size only with partial frames (ADR-109). The estimate in the application assumes the CPU engine; the engine's final check counts GPU buffers. GPU processing with this tile was not tried.

## ADR-116: Partial-frame report shows where, why and with which fit (2026-11-02, local)

**Status:** Owner-requested on 2026-11-02 after the report could not say how large the inaccurate area of a partial frame was. Local, not published.

**Context.** `partial_field` (ADR-109) gave only tile counts. A refused tile could mean a wrong fit or just too few stars, nothing said where it lay, and `registration_validation` described the first attempt, not the fit the frame stacks with.

**Decision.** Each partial frame now reports `fit` (`rigid`, `similarity` or `cubic`), `fit_rmse_px` and `fit_maximum_error_px` for the fit in use, over all its witness stars, and `refused_tiles` sorted by row, then column. A refused tile gives its position and size in output pixels, the stars in its test window, their RMS and maximum error, and a reason: `too_few_stars`, `rms_above_1px`, or `star_above_3px` when only single stars miss. The console and Results line splits refused tiles by reason. The tile test itself and the report's other fields are unchanged, and `registration_validation` still explains why the whole frame was refused. The CSV stays without partial-frame columns.

**Evidence (Windows 11, release, the tester's 166 frames, 512 px tiles).** Master, coverage map and high-rejection map are byte-identical to the stored run of ADR-109. Wall time 141.3 s, peak RAM 8.32 GiB. Of the 27 refused tiles, 16 are inaccurate and 11 have too few stars. Only 2 of the 6 frames have inaccurate tiles, 9 and 7 of 247, all in one corner of the field (x below 2048 px, y from 5120 px). The other 4 frames were refused only for too few stars, on 1 to 3 tiles at the field's edge. The fit in use has RMS 0.40–0.66 px and maximum 2.1–3.0 px, while the first attempt reported 0.49–1.78 px. The lower values in 5 of 6 frames explain how tiles pass after the whole frame failed. All 6 similarity fits failed `outer_field_accuracy_failed`.

**Verification.** `cargo fmt --check` and clippy, default and all features: PASS. `cargo test --workspace --release`: 268/268 default and 272/272 with all features, including real FITS, AVX2 and GPU.

**Limits.** The test window reaches 768 px beyond the tile, so a refused tile marks where the evidence ends, not the exact edge of an error, and a refused tile can sit next to accurate sky. Stars near the bad region also affect neighbouring tiles. The window still includes stars used for the fit. One set was run, with no external reference; that path is covered by a unit test only. Package 0.12 does not contain this report.

## ADR-115: Consolidated main publication of ADR-101–ADR-114 and package 0.12 (2026-11-01)

**Status:** Owner-authorized on 2026-11-01 for this batch only.

**Scope.** One commit to `main` with `[skip ci]`, holding ADR-101–ADR-114, their documentation, version 0.12 and the About page, and one Windows package built from that commit with an updated Czech guide. The earlier local entries marked "local" in this file and in the status page are published by this commit. Actions configuration is unchanged. Images, outputs, preferences, raw telemetry and binaries are excluded. The package stays local.

**Version.** The workspace version goes from 0.1.0 to 0.12.0. The application shows 0.12 in the window title, and the CLI and the Windows file properties report 0.12.0. About holds a project description and release notes since the alpha package. Its text is written by hand in the application and has no link to the ADR list.

**Verification before publication (Windows 11, release, local).** `cargo fmt --check` and clippy, default and all features: PASS. `cargo test --workspace --release`: 267/267 default and 271/271 with all features, including real FITS, AVX2 and GPU. The speed work ADR-111 to ADR-114 kept masters byte-identical on every real set compared, including the reference DS-A against the owner's stored master.

**Limits.** The application screens of this batch, including Masters, Registration geometry, partial frames, the Cameras row and About, were reviewed by the owner in part or await manual acceptance. Multi-camera stacks and partial frames rest on the sample sets named in their ADRs. Only one Windows 11 computer with AVX2 and an integrated GPU was tried. The path without AVX2 ran in unit tests only, and macOS was not tried. Per-filter grouping has only synthetic evidence. The package's own hashes cannot be part of the commit it is built from, so [distribution](WINDOWS_DISTRIBUTION.md) in this commit still lists the previous package.

## ADR-114: Faster median, and winsorized clipping reviewed (2026-11-01, local)

**Context.** After ADR-111 the median was slower than ESD, because it still sorted every sample together with its weight to read the middle one. The owner asked to speed it up and to look at winsorized clipping as well.

**Decision.**
- The median selects the middle sample without sorting and works on integer keys that follow the `f32::total_cmp` order. When all channels of a pixel share their mask, the weight total comes from the coverage sum already computed.
- Winsorized clipping selects on the same keys.
- Median, winsorized and linear fit test a repeated plane once, as ESD does since ADR-111.

**Winsorized review.** A timing test on synthetic stacks compared the key selection with sorting neighbouring pixels together, as ADR-111 does for ESD. Results are per pixel channel at 24 threads.

| Samples | Current | Key selection | 8-column sort |
| --- | --- | --- | --- |
| 941 | 25.4 µs | 21.9 µs | 30.6 µs |
| 100 | 3.9 µs | 3.7 µs | 2.9 µs |
| 22 | 1.5 µs | 1.4 µs | 0.8 µs |

Column sorting gives the same thresholds, but it is slower on large stacks and helps only small ones, where reduction is already a small share of the run. It would add about 100 lines of bookkeeping, so it is left out.

**Results (Windows 11, release, CPU engine, 24 threads, geometric correction on).** Master, coverage and high-rejection maps are byte-identical to the previous build in all four comparisons. Each ran new, old, new.

| Stack | Method | Reduction s, before → after | Total s, before → after |
| --- | --- | --- | --- |
| Seestar DS-A, every fifth frame, 184/228 | median | 13.9 → 3.9 | 35.2 → 23.8–25.5 |
| | winsorized | 8.4 → 8.0–8.3 | 27.1 → 27.1–28.3 |
| ASI6200 LBN 437, every fourth frame, 42/42, mono | median | 16.8 → 5.7–6.2 | 58.1 → 47.0–54.6 |
| | winsorized | 16.8 → 8.8–9.0 | 56.5 → 47.9–49.1 |

On colour stacks winsorized clipping barely gains on real data: the synthetic 14 % does not show up, and that difference is within run spread. Its gain is on mono stacks, about 47 %; reduction does not fall to a third because packing the samples and the weighted sum stay. The full DS-A reference stack was not rerun.

**Verification.** fmt and clippy (default and all features) PASS; `cargo test --workspace --release` 267/267 default, 271/271 all features (real FITS, SIMD and GPU variables). New tests: the sample keys against the total order, the selected median against a full sort with ties and signed zeros, and a repeated plane against separate channels for all four methods. The frozen winsorized reference test still passes.

## ADR-113: Faster reconstruction with identical masters (2026-11-01, local)

**Context.** After ADR-111 and ADR-112, reconstruction was the largest phase of a run. Timing each step on sampled real frames showed two different patterns. Sets with geometric correction spent 36–54 % of reconstruction solving the inverse distortion for every pixel. Rotated Seestar frames spent it on ROI corners: the ROI is the bounding box of the rotated tile, so 44 % more pixels were reconstructed than the output used. Removing nested parallelism inside an ROI gave nothing, so it stays.

**Decision.** (Reconstruction gain 13–40 %, per set below.)
- **Inverse distortion.** The solver runs four pixels at a time with AVX2, each lane repeating the scalar operations in the same order. The convergence test compares squares instead of calling `hypot`, in both paths.
- **Row spans.** On frames without a distortion model, the outline of the rotated tile gives each ROI row the columns any output sample can read: the 4x4 interpolation footprint for demosaicing, plus the 5x5 demosaicing window for cosmetics. Everything else is skipped. Frames with a distortion model keep the whole ROI.
- **VNG memory.** The vector kernel no longer allocates ten helper arrays per row, and the RGB buffer is no longer zeroed for each ROI.

The GPU path is unchanged. The report field `corrected_roi_pixels` counts only scanned pixels now, so on rotated frames it is lower than before and lower than the GPU count. The rest of the report keeps its meaning.

**Results (Windows 11, release, CPU engine, 24 threads, geometric correction on).** Master, coverage and high-rejection maps are byte-identical to the previous build on all four sets. Each set ran new, old, new.

| Stack | Frames | Reconstruction s, before → after | Total s, before → after |
| --- | --- | --- | --- |
| Seestar DS-B | 27/32 | 2.1 → 1.7–1.8 | 5.4 → 4.7–5.2 |
| Seestar DS-A, owner-authorized rerun | 941/1140 | 63.1 → 52.6–55.1 | 122.4 → 114.6–122.4 |
| Seestar DS-A, every fifth frame | 184/228 | 14.5 → 12.0–12.2 | 31.1 → 28.7–29.1 |
| Dwarf3 with darks | 856/1098 | 84.2 → 69.3–72.0 | 168.3 → 159.7–165.4 |
| ASI6200 LBN 437, mono | 160/166 | 133.3 → 80.1–81.1 | 205.2 → 153.8–155.3 |

Peak RAM did not change beyond run-to-run spread. The DS-A master is also byte-identical to the owner's stored `master_v54.fits`. With ADR-111 and ADR-112, DS-A at 941 frames went from 242.3 s to 114.6–122.4 s, including frame measurement. On DS-A the spread between runs of the same build (about 8 s) is as large as the gain in total time, so the reconstruction times are the reliable measure there.

**Limits.** The `hypot` replacement could in principle change a pixel whose error sits exactly at the 1e-7 px limit; no master changed. Only an x86-64 AVX2 machine was tested. The file read pattern is unchanged: a mono set with 512-pixel tiles still reads about 17 times the bytes it uses.

**Verification.** fmt and clippy (default and all features) PASS; `cargo test --workspace --release` 264/264 default, 268/268 all features (real FITS, SIMD and GPU variables). New tests: the vector inverse against the scalar solver bit for bit, with solved and refused pixels; every footprint and window pixel inside its spans at ten rotations; rotated tiles with spans against whole-ROI reconstruction on poisoned buffers; VNG and cosmetics on column spans for both kernels.

## ADR-112: Faster frame inspection with identical measurements (2026-11-01, local)

**Context.** After ADR-111 inspection was about 15 % of a Seestar run. Timing each step showed that most of it was memory handling and waiting, not measurement.

**Causes found.**
- Frames ran in batches of 16. Each batch waited for its slowest frame, and all 16 started reading at once.
- The red and blue sky levels built a whole image plane for each frame to read about 65,000 samples from it.
- The hot-pixel-suppressed copy of the frame took fresh memory for every frame, and the detection grid was computed twice.
- The read path held the raw bytes and the decoded frame together, both in fresh memory.
- The peak scan worked out the sky cell and limit for every grid cell.

**Decision.**
- Up to 16 workers take the next frame when free. Results keep file order, so the batch limit on frames in memory stays, and progress still counts upward.
- The sky of a colour plane reads its samples directly from the frame.
- Each worker reuses the memory for the decoded and the suppressed frame for the whole inspection, and frees it at the end.
- Only grid cells holding a replaced hot pixel are recomputed.
- Frames decode in 1 MiB chunks, so the raw bytes are never held in full on the sequential read path.
- The peak scan uses one table of limits per sky cell.

**Results (Windows 11, release, 24 threads).** Each set ran new, old, new. Every per-frame result, including star lists, is identical to the previous build; only timing and RAM fields differ. A full DS-B stack gives a byte-identical master and maps.

| Set | Frames | Inspection s, before → after | Peak RAM GiB, before → after |
| --- | --- | --- | --- |
| Seestar DS-A | 1140 | 24.5 → 9.8 | 1.02 → 1.14 |
| Dwarf3 with darks | 1098 | 34.5 → 17.6–19.0 | 1.04 → 1.18 |
| ASI2600MC M42 | 351 | 26.3 → 10.1–10.4 | 3.22 → 3.42–3.53 |
| ASI6200 LBN 437, mono | 166 | 33.3 → 18.8–23.5 | 7.51 → 8.32 |
| Seestar DS-B | 32 | 0.5 → 0.3 | 1.02 → 1.06 |

**Trade-off.** Peak RAM rises by up to 11 %: the 16 frames in work are now always all present and keep their memory between frames. The largest set stays below the 12 GB target.

**Verification.** fmt and clippy (default and all features) PASS; `cargo test --workspace --release` 260/260 default, 264/264 all features (real FITS, SIMD and GPU variables). New tests: file order and progress with more frames than workers, the plane sky against a whole plane on all four Bayer patterns, the patched grid against a full rebuild, and reads on reused memory.

## ADR-111: Faster ESD with identical masters (2026-11-01, local)

**Context.** The owner's 1140-frame Seestar set went from about 121 s (2026-10-27, 932 frames, winsorized) to 266 s (2026-11-01, 1083 frames, ESD). Of the extra 144 s, 126 s are reduction: ESD added about 111 s and the extra frames the rest. Reconstruction per frame did not slow down. Going back to winsorized is no answer: on this set its master has about 8–13 % more noise than ESD.

**Causes found in the CPU path.**
- Every pixel channel sorted all its samples together with their weights, and the test used a square root in each of about 300 steps.
- Critical values were cached per band, and a tile has one band per worker. Every band recomputed them for every sample count it met, at milliseconds each, and a rotated or dithered stack has many sample counts.
- A mono stack keeps three identical channels and ran the whole test three times.

**Decision.** ESD gets its own band path, `esd_columns.rs`:
- Sample columns of neighbouring pixels are sorted eight at a time by a data-oblivious AVX2 bitonic network. Shorter columns are padded with +inf, and comparators that reach rows beyond the longest column are skipped. Without AVX2, each column is sorted alone.
- The test runs on the sorted column with the same running sums as before, compares squares and uses a table of inverses. Survivors are summed in frame order by their value range.
- When equal samples straddle a rejection limit, the old scalar path decides, because a value range cannot tell them apart.
- Critical values are cached once per sample count for the whole process.
- Channels with identical samples and weights are tested once.
- AVX-512 gave 7–13 % more in a timing test, but it needs Rust 1.89 and the raised minimum version switches on clippy lints across the workspace. It is left out.

The GPU path and the other methods are unchanged.

**Results (Windows 11, release, CPU engine, 24 threads, geometric correction on).** Master, coverage and high-rejection maps are byte-identical to the previous build on all three sets, and rejected sample counts are equal.

| Stack | Frames | Reduction s, before → after | Total s, before → after |
| --- | --- | --- | --- |
| Seestar DS-B | 27/32 | 3.7 → 1.3 | 7.8 → 5.6 |
| Seestar DS-A, owner-authorized rerun | 941/1140 | 139.7 → 44.2–45.3 | 242.3 → 142.1–153.2 |
| Dwarf3 with darks, colour | 856/1098 | 110.0 → 43.4–45.8 | 246.5 → 175.9–177.8 |
| ASI6200 LBN 437, mono | 160/166 | 108.8 → 24.4–24.9 | 282.0 → 196.0–196.3 |

Reduction CPU time fell to 32–42 % on the colour sets and 21–23 % on the mono set. Peak RAM did not change beyond run-to-run spread. Each set ran new, old, new, so disk cache favours neither build. Totals include frame measurement. The DS-A master is also byte-identical to the owner's stored `master_v54.fits`. The owner's 1083-frame selection exists only in the application, so it was not measured.

**Limits.** Sum order differs from the scalar path, so one f32 step in a master pixel is possible in principle; none occurred on these sets. Only an x86-64 AVX2 machine was tested; the sorted-column fallback ran in unit tests only, not on macOS.

**Verification.** fmt and clippy (default and all features) PASS; `cargo test --workspace --release` 257/257 default, 261/261 all features (real FITS, SIMD and GPU variables). New tests: the sorting network on every zero-one column up to 16 rows (which proves it for those lengths) and on random columns up to 2100 rows; bands against the scalar ESD on 1–941 frames with outliers, ties, masks, non-finite samples and mono stacks; all kernels bit-identical to each other.

## ADR-110: One stack from several cameras on an external reference (2026-11-01, local)

**Context.** The owner wants one stack from several cameras or lenses with different field curvature. Frames of another sensor size were refused, and partial frames (ADR-109) were off with an external reference, which is their most common use.

**Decision.** With an external reference, a run with several usable sensor sizes registers per camera:

- Each sensor size is one camera and needs its own light set. It registers natively on its own anchor, and that anchor bridges to the reference, so each camera gets its own whole-field and cubic model. The camera with most usable frames comes first, the larger sensor on a tie, and sets the brightness and the master header. A camera that fails only loses its own frames; the next one that registers takes first place.
- Brightness between cameras comes from stars both anchors show at the same reference position: the median of flux × pixel scale², because a resampled star spreads over the area of its source pixels and no area multiplier is applied. Each camera joins through the known camera it shares most stars with, so two small fields can meet through a larger one. At least ten stars are needed; otherwise the camera is refused and says why. These pairs use the anchors' longer star lists.
- Sky is matched per channel to the first camera. Weights compare HFR in reference pixels.
- When the anchor does not pass on the ordinary reference catalog, its bridge is solved again on a deeper one (2000 instead of 500 stars). The ordinary catalog caps each cell, so a rich field or a small field on the reference hides true stars. Runs that pass the first time are unchanged.
- A frame that shares too few stars with the anchor's list is checked directly against the reference under the same limits (RMS ≤ 1 px, max ≤ 3 px, coverage ≥ 80 %); stars no candidate map was fitted on are the held-out ones. This also applies to one camera. Without it the sharpest anchor refused most frames of its own set.
- On a deep catalog a star can pair by chance inside the 12 px window. A pair above 3 px whose neighbours sit below 1 px is dropped: a true error is shared by the neighbours. If more than 5 % would go, none does.
- Partial frames now work with an external reference: when no map passes, the most accurate refused map keeps the tiles its witnesses confirm, in reference pixels.
- The OBJECT card of a reference no longer refuses it (ADR-093: stars decide).

**Limits.** Only one three-camera combination was tested, on uncalibrated samples. Colour comes from whatever the cameras see, and mono frames enter all channels. Different star sizes between cameras raise pixel rejection near stars. The master header comes from the first camera. Native registration without an external reference still refuses another sensor size.

**Evidence.** M 33 samples on the owner's ASI2600 master, geometric correction on: Seestar S50 32/40, ASI2600 9/12 and ASI1600 10/12 frames, 51/64 stacked in 43 s at 2.0 GiB. Refused frames failed quality limits, plus one ASI1600 frame at 78 % coverage, which partial frames kept with 64 of 117 tiles. Per-camera masters agree on a large aperture over the M 33 nucleus within +4 % (ASI1600/ASI2600), +9 % (ASI2600/Seestar) and +12 % (ASI1600/Seestar) of the star-based scale, and star apertures ASI2600/Seestar within −3 %.

## ADR-109: Experimental partial frames per processing tile (2026-11-01)

**Status:** Owner-requested experiment, off by default. Local, not published. Awaits manual review.

**Decision.** With "Keep accurate parts of refused frames" (CLI `--partial-frames`, requires geometric correction), a frame that fails whole-field validation is not rejected when it has at least 50 witness stars. It keeps its best fit, the scale fit if one exists. For each processing tile the frame joins only if witness stars around the tile pass the whole-field limits: RMS at most 1 px, maximum at most 3 px, at least 5 stars. A 512 px tile alone holds about one star of a registration list, so the window reaches 768 px beyond the tile. The decision follows the processing tiles by the owner's choice, so the master depends on tile size. External geometry references switch the option off, because witness errors are in anchor pixels. Reports give `partial_field` with witnesses and tiles per frame; the console and Results give the sum.

**Evidence (Windows 11, release, the tester's 166 frames, geometric correction, 512 px tiles).** All 6 refused frames joined, 1455 of 1482 tiles (98 %), and mean coverage rose from 95.4 to 99.0 %. Master noise fell from 6.93 to 6.82 ADU (−1.6 %), and eccentricity rose from 0.432 to 0.443, HFR from 1.51 to 1.52 px. Steps between neighbouring tiles in the master change: median 0.017, maximum 0.19 ADU, far below the noise. 289.5 s against 294.6 s, one run each. A synthetic gate covers accurate tiles, inaccurate tiles, too few stars and one star beyond 3 px.

**Limits.** On this set the tile test passes nearly everything that the whole-field test refused, and the small noise gain costs some star shape, so the option is not recommended for it. Stars in the window include those used for the fit, so the test is weaker than held-out validation. `NCOMBINE`, frame counts and total exposure count a partial frame as a whole frame. One set was run, and it is not a case with many refused frames. That no other stacking program offers this was not checked.

## ADR-108: Flux-ranked registration lists, and candidates measured by their own map (2026-11-01)

**Status:** Owner decision on 2026-11-01. Local, not published. Supersedes the 1000-star catalog of ADR-107; its Auto reference rules stay.

**Decision: dense fields.** From 20000 detected stars a frame registers on two lists of up to 240 stars, built from connected bright regions of the detection grid and ranked by total flux, which does not depend on seeing. The first holds the brightest stars with saturated ones as positions, for geometry. The second holds unsaturated stars from a fixed flux rank in each 4x4 cell, measured precisely, for photometry; a rank tied to the faintest saturated star moved with seeing and left too few shared stars. Quality metrics still come from the ordinary 250 stars. The anchor's precise witnesses come from the same flux ranking, up to 2000 stars.

**Decision: validation.** A scale or cubic fallback used to be judged with witnesses and coverage found through the rigid map, so a plate-scale change of 0.05 % put edge stars outside the 3 px window and they counted as missing. Each candidate now finds its own witnesses in the same window and its own coverage, and a star outside its fit counts as held out. The limits are unchanged, and coverage of 80 % still decides, so a wrong map cannot pass by choosing its witnesses. This refines the fixed-identity rule of ADR-066. A frame given a wrong starting pose may now be corrected and accepted when its stars truly land on the catalog. The reason a fallback failed is recorded as `fallback_failure`.

**Evidence (Windows 11, release, the tester's 166 frames).**

| Run | ADR-107 | Now |
|---|---|---|
| Shift and rotation | 166/166, registration 46 s | 166/166, registration 21 s |
| Geometric correction on (application default) | 126/166 | 160/166 |
| Whole-field RMSE, median | 0.34 px | 0.13 px |

Master with geometric correction: 160 frames, HFR 1.51 px, eccentricity 0.432, noise 6.93 ADU, 304 s, peak RAM 8.4 GB, against 126 frames, HFR 1.60 px, 0.424 and 7.42 ADU. Shared flux-measured stars per frame: median 133, minimum 34. Six frames stay out, five on absolute accuracy and one on the outer field; their fallbacks failed on outer-field accuracy. Controls with geometric correction against the alpha build: ASI2600 M 33 74 → 73/81 frames, HFR 1.39 → 1.38 px; Dwarf3 sample 123 → 124/157, HFR 1.48 → 1.45 px, noise about 9 % lower; the alpha build also lacks ADR-102 to ADR-107. Gates: fmt and clippy default and all features PASS; 245/245 and 249/249 tests with real FITS, AVX2 and GPU, with the misregistration gate tightened to require that an accepted frame's stars land on the catalog and that stars of another field stay refused.

**Limits.** One dense set was run. Saturated positions were checked on the ASI6200, which has no blooming; a blooming sensor is unverified. The band rank assumes at least 4000 bright stars; sparser cells start at half their stars. Which frame ASI2600 M 33 lost, and why, was not examined.

## ADR-107: Longer star catalog in dense fields, and Auto reference that continues (2026-11-01)

**Status:** Owner-requested on 2026-11-01 after a tester's ASI6200 set stopped with "no rectangle covered by 80%". Local, not published.

**Problem.** The 166 frames (LBN 437, three nights, 36000 to 53000 detected stars, about 3000 saturated per frame) all passed quality, yet 38 failed registration, among them the whole first night. Frames carry the 250 brightest unsaturated stars, and with thousands of eligible stars that list changes with seeing: neighbouring frames shared 81 of 250, different nights 8 to 19. Auto reference then required coverage by 80 % of all 166 frames, which the 128 mapped frames could not reach, and stopped.

**Decision.** From 20000 detected stars a frame keeps a catalog of 1000 stars. Its first 250 are the ordinary catalog, and quality metrics and reports use only those, so HFR, weights and acceptance do not change; registration and photometry use all of it. A frame below that count takes the unchanged path. Saturated stars stay out, because they carry no photometry: a flux-ranked list with saturated stars shared more than 90 % in an offline test, but 1000 unsaturated stars proved sufficient and need no second list. Auto reference counts its 80 % over mapped frames. A frame or whole night that stays unmapped is reported as a warning in the console and Results, with `unmapped_frames` and `unmapped_nights`, and the rest stacks. Scattered framing with no common rectangle keeps the preliminary reference.

**Evidence (Windows 11, release, the tester's 166 frames).**

| Run | Before | Now |
|---|---|---|
| Shift and rotation | 128/166, first night 0/27 | 166/166, all nights; matched stars median 54 to 576 |
| Geometric correction on (application default) | stopped; chronological reference 64/166 | 126/166 |

Masters: geometric correction on, 126 frames, HFR 1.60 px, eccentricity 0.424, noise 7.42 ADU, 234.5 s, peak RAM 7.9 GB; off, 166 frames, HFR 1.65 px, eccentricity 0.544, noise 7.50 ADU, 271.2 s, peak RAM 7.4 GB. Quality metrics of the frames are unchanged. Registration of 166 frames with Auto takes 77 s against 32 s. A synthetic gate covers the continuation with one unmapped night. Gates: fmt and clippy default and all features PASS; 245/245 and 249/249 tests with real FITS, AVX2 and GPU.

**Limits.** With geometric correction on, 40 frames still fail whole-field validation, 32 of them for insufficient evidence, so the corrected master leaves them out, yet it has rounder stars and lower noise than the master of all 166. The threshold rests on this set and on stored sets of up to 30000 stars; the isolated ASI2600 M 33 frame above it takes the dense path. Reports list only the first 250 stars, so `matches` may point past the reported list. No other dense set was run.

## ADR-106: Saturation level of sensors below 16 bits (2026-11-01)

**Status:** Owner-requested on 2026-11-01. Local, not published. Supersedes the saturation limit in ADR-105.

**Problem.** Saturated stars were recognized from 62000 ADU and saturated samples in calibration from 65000 ADU. Data ending at 16383, as from a 14-bit camera converted by Siril, never reached either limit, so clipped stars entered the catalog and photometry, and calibration moved clipped samples.

**Decision.** Inspection reads each integer frame's level before calibration: when at least 16 samples share the frame's top value and that value is below 62000, the value is the sensor's clipping point, recorded as `saturation_adu` in the report. The fixed limits keep their share of it: stars from 94.6 %, calibration from 99.2 %, and clipped samples stay at that level. A frame with no pile has no clipped sample to protect. The stacked master uses the highest level of its frames for its star measurement. Float frames have no ties to read and keep the fixed limits, and data from a cropped or scaled sensor reads whatever level it shows.

**Evidence (Windows 11, release).** Twelve EOS R8 frames via Siril: 12/12 frames report 16383, the brightest catalog star peaks at 15479 ADU, stacked in 11.5 s, peak RAM 2.3 GB. Three frames each from Seestar S50, ASI2600MC, ASI071 and ASI2600MM top out at 65528 to 65535 and the rule does not fire, so 16-bit behaviour is unchanged. New gates cover the detection, the star catalog and calibration. Gates: fmt and clippy default and all features PASS; 244/244 and 248/248 tests with real FITS, AVX2 and GPU.

**Limits.** Output keeps the source ADU / 65535 scale, so a 14-bit master ends near 0.25; this is linear and only needs a stronger stretch. A camera with a different clip per colour channel is limited by the highest one. The R8 master's HFR stays below the frames' (1.07 against 1.16 px), so saturated stars were not the cause, and it remains unexplained.

## ADR-105: BOTTOM-UP FITS frames are read top-down (2026-11-01)

**Status:** Owner-requested on 2026-11-01, so that frames converted by Siril stack correctly. Local, not published.

**Problem.** Lights ignored `ROWORDER`. Siril stores rows bottom-up and writes BAYERPAT for the top-down image, so on an even height every colour site was misread, without any warning. The master was also upside down.

**Decision.** A frame with `ROWORDER = 'BOTTOM-UP'` is read in top-down order: whole frames and tiles reverse their rows at the reader, so BAYERPAT holds and calibration, previews, registration and the master see the true image. FITS master files follow the same rule. Masters stay `ROWORDER = 'TOP-DOWN'`. A missing or `TOP-DOWN` card changes nothing, and the block cache is bypassed for these frames. XISF rows are always top-down by specification.

**Evidence (Windows 11, release).** A synthetic gate stacks the same six frames stored both ways to identical master pixels. Twelve EOS R8 frames converted by Siril 1.4.5: 12/12 in 11.9 s, peak RAM 2.3 GB; master noise R 14.87 / G 8.34 / B 5.71 ADU against about 10 ADU in every channel with the misread mask; the master correlates 0.84 with the frame in reversed rows and −0.16 in stored rows. Gates: fmt and clippy default and all features PASS; 243/243 and 247/247 tests with real FITS, AVX2 and GPU.

**Limits.** Evidence for this writer is Siril on one camera. Saturation levels stay fixed at 62000 and 65000 ADU, so 14-bit data ending at 16383 is not recognized as saturated. That master's HFR is below the frames' (1.07 against 1.17 px); it also appears with the correct pattern forced by hand, so the row order is not its cause, and the cause is not explained.

## ADR-104: XISF input, FITS and XISF only (2026-11-01)

**Status:** Owner decision on 2026-11-01: input stays FITS and XISF; other formats, such as camera RAW, are converted outside FulgurStack. Local, not published. Application dialogs await manual review.

**Scope.** XISF has one format version, 1.0, with specification revisions 1.00, 1.01 and 1.02. The reader covers monolithic `.xisf` and distributed `.xish` units with `.xisb` block files; attached, embedded, `path(...)`, `@header_dir` and local `url(file:...)` blocks; zlib, LZ4, LZ4HC and Zstandard with and without byte shuffling, compression subblocks and uncompressed subblocks; SHA-1, SHA-256, SHA-512, SHA3-256 and SHA3-512 checksums; UInt8 to UInt64, Float32 and Float64; planar and normal storage; both byte orders; `bounds`, `offset`, several images per unit, `Reference` elements and detached signatures. Remote `http(s)` blocks are refused, so stacking never touches the network. Complex, CIELab, three-dimensional and non-Bayer colour filter images are unavailable, and the rest of a unit stays readable, as the specification requires. New dependencies are pure Rust.

**Decision.** Samples are presented in the pipeline's units: 16-bit data unchanged, other formats mapped onto 0..65535 through their bounds. FITSKeyword elements and standard properties become the same private cards as a FITS header, so metadata, reconstruction of missing values, location privacy and master headers share one code path. The FITS path is unchanged. Uncompressed blocks are read by rows as FITS is. Compressed blocks have no random access, so free memory holds a band of decoded rows per frame, and whole-frame decoding is limited by the same budget; with too little memory each tile decodes again, slower but bounded.

**Debayered frames.** Three-plane frames form a new layout, `Rgb`: no calibration, cosmetic correction or demosaicing. Green is the measurement plane, sky is measured per plane, and the master records `DEBAYER = 'EXTERNAL'`. Calibration refuses them, because bias, darks and flats act on sensor samples. Full GPU reconstruction refuses them; GPU pixel rejection works.

**Evidence (Windows 11, release).** All 2128 XISF units below `C:\Astrofoto` open, written by PixInsight 01.08.04 to 1.9.4: 734 mono, 4 Bayer, 1390 RGB. The sampled units are uncompressed and unsigned, so compression has synthetic and converted evidence only. 30 Seestar frames rewritten as XISF, plain, zlib+sh, and big-endian with SHA-1: master pixels and coverage identical to the FITS stack. Real stacks: ASI2600MC `dc_` Float32 RGB 33/33 in 48.8 s, peak RAM 3.5 GB; NGC 6888 `dLight` 8/12 in 12.2 s; Seestar PixInsight-debayered sample 27/30 in 8.8 s; mono `rc_` UInt16 24/25 in 13.7 s; the 2017 `n_r_c_` Red set 11/11 in 5.0 s with its exposure entered. Synthetic gates cover each codec, shuffling, subblocks, checksums, damaged units and distributed units. Gates: fmt and clippy default and all features PASS; 242/242 and 246/246 tests with real FITS, AVX2 and GPU.

**Limits.** Frames already registered by another program are read like any other: FulgurStack registers them again, which adds a second interpolation, and their zero borders are ordinary samples rather than missing data. Files without any exposure, such as the 2017 `n_r_c_` set, need it entered. Float FITS lights are still not accepted. No third-party compressed or distributed XISF was available, so those paths rest on units written from the specification.

## ADR-103: Geometric correction on by default in the application (2026-11-01)

**Status:** Owner decision on 2026-11-01. Local, not published. Awaits manual review.

**Decision.** "Correct geometric distortion" moves from Advanced to the first Settings card, renamed "Registration geometry", and is on by default in the application. This supersedes the default-off part of ADR-065. Settings version 1 stored the old default rather than a choice, so such a file is switched on once and saved as version 2; turning the option off later is kept. An older build refuses a version 2 settings file. The CLI keeps `--geometric-correction` as an opt-in flag, so scripted runs do not change.

**Consequences.** With the option on, every frame must pass whole-field validation, so a default run can stack fewer frames than before and takes longer. Measured so far: +66 % on one small paired observation (ADR-065). The engine is unchanged; only the application's default differs.

## ADR-102: Sky offset per colour channel, and visible normalization (2026-11-01)

**Status:** Owner-requested on 2026-11-01 after the first alpha testers looked for additive with scaling. Local, not published. Application text awaits manual review.

**Context.** Frames were already normalized before rejection with `y = a·x + b`, which is additive with scaling. Nothing in the application said so, and `b` came from the green detection plane and applied to all colours. Red and blue sky therefore kept a difference between frames: on ASI2600 M 33 up to 0.90 and 1.94 pixel-noise sigmas. Those differences inflated the spread that rejection measures, so it hardly acted in those channels.

**Decision.** Inspection measures the sky of each CFA colour with the frame sky's own cells and robust location. Green stays the detection level, so green stays exact. The offset is `b[c] = BG_ref[c] − a·BG_i[c]`. The scale stays one star-flux value for all channels, which keeps star colours (PROJECT_SPEC §4.4 amendment). Split masters rebase per channel too. Mono frames have no channel sky and keep the single offset. Settings states the normalization, Results shows its range, reports carry `normalization` and `channel_offsets_adu`, and the registration CSV gains three offset columns at its end.

**Evidence (Windows 11, release; control is the alpha build e2fb4fd on the same data, unless noted).**

- ASI2600 M 33 colour, 75/81 frames both. High-rejected share R 0.042 → 0.149 %, G 0.190 % unchanged, B 0.015 → 0.176 %. All channels now behave alike. Total rejected samples rise 13.6 → 16.7 M: low 8.4 → 6.0 M, high 5.2 → 10.8 M, because rejection now sees real outliers in red and blue. Master noise B 4.78 → 4.69 ADU, R and G unchanged. The green plane does not change. Master sky follows the reference frame: R −7, B +39 ADU.
- ASI071 colour, 85/86 frames, 60 s and 300 s, control is the ADR-101 build. Rejected 36.9 → 33.7 M. Noise R 2.86 → 2.88, G 2.37, B 3.20 → 3.19 ADU.
- Mono IC 1805, 22/22 frames: master and coverage map byte-identical to the control.
- Time 53.5 s against 49.1 s on ASI2600, one run each, so the difference is not a measured cost.
- Gates: fmt and clippy default and all features PASS; 226/226 and 230/230 tests with real FITS, AVX2 and GPU. New synthetic gate: six frames differing only in red and blue sky lose under 2 % of samples and get the expected offsets.

**Limits.** The DS-A reference stack was not rerun, so its effect on Seestar sets is known only from the sky measurement (residuals up to 0.31 sigma). One scale still serves all colours, so colour-dependent extinction stays uncorrected; it has not been measured. Gradients that differ between frames need local normalization, which is not implemented. The application text was release-compiled, not reviewed in the window.

**Related finding, no change made.** A tester's mono ASI6200 report (19/20 frames) shows 2.8 % rejected samples with winsorized 3/2.5, close to the 2.5 % that noise alone gives at these limits in a model of 19 frames. Mono IC 1805 with 22 frames: winsorized 3/2.5 rejects 43.6 M at noise 5.49 ADU, winsorized 4/3 rejects 18.3 M at 5.31, ESD rejects 10.0 M at 5.28 in 14.7 s against 11.1 s. Auto uses winsorized below 25 frames; that threshold awaits the owner's decision.

## ADR-101: One registration, separate masters (2026-10-31)

**Status:** Owner-requested on 2026-10-31. Local, not published. The application screens await manual review.

**Decision.** A run can end in one master (default), one master per filter or one master per light set: CLI `--masters combined|per-filter|per-set`, Settings > Output > Masters. Registration, selection and the output grid stay shared, so all masters of a run lie on one pixel grid and need no second alignment or interpolation. Rejection and weighting happen inside each master, so real nebula differences between filters are never treated as outliers. Sets without a FILTER card stay apart under per-filter. A split that yields fewer than two masters is the plain combined run and stays byte-identical.

**Photometry.** A combined run scales every frame onto the session anchor, which would make a filter's brightness depend on another filter's frames. Each master is therefore rebased onto its own frame of middle scale: `a' = a / a_anchor`, `offset' = BG_anchor − a'·BG_i`, weights divided by the group's largest. Scale and sky come from the existing inspection, so no pixel is read twice for this. The master's FITS header comes from that anchor, so FILTER, EXPTIME and GPS provenance belong to the group. Coverage and high-rejection maps use the group's input frames as their denominator. Masters are built one after another, so peak memory stays that of one integration.

**Names.** Each master adds its label: `<master>_<label>.fits`, `<master>_<label>_coverage.fits` and so on in the application, `<map stem>_<label>.fits` in the CLI. Labels are reduced to file-safe characters and made unique without regard to case. All new names are chosen before the first master is integrated. The application takes the next free `_vN` and the CLI refuses an existing file, as for one master. Each master has its own report with a `master_group` block. Results shows one master at a time with a picker.

**Evidence (Windows 11, release, ASI2600MM M 33, geometric correction on).** Ha 57 and OIII 39 frames, `--masters per-set`: 84/96 accepted (Ha 46/57, OIII 38/39), 81 s, peak RAM 2982 MiB. Star positions of the two masters agree with median offsets −0.016 / −0.009 px and median radial difference 0.085 px over 371 stars. Signal-to-noise of the Ha master against Ha stacked alone: ratio 1.006. Star flux and noise differ by the same 1.47× because a different anchor frame sets the units, which is the behaviour of any stack. Synthetic gates: star flux ratio 3 is kept between two filters on one grid (CLI), and one run in the application's worker publishes two complete masters and switches between them.

**Limits.** These two sets carry no FILTER card, so the real run used per-set; per-filter grouping has only synthetic evidence. The picker and Settings control have not been reviewed in the window. Earlier masters of a split run write within the Integrate phase of the Processing table. A stop keeps masters already finished, as for any final file, and lists them. The linear channel mix is the next planned step and is not part of this decision.

## ADR-100: Consolidated main publication of ADR-091–ADR-099 and the alpha package (2026-10-31)

**Status:** Owner-authorized on 2026-10-31 for this batch only. The owner considers the application ready for the first alpha testers.

**Scope.** One commit to `main` with `[skip ci]`, holding ADR-091–ADR-099 and their documentation, and one Windows package built from that commit, which also carries the Czech guide for alpha testers. Actions configuration is unchanged. Images, outputs, preferences, raw telemetry and binaries are excluded. The package stays local.

**Verification before publication (Windows 11, release, local).** `cargo fmt --check` and clippy, default and all features: PASS. `cargo test --workspace --release`: 218/218 default and 222/222 with all features, including real FITS, AVX2 and GPU. Real runs: Seestar DS-B 27/32 frames, ASI2600MC 75/81 with bias, darks and flats, ASI2600MM Ha 23/30 with all four kinds.

**Limits.** Screens, drag and drop and window memory were reviewed by the owner in part and otherwise await manual acceptance. ADR-092 drag and drop, ADR-097 previews and ADR-098 plots were checked only by the owner's use and by release compilation, without automated UI tests. `wall_seconds` of calibrated runs now includes the calibration masters (ADR-096). No other Windows version, machine or discrete GPU was tried.

## ADR-099: Scripted guide screenshots (2026-10-31)

**Status:** Owner-requested on 2026-10-31 for the alpha testers' guide. Local, not published.

**Decision.** `fulgurstack-gui --ui-guide <output-folder> <prefix> <measure|stack> <lights|saved> [calibration folders]` runs a real measurement and stack and captures the application's own window, so guide screenshots need no desktop control. Like the UI smoke modes it never writes settings; the `saved` form reads the saved workspace and takes whatever data it names, so normal use passes explicit folders. The Results sidebar widens from 280 to 340 points, because its button row and noise line were cut off.

**Dark flats written as darks.** Sorting a calibration folder by headers put dark flats whose IMAGETYP says "Dark" among the darks: an ASI2600MM set gave 121 darks of 1 s and 300 s, and the master dark mixed them. A dark now counts as a dark flat when its file or folder name says so, or when the scan holds flats and darks of two exposures and its exposure matches the flats (within 1 %). The same set now gives 21 darks of 300 s and 100 dark flats, with no warning. Darks that match the flats and stand alone stay darks, because nothing shows otherwise.

## ADR-098: Plots open frames, and the window is remembered (2026-10-31)

**Status:** Owner-requested on 2026-10-31. Local, release-compiled and awaiting manual review. Display only.

**Decision.** At the default window size the frame list fell below the plots. The toolbar above the list now has a ▼/▶ Plots button, and that choice is remembered. Measuring no longer reopens hidden plots. A click in the plots selects the frame, scrolls the list to it and opens its preview. The main window's size, position and maximized state are saved in `settings.json` and restored at start; a position whose title bar is on no monitor is dropped and the window is centred, so an unplugged screen cannot strand it. UI smoke runs ignore the saved place.

The Processing page now gives the phase table a fixed 590-point column, so that its wider rows no longer overlap the This run card. Below about 920 points the two cards stack.

## ADR-097: Previews stay at full quality at every zoom (2026-10-31)

**Status:** Owner-requested on 2026-10-31. Local, release-compiled and awaiting manual review. Display only.

**Why.** Overviews had 640 pixels on the long edge and took every Nth source pixel, so below about 40 % zoom an image was enlarged from a coarse sample. Every pan outside the view, and every change of frame, dropped the native detail and fell back to that overview for a moment.

**Decision.**
- Overviews have 2048 pixels on the long edge and average every source pixel. This replaces the 1920-pixel map overview of ADR-095. They shrink through mipmaps.
- Native detail starts at about 37 % on a 6K sensor. The read limit rises from 8 to 16 million pixels, and shrunken detail is averaged instead of sampled.
- Detail is read 1.4× wider than the view, so moving inside that area needs no read. The last detail stays on screen until the next one is ready, also across frames of the same size, and a new frame's detail starts at once instead of after its overview.
- Preview tasks use up to 8 workers, half of the logical processors, instead of one. They never run alongside integration.
- Auto stretch still comes from unaveraged samples, so its look does not change and it fits native detail.

**Cost.** Each overview holds about 40 MB and each detail read up to about 200 MB, briefly. Raw CFA keeps its pixel sampling, because a raw view must not blend the Bayer positions.

## ADR-096: Calibration masters are a phase, and processing time counts every phase (2026-10-31)

**Status:** Owner-requested on 2026-10-31. Local, release-compiled and awaiting manual review. The stacked pixels are unchanged.

**Decision.** Building the calibration masters ran before Inspect with no row in the phase table, and its time was left out of the processing time. It is now the first phase, "Calibration masters", marked "Not used" when a run has no calibration. The elapsed clock, the Results page and the top bar now count every phase, including the calibration masters and inspection of an earlier Measure quality run whose results a stack reuses. Quality-review waiting stays excluded.

**Report.** `wall_seconds` is this run and now includes the calibration masters. `total_processing_seconds` adds `earlier_measurement_seconds`; `calibration_masters_seconds` is the first phase. Comparisons with older reports use `wall_seconds` of uncalibrated runs, whose meaning is unchanged.

## ADR-095: Readable rejection overview and calibration details on Results (2026-10-31)

**Status:** Owner-requested on 2026-10-31. Local, release-compiled and awaiting manual review. Display only: the saved map and the report are unchanged.

**Rejection overview.** The overview took the peak of each bin, to keep rare peaks visible. On a real 81-frame map 11.5 % of the pixels are non-zero, nearly all from one frame, so 99.8 % of a 640-pixel overview exceeded a fifth of the display range and the real trails and star cores vanished. The overview now means every sample at 1920 pixels on the long edge. On the same map 0.4 % of it exceeds that level and the trails stay visible. Single one-frame rejections fade below the display range, as intended for an overview; they appear in the native view. The full-map statistics above the image are unaffected. Non-stretched views no longer compute stretch parameters for each detail load.

**Calibration details.** The Results sidebar lists, per light set, the frames calibrated, the formula, the pedestal, each master (source, combine method, exposure, gain, temperature, median), the flat's offset and per-position level, repaired dark defects, samples below zero and all warnings.

## ADR-094: Flat views without the Bayer pattern (2026-10-31)

**Status:** Owner-approved on 2026-10-31 after a prototype on real flats. Local, release-compiled and awaiting manual review. Display only: nothing reaches calibration or a file.

**Why.** A one-shot-colour flat stored its channels at about 37,900, 31,000 and 15,350 ADU, and the preview sampled every Nth pixel. The Bayer pattern therefore showed as a beat pattern of about 35 % at most zoom levels and hid vignetting and dust shadows of about 1 %. The auto stretch for the night sky does not suit a flat either.

**Decision.** Flats and master flats get three views: **Vignetting**, **Dust** and **Raw CFA**. Colour flats first average each 2 x 2 cell. Every cell holds all four Bayer positions, so the pattern cannot survive and no knowledge of the layout is needed. Mono flats skip this step and keep full size. Vignetting maps the 0.2–99.8 % range of the averaged field. Dust divides the flat by its own large-scale shape (three box blurs on a coarse image, about 8 % of the long edge) and shows a fixed ±3 %, so frames can be compared. Overviews average every source pixel instead of sampling. Bias, darks and dark flats keep the pixel view, because averaging would hide hot pixels.

**Consequence.** The views are half size for colour flats, so 1:1 shows one 2 x 2 cell per screen pixel and the zoom percentage refers to that image. Single pixels are checked in Raw CFA.

## ADR-093: A different target name only warns (2026-10-31)

**Status:** Owner decision on 2026-10-31. This amends the first-release input scope in `AGENTS.md`. Local, not published.

**Why.** On an 81-frame M 33 set, 29 frames from the second night had `OBJECT = "M 33 b"` and were rejected as `incompatible_target_filter_or_sensor` before any quality check, although they show the same sky. The name in the header is a label typed in the capture program, not evidence about the field.

**Decision.** The OBJECT name no longer rejects a frame inside a set, between sets or in registration, and no longer skips the geometric correction. Star matching decides: a frame of another sky field finds no match and fails registration, so it does not enter the master. The inspection report lists `target_names`. Several names show a warning in the set overview, in the status bar after measuring and on the CLI. The warning does not stop a run.

**Consequences.** The quality limits of a set now use the frames of every name together. A truly different target in the same set therefore shifts those limits before it fails registration; the warning names it. `compatible` still separates filter, gain, sensor size and mosaic inside one set.

**Unchanged.** An external registration reference with another OBJECT is still refused. It is one chosen file, so a mismatch points to a wrong file rather than to a naming habit. The `incompatible_target_filter_or_sensor` reason keeps its name for reports.

## ADR-092: Calibration folders, single-frame review and drag and drop into sets (2026-10-31)

**Status:** Owner-requested on 2026-10-31. Local, release-compiled and awaiting manual review.

**Folders.** Each calibration card can add folders as well as files. A folder adds every FITS file inside it, subfolders included. The list is expanded when the folder is chosen and saved as individual frames, so the run and the settings format are unchanged.

**Single-frame review.** Next to the master preview, the preview pane shows the frames of a kind one by one in the auto-stretched CFA plane, with file name, exposure, gain, temperature, median, minimum and maximum. Arrow keys or buttons step through them with the zoom kept. "Remove from set" leaves a frame out of the master; the file stays on disk and the master in memory is dropped. Frames are read one at a time, so stepping adds one plane to RAM. Stepping faster than the disk skips frames instead of queueing reads.

**Drag and drop.** Dropped folders and files land by pointer position: a light set (rail or its open view) gains them as sources, a calibration card takes them as frames of that kind, a calibration set (rail or open view) sorts them by FITS headers, the calibration part of the rail makes a new calibration set, and anywhere else makes a new light set as before. When exactly one file lands on a calibration card, a dialog asks whether it is a master or a single frame, because only the owner can say for sure. Frames join frames of the same kind; a master replaces what the kind had. The target is outlined during the drag. winit 0.30 reports no pointer position during a file drag, so on Windows the position comes from `GetCursorPos`; other platforms fall back to the last known pointer position and are unverified.

**After Measure quality.** When measuring ends, or a stack waits for review, the workspace opens the whole frame list with its plot and preview, scrolled to the registration reference with keyboard focus on that row. A column filter that hides the reference is cleared.

## ADR-091: Derived values are confirmed in a start warning (2026-10-31)

**Status:** Owner-requested on 2026-10-31. Local, release-compiled and awaiting manual review.

**Decision.** Values derived for missing FITS header cards no longer block the primary action through a checkbox in the light-set details, which the owner found hidden. Measure quality and Stack now open a modal warning that lists the derived values of every included set, with **Return** and **Start Anyway**. Start Anyway is the owner's confirmation under ADR-084: it sets the same per-set confirmation and the run receives `accept_derived` exactly as before. Return, Esc or a click outside opens the first affected set for correction.

**Unchanged.** A missing exposure or an undeclared colour layout still blocks the start, because there is no value to use. The confirmation still resets when the derived values change and is not kept between sessions. The CLI still requires `--accept-derived`. The engine is untouched.

## ADR-090: GPU pixel rejection; coverage crop out of scope; weight plot (2026-10-30)

**Status:** Owner-requested on 2026-10-30. The request had three parts: drop the crop, which is done later during post-processing; add the weight plot; and prepare and test an optimization that uses GPU computing power. Published to `main` with the owner's authorization of 2026-10-30. M5 remains open.

**Coverage crop preview removed from scope.** The M5 deliverable "coverage crop preview" is struck from the specification. Masters keep the full reference canvas and are cropped during post-processing. The explicit CLI `--canvas reference|coverage-optimized|common-intersection` policies stay.

**Weight plot.** The Frames quality plot has a fourth lane, "Weight", next to HFR, eccentricity and sky. Like the others it follows the list order and filter. The change is UI only and was release-compiled for manual review.

**GPU pixel rejection.**
- **Why.** Measured before this work, the first-slice GPU reconstruction (cosmetics and VNG) was 2–3× slower than the AVX2 CPU path on the integrated Radeon 890M (DS-B 14.2 vs 5.6 s, Flaming Star 42.4 vs 21.3 s). Pixel rejection and the weighted mean dominated integration and ran only on the CPU (Dwarf3: 94.8 of 173.3 s).
- **What.**
  - **Kernel.** A new WGSL reducer gives one workgroup to each output pixel channel. It bitonic-sorts the samples of all frames in workgroup memory, applies the ADR-087 method and sums the survivors' weights.
  - **Method details.** All four methods keep their CPU thresholds and minimum-sample rules. ESD critical values come from the CPU in f64, tabulated per sample count. Winsorized median and MAD use binary searches and a two-list k-th selection on the sorted samples.
  - **Sums.** f32 Kahan, around the sample median.
  - **Survivors.** Linear fit marks rejections per sorted position. The other methods keep a value range, so their sort moves keys only and the survivors are summed from the unsorted run; samples tied exactly at a boundary value are then all kept.
- **Data path.** Each pixel chunk goes up as one contiguous run of all frames per pixel channel:
  - four CPU threads write it through a cache-resident block;
  - on an integrated GPU, straight into mapped device memory (`MAPPABLE_PRIMARY_BUFFERS`), so there is no staging copy;
  - two upload slots alternate.
- **Overlap.** The device reduces tile k on a worker thread while the CPU reconstructs tile k + 1. The tile memory plan counts two tiles of samples plus the device buffers, which are system memory on an integrated GPU.
- **Interfaces.**
  - `--engine gpu` now means CPU reconstruction with GPU rejection, the fastest measured split.
  - `--gpu-stages reconstruction|all` selects the first-slice GPU reconstruction, alone or with GPU rejection.
  - GPU rejection also handles monochrome stacks. GPU reconstruction stays colour-only.
  - The CPU thread cap stays 24 unless GPU reconstruction is selected.
  - The GUI engine choice "GPU" uses the default split.
  - Reports name the split in `integration.backend` and carry `integration.gpu_reduction` telemetry. With GPU rejection, `reduction_wall_seconds` is the wait not hidden behind reconstruction.
  - No silent fallback: a stack larger than the device's workgroup sort fails with a request to use the CPU.

**Results (Windows 11, release, Radeon 890M, ESD).**

| Stack | CPU integration / total s | GPU integration / total s |
| --- | --- | --- |
| DS-B, 27 frames | 5.8 / 7.5 | 3.7 / 5.5 |
| Flaming Star, 30 frames | 19.7 / 25.8 | 12.9 / 20.5 |
| M42, 235 frames | 153.9 / 182.9 | 103.8 / 133.8 |
| Dwarf3, 856 frames | 173.3 / 230.2 | 115.7 / 180.9 |

- **Agreement.** Masters agree with the CPU at normalized RMS ≤ 9.7e-8 and maximum ≤ 4.3e-4, within the pre-declared T-NUM-01 limits of 1e-5 and 1e-3. Coverage and high-rejection maps differ in 8–41 pixels per set. There, a sample at a rejection threshold in f32 against f64 changes the ESD outcome; these differences are reported as T-NUM-01 requires. Rejected sample totals agree within 24 samples per set, and master noise is identical.
- **Trade-off.** Reconstruction slows while the GPU works (Dwarf3: 78 → 103 s), because the integrated GPU, the upload threads and the reconstruction workers share one memory bus. Dwarf3 peak RAM grows from 3.05 to 5.90 GiB for the second tile of samples.
- **Unchanged.** The CPU engine is byte-identical (DS-B master and maps).

**Verification.**
- fmt and clippy (default and all features) PASS; `cargo test --workspace --release` 213/213 default and 217/217 with all features (real FITS, SIMD and GPU variables).
- Hardware gate (FULGURSTACK_TEST_GPU=1): GPU and CPU reducers compared on random stacks with noise, outliers, invalid samples and unequal weights, with 1–856 frames and all methods.
- The existing GPU reconstruction gates run with `--gpu-stages reconstruction`.
- Shader validation for every method and sort size, plus packing and memory-plan unit tests.
- See the [review](M5_GPU_REDUCTION_REVIEW.md) and [evidence](M5_GPU_REDUCTION_EVIDENCE.json).
- Only the integrated Radeon 890M was tested. Discrete GPUs (staging uploads) and Metal are untested. The full DS-A reference run was not repeated.

## ADR-089: Portable Windows distribution package (2026-10-30)

**Status:** Owner-requested ("prepare a new distribution file for Windows"). Local; the package is not published. M5 remains open.

**Decision.** Distribute FulgurStack for Windows as a portable ZIP rather than an installer:
- `fulgurstack-gui.exe` and `fulgurstack.exe`, both with the GPU feature compiled in;
- an owner-language guide `CTI_ME.txt` with news, requirements, privacy notes, quick start and SHA-256 of both executables.

The executables are built with `RUSTFLAGS="-C target-feature=+crt-static"` into a separate `target/dist` directory. The Visual C++ runtime is therefore linked in, and the standard `target/release` build, the desktop shortcut and incremental builds stay untouched. Static linking adds no C/C++ library dependency and does not change the computation.

**Checks per package:**
- PE imports contain no `VCRUNTIME*`, `MSVCP*` or `api-ms-win-crt-*` DLLs;
- a DS-B stack with the packaged CLI gives a master, coverage map and high-rejection map byte-identical to the standard release build;
- the packaged GUI starts.

Packages live in the Git-excluded `outputs/dist/`. Their name carries the version and the commit, so a package is built only from committed sources. No code signing; SmartScreen may ask on first start.

**Current package (97caf3b):** 8.5 MB ZIP, all checks PASS on Windows 11 x64. DS-B Auto (ESD) took 8.6 s with a peak of 835 MiB. Hashes and the procedure are in [Windows distribution](WINDOWS_DISTRIBUTION.md). Other Windows versions and a machine without a toolchain are untested.

## ADR-088: Consolidated main publication of ADR-084–ADR-087 (2026-10-30)

**Status:** Explicitly authorized by the owner on 2026-10-30 ("make a commented commit and push") for this completed batch only.

Publish the completed local work since ADR-083 in one documented commit on `main`:
- reconstruction of missing light-header values with a 69-entry camera catalogue and owner confirmation (ADR-084);
- dense-field witness recheck and depth-aware Auto reference (ADR-085);
- quality metrics of the stacked master with per-channel noise (ADR-086);
- pixel rejection methods median, linear fit, generalized ESD and Auto (ADR-087);
- GUI iterations: collapsible light-set details, scrollable cards without overlaps, a wider preview divider, the derived-value confirmation button, quick frame selections (all, best 95/90/80/50 %), a three-lane quality plot in list order, and the Settings card "Stacking and outlier rejection" (moved out of Advanced).

Engine gates were run locally on Windows 11 after ADR-087: fmt and clippy (default and all features) PASS, `cargo test --workspace --release` 210/210 default and 213/213 with all features and the real-data, SIMD and GPU variables. The later Settings card is UI only: release-compiled, clippy-clean, awaiting the owner's manual acceptance. Fast-forward only, never force-push; use `[skip ci]`; do not change, trigger or poll GitHub Actions. Exclude datasets, FITS/XISF images, outputs, raw reports, preferences and binaries. Future publishing and remote CI remain unauthorized. See [header reconstruction review](M5_HEADER_RECONSTRUCTION_REVIEW.md).

## ADR-087: Pixel rejection methods — median, linear fit, ESD and Auto (2026-10-29)

**Status:** Owner-requested ("yes, the recommended ones": median, linear fit clipping, ESD and an automatic choice by frame count). Published with ADR-088. M5 remains open.

**Methods.** Each method decides which samples of a pixel survive. Survivors are averaged with their frame weights, as before. Reduction stays on the CPU in output tiles for both engines, so GPU reconstruction works with every method and memory is unchanged.
- **Winsorized** (unchanged): median/MAD winsorized sigma clipping, low 3.0 / high 2.5 σ.
- **Median**: unweighted median of all samples. Nothing is counted as rejected.
- **Linear fit clipping**: a line is fitted to the sorted surviving samples against rank. Samples more than 5 σ below or 4 σ above it are rejected (σ = 1.2533 × mean absolute residual), iterated up to ten times.
- **Generalized ESD** (Rosner 1983): α = 0.05, up to 30% of the stack. The stack is sorted once and running sums make each step constant time. Critical values come from a Student-t quantile, computed with the incomplete beta function and bisection, and are cached per stack size.
- Stacks of fewer than 5 samples are averaged without rejection by every method except median.

**Auto (CLI and GUI default).** Median below 6 frames, winsorized below 25, ESD from 25. The first proposal used linear fit for 25–49 frames and ESD only from 50. The real-data comparison below showed ESD with the lowest master noise at 27 and 30 frames, faster than linear fit, so linear fit is an explicit choice only. `IntegrationConfig::default()` keeps winsorized, so library callers and the existing tests are unchanged. CLI `--rejection auto|winsorized|linear-fit|esd|median`, GUI Settings › Stacking and outlier rejection (a card of its own with a hint per method and the method Auto applies to the next stack; first placed under Advanced), saved with the preferences. Reports record the requested method (`config.rejection`) and the applied one with its parameters (`integration.rejection`, `integration.rejection_rule`). The CLI prints it, and Results shows it.

**Behaviour change.** With Auto, stacks of 25 or more frames now use ESD instead of winsorized clipping, so their masters differ from earlier runs. Explicit `--rejection winsorized` reproduces earlier masters byte for byte (DS-B and Flaming Star verified).

**Real data (Windows 11, release; integration time, master noise per channel and HFR from ADR-086).**

| Stack | Method | Integration s | Rejected low / high | Noise R / G / B ADU | HFR px |
| --- | --- | --- | --- | --- | --- |
| Seestar DS-B, 27 × 10 s | winsorized | 4.6 | 0.59% / 2.35% | 18.96 / 13.23 / 17.25 | 1.714 |
| | linear fit | 7.6 | 0.11% / 0.67% | 18.10 / 12.76 / 16.69 | 1.725 |
| | **ESD** | 6.8 | 0.19% / 0.52% | **18.11 / 12.74 / 16.54** | 1.717 |
| | median | 4.0 | 0 / 0 | 22.01 / 15.41 / 19.93 | 1.710 |
| ASI2600MC Flaming Star, 30 × 300 s | winsorized | 16.0 | 0.90% / 1.21% | 11.36 / 10.68 / 9.67 | 1.402 |
| | linear fit | 24.3 | 0.23% / 0.44% | 11.09 / 10.62 / 9.30 | 1.407 |
| | **ESD** | 19.7 | 0.26% / 0.19% | **11.07 / 10.45 / 9.27** | 1.398 |
| | median | 14.7 | 0 / 0 | 12.86 / 11.67 / 11.83 | 1.432 |
| ASI2600MC M42, 235 frames 30–300 s, 4 nights | winsorized | 109.1 | 0.67% / 2.54% | 0.99 / 0.92 / 0.78 | 1.643 |
| | linear fit | 239.3 | 0.86% / 2.16% | 0.99 / 0.91 / 0.78 | 1.833 |
| | **ESD** | 153.9 | 0.07% / 0.17% | **0.98 / 0.88 / 0.75** | 1.831 |
| Dwarf3 C 20, 856 × 60 s, darks | winsorized | 117.3 | 0.66% / 2.49% | 0.61 / 0.47 / 0.39 | 1.463 |
| | **ESD** | 173.3 | 0.15% / 0.40% | **0.60 / 0.47 / 0.38** | 1.472 |

Winsorized clipping at 3.0 / 2.5 σ rejects about 3% of all samples, mostly high. ESD rejects 0.2–0.7%, so more signal survives and the master noise is lowest in every set. The median costs 16–22% more noise. On M42 (mixed 30–300 s exposures), winsorized clipping also trimmed the wider star profiles of the long exposures (HFR 1.64 px). ESD and linear fit keep them (1.83 px), which matches the true mean star profile; the owner should judge the visual result. ESD costs about 40–50% more integration time than winsorized on the large stacks (M42 +45 s, Dwarf3 +56 s). Peak RAM is unchanged (≤ 3.6 GiB).

**Verification.** fmt and clippy (default and all features) PASS; `cargo test --workspace --release` 210/210 default, 213/213 all features. Synthetic tests:
- Student-t quantiles and ESD critical values match tables (NIST n = 54: λ₁ = 3.159, λ₁₀ = 3.085);
- satellite samples (3 of 43, or 1 of 60) are rejected by linear fit, ESD and winsorized, while the median ignores them;
- an even 900–1100 level spread survives linear fit with the exact weighted mean;
- pure Gaussian noise loses at most 2 of 200 samples with ESD;
- stacks under 5 samples are not clipped;
- Auto thresholds and key parsing work;
- CLI: a 3-frame stack reports Auto → median.

## ADR-086: Quality metrics of the stacked master (2026-10-29)

**Status:** Owner-requested ("add HFR/FWHM, eccentricity and sky background ADU for the final master"; then "add noise, per colour channel for colour masters"). Published with ADR-088. M5 remains open.

**Method.** After integration and before export bounding, the master is measured in RAM with the frame detector: the 2x2 detection grid, the hot-pixel guard and the 250-star catalogue. The measurement plane is the green channel of a colour master or the mono plane. Pixels with no surviving sample in any channel are filled with the green sky median, so the canvas edge never becomes a star or a gradient. The following are reported:
- star metrics: median HFR, moment FWHM and eccentricity;
- sky: the local-sky median of the measurement plane in reference-anchor ADU and on the 0..1 FITS scale (÷ 65535), plus covered-pixel medians per channel;
- per channel (R, G, B; equal values for mono): sky level and per-pixel sky noise. Noise is a robust standard deviation (MAD × 1.4826, 3σ clipped) around the local sky on the same 16×16 cell grid and sparse sampling as the frame sky, so stars and nebulae barely affect it. Cells without covered samples are ignored;
- the medians of the stacked frames in master pixels (scaled by the external-reference factor), for comparison.

The master FITS and maps are unchanged. Nothing is written to disk besides the existing report.

**Interfaces.** CLI `stack` prints a "Master quality" line, and the report carries `master_quality`. The GUI report carries the same block, and Results lists master HFR, FWHM, eccentricity (each with the frame median), sky background, sky noise per channel and the number of measured stars. A failed measurement never fails the stack (the block is then absent).

**Verification (Windows, release, local).** fmt and clippy (default and all features) PASS; `cargo test --workspace --release` 205/205 default, 208/208 all features. Synthetic tests: Gaussian stars with σ 1.6 px give FWHM 2.355σ and HFR 1.177σ within 0.4 px, round stars have eccentricity < 0.3 and an axis ratio of 2 gives 0.87 ± 0.1. The sky is recovered within 5 ADU and the uncovered border is excluded. Channel sky levels and noise follow an 0.8/1/0.6 colour cast. The CLI stack report holds the block with the master HFR close to the frame median. Real data (masters byte-identical to earlier runs; measurement 0.1–0.7 s):

| Master | HFR px (frames) | FWHM px (frames) | Eccentricity (frames) | Sky ADU | Noise ADU |
| --- | --- | --- | --- | --- | --- |
| Seestar DS-B, 27 × 10 s | 1.71 (1.78) | 3.62 (3.85) | 0.455 (0.509) | 2217 | R 18.96 / G 13.23 / B 17.25 |
| ASI2600MC Flaming Star, 30 × 300 s, calibrated | 1.40 (1.35) | 3.41 (3.34) | 0.284 (0.297) | 1399 | R 11.36 / G 10.68 / B 9.67 |
| ASI2600MM Sadr, 40 × 300 s, calibrated | 1.29 (1.29) | 3.13 (3.15) | 0.246 (0.291) | 816 | 13.04 |
| ASI2600MC M42, 235 frames 30–300 s, 4 nights | 1.64 (1.40) | 3.64 (3.29) | 0.209 (0.281) | 86 | R 0.99 / G 0.92 / B 0.78 |

The M42 master's stars are larger than the frame median because its pre-calibrated 30–300 s frames from four nights differ in focus and seeing, and long exposures widen bright stars. Noise on a stacked master is lower than in single frames, and the absolute values depend on the photometric reference frame.

## ADR-085: Dense-field witness identity and depth-aware automatic reference (2026-10-29)

**Status:** Found while analysing the owner's failed GUI stack of the ASI2600MC M42 set ("Auto reference found no rectangle covered by 80% of Use-on frames"). Published with ADR-088. M5 remains open.

**Cause 1 — misidentified accuracy witness.** With geometric correction on (the GUI default), whole-field validation pairs catalog stars within 12 px. In the M42 field one source star whose counterpart the other truncated 250-star catalog lacks was paired with a neighbour about 10 px away. Validation RMSE was 0.14 px, but the single 9.6–11.9 px error broke the 3 px maximum-error limit. As a result only 131 of 255 frames were mapped, and no 80% rectangle existed. **Fix:** an accuracy failure (not only too few witnesses, ADR-082) with passing coverage is re-assessed against the anchor's 1000 precisely measured stars with a 3 px window. The limits are unchanged: RMSE ≤ 1 px, held-out RMSE ≤ 1 px, maximum error ≤ 3 px, outer quadrants. A pose that is really wrong still fails: errors above 3 px leave too few witnesses, and smaller errors exceed the RMSE limit or trigger the ADR-074 refit.

**Cause 2 — reference depth.** The multi-night coverage estimate then chose a 300 s frame as reference. Against it, 30–60 s frames shared too few unsaturated precise stars, and the night of 2026-01-18 found no pose at all, so only 76/255 frames were mapped. The same happens without geometric correction (229/255 against that frame). **Fix:** when the chosen reference maps fewer Use-on frames in final registration than the preliminary reference did, the preliminary reference is kept. The report records `kept_preliminary_reference` and `chosen_reference_mapped_frames`. Predicted area never outweighs mapped frames.

**Verification (Windows, release, local).**

| Dataset | Before | After |
| --- | --- | --- |
| M42 with geometric correction | GUI failure; CLI 131/255 with median reference, 76/255 with Auto | 235/255 registered and stacked (92%), 127 s, 3.27 GiB |
| Dwarf3 with darks | 851/856 | 856/856 registered (preliminary reference kept) |

Flaming Star (masters) and Sadr (full calibration) masters are byte-identical, and so is Seestar DS-B. The 20 frames still refused for M42 are 300 s frames from 2026-01-21 whose catalogs cover only 64–74% of a 30 s anchor (80% rule, unchanged). They register without geometric correction. New synthetic test: a witness misidentified 8 px away is rechecked and accepted with the precise catalog, while a pose 2 px off is refused before the refit.

## ADR-084: Reconstruction of missing light-frame header values with owner confirmation (2026-10-29)

**Status:** Owner-requested ("already manually calibrated frames whose calibration did not store part of the FITS header; reconstruct missing values from other header parts, with lower priority from the file name, derive them from the camera name; research the market and keep camera parameters such as the Bayer mask in a configuration JSON; show the derived values and let the owner confirm or correct them"). Published with ADR-088. M5 remains open.

**Trigger.** ASI2600MC Air lights calibrated by another program (`c_Light_M 42_…_30.0s_Bin1_2600MC_gain100_…fit`) carry no EXPTIME, BAYERPAT, GAIN or OBJECT. Inspection stopped with "invalid FITS" because a light without EXPTIME was unreadable.

**Priority.** For each missing value: (1) the primary card; (2) another header card with the same meaning — `EXPOSURE` for EXPTIME, `COLORTYP`/`CFAPAT`/`CFA-PAT`/`CFATYPE` for BAYERPAT; (3) a file-name token — exposure `30.0s`/`300s`/`120sec`/`500ms`, `gain100`, a Bayer token such as `RGGB`, the target after `Light_`; a file name gives a value only when it names exactly one; (4) the embedded camera catalogue (`crates/fulgur-core/data/cameras.json`), matched on INSTRUME, CAMERA, TELESCOP or CREATOR and then on whole file-name tokens (e.g. ASIAIR's `2600MC`); the longest matching token wins, so `ASI2600MC` never matches `ASI2600MM`. Calibration frames are not reconstructed (ADR-080 warnings unchanged).

**Camera catalogue.** 69 entries (55 colour, 14 mono): ZWO ASI colour and mono, Seestar S30/S50 (Pro), DWARF II/3/mini, Vespera 3, Celestron Origin, QHY, Player One, SVBONY, ToupTek-based OEM bodies, Moravian, and DSLR/mirrorless families (Canon EOS, Nikon, Sony Alpha, Fujifilm X-Trans). Each entry records manufacturer, model, match tokens, sensor, colour/mono, Bayer pattern (as the vendor's capture software writes BAYERPAT for a full-sensor bin-1 frame), native size, pixel size, ADC bits, a confidence level (`verified_local`, `vendor`, `retailer`, `community`, `typical`) and sources. Where vendors or users disagree (QHY5III, SVBONY SV705C/SV405CC, Nikon, ASI120MC) no pattern is catalogued and the owner must choose; Fujifilm X-Trans is recorded as unsupported. A frame whose size differs from the native sensor size is flagged because an ROI or crop can shift the pattern.

**Confirmation.** Every reconstructed value is recorded per frame (`frame.derived`: field, value, source, needs_confirmation) and listed to the owner, grouped per light set. Exposure and colour layout change the result and must be confirmed: CLI `--accept-derived`, GUI checkbox "I checked these values" in the light set; otherwise frames are rejected with `unconfirmed_derived_metadata` and the GUI blocks Measure/Stack. Corrections replace what they correct: `--sensor`/layout radios for the colour layout, `--exposure <s>`/"Use this exposure for every frame" for exposure. Gain and object are labels only and need no confirmation. A light with no exposure anywhere reads as 0 s, is rejected with `missing_exposure`, and the CLI asks for `--exposure`. A recorded but non-positive or non-numeric EXPTIME is still invalid. A catalogued mono camera keeps the ADR-078 mono default without any confirmation; a colour camera with an uncatalogued pattern must be declared. Confirmation is not persisted between GUI sessions and resets when the derived values of a set change; an entered exposure is persisted.

**Unchanged.** Files with complete headers produce no derived values and process exactly as before (DS-B byte-identical, see review). Source files are only read. Reports carry no GPS; source strings contain only camera names and file-name tokens.

## ADR-083: Consolidated main publication of ADR-076–ADR-082 (2026-10-29)

**Status:** Explicitly authorized by the owner on 2026-10-29 ("once the tests pass, make a well-documented commit and push to GitHub") for this completed batch only.

Publish the completed local work since ADR-075 in one documented commit on `main`: dark calibration and master files (ADR-076/077), mono-by-default layout and broader master files (ADR-078), the calibration screen with master previews and masters of every type (ADR-079), warning-only exposure/gain differences (ADR-080), extended witnesses for dense fields (ADR-081) and full calibration with bias, scaled darks, flats and dark-defect repair plus precise accuracy witnesses (ADR-082), with tests, reviews and sanitized evidence. Fast-forward only, never force-push; use `[skip ci]`; do not change, trigger or poll GitHub Actions. Exclude datasets, FITS/XISF images, outputs, raw reports, preferences and binaries. GUI screens are release-compiled and await the owner's manual acceptance. Future publishing and remote CI remain unauthorized. See [calibration review](M5_CALIBRATION_REVIEW.md).

## ADR-082: Full light calibration — bias, exposure-scaled darks, flats, dark-defect repair (2026-10-29)

**Status:** Owner-requested ("implement the calibration; test with the mono sets of the last test and the colour lights and calibration files from 03 March; analyse and fix negative ADU or registration below 90%; document the fixes"). Supersedes the "bias and flats not applied" parts of ADR-076–ADR-080. M5 remains open.

**Arithmetic.** `calibrated = (light − bias − (dark − bias)·t_light/t_dark) / normalized flat + pedestal`, applied before star measurement and before cosmetic correction/demosaicing (CPU and GPU). Exposure scaling needs a bias and a known dark exposure; otherwise the dark is subtracted unscaled with a per-frame warning (ADR-080). The pedestal is the rounded master-bias median (else master-dark median). Dark-only arithmetic is unchanged from ADR-076 except the two safeguards below.

**Flats.** Master flats from frames subtract the master dark flat (else the master bias) before median scaling; loaded integer master flats without `OFFSETRM = T` are treated as raw and get the offset subtracted when calibration is prepared (float masters are assumed calibrated). The flat is normalized per 2×2 Bayer phase, so the flat lamp's colour does not tint the lights; values outside 0.05–20 are not divided by. Saved master flats carry `OFFSETRM`.

**Safeguards.** (1) Raw samples ≥ 65000 ADU stay saturated. (2) A master-dark hot pixel (scaled thermal > 50 ADU) whose light holds less than half of that signal, or whose over-subtraction would go negative, is replaced by the median of its calibrated same-phase neighbours (`calibration_repaired_pixels`, `calibration_negative_samples` per frame). A per-light global dark optimization was evaluated and rejected: it reduced negatives on cold Dwarf3 frames (1419 → 298) but raised them on warm frames (293 → 12 085) because the year-old dark library contains hot pixels the lights no longer have.

**Dense-field accuracy witnesses.** When whole-field coverage passes but the ordinary 250-star catalogs share fewer than 50 accuracy witnesses (a 25 000-star colour field), the anchor's 1000 precisely measured stars are used as accuracy witnesses within 3 px (`extended_witnesses`); the accuracy limits are unchanged and frames that pass with ordinary witnesses are untouched.

**Interfaces.** CLI `--bias/--bias-master`, `--dark/--dark-master`, `--flat/--flat-master`, `--dark-flat/--dark-flat-master`, `--dark-combine`, `--calibration-combine`; reports carry `calibration` (formula, pedestal, masters, flat phase medians, defects, warnings). The GUI applies every switched-on item of a light set's calibration set (masters cached in RAM; an unchanged calibration keeps reusable measurements), Stack master for flats uses the set's dark flats (else bias), and the compatibility check warns when flats and lights differ in rotator angle by more than 5°. FITS HISTORY records the formula; `bias_subtracted` reports whether a master bias was applied.

**Verification (Windows, local).** Synthetic: full chain (bias pattern, hot pixels, 3× exposure scaling, vignetting) recovers a flat sky within 3 ADU with no negative sample; per-phase normalization removes a coloured lamp while keeping the sensor response; saturated pixels stay saturated; a raw integer master flat loses its offset; a dark hot pixel missing from the light is repaired; CLI stack with bias, darks and flats. Real data: see [review](M5_CALIBRATION_REVIEW.md) — ASI2600MM Sadr 40/40, ASI2600MC Flaming Star 30/30 (both 100% registered, no negative samples), Dwarf3 851/856 registered (99.4%), DS-B byte-identical. Format and default/all-feature Clippy PASS; release suites **194/194 default and 197/197 all-feature** with real FITS, required AVX2 and actual GPU (0 ignored).

## ADR-081: Extended reference witnesses for whole-field coverage (2026-10-29)

**Status:** Owner-approved fix ("implement the extended reference catalog; keep the 80% rule"). M5 remains open. No commit, push or remote CI.

**Problem.** A real ASI2600MM Sadr DualBand set (40 × 300 s, one night, dark-calibrated, geometric correction on) stacked only 10/40: the 30 refused frames registered with 0.03–0.5 px residuals but failed the coverage rule (at least 80% of a frame's in-canvas catalog stars must have a counterpart in the reference catalog), reaching 58–79%. About 11 000 stars are detected per frame and each 250-star catalog (16 regions, brightest peaks, saturated stars excluded) picks a different subset; unmatched stars had the same brightness as matched ones. A first extension (anchor measured with 1000 stars) recovered 31/40; the remainder were the poorer-seeing frames (HFR 1.45–1.76 vs 1.28), whose brightest stars are saturated, and therefore absent, in the sharper reference.

**Decision.** When geometric correction validates a native frame, the master anchor is read once more (with its light set's in-RAM master dark when it was measured dark-calibrated) and the positions of its 5000 brightest detections, saturated stars included, form a witness catalog (3×3 detection-grid centroid, about 1 px; hot-pixel suppression and the natural-profile test as in ADR-073). The 80% coverage rule counts a source star as covered when an extended witness lies within 3 px of its mapped position (`covered_stars`, reported per frame); `fixed_stars`, the accuracy witnesses, all-star/independent/outer RMSE and maximum-error limits are unchanged, so no previously accepted frame can be refused. If the anchor cannot be read again or its dark is unavailable, the ordinary rule applies. External-reference validation is unchanged.

**Verification (Windows, local).** Synthetic: a reference catalog missing a third of the stars fails the ordinary rule and passes with the extended catalog (60 accuracy witnesses, 90/90 covered), while a 40 px misregistration still fails; witness positions include saturated stars that the ordinary catalog skips (16 vs 12) within 1.5 px. Real data: Sadr DualBand **10 → 40/40** stacked (minimum coverage 99.2%), 21.2 s; Dwarf3 mini 859/1098 unchanged with identical transforms; DS-B with correction (native and 200% template) 27/27 with byte-identical masters. Format and default/all-feature Clippy PASS (after an argument-count allowance on `assess`); release suites **189/189 default and 192/192 all-feature** with real FITS, required AVX2 and actual GPU (0 ignored).

## ADR-080: Exposure and gain differences in calibration only warn (2026-10-29)

**Status:** Owner decision ("different exposure or gain of calibration frames is not a reason for a critical error, only a warning to the user"), superseding the exposure/gain refusals of ADR-076/077/079; plus a UI-only fix (calibration cards now share one width: the column is 50% of the view, 480–640 px, and the Combine/Stack/Save row wraps instead of widening its card). M5 remains open. No commit, push or remote CI.

**Masters.** Frames of one master may differ in exposure or gain: the master is built and its summary carries warnings (e.g. "dark frames have different exposures (30 s, 60 s)"), shown in the GUI status line and printed by the CLI; the master's exposure is the median of its frames. Darks or dark flats without an exposure, and master-dark files without EXPTIME, load with a warning (exposure 0). Different dimensions remain an error.

**Lights.** A light whose exposure (±1%) or gain differs from its set's master dark is still dark-calibrated: the dark is subtracted unscaled and the frame records `calibration_warnings` (`dark_exposure_differs`, `dark_gain_differs`); the GUI reports how many frames were affected after measuring, and the compatibility check shows these cases as warnings instead of errors. Different dimensions still reject the light (`dark_size_mismatch`). Dark scaling by exposure is not implemented.

**Verification (Windows, local).** Synthetic: a mixed-exposure master builds with the warning and the median exposure; light exposure/gain differences yield warnings, not incompatibility; zero-exposure frames used as darks and a master without EXPTIME warn; mixed dark-flat exposures warn; a CLI stack with 30 s darks for 10 s lights succeeds with `calibration_warnings` on every frame. Real data: the owner's 10 × 180 s ASI2600MM darks stacked and saved (pedestal 500 ADU) and used for 22 × 300 s IC 1805 lights: 22/22 stacked in 11.9 s, every frame warned `dark_exposure_differs`. The owner's bias, dark and flat folders all record gain 100 (the earlier "different gains" message came from another flat selection). Format and default/all-feature Clippy PASS; release suites **187/187 default and 190/190 all-feature** with real FITS, required AVX2 and actual GPU (0 ignored); the card-width fix is UI-only.

## ADR-079: Calibration master preview, masters of every type and Measure on Processing (2026-10-28)

**Status:** Owner-requested ("add 1:1 detail to calibration master previews; prepare UI to load, stack, show and save masters of flats, dark flats and bias like darks, without implementing their calibration; show Processing with progress during Measure quality and return to Frames"). The calibration screen layout (one column of Bias/Darks/Flats/Dark flats with a master preview pane) is a UI-only iteration awaiting manual review. M5 remains open. No commit, push or remote CI.

**Preview.** The right-hand MASTER PREVIEW pane shows the chosen (or first available) master: a RAM master is shown directly; a master file is read in the background with the existing reference-plane reader (16/−32 bit, mono or RGB). Pan/zoom and the bounded 1:1 detail now read the full-resolution plane (`DetailSource::Plane`, compared by identity, no copy of a RAM master); ADU or normalized 0..1 values are displayed with auto stretch only.

**Masters of every type.** `calibration::build_master(paths, kind, method)` builds Bias, Dark, Flat and Dark-flat masters in RAM with the same strip-wise combine methods. Darks and dark flats need one positive exposure; bias frames may have a missing or zero EXPTIME (`read_calibration_header`; lights still require a positive exposure); flats are first scaled to the mean of their medians (16 sparse full-width row bands), with no offset removed yet (recorded in the summary and as `FLATNORM='median'`). Saved files carry `IMAGETYP='Master Bias|Dark|Flat|Dark Flat'` and a per-type name (`MasterFlat_2s_gain60_4x_average.fits`). The GUI offers Combine, Stack master and Save master… for every type; processing still applies darks only (bias, flats and dark flats are labelled "not applied to lights yet"), and only master darks are loaded from files for processing.

**Measure quality.** Measuring switches to Processing (Inspect phase progress and elapsed time) and returns to Frames when the measurements arrive; a failed run stays on Processing so its message remains visible.

**Verification (Windows, local).** Format and default/all-feature Clippy PASS; release suites **187/187 default and 190/190 all-feature** with real FITS, required AVX2 and actual GPU (0 ignored). New synthetic gate: zero-exposure bias median master; flats at four brightness levels keep their vignetting shape after normalization (corner/centre ratio within 1e-3) at the mean level; dark flats refuse mixed exposures; saved bias/flat masters carry their IMAGETYP. GUI screens and detail view: release-compiled for manual review.

## ADR-078: Missing BAYERPAT means mono; broader master-dark files (2026-10-28)

**Status:** Owner decision ("if the FITS header has no Bayer mask information, treat the frame as monochrome and allow processing"), superseding ADR-071's rule that a missing BAYERPAT stays unknown until declared; plus a fix for the owner's real master dark. M5 remains open. No commit, push or remote CI.

**Layout.** A FITS header without BAYERPAT now reads as `mosaic = mono` with `mosaic_source = "no_bayer_header"`; `LayoutChoice::Auto` keeps it, and a light set may still declare a colour pattern (or mono). The GUI asks for a choice only when the files of one set disagree (some with, some without BAYERPAT); the set label reads "Mono · no Bayer pattern in headers". `Mosaic::Unknown` remains refused wherever it can still occur.

**Master-dark files.** The owner's ASI2600MM master from another stacker (6248 × 4176, BITPIX −32, three identical normalized RGB planes, `BUNIT='normalized'`) was refused ("must be a two-axis BITPIX 16 or -32 FITS image"), which blocked Measure quality. Loading now accepts BITPIX 8/16/32/−32/−64 with one plane, `NAXIS3 = 1`, or three planes that are identical within 1e-5 relative (read plane by plane, only the first kept); genuinely different planes (a debayered colour master) are refused with a clear message. Normalized float detection (no `BUNIT='ADU'`, all values in 0..1, ×65535) now applies to −32 and −64.

**Verification (Windows, local).** Synthetic: missing BAYERPAT reads as mono and Auto keeps it; explicit Unknown is still refused by star measurement; a CLI stack without `--sensor` of BAYERPAT-free frames succeeds and is byte-identical to `--sensor mono`; GUI blocks only mixed headers; a normalized three-identical-plane master loads within 0.01 ADU and a colour master is refused. Real data: 22 ASI2600MM 300 s Ha frames (IC 1805) recognized as mono without a choice; the owner's master dark loads in 0.62 s (pedestal 500 ADU, normalized input scaled); inspection 22/22 accepted in 2.7 s (peak 3.1 GiB); stack 22/22 in 11.5 s, peak 2.74 GiB, one-plane master with `DEBAYER='NONE'` and the dark-calibration HISTORY line. Format and default/all-feature Clippy PASS; release suites **186/186 default and 189/189 all-feature** with real FITS, required AVX2 and actual GPU (0 ignored). The ADR-077/078 GUI display fixes (one-plane detail view, 64 Mpx frame preview) are UI-only and await manual review.

## ADR-077: Calibration master files and dark combine methods (2026-10-28)

**Status:** Owner-requested local change before bias/flat application ("work with calibration MASTER files; add Stack master and Save; offer Average, Median and Average sigma-kappa 2 for darks"). Amends ADR-076. M5 remains open. No commit, push or remote CI.

**Combine methods (darks).** `Average` (mean, f64 compensated), `Median` (mean of the two central values for an even count) and the default `Average, sigma-kappa 2`: iterative clipping at mean ± 2 population standard deviations, at most five passes, stopping when nothing more would be clipped or before fewer than half of the frames would remain, then the mean. This replaces ADR-076's single median/MAD 3-sigma pass. A single outlier is at most (n−1)/√n sigma from the mean, so sigma-kappa 2 needs at least 6 frames to reject it; the GUI tooltip recommends Median for fewer frames.

**Master files.** A master is still built in RAM and reaches disk only when the owner saves it: a 32-bit float two-axis FITS in ADU (not normalized) with `IMAGETYP='Master Dark'`, `BUNIT='ADU'`, `EXPTIME`, `GAIN`, `NCOMBINE`, `BAYERPAT`, `CCD-TEMP`/`TEMPMIN`/`TEMPMAX`, `COMBINE`, `PEDESTAL`, `CREATOR` and HISTORY, written atomically and never overwriting. Loading accepts BITPIX 16 or −32 two-axis masters with an exposure; a float master without `BUNIT='ADU'` whose values all lie in 0..1 is treated as normalized (e.g. PixInsight) and scaled by 65535, which the summary records. A loaded master is subjected to the same light compatibility checks (size, exposure ±1%, gain) as a built one.

**Interfaces.** CLI: `master-dark --dark <files|folders> --combine average|median|sigma-kappa-2 --output <file>`; `stack`/`register`/`inspect` accept `--dark-combine` and `--dark-master <file>` (exclusive with `--dark`). GUI calibration set, Darks item: Combine selector, **Stack master** (background build into the session RAM cache that processing reuses), **Save master…** (file dialog with a suggested name such as `MasterDark_60s_gain60_37x_sigma_kappa_2.fits`; the item then uses the saved file), and a status line (stacking / in memory with frames, method and pedestal / built automatically at processing). Master files added with *Add master…* now work in processing (cached by file identity). Bias and flat Stack/Save buttons are shown disabled until their calibration exists. The combine method persists with the calibration set.

**Verification (Windows, local).** Format and default/all-feature Clippy PASS; release suites **185/185 default and 188/188 all-feature** with real FITS, required AVX2 and actual GPU (0 ignored). One existing GUI review test failed once under full parallel load because the app could consume the worker's stop event before the test read it; the test now accepts either order (the app records the stop) and passed in the rerun. Synthetic: each method against its definition (cosmic ray clipped only by sigma-kappa, even-count median, zero-sigma case, documented five-sample limit); save → load exact f32 round trip for every method with metadata, no overwrite, no temporary files; 16-bit master, normalized float master scaled within 0.01 ADU, missing exposure and non-FITS refused; CLI `master-dark` then `stack --dark-master` produces a master byte-identical to `stack --dark --dark-combine median`. Real Dwarf3 (37 darks): build+save 0.48 s (Average), 0.61 s (Median), 1.11 s (sigma-kappa 2, 9.5% of samples clipped); method differences robust sigma ≈ 1.0–1.1 ADU per pixel, median level 192.7–193.0 ADU. A full Dwarf3 stack with the saved sigma-kappa master is byte-identical (master and both maps) to the stack built from the 37 frames (851/1098; master file loaded in 0.04 s vs 0.95 s build).

## ADR-076: First calibration — master dark subtraction (2026-10-28)

**Status:** Owner-requested local engine and application change ("implement the first calibration with DARK frames"), with real Dwarf3 mini darks. Supersedes the "calibration not implemented" parts of ADR-071 for darks only; bias, flats and dark flats remain unapplied. M5 remains open. No commit, push or remote CI.

**Master dark.** Built in RAM only (never written to disk) from at least three dark frames of one size, exposure (±1%) and gain. Rows are combined in 32-row strips read through the phase-preserving ROI reader, so work memory is frames × 32 × width × 4 B per worker, independent of frame count. Per pixel: 3-sigma clipping around the median (sigma = 1.4826·MAD, at least 1 ADU for integer data), mean of the kept samples with f64 compensated sums. The summary records frames, exposure, gain, the temperature range (CCD-TEMP or DET-TEMP), median, pedestal, clipped samples and build time.

**Application.** `calibrated = light − master + pedestal`, pedestal = rounded master median. Subtraction happens right after decoding, before star measurement (inspection, hence registration catalogs and photometry) and before cosmetic correction/demosaicing in integration (CPU and GPU ROI paths), so measurement and stacking always see the same pixels. The constant pedestal keeps the calibrated sky above zero, so the 0..1 export never truncates background noise; a constant cancels in the per-frame background normalization. A light whose exposure (±1%), gain or size does not match its set's master is rejected (`dark_exposure_mismatch`, `dark_gain_mismatch`, `dark_size_mismatch`), never stacked raw. A frame measured with a dark must integrate with the same master (error otherwise). Darks are opt-in per light set; nothing is subtracted by default, so Seestar precalibrated lights are unchanged (the existing double-correction warning stays). No BIAS card is used.

**Interfaces.** CLI `--dark <file|folder>` (repeatable) on `stack`, `register` and `inspect` applies one master to all light sets and prints a one-line summary; reports carry `dark_calibration` and per-set `light_sets[].dark`, per-frame `dark_calibrated`. The GUI builds the master of each light set's switched-on Darks item when Measure/Stack starts ("Master dark" progress), caches up to four masters in RAM keyed by file identity (reused while files are unchanged), and refuses to reuse quality measurements made with a different master. Saved master-dark files are not supported yet (clear error). Calibration folders without IMAGETYP (DWARF) are sorted by file or folder name; camera labels that differ only in spelling/roman numerals ("DWARFIII" / "DWARF 3") match. FITS HISTORY: "N frames dark-calibrated: light - master dark + constant pedestal."

**Verification (Windows, local).** Format and default/all-feature Clippy PASS; release suites **182/182 default and 185/185 all-feature** with real FITS, required AVX2 and actual GPU (0 ignored). Synthetic: master equals the true dark (bias, column ramp, hot pixels) within 0.5 ADU with a cosmic ray clipped, strip remainder handled, sources byte-identical and no extra files; ROI and full-frame subtraction identical; fewer than three darks, mixed exposures, exposure/gain/size mismatches and cancellation refused; CLI end-to-end stack with `--dark` (3 lights with a sensor-fixed hot pattern): RAM-only master, all frames dark-calibrated with zero residual suppressed hot pixels, FITS HISTORY line, only requested products, sources unchanged; mismatched-exposure darks refuse the stack without outputs; GUI name-based dark sorting and camera-label matching. Real Dwarf3 mini (1098 × 60 s Duo-Band, 37 darks 60 s, gain 60, 26–31 °C; lights 27–35 °C): master in 0.93 s (peak 451 MiB during the build), pedestal 193 ADU; suppressed hot-pixel samples per frame 37 683 → 1 015 (median), defect detections refused 512 → 11, detection-grid noise 9.64 → 7.47 ADU; quality-accepted 856 (878 without darks; limits are relative within the set), registration refusals 19 → 4; **852/1098 stacked** in 92.5 s, peak 3.02 GiB (859 in 150.5 s without darks); master background robust noise R/G/B 2.38/1.99/1.52 → 1.85/1.34/0.79 ADU (−22/−33/−48%), no export limiting. DS-B without darks: master and both maps byte-identical to ADR-074 (native and 200% template). **Owner review (2026-10-28): dark calibration result PASS** ("looks good"); bias/flats remain open and M5 remains open.

## ADR-075: Consolidated main publication of ADR-069–ADR-074 (2026-10-28)

**Status:** Explicitly authorized by the owner on 2026-10-28 ("prepare a well-documented GitHub commit and push") for this completed batch only.

Publish the completed local work since ADR-068 in one documented commit on `main`: AVX2 VNG (ADR-069), reused integration buffers (ADR-070), light/calibration sets, declared sensor layout and the four-step workflow (ADR-071 with its external-reference addendum), the Frames usability iteration (ADR-072), hot-pixel-robust star measurement (ADR-073) and the scale/deformation registration fallback (ADR-074), with tests, reviews and sanitized evidence. Fast-forward only, never force-push; use `[skip ci]`; do not change, trigger or poll GitHub Actions. Exclude local datasets, FITS/XISF images, generated outputs/raw reports, preferences and binaries.

Verification limits are stated, not upgraded by publication: engine gates for this snapshot are format, default/all-feature Clippy and 177 default / 180 all-feature release tests with actual FITS/AVX2/GPU on Windows. ADR-072 and the ADR-071 screens are release-compiled UI iterations awaiting the owner's manual acceptance (UI-only policy: no automatic functional tests). Calibration arithmetic is not implemented. Owner PixInsight review of the Dwarf3 master passed after publication (recorded locally under ADR-074); real mono/calibration data and macOS remain unverified; M5 remains open. Future publishing and remote CI remain unauthorized. See [current state](CURRENT_STATUS.md).

## ADR-074: Scale and local-deformation fallback for native registration (2026-10-28)

**Status:** Owner-requested local engine change ("use 30–333% scale and local deformation when shift and rotation fail; the algorithm is already tested for external references"); M5 remains open. No commit, push or remote CI.

**Problem.** After ADR-073, 107 of 878 quality-accepted Dwarf3 frames (9 nights) failed absolute whole-field validation. A direct rigid refit did not help (corner change ≤ 0.06 px); a similarity fit passed for 88 of them. Plate scale differs from the reference night by +2…+6·10⁻⁴ (refocus/temperature per night, drifting towards morning; slight anisotropy suggests refraction), i.e. 1–1.3 px at the corners, which the rigid (scale 1) native pose cannot represent. Single-night Seestar data have no such change.

**Decision.** Only with geometric correction enabled (the only path with absolute validation), and only for a frame whose current mapping is refused:
1. **Rigid pose fails absolute validation** → fit a similarity on the training witnesses (fixed 12 px witness identities, spatially reserved held-out witnesses excluded, one trimmed refit), require 30–333% scale, and validate it with the unchanged `validation::assess` on the same witnesses. If it fails, try the bounded, invertible cubic candidate that the distortion solve fitted on non-held-out stars but did not apply for lack of a confirmed improvement, validated on the index-mod-5 held-out witnesses (as in the external path). `initial_correction_reason` keeps the rigid failure; methods `native_similarity_validated` / `native_cubic_fallback_validated`.
2. **Rigid registration found no consensus** (`registration_failed`) and no cubic was confirmed → run the external-reference similarity search (`solve_similarity`, 30–333%) against the master anchor, re-establish photometry on its direct master matches (`native_similarity_recovered`), then apply the same absolute validation and fallback chain.
3. Frames that pass rigid validation are untouched (bit-identical). The native transform may now carry `scale_factor ≠ 1`; integration already validates the 30–333% range, and the external-reference bridge composes it. The report's transform convention states the fallback.

**Verification (Windows, local).** Format and default/all-feature Clippy PASS; release suites **177/177 default and 180/180 all-feature** with real FITS, required AVX2 and actual GPU (0 ignored). New synthetic gate: +0.12% scale about the sensor centre (rigid refused first, similarity recovered), 200% with 0.5 rad and 50% with −0.3 rad (rigid registration fails, similarity search recovers), all with exact scale/rotation (< 1e-6) and validation max error < 0.001 px; 400% is never accepted. Real Dwarf3 mini (1098 × 60 s, 9 nights, manual reference, correction on): **771 → 859** stacked frames (all previous 771 unchanged; 88 `native_similarity_validated`, scales 1.00024–1.00070; cubic fallback not needed); 19 remain refused by a single mismatched witness (4–12 px). DS-B with correction on, native and against the 200% template: 27/27, no fallback triggered, so Seestar products are unchanged by construction. **Owner review (2026-10-28): the Dwarf3 master checked in PixInsight — PASS** for ADR-073/074 on this dataset; M5 remains open.

## ADR-073: Hot-pixel-robust star measurement (2026-10-28)

**Status:** Owner-requested local engine change ("filter hot pixels in quality measurement and registration; use only real stars with a natural profile"); M5 remains open. No commit, push or remote CI.

**Problem.** Uncalibrated Dwarf3 mini lights (60 s, sensor 27–35 °C, no darks) carry tens of thousands of warm/hot green samples per frame. About half of the 250 catalog stars sat on identical sensor pixels across nights; sequential registration locked onto that fixed pattern (near-identity transforms), and absolute field validation then rejected 836 of 837 quality-accepted frames (`native_insufficient_field_evidence`, 0–9 of ≈245 witnesses). Defects inside real stellar apertures also pulled centroids by 2–3 px, biased HFR/eccentricity and produced isolated >3 px witness errors.

**Decision.** Star measurement (`stars::analyze`, shared by quality inspection and native registration) works on a cosmetically suppressed copy of the measurement plane; source pixels, sky background/noise statistics, integration and its existing cosmetic correction are unchanged.
1. A measurement-plane sample (green on CFA, every pixel on mono) is a defect when it exceeds the median of its four nearest plane neighbours (diagonal greens / orthogonal pixels) by more than 10 pixel sigmas while those neighbours hold less than 10% of its excess over the sky cell. It is replaced by that median in the measurement copy only. Replacements are computed from the untouched frame; sky statistics are taken before suppression, so `background_adu`/`noise_adu` are bit-identical.
2. Every remaining detection must have a natural profile: the brightest plane sample of the detection cell must leave at least 10% of its excess (over a local plane-sample ring) on the median of its four nearest plane neighbours. Refused detections do not count as stars.
3. The 10% share is evidence-based: sensor-fixed Dwarf3 defects stay below 0.04 (99th percentile), real stars sit near 0.4–0.55 (Seestar 1st percentile 0.23; 1 of 8000 Seestar catalog stars below 0.1).
4. Inspection JSON gains `suppressed_hot_pixels` and `refused_defect_detections` per frame. External geometry-only reference catalogs (already integrated templates) are unchanged.

**Verification (Windows, local).** Format and default/all-feature Clippy PASS; release suites **176/176 default and 179/179 all-feature** with real FITS, required AVX2 and actual GPU (0 ignored). New synthetic gate: CFA (RGGB, GRBG) and mono frames with sharp (σ 1.0) and broad (σ 1.8) stars, warm defect clusters that satisfy the old 3×3 support gate, and defects 4 px inside sharp-star apertures: every defect suppressed, every star kept, centroids < 0.1 px. Real Dwarf3 mini set (1098 × 60 s, C 20, manual reference, geometric correction on): quality-accepted 837 → 878 (eccentricity rejections 259 → 220); stacked **1 → 771** frames (12.85 h) in 156.2 s, peak 2.75 GiB; accepted inlier RMSE median 0.34 px; ≈37 700 suppressed samples per frame (median); remaining 107 registration rejections are absolute/outer-field geometry failures, not defects. Inspection of the set 19.1 → 22.5 s, peak 721 → 1027 MiB; the optimized suppression pass yields catalogs identical to the unoptimized reference pass. DS-B (Seestar, precalibrated): 0–8 suppressed samples per frame, identical quality decisions and the same 27 stacked frames with and without the template; transforms move ≤ 0.002 px (Auto) / ≤ 0.018 px (200% template); master RMS difference 6.6e-6 / 8.7e-6 at mean level 0.034, so DS-B products are no longer byte-identical to ADR-070/071 by design. No source file changed.

## ADR-072: Frames usability iteration (2026-10-28)

**Status:** Owner-requested UI-only iteration under the manual-review policy (no automatic functional tests, stacks or benchmarks; release build for owner review). No commit, push or remote CI.

1. The sets rail has **Clear…** with *Clear light sets*, *Clear calibration sets* and *Clear both*, each confirmed; sets are removed from the workspace only, never from disk; clearing lights also clears quality measurements but keeps the last result.
2. In the frame list, **↑/↓** move the selected row (and its preview) while the list has focus; **Shift+↑/↓** extend the marked range; the list scrolls to keep the row visible.
3. The frame preview keeps **zoom and position** when moving to another frame of the same size (it resets only for another size).
4. The **registration reference row** is highlighted: wine for the automatic reference (the last stack's anchor, otherwise the predicted chronological median of Use-on frames, which is what Auto uses for one night), lighter red for a manually chosen frame; the preview and tooltip name it. External FITS references have no list row.
5. **Blink**: the preview can alternate the selected frame and the reference every second with shared zoom/position and their own native-detail caches. Frames are shown as recorded (before registration), so field rotation/offset between them remains visible.

## ADR-071: Light sets, calibration sets, declared sensor layout and four-step workflow (2026-10-28)

**Status:** Owner-requested local implementation of the approved clickable mockup; M5 remains open. Calibration arithmetic is explicitly out of scope until the owner supplies real calibration data. No commit, push or remote CI.

**Owner decisions recorded (2026-10-28):** inputs are grouped into light sets (each added folder becomes a new set); calibration sets (bias, darks, flats, dark flats as individual frames or masters) are prepared and assigned per light set; masters will live in RAM only; several filters may be stacked together but the user is warned; frames without a Bayer mask skip demosaicing; when headers do not decide the layout the user must declare it.

**Sensor layout.** `FrameInfo` now carries `bayer_pattern: Option`, `mosaic` (bayer/mono/unknown) and `mosaic_source` (header/user). A missing BAYERPAT is **unknown**, never assumed mono. A light set's `LayoutChoice` (auto/mono/explicit pattern) is applied at inspection and travels through the report into integration sources and previews. Unknown frames are rejected (`unknown_sensor_layout`) and cannot reach star measurement, demosaicing or integration; the GUI blocks Measure/Stack until the owner chooses. Monochrome frames: star detection on a 2×2-mean grid with full-plane centroid/half-pixel Lanczos-3 HFR quadrature (support 18 of 113 aperture pixels, SNR over 113 samples), the same pattern-independent isolated hot-pixel correction, no demosaicing (all three channels hold the corrected sample). A purely mono stack is written as one plane (`NAXIS=2`, `DEBAYER='NONE'`, maps likewise); a mixed mono/colour stack stays RGB and records how many mono frames feed R, G and B equally. GPU reconstruction refuses mono sources explicitly (no silent fallback). The CFA path's arithmetic is untouched.

**Light sets in the engine.** `inspect_sets` classifies HFR, star-count and eccentricity limits within each set, then rejects frames of other sets whose target (`different_target`) or sensor dimensions (`different_sensor_size_unsupported`) differ from the largest set, and normalizes provisional weights over all sets. One set reproduces the previous single population bit for bit. Native registration and distortion validation no longer require equal filter, gain, BZERO or colour layout, only the same target and sensor dimensions; per-frame photometric scale/offset already normalizes flux. Cross-camera registration of sources at another scale is **not** implemented (the existing 33–300% similarity applies to an external reference only), so different-sensor sets are rejected with an explicit reason and flagged in the UI. CLI: `--sensor auto|mono|<pattern>` for stack/register/inspect/debayer-single and `--input-sets` (each `--input` is its own light set).

**Workflow UI.** Four steps (Frames, Settings, Processing, Results); contextual primary action and STOP at the right of the top bar with a menu for Stack directly / Measure again; a one-line status bar with no duplicate progress bar; Processing is a screen (phase grid, elapsed/remaining, This run with sets and calibration assignment) instead of a modal; Settings on one page (reference left, output right, Advanced collapsed). Frames has a rail (All light frames, Light sets, Calibration sets), a set overview with mixed-filter/mixed-layout warnings, a light-set detail (properties from headers, include, layout declaration, calibration choice with header-based compatibility checks: camera, size, flat filter, flat night, dark exposure/gain/temperature, Seestar double-correction warnings) and a calibration-set detail (IMAGETYP-sorted folder import, frames or master per kind, enable, formula). Stacking mixed filters asks for confirmation. The frame list keeps its behaviour, adds a Set column and filters to the selected set. Light/calibration sets and layouts persist; the earlier flat input list migrates to one set per entry. Every GUI report records the light sets, their assigned calibration set and `calibration_applied: false`.

**Verification (Windows, local):** format, default and all-feature Clippy PASS; release suites **175/175 default and 178/178 all-feature** with real FITS, required AVX2 and actual GPU. New gates: mono HFR/centroid/eccentricity over 48 Gaussian phase cases (<3% HFR error) and a 25-star full-frame analysis; unknown-layout refusal; mono reconstruction equals the corrected sample in every channel; per-set classification, cross-set rejections and single-set bit identity; end-to-end CLI mono stack (refused without declaration, single-plane master and maps with `--sensor mono`, sources unchanged); header metadata/IMAGETYP classification/compatibility and GUI set persistence, migration, layout blocking and mixed-filter confirmation. DS-B CLI stack: master, coverage and high-rejection FITS **byte-identical** to the ADR-070 products; the registration report differs only by the new provenance fields and one description string. Real Seestar S30 Pro data as two sets (IR 10 s ×4, LP 20 s ×83): 75/87 stacked in 11.8 s, peak 713 MiB, sources unchanged; IR frames show up to 5.3× the LP transparency, as expected across filters. No real monochrome data, real calibration data or cross-camera data exist yet; native visual review of the new screens is the owner's manual step. See [review](M5_LIGHT_SETS_REVIEW.md).

**Addendum (owner, 2026-10-28): external reference filter and scale range.** An external reference supplies star geometry only and never enters the stack, so its FILTER is no longer compared (no warning); a different OBJECT is still refused as a wrong file. The supported source-to-reference scale is widened from 33–300% to **30–333%** (reciprocal bounds `MIN_GEOMETRY_SCALE = 0.30`, `MAX = 1/0.30`, shared by seeding, Procrustes clamps, distortion/integration validity and master provenance). Synthetic similarity, fallback, resampling, integration-plan and rendered-reference sweeps now include 0.30 and 1/0.30 (and 3.25) and reject 0.27/3.7. The S50 DS-B stack against `RegisteringTemplate200pct.fit` is byte-identical (master and both maps) to the ADR-070 control binary. Real S30 Pro frames (160 mm) against that S50 template (260 mm, 200%, cropped 4000×4000) need 325% and are now in range, but still cannot be registered: the template covers only ≈14% of the S30 Pro field, where the S30 Pro catalog holds about 20 detected stars, below the robust ten-match triangle consensus; mirrored and 180° variants and looser tolerances were probed and also fail, and a density-matched catalog fallback was tried and removed because it did not help. Gates: format/Clippy PASS, 175/175 default and 178/178 all-feature release tests.

## ADR-070: Reused CPU integration buffers (2026-10-27)

**Status:** Owner-requested local engine optimization on top of ADR-069; M5 remains open. No commit, push or remote CI.

Stop requesting fresh, operating-system-zeroed memory for every source and tile. Keep a pool of per-slot ROI scratch (raw CFA, cosmetic CFA, VNG RGB), never larger than the active slot count; recycle the previous tile's sample buffers as the next tile's outputs, each claimed by exactly one source index; reuse the tile reduction buffer. Every reused buffer is fully reinitialized (ROI decode overwrites all samples, cosmetic starts from an exact CFA copy, VNG writes every pixel, resampled samples reset to NaN), so only capacity carries over. Keep the single-use public functions as wrappers. GPU path, FITS cache semantics, tile plan and all scientific operations are unchanged; retained scratch is covered by the existing per-worker ROI estimate and recycled samples replace the per-tile set.

Gate with poisoned-buffer differential tests across growing/shrinking/edge/uncovered tiles, cache off/on, plus reused VNG storage across sizes and all existing integration gates. Measured against the preserved ADR-069 profiler on the same stored native plan: format/default-all-feature Clippy PASS; release suites 163/163 default and 166/166 all-feature with real FITS, required AVX2 and actual GPU. Central 24-thread medians 14.6702947 → 13.6289897 s integration (−7.098%), kernel CPU 26.171875 → 12.421875 s (−52.537%), user CPU unchanged. Complete grid single pair 112.7421006 → 105.8856032 s (−6.082%), kernel CPU 257.171875 → 131.609375 s. DS-B CLI 6.2910175/5.7075395 → 5.2639911/5.3319585 s with exact equivalence. All products equal ADR-069; all 1140 DS-A and 32 DS-B source fingerprints unchanged. Peak working set +31 MiB (complete pair) to +155 MiB (central region), within the ceiling. Control timings differ between days; only same-session pairs are claimed. Windows only. See [review](M5_CPU_BUFFER_REUSE_REVIEW.md).

## ADR-069: Exact AVX2 VNG demosaicing (2026-10-26)

**Status:** Owner-requested local engine optimization; M5 remains open. No commit, push or remote CI.

Generate an eight-lane AVX2 copy of the four VNG phase kernels in the build script from the same shared stencils as the scalar kernels. Each lane repeats the scalar IEEE f32 operations in the same order (left-folded gradient terms, min/max/threshold folds, per-direction sample additions as blends instead of branches, unchanged final division); no FMA, reassociation or reduced precision. Process interior pixel pairs in groups of eight same-phase pixels from small even/odd column planes of the five input rows; reflected edges and remaining pairs stay scalar. Select AVX2 at runtime and keep the portable scalar path. Vectorize a row only when all five input rows are finite and within ±1e30, so NaN/infinity/overflow cannot reach min/max/compare, where vector semantics differ; real 16-bit ADU input is always inside this domain. No image cache, intermediate image, file or precision change.

Gate with forced-scalar vs AVX2 bit equality across all Bayer patterns, all group/tail widths, signed zeros, subnormals, bounded and unbounded extremes, non-finite mixtures and a real frame before/after cosmetics, plus the existing frozen VNG reference and full suites. Measure with a preserved pre-change control profiler on a stored frozen native plan (the ADR-062 plan file is no longer present; use the stored native register report) and an end-to-end DS-B CLI pair against a control built from the published commit.

Measured: format/default-all-feature Clippy PASS; release suites 162/162 default and 165/165 all-feature with real FITS, required AVX2 and actual GPU. Central 24-thread region medians 21.8704137 → 15.8733048 s integration (−27.421%), reconstruction −36.170%. Complete native grid single pair 162.2608958 → 130.0071238 s integration (−19.878%), reconstruction 119.7366403 → 84.9744411 s. All runs produce identical raw samples and whole master/map FITS; complete-grid maps equal the stored ADR-062 products. DS-B CLI 6.654397/6.6038844 → 5.6494159/5.4470505 s with exact FITS and scientific report equivalence. All 1140 DS-A and 32 DS-B source fingerprints are unchanged. The same-session control is slower than the historical 135.0441689 s observation under uncontrolled conditions; only paired comparisons are claimed. Peak working set 3339.7/3512.6 MiB in the complete pair, within historical run variation and far below the ceiling. Windows/AVX2 only; no macOS or non-AVX2 hardware run. See [review](M5_CPU_VNG_SIMD_REVIEW.md).

## ADR-068: Consolidated main publication and current documentation (2026-10-26)

**Status:** Explicitly authorized by the owner for this completed batch.

Publish the completed native registration scheduling, optional distortion correction, whole-field acceptance repair and Auto reference implementation/tests/evidence together, on top of the existing Compact Desk and CPU integration history. Fast-forward `main` when its fetched tip is an ancestor; preserve remote history and never force-push. Include current-state documentation and mark earlier reviews as historical without changing their measurements or failed acceptance outcomes. Use `[skip ci]`; do not change, trigger or poll GitHub Actions. Exclude local datasets, generated images/raw reports, preferences and binaries.

Latest Windows engine gates are 160 default / 163 all-feature release tests, format and default/all-feature Clippy, including actual FITS/AVX2/GPU. A CLI help description is corrected to reflect explicit-reference precedence; no scientific operation changes during publication. Reuse stored benchmark/scientific evidence rather than rerunning a full reference stack. Native visual/PixInsight review, real multi-night data and macOS remain unverified; M5 remains open. This authorization does not enable future automatic publication or revoke the UI-only manual-review policy. See [current state](CURRENT_STATUS.md).

## Decision history

ADRs below retain their decision-time context. Later owner amendments take precedence, especially ADR-066 over unsafe fallback in ADR-065 and ADR-067 over the chronological-only automatic reference default. Historical local-only publication restrictions were lifted only for their explicitly authorized batches; scientific guardrails remain binding.

## ADR-067: Automatic multi-night count-based geometric reference

2026-10-26, owner authorized; M5 remains open. Auto is the new default; explicit
chronological median and manual/external choices remain. Missing accepted header
times require a manual reference. Accepted acquisition groups use gaps >6h,
crossing midnight normally. Use-off inputs and diagnostic fits do not vote.

For multiple accepted nights, register catalogs provisionally to the legacy
median using existing scientific gates. All selected frames stay in the count
denominator, with zero votes for unmapped frames. Refuse to select if an entire
selected observing group cannot be mapped. Rasterize conservative geometric
footprints onto an at-most-1024-square cell-center count grid (<=4 MiB); evaluate
every accepted source's own output grid on an at-most-128-side aspect-preserving
grid, applying its actual nonlinear map where present. Maximize the largest
all-above-80% rectangle, mean count coverage, representative center/orientation,
then lower HFR, more catalog stars and filename. Framing angular distance uses
sin(abs difference), giving 180-degree periodicity without changing the true
360-degree stellar registration. This is a bounded approximation, not an exact
pixel optimum or a post-rejection Coverage prediction.

Reuse the provisional native plan when it wins; otherwise rerun the unchanged
direct-anchor registrar with the selected real source. Do not change the pixel
engine, interpolation count, source data, rejection, provenance or memory ceiling.
Final registration must retain at least one accepted frame in every selected
observing group. Existing per-source quality/failure gates still apply. Record
the preliminary/scoring timings with their nesting scope, chosen/median metrics,
candidate counts and prediction limitations; keep source identity and native
photometric anchor truthful. One-night scientific behavior stays exact. External
geometry keeps its median native photometric anchor and bypasses Auto.

The existing Processing bar counts both catalog passes, scoring and reuse skips,
with STOP checked throughout; no duplicate progress UI. GUI mode IDs 1/2 retain
manual/external meaning, 0 becomes Auto and 3 explicitly requests the old median.
The selected strategy is frozen into the job and updated through quality-review
Continue. CLI register/stack expose --reference-strategy auto|chronological-median.
All work/verification stays local without publishing or remote CI.

## ADR-001: Product Identity and Language (2026-10-21)

**Status:** Accepted by owner instruction.

Use FulgurStack as the product name, `fulgurstack` as the executable, `fulgur-core`, `fulgur-pipeline`, and `fulgur-cli` as the initial workspace crates, and `FULGURSTACK_TEST_FITS` / `FULGURSTACK_TEST_FITS_DIR` for optional real-data test inputs. No compatibility aliases are required because no implementation exists yet. All artifacts are English; owner-facing communication is Czech.

## ADR-002: Mixed Exposure Durations (2026-10-21)

**Status:** Required by owner; implementation pending within M1-M4.

First-release stacks support one target and filter with different positive exposure durations and compatible camera settings. Keep output intensities on the reference anchor's scale. Matched unsaturated star fluxes determine normalization; derive atmospheric transparency from exposure-corrected flux ratios. Weight normalized samples using noise in those same units, with the specified HFR factor, without applying exposure or transparency twice.

M2 weights remain explicitly provisional until M3 provides star matches. Star-count rejection must account for exposure-duration groups. Synthetic tests cover mixed durations, invalid exposure, saturation, and normalized flux preservation; the available real dataset covers 10 s only. The output distinguishes reference exposure, accepted total acquisition exposure, and accepted frame count. Coverage and per-pixel clipping prevent interpreting the total acquisition exposure as uniform effective exposure.

## ADR-003: GPS Preservation and Provenance (2026-10-21)

**Status:** GPS retention requested by owner; reference-anchor selection is an engineering decision.

Preserve valid GPS cards from the chosen reference anchor in the final FITS, including single-frame exports. FITS history identifies them as reference-anchor coordinates; they do not imply that all sessions share a location. Do not average sites or invent coordinates. Coordinates remain excluded from console output, default logs, and default reports.

## ADR-004: Reference Dataset and Benchmark Evidence (2026-10-21)

**Status:** Header inventory verified; performance and image quality unverified.

The owner-provided DS-A directory contains 1,140 compatible 10 s M33/IRCUT FITS files, rather than the inherited 1,150-frame estimate. The owner-provided immutable XISF reference is RGB Float32 at the input sensor dimensions and declares 11,400 s acquisition exposure per channel, consistent with the input exposure sum. XISF is an external evaluation artifact; native XISF support is outside the first release.

The owner confirmed on 2026-10-21 that approximately three hours represents the entire PixInsight processing workflow. The exact reference input selection is not yet verified. Keep the <=18 min target and qualify speedup against this baseline as approximate. Acquisition exposure metadata must never be used as processing duration. Compare equivalent input selections and processing scopes. No performance or quality acceptance gate has passed from header inspection alone.

## ADR-005: Small Regression Dataset and PI Comparison (2026-10-21)

**Status:** Dataset header inventory and SHA-256 manifest recorded; timing/settings supplied by owner; image quality unverified.

Use project-local DS-B (32 source FITS frames) for quick milestone regressions and the supplied PI debayered, registered, and integrated images as immutable reference artifacts. Retain DS-A as the complete performance and quality gate. The sum of owner-reported DS-B stage timings is approximately 115 s; do not extrapolate this linearly to DS-A.

The PI integration uses ESD rejection and PSF Signal Weight, while FulgurStack retains the specified Winsorized Sigma Clipping and quality weighting. Compare image quality in compatible geometry and intensity units, document input-set differences, and do not require bitwise identity. Treat the provided PI memory allocations as separate diagnostics, not measured peak RSS. Exact paths, configuration, timing limitations, and milestone usage are recorded in `docs/BENCHMARKS.md`; the source and FITS reference fingerprint inventory is `docs/ds-b-manifest.json`.

## ADR-006: Isolated Development Environment Validation (2026-10-21)

**Status:** Windows environment verified; application implementation pending.

Validate the owner-installed Windows MSVC Rust toolchain using a dependency-free scratch package under the ignored `target/` directory. This checks formatting, lint tooling, release compilation, linking, and executable startup without introducing a placeholder product workspace. The scratch harness has no product tests and does not satisfy any M1 gate. Product workspace creation, meaningful FITS/image tests, and cross-platform CI remain part of M1. DS-B source/reference fingerprints were independently rechecked after the smoke run.

## ADR-007: M1 FITS I/O and Atomic Publication (2026-10-21)

**Status:** Implemented and verified on Windows; owner visual acceptance pending. The original ADU export scale is superseded by ADR-010.

Use a bounded pure-Rust primary-image FITS parser with immutable buffered reads and signed big-endian decoding. Retain original header cards privately; expose only an explicit metadata whitelist through reports and Debug. M1 supports 16-bit primary CFA images, BSCALE=1, BZERO=0 or 32768, valid positive EXPTIME, and four standard Bayer phases. Reject BLANK pixels, nonzero Bayer offsets, malformed/truncated arrays, and unsupported metadata explicitly rather than silently guessing. Limit raw payloads to 512 MiB for the M1 full-frame path.

Write planar RGB big-endian float FITS in source ADU without stretching, clipping, dark/BIAS subtraction, or copying stale CFA/scaling/WCS/checksum cards. Preserve observation metadata and valid reference GPS. Use `tempfile` to create exactly `<output>.tmp` exclusively, flush/sync it, and publish with no-clobber persistence. Existing destinations, hardlink aliases, and stale temp files are protected. On supported systems no-clobber persistence may use a link operation rather than a replacing rename; atomic visibility and source protection take precedence over replacing any existing file. Ctrl-C/SIGTERM cancellation unwinds and removes owned incomplete output.

References: [FITS data representation](https://fits.gsfc.nasa.gov/standard30/htmlfiles/fits_standard3000047.html), [FITS integer offsets and floating-point data](https://fits.gsfc.nasa.gov/standard30/htmlfiles/fits_standard3000048.html), and [`tempfile` no-clobber persistence](https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html#method.persist_noclobber).

## ADR-008: M1 Reconstruction and Conservative Cosmetics (2026-10-21)

**Status:** Implemented; synthetic gates passed and real export generated. The original ADU export/import advice is superseded by ADR-010; reconstruction remains in ADU.

Implement the published MHC equations independently in Rust with Rayon, preserving measured CFA samples exactly after optional cosmetic correction. Reflect sensor boundaries without changing CFA parity. Retain negative/overshoot interpolation values in f32 rather than clipping them; FITS output remains linear in physical ADU. PixInsight may require its floating-point range rescale option on import.

A five-sigma rule based on eight noisy neighbors alone altered 92,175 samples on the first real frame. Confirm candidates using a 24-neighbor same-phase MAD estimate and preserve spatially supported stars; the final real frame changes 5,140 samples (0.062%). The synthetic Gaussian-noise gate limits false corrections to less than 0.1%, and a separate undersampled-star test protects the peak. Cosmetics leaves the outer four sensor rows/columns unchanged. Combined cosmetic/MHC/bicubic support requires an eight-pixel raw halo in future integration, recorded in the specification without implementing M2-M4.

References: [original MHC paper](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/Demosaicing_ICASSP04.pdf) and [PixInsight floating-point FITS range handling](https://pixinsight.com/forum/index.php?threads/scaling-of-fits-on-open.626/). The implementation does not copy the separately distributed IPOL C source.

## ADR-009: Bounded Survey and Telemetry (2026-10-21)

**Status:** Implemented; DS-A and DS-B release I/O runs passed.

Use bounded per-worker read buffers, not decoded full images, for `survey`. CPU workers default to available parallelism capped at 16, with a configurable limit. Report effective full-file throughput with uncontrolled OS cache state; do not present cache-assisted throughput as isolated SSD hardware speed. On Windows, query the process peak working set through generated Rust Windows API bindings; report null on unsupported telemetry platforms rather than estimating RAM. Dependencies require no separately installed C/C++ image libraries.

## ADR-010: Fixed Linear Export and Explicit PI Import (2026-10-21)

**Status:** Implemented for the owner's direct-comparison request; numerical gates passed, native PI and owner visual verification pending.

Keep reconstruction in source ADU, but divide each exported sample by the fixed 65,535 sensor full scale. Use the same divisor across frames/channels, without pedestal, per-image extrema normalization, stretch, or clipping. The supplied PI single-frame reference already uses these units. `FLXDIV=65535`, `BUNIT='adu/65535'`, and history describe the representation; `FLXDIV` does not instruct PI to rescale. Preserve GPS and observation cards. Report schema 2 adds the divisor, zero offset, output extrema, preserved out-of-range counts, and zero clipped samples.

Simply dividing the old export is insufficient for ordinary PI File > Open: 66 negative samples and 48 samples above one remain in this MHC reconstruction. PI's default FITS policy can therefore rescale the entire image again. Clipping those samples or adding an image-dependent offset would violate the owner's requirements. FITS has no supported header that overrides PI's range policy. Supply `scripts/OpenFulgurStack.js`, using the documented `ImageWindow.open` format-hints argument with `ignore-out-of-range`, `top-down`, `verbosity 0`, and a fresh image copy. It performs no pixel arithmetic, changes no global preferences, and rejects legacy ADU exports without the new marker. The CLI remains independent of PixInsight; no native XISF feature or FulgurStack GUI is added.

Verified all 24,883,200 new output samples equal the old reconstruction divided by 65,535 at Float32 precision, including all 114 out-of-range samples. Reference-scale signed median residuals against PI VNG are zero in all three channels. Different interpolation/cosmetic methods still require quality review. Import-adapter tests use host API doubles; they do not claim native PI execution.

References: [official FITS module range-policy implementation](https://gitlab.com/pixinsight/PCL/-/raw/master/src/modules/file-formats/FITS/FITSInstance.cpp), and the installed PixInsight `doc/pjsr/objects/ImageWindow/ImageWindow.html` documentation for the `ImageWindow.open(url, imageId, formatHints, copy)` signature. The range-policy implementation ignores the fix mode only with `ignore-out-of-range`; it otherwise rescales or truncates even after fixed-scale export.

## ADR-011: Preserve Bright Stellar Cores During Cosmetics (2026-10-21)

**Status:** Owner rejected the prior normalized export because of dark stellar-core dots. Fix implemented; automatic and real-data numerical gates passed; renewed visual review pending. Supersedes the MAD-only neighbor support rule in ADR-008.

The former support guard interpreted a star's spatial intensity slope as neighbor noise. It therefore failed to protect some genuine CFA samples before MHC reconstruction. For example, source/reference 65,535 ADU at a saturated red core became 11,403.5 ADU; this is a destructive cosmetic error, not an expected MHC/VNG difference or an export-range issue.

Keep the original five-sigma candidate checks and the two-neighbor spatial support requirement, but cap each neighbor's support threshold at one eighth of the candidate's excess above its same-phase median. Use `min(3 * neighbor_noise, candidate_excess / 8)` above the neighbor's same-phase median. The cap is a conservative morphological safeguard; it is not a new noise estimate or a measured probability. It preserves coherent stellar profiles when local MAD is inflated by gradients or saturation. Flat-sky isolated defects still lack spatial support. Ambiguous defects may remain near stars; avoiding stellar flux loss takes priority over aggressive single-frame cleaning. CFA support/halo, MHC equations, export scale and unclipped output remain unchanged.

Regression coverage includes the anonymized 9x9 intensity neighborhood that reproduced the real failure, 60 analytic colored-star cases spanning five PSF widths, four amplitudes including saturation and three subpixel positions, the existing undersampled-star test, ordinary Gaussian noise, and isolated-hot-pixel suppression. The final real export changes 2,178 candidates instead of 5,140. It restores 2,962 native samples, including all 71 previously changed source samples above 10,000 ADU. All 42 saturated source samples survive. This threshold/count observation is specific to DS-B's selected frame, not a claim that every restored sample is a proven star.

Prepared review output: `outputs/M1_M33_star_safe.fits`. Independent evidence: `outputs/M1_star_safe_validation.json`. All source/reference fingerprints and previous exports are unchanged. M1 remains unapproved; no M2 work is authorized by these numerical checks.

## ADR-012: Replace the Rejected MHC Default with Unclipped VNG (2026-10-21)

**Status:** Engineering revision in response to the owner's second stellar-artifact rejection. VNG is the new M1 default/review candidate; owner visual approval remains pending. Supersedes the default-MHC choice in ADR-008 and specification v2.5. MHC remains explicitly selectable for reproducible diagnostics.

The second screenshot shows colored satellites after the core-erasure fix. Numerical evaluation reproduces them with MHC applied directly to untouched raw CFA: they arise from fixed cross-channel Laplacian corrections, independently of cosmetics or export scaling. A satellite near the reported red star has native R 4,858 ADU, but MHC estimates G -338.5 and B -3,162.625 ADU. Merely preserving bright native cores therefore did not establish acceptable reconstructed color. Do not conceal this failure by clamping samples, painting stars or applying a display stretch.

Implement threshold-based VNG independently in Rust/Rayon using geometric directional stencils rather than another implementation's lookup tables or code. Use four cardinal and four diagonal gradients on a 5x5 CFA window, threshold `min + max/2`, directional per-color means, and missing-channel color differences relative to the measured channel. Diagonal stencils are symmetric under quarter turns. Preserve the native sample exactly after unchanged conservative cosmetics. Remain in f32 and retain all negative/above-full-scale interpolation excursions. Border reflection keeps CFA parity and the existing eight-pixel composed support for future tiling.

Default `debayer-single` to `--demosaic vng`; keep `--demosaic mhc` as the rejected comparison baseline. The output carries the actual method in `DEBAYER`, FITS history and JSON `demosaic_method`. This adaptive reconstruction is not a linear operator on arbitrary images; its output is linear-light in the same fixed ADU/65,535 units, with no gamma or tone curve. Common gain/offset equivariance, unchanged native samples and unclipped excursions are tested explicitly. No new dependencies, intermediate images or later-milestone features are introduced.

The official PI source is useful for evaluating compatibility but is not the output contract: PI clamps reconstructed channels to 0..1 and its inspected non-green SE/SW diagonal formulas have unequal coefficients. FulgurStack uses symmetric coefficients and deliberately omits clipping. PI also uses a different border policy and the supplied reference has no equivalent cosmetic step. Bitwise identity is therefore neither promised nor required. The remaining 40 above-one samples in the real VNG export survive rather than being silently removed. Use the PI import adapter; its view IDs now include the output filename to avoid mixing review versions.

Validation: 40 Rust release tests and four adapter contract tests passed locally. New coverage includes all CFA phases, flat/fractional color, native samples, affine photometric equivariance, quarter-turn symmetry, a golden satellite neighborhood and overshoot from bounded sensor data. An independent Float64 prototype matches 1,024 sampled output positions within 0.000227 ADU; all 8,294,400 native samples match the prior conservative cleaning, including 42 saturated samples. In the reported red-star region, PI residual RMS falls from about 1,194 to 11.8 ADU. Its green aperture flux differs from PI by 0.024% and HFR by 0.039%; these are local diagnostics, not whole-field/stack quality approval. Whole-frame R/G/B residual RMS falls from 59.0/36.2/63.0 to 17.4/11.9/12.8 ADU. Residuals are method differences, not estimated additional noise.

Prepared output: `outputs/M1_M33_vng.fits`; evidence: `outputs/M1_vng_validation.json`. Release run: 0.463 s, 164.5 MiB peak working set. Source/reference fingerprints and all prior exports are unchanged. Numerical success does not override the owner's failed visual gates; the new candidate must be reviewed before M1 approval or M2.

References: [original MHC paper](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/Demosaicing_ICASSP04.pdf), [official PI VNG implementation and cited algorithm](https://gitlab.com/pixinsight/PCL/-/raw/master/src/modules/processes/contrib/spool/Debayer/DebayerInstance.cpp). Algorithm attribution: Edward Chang, Shiufun Cheung and Davis Y. Pan, *Color Filter Array Recovery Using a Threshold-Based Variable Number of Gradients* (1999). Directional arithmetic is formulated independently; no PI source tables/code or external C/C++ runtime are included.

## ADR-013: Prevent PixInsight Display Wrap Without Changing Scientific Samples (2026-10-21)

**Status: FAILED owner review; superseded by ADR-014.** The display STF did not remove cyan pixels. The following records the abandoned approach, not current instructions. Its transformed-copy test did not verify the actual viewport conversion path.

At x=1694, y=1409 the current VNG export and PI reference contain identical RGB values. Seven adjacent channel samples exceed one (six red, one blue). Their screenshot bytes exactly match rounded value times 255 modulo 256: for example R=1.04744029045 appears as 11 instead of 255. Permissive FITS import correctly preserves scientific data, but identity screen rendering can convert unbounded floats directly to bytes. Earlier API-double tests did not exercise this rendering path.

Enable a linked linear screen transfer through the native `ScreenTransferFunction` process: shadows 0, highlights `1 - 2^-20`, midtones 0.5, output range 0..1, and identity lightness. A highlight point slightly below one forces a non-identity clipping path before byte conversion; exactly one would retain the unsafe identity optimization. Display gain is less than one ppm. The process also enables STF when disabled and does not update pixel history. Do not apply a histogram process to the scientific view, clamp the export, introduce a pedestal, or normalize by frame extrema. Disabling/resetting STF can expose the upstream renderer behavior again; reopening through the adapter restores the guard.

Seven host/transfer-model tests cover the import contract, rejected formats, cancellation, process failure cleanup, transfer parameters and the seven measured overshoots. A separate native PJSR regression runs in a new PI automation instance without saved preference changes. All 24,883,200 loaded samples match a permissive baseline exactly, including all 40 above-one values and maximum 1.1716045141220093. Rendering a tiny in-memory display copy through PI's histogram/bitmap primitives reproduces all seven original wraps and gives corrected RGB bytes identical to the reference. This is native transfer verification, not automated capture of the actual viewport. Only JSON is saved; all created image windows are closed. The existing FITS SHA-256 remains `1298113d5582c9ff89a60ec69f3deac5e40347b73b7737a198115986c956b79a`.

References: [official STF process](https://gitlab.com/pixinsight/PCL/-/raw/master/src/modules/processes/IntensityTransformations/ScreenTransferFunctionInstance.cpp), installed `include/pcl/HistogramTransformation.h` and `include/pcl/PixelTraits.h`. The renderer's integer conversion and identity fast path are external PI behavior; this is a local interoperability workaround, not a modification of PixInsight itself.

## ADR-014: Owner-Approved Sensor Saturation in the FITS Output (2026-10-21)

**Status: ACCEPTED — M1/G1 PASS.** Implemented after explicit owner authorization and visually approved on 2026-10-21. Supersedes ADR-010/012's unbounded final-export requirement and removes the failed ADR-013 workaround.

The owner accepts limiting reconstructed colors to the normalized sensor range as compatible with the product's linearity/no-data-clipping requirement. This is a FulgurStack output contract, not a FITS standard limitation. PI VNG bounds interpolated channels to 0..1; the previous FulgurStack export deliberately retained 40 above-one channel values. A loading/display script did not eliminate the observed cyan pixels or satisfy ordinary opening.

Complete reconstruction at final export with `(source_adu / 65535).clamp(0, 1)` for both VNG and diagnostic MHC. Keep internal interpolation unbounded for diagnostics and never mutate the input RGB buffer. Supported Seestar native measurements are already within sensor range; only interpolated excursions change. Every in-range sample remains bit-identical. Fixed divisor, zero offset, common channel scale and the absence of a tone curve/extrema normalization remain unchanged. The operator has saturation at its limits: linear-light describes the approved product convention with sensor saturation, not global mathematical linearity through those limits.

Record `RGBBOUND='0:1'` and truthful FITS history. Report schema 3 distinguishes lower/upper limited reconstruction samples, their total `samples_clipped`, and final output out-of-range counts (zero). These count color channels, not rejected frames or cosmetic candidates. Preserve input files and valid GPS/observing metadata; keep coordinates private in diagnostics. Current FITS opens with File > Open; retain the importer only for legacy diagnostics and remove its failed STF guard.

Release: 0.503 s, 164.4 MiB peak working set, 16 threads. Candidate `outputs/M1_M33_vng_bounded.fits` has range 0.027237353846..1, 40 upper limits and no lower limits. All other 24,883,160 samples and all 8,294,400 native CFA samples match the previous export; all 42 saturated native samples survive. All manifest/reference/source hashes and GPS/date/exposure cards are unchanged; no temporary images remain. SHA-256: `d60369afde3915ebb3a46d1cd159e8e674f4ae351e7b06ed33ce09959118d411`.

Verification: 40 release Rust tests, warnings-denied Clippy/format checks and four legacy importer contracts pass locally. The new native PJSR regression uses ordinary import, identity STF and direct rendering of the actual bounded image, with no histogram process or pixel modification. All loaded samples match expected output; all seven reported stellar RGB triples and display bytes match PI. This avoids ADR-013's invalid inference from a transformed copy. The owner subsequently confirmed satisfactory visual inspection of the actual output on 2026-10-21, closing G1. Only final FITS and JSON reports are saved; no M2 work is started. See the acceptance fingerprint and CI evidence in docs/M1_REVIEW.md.

## ADR-015: M2 Green-CFA Quality Inspector and Provisional Weights (2026-10-21)

**Status:** Implemented after owner M1/G1 approval and explicit M2 authorization. Local automatic gates and DS-B/full DS-A benchmarks pass; owner manual review remains pending and platform CI is tracked in the draft PR. No M3 work is started.

Implement `inspect --input --report --csv` in the existing pure-Rust CPU workspace without new dependencies. Separate uncapped supported-star detections from a spatially balanced 250-star quality catalog. Use the averaged native-green plane for local-maximum detection and bounded stratified median/MAD sky cells; reject isolated unsupported peaks and saturated cores. Estimate centroids in source coordinates and evaluate finite-aperture cumulative half flux on the native-green lattice using phase-tabulated Lanczos-3 and fractional-area radial bins. This avoids CFA holes and the small-star width bias observed with simpler interpolation. Interpolation exists only inside measurement; no RGB frames or image intermediates are materialized on disk. Report moment FWHM honestly as Gaussian-equivalent, not a PSF fit.

Classify a dominant compatible target/filter/sensor cohort, so one first outlier cannot poison all later rows. Report per-file unsupported/invalid inputs instead of aborting the whole batch. Use the specified robust HFR/count/eccentricity defaults, a visible 0.05 px HFR scatter floor, at least five population comparisons and ten measured stars. Count-based rejection uses exposure-duration groups, while HFR compares the compatible population. Provisional weights normalize noise by exposure and are explicitly not measured transparency. M3 must apply matched-star normalization and finalize registration/weights; do not fabricate registration failure or atmospheric transmission values in M2.

Keep a fixed maximum of 16 simultaneous raw frames in batches. Reuse immutable FITS parsing with frame-local decoding, forward-only 1 MiB payload reads and no redundant seek after the header. On Windows add FILE_FLAG_SEQUENTIAL_SCAN only to read-only frame-worker handles, using the standard library; the portable macOS path remains dependency-free. Full-frame tests compare every source pixel independently and both decode modes. Smaller worker counts were measured but did not solve the full-set bottleneck. The final full command takes 14.342 s versus about 16.9..17.1 s before the forward-read change; cache is uncontrolled, so improvement is not attributed solely to one change. No system-wide cache or preference settings are changed.

Deliver privacy-safe JSON catalogs, a BOM/semicolon CSV (with formula-safe quoted text fields), and an English 32-row manual-review table. Rejections never move/delete sources. Local gates: 52 Rust release tests, four legacy adapter contracts, 192 Gaussian/CFA/phase cases with maximum HFR error 2.937751% (<3%), synthetic classification/mixed-exposure/privacy/cancellation checks, independent CSV/counter/catalog-median validation and all 34 DS-B source/reference fingerprints unchanged. Final DS-B selection: 27/32, five flagged for review. Full DS-A: 940/1,140; its complete command satisfies the <15 s gate. See docs/M2_REVIEW.md for exact artifacts, timings, limitations and reproduction.

Reference for the Windows per-handle hint: [Microsoft CreateFileW caching behavior](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew). This is an I/O performance hint, not an input-data or global-system modification.


## ADR-016 — M2 Acceptance and M3 Rigid Registration Plan (2026-10-22)

**Status:** M2/G2 accepted by the owner; M3 authorized, pending owner G3 review.

The owner approved the M2 CSV and explicitly closed M2 on 2026-10-22. Accepted implementation: `97c0569314f98da3ac2845e1d40b8dfa66cb320c`, Windows/macOS CI run `39682032218`. Begin M3 on `m3-rigid-registration`, based on M2; do not merge earlier PRs automatically.

M3 uses the existing original-CFA catalogs without RGB debayering. Choose a quality anchor in the middle half of accepted temporal ranks. Header timestamps define order and sessions separated by >6h; report the missing-time fallback. Sequential constant-velocity predictions seed direct session-anchor matching. Rotation-invariant canonical local triangle descriptors and rigid transform voting provide fallback, including 180-degree flips. Deterministic pair RANSAC and SNR-weighted Procrustes SVD enforce SE(2), with ten unique inliers and 1.5 px residual gates. Pure-Rust `nalgebra` (std only, no BLAS/C runtime) supplies SVD; `chrono` (std only, no clock/timezone backend) parses FITS ISO timestamps. No scale, affine or image-resampling fit is introduced.

Session-to-master transforms compose analytically with frame-to-session fits. Store local and bridge correspondences so residuals can be audited against their named catalogs. Use direct unsaturated master-star flux matches for separate forward/reverse flux medians; label the session-bridge product approximation when direct photometric overlap is insufficient. Exposure correction affects reported transparency only; photometric scale already includes exposure. Final weights use normalized noise and HFR once. Rejected M2 frames may register diagnostically but never contribute weight or coverage.

Exact transformed convex footprints include the strict bicubic source support. Sutherland-Hodgman intersection supplies the common polygon. Weighted polygon edge events yield exact horizontal center-coverage intervals; a histogram computes a largest safe rectangular canvas with O(width) working memory, for both common and >=80% final-weight coverage. This avoids pixel-by-frame coverage storage. Canvas coordinates refer to anchor pixel centers and may extend outside its sensor. Empty overlap has no common canvas. M3 writes JSON/CSV only; actual image warp and integration remain M4.

Verification includes rotation/noise/outlier sweeps, rejected-frame preservation, mixed-exposure photometry, two-session composition, brute-force small weighted-coverage checks, immutable-source/atomic-report/privacy CLI gates, and optional real-CFA rotation+shift tests performed entirely in memory. Actual DS-B/DS-A evidence and limitations are recorded in M3_REVIEW.md.


## ADR-017 — M3 Acceptance and Memory-Bounded M4 CPU Integration (2026-10-22)

**Status:** M3/G3 owner-approved; M4 implementation authorized, G4 pending product review and remaining quality gates.

The owner approved M3 on 2026-10-22. Accepted implementation `565d045db02e7348418118a917b61be47005ecba` passed Windows/macOS CI run `39686898966`. M4 builds on `m3-rigid-registration`; no earlier PR is merged automatically.

Use output-tile-major 256 x 128 spatial tiles instead of full-width strips. At large Alt-Az rotations this reduces waste in inverse-ROI bounding boxes, while preserving the required bounded-memory architecture. Each even-origin raw CFA ROI contains the full 8 px cosmetics/demosaic/Keys support. Reuse accepted M1 VNG and conservative cosmetics; verify exact agreement with full-frame reconstruction at sensor edges and internal seams. Decode signed big-endian i16 plus BZERO directly into f32, without BIAS subtraction. Batch at most 16 ROI reconstructions so nested Rayon tasks cannot retain an unbounded number of full decoded regions. No full registered images or disk intermediates are created.

Expose IntegrationEngine over raw source views, transforms, photometry and weights. A strict inverse Keys a=-0.5 footprint excludes invalid samples before photometric normalization and pixel rejection. Winsorized median/MAD estimates use up to three passes; final weighted means retain original surviving values. Use 1e-6 ADU robust-scatter floor on quantized constant stacks and bypass clipping below five samples. Neumaier f64 accumulation covers both numerator and denominator. Report low/high channel-sample rejection separately from frame rejection and geometric versus surviving coverage.

Account for one master, N-frame current-tile buffers, bounded ROI scratch and a 768 MiB runtime/catalog reserve. Enforce 1024..12288 MiB budgets, reduce tile height or fail before unsafe allocations, and check actual Windows peak working set during integration. No unavailable measurement is reported as zero. Multiple input directories share one combined inspection population and canonical-path deduplication. Read-only originals remain untouched.

Extend the atomic float writer with accepted exposure/count/crop provenance while preserving M1 exports. Fixed ADU/65535 output bounds remain final-only; no extrema rescaling or tone stretch is introduced. The accepted reference GPS is copied only into FITS, no false WCS is written, and EXPTIME remains the anchor intensity scale rather than the exposure sum.

The initial absolute analytic colored-Gaussian flux check failed because the accepted M1 VNG reconstruction already differs from the ideal profile (green about 4%, blue about 11% for that case). Separately measure the new integration contribution: it preserves the accepted reconstruction flux within 0.657% across fractional shifts/mixed exposures. Retain and disclose the failed stronger analytic result; do not weaken a threshold or claim absolute CFA photometric correctness. The predeclared actual PI master comparison uses unchanged flux/width/color/noise limits. M4/G4 remains pending owner visual review and complete reference-quality acceptance. See M4_TEST_PLAN.md and M4_REVIEW.md.


## ADR-018 — Median/Manual Reference, Reference Canvas and Final Diagnostic Maps (2026-10-22)

**Status:** Explicitly requested by the owner within M4; supersedes the default reference ranking and automatic coverage crop of ADR-016/017. G4 remains pending.

Filter by M2 quality acceptance, sort by full acquisition DATE-OBS in UTC (subseconds retained), then choose rank (N-1)/2. This defines the earlier central frame when N is even; equal times use stable source-name order. No quality-weight preference remains. If an accepted timestamp is missing or invalid, automatic selection fails with a request for --reference, instead of inventing acquisition order from filenames. Explicit references identify a single accepted input by path or unique filename; missing, ambiguous and quality-rejected references fail clearly. Standalone registration and stacking share this policy.

Default canvas is the exact reference FITS array, origin (0,0), including its existing bounds. Crop policies remain optional explicit commands. Keep strict inverse sampling validity and zero-fill genuinely uncovered output pixels; the map makes those pixels observable. No accepted exposure is blended with padded borders.

The owner requested two additional final outputs, explicitly overriding the earlier master/report-only output restriction for these diagnostic products. Use per-channel surviving sample fractions and upper-rejected sample fractions with ALL original deduplicated input frames as denominator. This avoids confusing used original data with final quality weights, and preserves RGB-dependent rejection. Export normalized 0..1 fractions for ordinary PixInsight import; 1 means 100%, with BUNIT=fraction, NINPUT and PCTFACT=100. Do not run the photometric ADU writer on diagnostic maps. Whole-frame rejection and missing coverage reduce used coverage but are not counted as high rejection. Final master saturation is not pixel rejection.

Count maps during the same tile rejection pass without registered disk images or a second scientific integration. Include all three final buffers in the memory ceiling calculation. Preflight all destinations, including derived map filenames, and reject aliases before publishing any output. Tests cover subsecond ordering, even medians, quality exclusion, manual/ambiguous references, unchanged reference dimensions, RGB count fractions independent of weights, invalid borders, exact aggregate rejection counts, tile invariance and map units.

Real DS-B tests exercise both median selection and the owner's original PI anchor. Details, performance and local product filenames are in M4_REFERENCE_MAPS_REVIEW.md.

## ADR-019 — Full Reference-Canvas Run and Overlap-Aware Profiling (2026-10-22)

**Status:** The owner approved the current DS-B master/maps visually and requested the full 1,140-frame DS-A run, profiling and optimization proposals. This authorizes measurement and analysis, not implementation of M5. Full-reference quality parity and the previously disclosed absolute analytic photometry limitation remain open.

Keep the accepted scientific algorithm unchanged. Add three non-overlapping integration wall timers: reconstruction batches (including I/O and task scheduling), sample gathering/rejection/compensated accumulation/maps, and final tile copying. Inspection, registration and final FITS writing retain their existing wall timers. Report the remaining processing time explicitly rather than forcing percentages to sum through overlapping timers. The complete external command timer includes startup and JSON serialization; the existing processing timer excludes JSON serialization.

Also sum per-ROI elapsed read, cosmetic, VNG and resampling/normalization times. These tasks overlap and nested Rayon scheduling may charge waits to their elapsed times: they are neither CPU samples nor additive wall-time partitions. Never label their relative totals as exact CPU or end-to-end percentages. This instrumentation is phase profiling, not a sampled call-stack profiler. Keep clocks outside pixel loops and verify byte-for-byte identity of all three DS-B FITS products against the owner-approved outputs.

Hash all 1,140 source files before and after the full run outside the timed interval. Independently reconstruct integer surviving/high counts from every Float32 map sample using rounding with an explicit quantization tolerance, since denominator 1,140 is not a power of two. Verify reference selection, reference dimensions, metadata/privacy, finite bounded samples, source integrity, no intermediate images and native ordinary PixInsight opening. Generated FITS and detailed reports remain local and ignored.

Optimization recommendations must follow the measured phase bottleneck. Distinguish code-inspection hypotheses from benchmarked improvements and Amdahl upper bounds from predicted speedups. A future GPU prototype must preserve bounded tiles, f32 reconstruction, accurate accumulation, and numerical/image parity. No GPU or GUI implementation is introduced in this revision.

## ADR-020 — Scaled External Geometry References Without Template Photometry (2026-10-22)

**Status:** Explicit owner authorization to extend reference registration to 33..300% and validate `RegisteringTemplate200pct.fit`; a revision within M4, not M5 or unconditional G4 sign-off. Supersedes ADR-016/018 input-only references for existing external FITS paths and rigid-only external alignment.

Keep all scientific inputs and native session/photometric registration unchanged. External template stars supply geometry only. A scale-invariant triangle index omits absolute triangle length in similarity mode; two-pair deterministic RANSAC and weighted proper-rotation Procrustes fit positive uniform scale/rotation/translation. Constrain final scale to [0.33,3], including endpoints, reject shear/reflection, require ten unique pairs and <=1.5 reference-pixel RMSE. Seed the native photometric anchor against the template, then refine every source directly against the template, preserving the native photometric transform separately. Native rigid matching remains on its original path to retain existing output behavior.

Stream one geometry plane from RGB/monochrome Float32 or signed16 FITS with scaling/orientation validation, without requiring science exposure metadata. RGB green is not a CFA mosaic even when stale BAYERPAT remains. A 96-million-pixel plane cap, checked dimensions/padding, cancellation and finite values bound the importer. Reference-only centroids are not input quality measurements or photometry. The external template contributes no input count, flux, acquisition exposure or weight. Preserve scientific intensity/exposure/GPS from the native accepted-median anchor and label the separate coordinate systems in JSON/FITS. Native input references still require quality acceptance; never silently reinterpret a rejected input as an external template.

Carry geometric scale through transform inversion/composition, exact footprints, ROI bounds and integration memory estimates. Default output keeps the template's exact dimensions/crop. Retain the specified Keys surface-brightness sampling without area multiplication. Report SIM(2), GEOMSCL and external-grid crop provenance; omit native detector pitch on the resampled grid and never copy false WCS. The existing source-normalized photometry policy remains unchanged. Downscaled interpolation is not a new absolute flux/alias-suppression guarantee; prior analytic photometry limitations remain disclosed.

Verification covers scale/rotation/outlier sweeps, invalid/reflected models, transform algebra, scaled tile boundaries/masks, oriented/malformed RGB imports, end-to-end geometry-only template semantics, unchanged unit-scale FITS and real 32-frame external-template registration/integration. Source/reference hashes and ordinary PixInsight imports are checked independently. See M4_SCALED_REFERENCE_REVIEW.md for measured evidence and owner commands.

Full-run follow-up to ADR-020: after owner visual approval of DS-B, the owner requested all 1,140 DS-A frames against the same 200% external reference without additional cropping. No scientific code changed. Automated execution, integrity, geometry/maps and native PI import pass: 1,140 registered, 940 integrated, exact 4000 x 4000 grid, 632.991 s complete command, 990.1 MiB peak working set. Details and remaining product review are in M4_FULL_SCALED_RUN_REVIEW.md.

## ADR-021 — M4 Owner Acceptance and First M5 Hybrid GPU Engine (2026-10-22)

The owner explicitly approved the full DS-A external-reference image, closed M4 and requested M5 GPU acceleration. Record acceptance without relabeling the earlier analytic colored-star counterexample or unverified equivalent PI workload as passes. Preserve the accepted CPU products and default behavior.

First accelerate cosmetics and VNG, which dominate the measured reconstruction workload, in bounded GPU ROI batches. Keep CPU f64 inverse geometry/photometry and the approved rejection/Neumaier accumulation for this slice. This avoids coupling shader numeric parity to geometric coordinate precision and still targets the main measured bottleneck. GPU warp/reduction and GUI remain later work. Reuse the same CPU-generated VNG direction plans in WGSL, not copied independent constants.

Use a pure-Rust fulgur-gpu crate with wgpu 26.0.1 (official MSRV 1.84, compatible with this workspace's 1.85 contract), WGSL, DX12/Vulkan/Metal, no default web/GLES/static-DXC or C/C++ build dependency. Optional pipeline/CLI feature gpu selects an explicit hybrid IntegrationEngine. Default build remains CPU; GPU requests fail on absent hardware or software-only adapters, with no silent fallback. Reuse buffers, split 16-ROI batches against device limits, measure complete transfer/compute/readback and reserve device/readback memory in the existing budget. CLI reports actual backend/stages and hardware.

Numeric gates are specified in PROJECT_SPEC section 7 before measurement. GPU-independent WGSL validation runs without an adapter; actual requested hardware gates must fail if initialization/parity fails. Start with real DS-B native and external-200% commands and preserve all source/reference hashes. Performance and GPU promotion follow measured quality gates, not theoretical GPU throughput.

Initial generic storage-indexed VNG shaders did not improve end-to-end performance. Specialize the exact shared CPU-generated stencils into constant WGSL window indices to avoid dynamically indexed local-array spills. Dispatch each Bayer phase separately so each wave follows a uniform stencil branch, including odd-size ROIs. No independent coefficient table or image cache is introduced. Keep the generic shader as the shader-validation baseline and validate generated WGSL as well. Report every measured revision and distinguish startup/cache effects from sustained reconstruction throughput. Explicit buffer telemetry includes transient overlap when buffers grow; driver allocations remain unmeasured separately from process RSS.

## ADR-022 — Reuse Pixel-Rejection Working Memory (2026-10-22)

The owner retained explicit CPU/GPU selection and authorized the first CPU-side optimization: reduce working-memory allocation during pixel rejection, verify exact master/map equality on DS-B, then benchmark all 1,140 DS-A frames against RegisteringTemplate200pct.fit. Keep this change within M5 and preserve the optional GPU path.

Use row-local ClippingScratch with preallocated f32 winsorized and absolute-deviation vectors sized to the accepted frame count. Clear and refill the deviation vector instead of allocating and freeing it during every clipping iteration of every output channel. Reuse both vectors across pixels and RGB channels. Keep input order, median selection, f64 deviation arithmetic followed by the same f32 conversion, clipping thresholds, iteration count, original-sample rejection and f64 Neumaier weighted sums unchanged. The same reduction path serves CPU and GPU reconstruction. Existing row-scratch memory estimates already cover these simultaneously live buffers; no full-frame cache, new dependency, precision reduction or image intermediate is added.

Predeclared gate: exact bit equality of output master, coverage and high-rejection FITS against the unmodified CPU baseline, plus identical scientific registration/selection and rejection counts. A differential unit gate compares the retained allocating implementation with reused buffers across sample counts 0..1,500, repeated population changes, fractional fields, saturated plateaus, outliers and four clipping configurations; both scratch addresses/capacities must remain stable. Run release tests for default and GPU features, real DS-B native/200% comparisons, then sequential full DS-A before/after commands with fixed 16 CPU workers and identical canvas/settings. Preserve sources/reference hashes and report wall phases, command times and peak RSS. Performance is measured, not promised; distinguish total time from rejection-phase time and historical timing from the new matched pair.

## ADR-023 — Measure and Tune Output Tile Geometry (2026-10-22)

The owner authorized tile size/shape optimization, exact DS-B master/map parity and a subsequent full 1,140-frame external-200% benchmark without cropping. Add explicit --tile-width (1..1024) and --tile-height (1..512) controls; extend the former height cap of 256 to permit larger square/tall candidates, retaining the same conservative memory-budget enforcement. Retain the current 256x128 default while measuring candidates. The existing memory planner may reduce requested height; report requested geometry in config and effective geometry in integration stats. Neither tiling nor a memory fallback may change the reference canvas or intensity/rejection mathematics. Preserve the 8-pixel source halo, even CFA ROI origins, global f64 sample coordinates, original source order and per-pixel accumulation order. CPU/GPU selection remains explicit.

Measure eight initial shapes in two different orders on all 32 DS-B frames at the 200% reference, then extend with 256x512, 384x384 and 512x512 plus interleaved 256x128/512x256 controls; retain unfavorable results. Select only shapes whose entire master/coverage/high-rejection FITS bytes match the accepted baseline. Rank repeated processing/reconstruction times and memory, then verify the selected shape on the native DS-B grid and benchmark it against 256x128 on the full DS-A set with fixed 16 CPU workers and unchanged scientific settings. Do not assume larger is faster or generalize one hardware/dataset measurement to every geometry. Promote a default only after parity and measured full-run evidence; preserve explicit overrides.

Extend exact verification with an explicit tile-change option that permits only requested/effective tile dimensions, tile count, planned bytes and decoded/corrected ROI work counts to differ. Those ROI counts include repeated halos and are not counts of unique sensor defects. Keep exact registration/quality/photometry, exposure, valid/low/high sample counts, export bounds and entire FITS byte equality mandatory. Add asymmetric/odd-edge scaled synthetic master/map comparisons and invalid CLI-dimension rejection. No image cache, intermediate files, precision changes or new rejection policy is introduced.

The 26-run DS-B search (11 distinct shapes) passed every exact byte gate. Select 512x512 as the candidate default for the final validation branch: the extended-series median was 20.853 s versus the interleaved 256x128 control's 25.431 s, with fewer decoded ROI pixels (165,253,188 versus 215,616,134). This is not yet full-set performance evidence. Validate native DS-B and retained GPU-mode byte equality before full paired CPU runs. Keep GPU adapter/driver/numeric-policy identity strict while allowing its explicitly enumerated transfer/buffer/timing counters to vary with tiling. Retain the candidate as the delivered default only if full scientific parity and performance gates pass; otherwise revise it. Advanced flags reproduce the old geometry and permit hardware-specific tuning without changing the image/canvas contract.

Full validation completed: retain 512x512 as the delivered default. The same-binary CPU pair measured 676.741/558.198 s processing and 481.696/367.423 s reconstruction, with 993.746/3412.633 MiB peak RSS. Exact full master/maps match the 256x128 control and the accepted M4 files; all 1,140 source hashes and scientific counts remain unchanged. Repeated ROI decoding fell 23.331%, consistent with the reconstruction improvement. Preserve the memory tradeoff and single-pair scope explicitly; no universal optimum or full GPU extrapolation is claimed. See M5_TILE_GEOMETRY_REVIEW.md for products, controls and full evidence.

## ADR-024 — Choose Verified Tiles from Available Physical RAM (2026-10-22)

The owner authorized automatic RAM-aware selection, a system reserve and manual control, explicitly reusing previously verified 128x128, 256x256 and 512x512 squares without repeating their performance search. Interpret the isolated 265x256 entry as the established 256x256 candidate.

Query physical memory after registration and before final-image allocation. Use max(2 GiB, ceil(total RAM / 10)) as additional system headroom, cap available-minus-reserve at the configured 1..12 GiB ceiling, and conservatively count existing pipeline allocations again rather than crediting them back. Select the largest fitting verified square using the established buffer model including all three images, source count, ROI concurrency, scale, GPU storage and runtime scratch. Never infer available RAM from total RAM, include swap, crop a canvas or change pixel math. Record the snapshot, reserve, ceilings, candidate estimates and decision in JSON and print the choice before integration. A missing measurement fails automatic mode, while manual mode clearly reports configured-ceiling-only enforcement.

Default/no flags and --tile-size auto mean automatic selection. --tile-size 128/256/512 means manual square; either old dimension flag means manual geometry, with 512 for an omitted companion. Forbid combining the square and rectangular controls. Replace old silent height shrinking with a clear failure when a manual request cannot fit. This makes user intent reproducible. The snapshot is not an OS memory reservation; dimensions remain fixed throughout the run. Monitoring/dynamic grid replanning is deferred because it was not requested.

Use sysinfo 0.33.1 with default features disabled and only system enabled. Its MSRV 1.74 fits the workspace 1.85 contract and its OS queries require no C/C++ build dependency. Refresh RAM only, without process lists, swap, identities or network enumeration. See the [memory API](https://docs.rs/sysinfo/0.33.1/sysinfo/struct.System.html#method.available_memory) and [published package manifest](https://docs.rs/crate/sysinfo/0.33.1/source/Cargo.toml). Check threshold boundaries, 2-GiB/10% reserve precedence, configured ceiling, exhausted/unknown memory, GPU and scale estimates, manual refusal, canvas edges, overflow and CLI conflicts. Run existing release scientific gates and one real automatic-mode smoke check; preserve source hashes and exact master/map bytes without another tile search or full-dataset benchmark.

## ADR-025 — Bounded Original-Byte FITS Blocks Within Measured RAM Headroom (2026-10-22)

The owner authorized a RAM-aware cache up to 1 GiB, exact DS-B master/map parity and a full uncropped DS-A performance comparison with RegisteringTemplate200pct.fit. Permit only original-byte blocks under a strict cap, superseding the earlier broad cache prohibition within this scope. Never cache reconstructed/registered datasets or write intermediates.

Choose tile geometry first; cache capacity is min(requested <=1 GiB, effective budget minus integration buffers minus bounded read transients). Disable if RAM is unknown or insufficient for one block per source plus overhead. Count entry/map/source overhead as well as payload within the cache budget and transient run/block buffers separately in total planning. Do not sacrifice a fast tile merely to fill a cache. Keep the system reserve and 12 GiB ceiling.

Use 128x128 source blocks with independent per-source LRU quotas and locks. A whole-frame LRU smaller than the source population would commonly evict frames before reuse; block quotas retain each source's recent spatial neighborhood across output tiles. Merge contiguous missing blocks into row reads, avoid loading row gaps, store unchanged signed-16 BE bytes and apply the exact existing i16+BZERO conversion only to the requested ROI. Cache origins do not redefine the reconstruction ROI or Bayer phase. All-hit reads skip file opens/reads but still validate size/mtime; misses open read-only and check again. Actual source checksums remain independently verified around real runs.

Expose --fits-cache-mib 0..1024, including uncached reference mode. The initial benchmark candidate default was 1024; retain default 0 after the measured performance regression described below. Report accounted capacity/payload/peak-sum, hits/misses/evictions, served/requested bytes, source-read bytes, calls/opens/metadata checks. The source-peak sum is a conservative upper bound, not a simultaneous RSS measurement. File-read bytes include block prefetch and OS cache service; do not label them physical SSD traffic. Compare one preserved release binary sequentially on DS-B and full DS-A with fixed 16 workers/512-square tiles and unchanged science. Require exact FITS/science/integrity gates; preserve unfavorable results and qualify cache/run-order variation. Optional GPU uses the same reader and retains its prior numerical policy.


Full paired result: exact master/map/source gates PASS, but cache-on processing was 604.148 s versus 512.633 s off (+91.515 s, +17.852%). Original ROI requests were unchanged; application source-read bytes rose 11,430,134,434 -> 14,511,738,880 and read calls 18,619,113 -> 25,858,816. Partial-block prefetch and quota evictions offset the retained-byte hits. Keep the feature available via an explicit --fits-cache-mib maximum with RAM-aware sizing, but set both CLI and library defaults to 0, preserving the faster measured path. This default-only policy change does not alter the explicitly controlled benchmark algorithms/settings or scientific products. Do not claim acceleration or generalize this single sequential pair to all hardware. Preserve negative small-run evidence as well.

## ADR-026 — Reuse Exact CPU Warp Coordinate Products and Contiguous Footprints (2026-10-22)

The owner requests CPU resampling optimization with the FITS cache disabled, exact DS-B master/map equality and a fresh full 1,140-frame before/after benchmark against RegisteringTemplate200pct.fit without cropping. Preserve the optional GPU engine; its existing CPU warp shares the optimized implementation.

Compute the inverse rotation sine/cosine once per ROI. Precompute the two f64 rotation products for each output column and compute the two row products once per row. Preserve Transform::apply's exact multiply/subtract-or-add/scale/translate evaluation order and integer global-coordinate construction. Do not use coordinate stepping, coefficient reassociation, approximate interpolation tables, fused arithmetic or reduced precision. The bounded column vector uses 16 bytes per tile column (at most 16 KiB per active ROI), covered by existing conservative runtime headroom. It is not an image or source-data cache.

Validate the same sensor/ROI footprint, then access four contiguous pixels per interpolation row with safe fixed-size slices. For valid positive coordinates, integer truncation is exactly floor and avoids redundant floor operations. Dispatch the unchanged Keys inner/outer polynomial directly for the four known footprint positions, retaining the original distance expressions and verifying integer/adjacent-representable boundaries independently against the prior branch-based implementation. For valid positive coordinates, integer truncation is exactly floor and avoids redundant floor operations. Dispatch the unchanged Keys inner/outer polynomial directly for the four known footprint positions, retaining the original distance expressions and verifying integer/adjacent-representable boundaries independently against the prior branch-based implementation. Preserve the Keys coefficient expressions, sixteen row-major f32 RGB contributions, f64 scalar photometric normalization, invalid NaN mask, cancellation checks and nonfinite error. No separable horizontal/vertical sum, source change, new dependency or scientific policy change is allowed.

The differential release gate retains the complete prior scalar sampler and geometry path as a test-only oracle. Compare every valid channel's bits and invalid masks over five scales (0.33..3), five rotations, negative/positive global origins, full/cropped ROIs, nonuniform high-dynamic-range/fractional/negative fields and nontrivial photometric scale/offset; verify cancellation. Existing scale/halo/flux/rejection/GPU hardware gates remain mandatory. For actual products require exact entire master/coverage/high-rejection FITS equality and unchanged scientific reports/counters, with all source/reference hashes preserved. The verifier may explicitly ignore only available-RAM snapshots and their derived effective headroom/candidate fit flags; configuration, allocation estimates, selected tiles, cache-off counters and total physical RAM/reserve remain strict.

Preserve the unchanged release binary from 8267d7c and build the optimized binary with the same locked compiler/profile/features. Run fresh sequential before/after pairs with 16 CPU workers, fixed 512-square tiles, 12 GiB ceiling, cache 0 and identical uncropped 4000-square canvas. Do not run builds/tests concurrently with measurements. Keep unfavorable measurements; report actual process/RSS and nonoverlapping wall phases separately from the overlapping per-task resampling timer. A single pair is observed evidence on this computer, not a portable speedup guarantee. M5 remains open.


Full verification PASS: 92/92 default and 93/93 all-feature release tests, explicitly required local GPU hardware gates and Windows/macOS CI. Entire DS-B/DS-A master/maps equal their fresh controls and prior accepted products; sources/reference hashes and all scientific/work counters remain exact. The fresh full CPU pair measured 523.466 -> 501.305 s processing (4.233% shorter), 344.081 -> 329.472 s reconstruction, and 3409.789 -> 3416.391 MiB peak RSS. Summed overlapping resampling task elapsed fell 6.091%, not an additive wall saving. Unchanged inspection/registration account for 6.094 s of total variation; do not credit all savings to the modified code. Retain the exact implementation with cache still off and preserve the preliminary slower DS-B candidate. See M5_CPU_RESAMPLING_REVIEW.md for complete evidence and limitations.

## ADR-027 — Generate Exact CPU VNG Kernels and Separate Interior/Border Access (2026-10-22)

The owner authorizes optimizing CPU VNG, exact 32-frame master/map verification and fresh full 1,140-frame before/after performance measurement with RegisteringTemplate200pct.fit, no crop and FITS cache explicitly 0. This remains an M5 slice; preserve the optional GPU engine and prior reconstruction/rejection/quality policies.

Move the existing independent geometric stencil factory to a standard-library-only source shared by the Rust build script and GPU coefficient packing. Generate four constant-index CPU pixel kernels for the four possible centered 2x2 channel cells. Generate coefficients with the same factory and serialize their exact f32 bits. Keep gradient contribution order and the original Iterator::sum behavior, threshold min + 0.5*max including ties, selected-direction sample order, channel sums, count division, missing-channel reconstruction and exact native-sample assignment. No handwritten duplicate coefficient table, approximation, fused/reassociated sums, alternate demosaic method or reduced precision is permitted. The build script only emits Rust into Cargo OUT_DIR and requires no external runtime/build dependency.

Select the two alternating kernels once per row and process adjacent interior pixels with direct contiguous 5x5 window loads. Reflect the five source rows once per output row and mirror x only for the first/last two pixels, preserving the prior reflected borders even for minimum/odd sizes and ROI halos. Keep row cancellation and input validation. Remove per-ROI CPU stencil allocation; retain the original GPU packing API/word order. No FITS/image cache, new frame buffers, source writes or image intermediates are introduced.

Retain the complete pre-optimization VNG implementation as a test-only independent oracle. Require exact pre-export RGB bits across all Bayer patterns, odd/minimum/asymmetric dimensions, flat/fractional/noisy/high-dynamic-range/negative fields, gradients and saturated colored stars, plus the actual real source before and after cosmetics. Compare every original GPU coefficient word and run the existing mandatory local hardware parity gates. Keep flux/phase/gain/offset/rotation/halo gates and known analytic-photometry limitations unchanged.

Preserve the d39ab46 release executable before edits and build both controls with the same locked toolchain/profile/features. Run default/all-feature format/lint/release gates, a fresh DS-B before/after exact master/coverage/high-rejection gate, then sequential full DS-A controls: CPU engine, 16 workers, fixed 512x512 tiles, 12288 MiB ceiling, cache 0, same external 4000-square reference at (0,0), sigma 3.0/2.5. No concurrent local builds/tests during measurements. Require identical science/work/I/O counters and source/reference hashes; only timing/RSS and explicitly allowed available-RAM snapshots/derived headroom may differ. Keep unfavorable results. Separate reconstruction/full wall time from summed overlapping ROI demosaic task elapsed. Measured benefit is not guaranteed or portable; M5 remains open.


Full VNG validation PASS: 94/94 default and 96/96 all-feature release tests, required local GPU hardware gates and Windows/macOS CI. All 32/1140 entire master/map files match fresh controls and prior accepted CPU/M4 products; source/reference hashes, science and work/I/O counters are exact. The fresh full CPU pair measured 510.694 -> 438.435 s processing (14.149% shorter), 334.964 -> 260.558 s reconstruction (22.213% shorter), 3422.172 -> 3404.977 MiB peak RSS. Summed overlapping VNG task elapsed fell 61.010%, not an additive wall saving. Unchanged inspection/registration saved 2.182 s and rejection/maps took 4.431 s longer; retain these variations without attributing them to VNG. Retain the implementation, cache default 0, explicit GPU control and previous limitations. See M5_CPU_VNG_REVIEW.md; M5 remains open.

## ADR-028 — Bounded Pixel-Major Blocks for Exact Pixel Rejection (2026-10-22)

The owner authorizes optimizing rejection data layout with exact 32-frame master/map verification and a fresh full 1,140-frame measurement against RegisteringTemplate200pct.fit, no crop, FITS cache explicitly 0. Preserve the optional GPU engine and all accepted scientific behavior; M5 remains open.

Retain the existing frame-major registered output tile, then copy only 32 neighboring output pixels into a small pixel-major RGB block per active reduction band. Read each frame's short run contiguously; destination stacks retain original frame order and exact f32 bits. Gather geometric red-channel validity/weights and independent per-channel finite samples from contiguous stacks. Keep the existing median/MAD, clipping thresholds/iteration count, original-sample rejection, f64 Neumaier accumulation order, coverage/high map definitions and final bounds unchanged. No full second tile, frame cache, original-byte FITS cache or image intermediate is introduced.

Partition a tile into at most min(Rayon workers,16) spatial bands, even if an external Rayon pool has more threads. Reuse packing, values and clipping buffers across all pixels/rows of a band. Grouping only integer counters and minima may change; per-pixel floating sums retain original source order. Cancellation is checked at every 32-pixel block. Explicitly include 32 * accepted_sources * workers * 12 bytes of worst-case packing storage in every conservative tile estimate, retaining all existing reserves and enforcing available-RAM/manual-budget rules. Full benchmark extra capacity is 5,775,360 bytes; small-set capacity 165,888 bytes. Report layout/block size/band cap/packing bound in integration telemetry. These are bounded working copies of the current tile, not retained FITS data.

The differential gate retains the prior direct frame-major reducer and compares exact master/map bits, valid/low/high/zero/min/max counts and f64 minimum geometric/surviving-weight bits across populations 1..1,500, short/partial/multiple blocks, nonzero offsets, four clipping settings, fractional/negative/saturated fields and independently invalid channels/NaNs/infinities. Input buffers remain bitwise unchanged; verify cancellation. Existing sparse/mixed-exposure/flux/halo/scaled/tile/GPU gates remain mandatory. Default/all-feature format/lint/release and required local real GPU tests must pass.

Preserve the unchanged 392bcf6 release executable and build the candidate with the same locked toolchain/profile/features. Verify entire DS-B master/map bytes, then run fresh sequential full CPU controls with 16 workers, fixed 512-square tiles, 12288 MiB ceiling, cache 0, same full external 4000-square grid at (0,0), sigma 3.0/2.5; no local concurrent builds/tests. Require immutable source/reference hashes/inventory and unchanged science, geometry and work/I/O counters. A narrow explicit verifier flag validates the new layout telemetry and exact calculated scratch allocation delta; timing/RSS and OS available-RAM snapshots/derived headroom remain the only other allowed differences. Do not silently relax cache/geometry/scientific comparisons. Report full/reduction/reconstruction wall and peak RSS, retain unfavorable measurements and qualify the single-pair/OS-cache/background-load scope. Retain the candidate only after measured exact-quality/performance evidence.


Full layout verification PASS: 95/95 default and 97/97 all-feature release tests, required local GPU and Windows/macOS CI. Entire DS-B/full-DS-A master/maps equal fresh controls and accepted products; source/reference hashes, science, geometry and work/I/O are unchanged. Extra planned packing capacity is exactly 165,888 / 5,775,360 bytes. Retain after the full pair measured 437.471 -> 430.546 s processing (1.583% shorter), 124.304 -> 118.462 s rejection/maps (4.699% shorter), RSS 3405.273 -> 3419.832 MiB. Preserve the 5.130% slower small-set rejection phase and slightly higher RSS. Unchanged phases vary; no isolated causal, universal acceleration or memory-reduction claim. Cache remains 0 and M5 open. See M5_REJECTION_LAYOUT_REVIEW.md.


## ADR-029 — Defer Cosmetic MAD Below the Unchanged Noise Floor (2026-10-22)

The owner requests faster cosmetic correction, exact DS-B master/map verification and a fresh full 1,140-frame CPU comparison with RegisteringTemplate200pct.fit, no crop and FITS cache 0. Preserve all accepted defect thresholds, stellar protection, Bayer phases, VNG, photometry, rejection and export policies. M5 remains open.

Retain the original generic eight-value total-order sort and median arithmetic. If the original sample is <= median + 5 ADU, it cannot exceed median + 5 * noise because the unchanged noise floor is >= 1 ADU. Skip only that sample's unnecessary MAD calculation; otherwise compute exactly the original MAD and candidate checks, including the rare 24-neighbor confirmation. Read the same eight samples through three row centers. Copy each immutable input row once, retain the unchanged four-pixel border and process only the interior. Stellar support can return as soon as two qualifying neighbors are found; its threshold and boolean result are unchanged. All decisions read the original immutable CFA. No new buffers, caches, dependencies or intermediates.

Retain complete previous scalar cosmetics as an independent test-only oracle. Compare exact corrected CFA bits/counts across Bayer phases, minimum/odd dimensions, fractional/noisy/gradient/bright-star/defect fields, noise-floor boundaries and a real input. Require all existing quality/local GPU gates, default/all-feature lint/release tests and entire DS-B master/coverage/high FITS equality before full measurements. Preserve unchanged bc3271d executable; use the same compiler/locked profile/features for both binaries. Run sequential CPU controls with 16 workers, 512-square tiles, 12288 MiB ceiling, cache 0 and the same uncropped 4000-square reference, with no concurrent builds/tests. Compare scientific/work/I/O/memory-plan fields and source/reference hashes strictly; only timing/RSS and explicitly allowed available-RAM snapshots may differ. Qualify single-pair measurements and overlapping task timers.

Retain unfavorable preliminary evidence: an exact 19-comparator sorting-network candidate passed bit gates but DS-B processing regressed 15.523 -> 16.646 s and summed cosmetic task elapsed 25.755 -> 37.068 s. Keep its local products/equivalence/binary; do not attribute that candidate's results to the final implementation or ship its slower sort. The final control pair uses separate output names.


Cosmetic verification completed: local format/default-all-feature lint and release gates PASS (98/98 default, 100/100 all-feature), required real local GPU gates PASS. Entire 32/full-1140 master/maps equal fresh controls and accepted CPU/M4 products; all sources/reference hashes, scientific/work/I/O and memory plans remain exact. Fresh full CPU processing 447.335/420.343 s (6.034% shorter), reconstruction 271.522/252.006 s, peak RSS 3411.090/3405.703 MiB. Summed overlapping cosmetic task elapsed is 18.258% shorter, not additive wall/CPU time. Retain the preliminary slower sorting-network evidence and all unchanged-phase variation; one pair with uncontrolled OS caches/load is not an isolated causal/portable guarantee. FITS cache stays 0, no crop and no quality-policy change. Remote Windows/macOS CI for this revision is BLOCKED by GitHub account billing/spending restrictions and is not claimed as passed. See [cosmetic review](M5_CPU_COSMETIC_REVIEW.md); M5 remains open.


## ADR-030 — Continuously Refilled Bounded CPU Reconstruction (2026-10-22)

The owner authorizes replacing CPU group barriers with continuous work assignment, exact DS-B master/map verification and a fresh paired full 1,140-frame run against RegisteringTemplate200pct.fit, without cropping or FITS cache. M5 remains open.

Use at most min(Rayon threads,16,source count) long-lived scoped tasks. Each claims one source index atomically, completes its entire immutable ROI reconstruction/warp and publishes to that source's one-write slot before claiming another. Nested Rayon work may execute other worker tasks but cannot create more logical ROI holders. Join all tasks and consume results strictly in original source order, preserving every image/sample bit, all timer/counter aggregation order, rejection and f64 Neumaier order. Never use an unbounded source parallel iterator or blocking permit acquired by arbitrary nested tasks. Stop new claims after an observed failure/cancellation and join/drop all live work before returning. Retain the existing GPU reconstruction batches.

Only current-tile output vectors and small source-index/result metadata are retained; raw/cleaned/RGB residency stays <=16 and existing conservative planning/reserve remains unchanged. Check measured process peak RSS after each worker-count group of completions and the final partial group, retaining the previous check count and budget. No image cache, extra image buffer, dependency, scientific/schema change or intermediate disk image.

Test forward progress past a deliberately delayed first source without waiting for its original group, exact source-order/bit publication under nested work with limits 1/3/16, exactly-once processing, cancellation/error cleanup and invalid limits. Require existing default/all-feature/local hardware GPU and real-source release gates. Preserve 4563b37 baseline and identical locked compiler/profile/features. Verify entire DS-B master/maps before fresh sequential full CPU controls, 16 threads, 512-square tiles, 12288 MiB, cache 0, complete 4000-square reference grid. Compare scientific/work/I/O/memory-plan/source hashes strictly; permit only timings/RSS and explicitly allowed available-RAM snapshots. Retain unfavorable measurements; distinguish nonoverlapping wall and overlapping task elapsed, with no portable or isolated causal speedup guarantee. Remote CI must be reported truthfully if account billing still blocks execution.


Continuous CPU scheduling verification: local format/default-all-feature lint and release tests PASS (101/101 default, 103/103 all-feature), required real GPU gates PASS. Exact whole DS-B and full DS-A master/maps match fresh controls and accepted CPU/M4 products; all input/reference hashes, science/work/I/O and memory-plan fields unchanged. At most 16 logical ROI workers continuously refill; nested work cannot exceed that bound and source-order publication is exact. GPU dispatch retains its original batches. Fresh full processing 408.271/383.990 s (5.947% shorter), reconstruction 246.191/220.943 s (10.255% shorter), peak RSS 3416.082/3398.227 MiB. Retain the slower small-set processing 15.885/16.558 s and all unchanged-phase variations; a single sequential pair with OS caches/background load uncontrolled is not an isolated causal/portable guarantee. Cache remains 0, full 4000-square reference without crop. Remote measured-code Windows/macOS CI is BLOCKED by GitHub account payment/spending restrictions and is not claimed as passed. See [continuous scheduling review](M5_CPU_SCHEDULING_REVIEW.md). M5 remains open.


## ADR-031 — Exact Four-Pixel AVX2 CPU Resampling (2026-10-22)

The owner requests the next CPU resampling optimization, exact 32-frame master/map equality and a full 1,140-frame paired measurement with RegisteringTemplate200pct.fit, no crop/cache. All work, tests and reports remain local; no commits, pushes, PR updates or remote CI. Preserve prior local policy edits and keep M5 open.

Runtime-detect AVX2 once per ROI, retaining the original scalar path on unsupported architectures/CPUs. Compute geometry/weights for four adjacent output pixels as independent f64 lanes, then accumulate two RGB triplets plus ignored register-only padding per f32 vector. Preserve exact f64 inverse coordinate expressions and Keys polynomials, separate multiply/add operations (no FMA), f32 weight conversion, all sixteen row-major f32 contributions per channel and f64 scalar photometric normalization before f32 output. Never change interpolation, precision, contribution order, invalid masks, pixels or final limiting. Mixed-validity groups and partial tails use the original scalar sampler.

Use standard-library intrinsics only. Audit unsafe boundaries: runtime CPU/OS feature detection; unaligned stores into exact four-element arrays; contiguous RGB addresses derived only after strict sensor/ROI and full backing-buffer footprint validation and exact integer-to-f64 coordinate range. Four-float loads require a proven next pixel; the last RGB triplet is assembled from exactly three checked values. Array RGB storage is contiguous; no out-of-buffer read or whole-image padded RGB conversion, whole-image cache or new image buffer. Temporary lane/weight arrays are fixed stack storage covered by existing runtime headroom; memory-plan estimates and <=16 logical ROI bound stay unchanged. GPU reconstruction stays optional and shares the CPU warp as before.

Require exact scalar/SIMD RGB bits/masks across scales 0.33..3, rotations/origins, full/cropped ROIs, minimum/odd/tail lengths, signed/fractional/subnormal/saturated fields, coefficient integer/adjacent-representable boundaries and the final backing-buffer pixel. Verify immutable buffers, malformed lengths and nonfinite failures, plus existing cancellation/halo/flux/clipping and actual local GPU gates. Require local SIMD explicitly with FULGURSTACK_TEST_SIMD=1. Preserve the unchanged b836b93 release executable and source fingerprints before edits; record uncommitted optimized source hashes and executable hashes. Run default/all-feature release/lint gates, fresh DS-B entire FITS equality, then sequential full CPU controls: 16 threads, 512-square tiles, 12288 MiB, cache 0, full 4000-square reference at (0,0). No concurrent builds/tests. Only timing/RSS and explicit available-RAM snapshots may differ; science/work/I/O/memory/source hashes remain strict. Retain unfavorable results and distinguish overlapping task elapsed from wall time. No portable or isolated causal speedup guarantee.


Preserve preliminary adverse evidence: indexed four-output AVX2 gathers passed all exactness gates but DS-B processing measured 15.309 -> 16.088 s, reconstruction 6.728 -> 6.937 s and summed resampling task elapsed 42.625 -> 41.513 s. Replace gathers with contiguous RGB loads and paired RGB vector registers; keep the original candidate binary/source/timings/gate locally and use separate final output prefixes. Do not credit an affected task-timer reduction as total wall acceleration.


SIMD CPU resampling local verification PASS: 104/104 default and 106/106 all-feature release tests, format/lint, explicitly required AVX2 and real GPU hardware gates. Whole DS-B/full DS-A master/maps equal fresh controls and accepted CPU/M4; all source/reference hashes, scientific/work/I/O and memory-plan fields remain exact. Runtime AVX2 computes four coefficient sets and paired RGB accumulations with original operation order and scalar fallback. Fresh full processing 387.618/350.600 s (9.550% shorter), reconstruction 222.171/184.033 s (17.166% shorter), peak RSS 3399.652/3391.395 MiB. Summed overlapping resampling task elapsed is 46.892% shorter, not additive wall/CPU time. Keep the slower preliminary gather candidate and unchanged-phase variations. One pair with OS caches/load uncontrolled is not an isolated causal/portable guarantee. Cache stays 0, full 4000-square reference without crop. All work/tests remain local and uncommitted; no pushes, PR updates or remote CI. Source fingerprints and binary hashes identify the measured local code. See [SIMD CPU review](M5_CPU_SIMD_REVIEW.md); M5 remains open.


## ADR-032 — Certified Exact Pixel-Rejection Fixed Point (2026-10-22)

Certify when another winsorization iteration cannot change the median or MAD. Retain the two central ranks from each existing selection. Both rounded f32 clipping endpoints must enclose the central value ranks, and their absolute f64-to-f32 deviations must be at least the upper central MAD rank. Then monotone clamping preserves both central ranks of both distributions, reproduces identical f64 thresholds and is idempotent after f32 rounding. Skip only the remaining scratch-copy iterations, never rejection of original samples or compensated accumulation. Otherwise execute the original up-to-three-iteration loop. Retain finite endpoint checks, signed-zero/rounding behavior, sigma floor, arbitrary positive configured clipping, independent RGB masks and all science/work/memory/cache policies. No new allocation, dependency or schema.

Require an independent frozen previous median/clipping oracle, central-rank/rounded-bound tests and broad finite-float, signed-zero, subnormal, extreme, asymmetric-threshold/population sweeps. Run all local default/all-feature release/lint, SIMD and real-GPU gates; verify entire 32-frame master/maps and strict report/source fields. Then measure a fresh preserved-baseline/optimized full 1,140-frame pair, RegisteringTemplate200pct.fit, reference canvas without crop, CPU 16 threads, 512-square tiles, 12288 MiB, cache 0. Record uncommitted source/binary hashes, retain adverse results and single-pair limitations. No commits, pushes or remote CI. M5 remains open. See ADR-032.


Pixel-rejection optimization locally verified: format/default-all-feature lint, release tests 106/106 and 108/108, explicitly required GPU/AVX2 PASS. Exact entire 32/full-1140 master/maps equal fresh controls and accepted CPU/M4; all input/reference hashes, scientific/work/I/O/memory fields preserved. Certified median/MAD fixed point skips only redundant scratch winsorization; original iterations remain the fallback. Full processing 366.351/322.559 s (11.954% shorter), rejection/maps 123.116/83.337 s (32.310% shorter), peak RSS 3400.324/3407.215 MiB. Keep higher small-set RSS and unchanged-phase variations; one pair with OS caches/load uncontrolled is not an isolated causal/portable guarantee. Cache 0, full 4000-square reference without crop. All work remains local/uncommitted; no pushes, PR updates or remote CI. See [pixel-rejection review](M5_CPU_REJECTION_REVIEW.md); M5 remains open.


## ADR-033 — Exact SIMD CPU Cosmetics with Stored Baselines (2026-10-22)

The owner requests CPU cosmetic optimization with exact 32-frame master/map verification and one full 1,140-frame measurement against RegisteringTemplate200pct.fit, no crop and FITS cache 0. Reuse the stored pixel-rejection run (322.5585164 s) and its immutable source/product fingerprints as the performance/scientific baseline. Do not execute another baseline stack. This explicit instruction supersedes fresh-pair requirements for this task. Subsequent routine optimizations should also reuse stored controls unless the owner authorizes a new reference run; separate historical observed differences from isolated causal or portable speedups.

Runtime-detect AVX2 and process eight independent neighboring CFA pixels per vector, retaining the scalar portable path, original total-order median/MAD arithmetic, candidate thresholds and exact scalar 24-neighbor/star-support confirmation. Preserve all signed float/zero behavior, four-pixel border, tails, cancellation, original reads, correction counts and all following reconstruction/integration operations. Use only standard-library intrinsics and fixed-size stack arrays; no added image allocation, cache, dependency or memory-plan change. Validate full row/backing footprints before unsafe loads and require exact scalar/vector outputs on broad finite data, stellar/defect/boundary sweeps and real sources. Preserve GPU option and actual local GPU parity gates.

Run local format, default/all-feature lint and release suites with actual AVX2/GPU required. Compare whole master, coverage and high-rejection FITS and strict scientific/work/I/O/memory reports against stored DS-B outputs, then run only the optimized full DS-A and verify stored full outputs and all source/reference hashes. CPU engine, 16 threads, 512-square tiles, 12288 MiB ceiling, cache 0, complete 4000-square reference canvas. Record binary and uncommitted source fingerprints. No commits, pushes, remote CI or intermediates; retain unfavorable evidence. M5 remains open. See ADR-033.


SIMD CPU cosmetic correction locally verified: format/default-all-feature lint, release suites 109/109 and 111/111, explicitly required real GPU/AVX2 PASS. Eight independent pixels use reversible f32 total-order integer keys and exact median/MAD arithmetic; scalar confirmation, borders, tails and portable fallback remain unchanged. Exact whole DS-B/full DS-A master/maps match stored accepted CPU/M4, source/reference hashes and all scientific/work/I/O/memory fields preserved. Reused stored baseline without reexecution: full processing 322.559/282.965 s (12.275% shorter), reconstruction 186.705/142.527 s (23.662% shorter), RSS 3407.215/3399.676 MiB. Cosmetic overlapping task elapsed is 86.078% shorter, not additive wall/CPU time. Retain unchanged-phase/small-set adverse variations; historical comparison with OS caches/load/temperatures uncontrolled is not isolated causal/portable acceleration. Cache 0, no crop, only new-version stacks, all local/uncommitted, no pushes or remote CI. See [SIMD cosmetics review](M5_CPU_COSMETIC_SIMD_REVIEW.md); M5 remains open.


## ADR-034 — Shared RGB Rejection Preparation (2026-10-22)

Reuse the stored 282.9648593 s SIMD-cosmetics run as the performance/scientific control. Run only new-version DS-B and full DS-A, RegisteringTemplate200pct.fit, reference canvas without crop, CPU 16 threads, 512-square tiles, 12288 MiB, FITS cache 0. No baseline reexecution, commits, pushes or remote CI. M5 remains open. See ADR-034.

Prepare finite RGB channel values and source weights together in one source-order traversal. Use separate contiguous f32 channel arrays and a shared contiguous f64 weight sequence while RGB masks agree. At the first independent channel mask, copy prior shared weights into the two already-reserved channel weight arrays and continue independent filtering/weights in the same traversal. Clear/reuse all storage per pixel; no raw-data cache. Compute geometric red-channel compensated coverage in this pass. Each color still computes its own exact certified median/MAD thresholds, rejection counts, original-survivor Neumaier weighted sum and maps; preserve operation/source order and f32/f64 precision. Retain prior tuple-based production combiner/reducer as test-only bit oracles. No unsafe code, dependency, image allocation or schema change.

At most 44 bytes per source per band cover all three f32 value arrays, three f64 weight arrays and two f32 clipping arrays. This remains within the existing conservative 64-byte source-scratch allowance per worker; fixed Vec headers remain within runtime overhead. Preserve the existing 32-pixel packing capacity and <=16 bands/ROIs, system reserve and all allocation estimates. Require bounded-capacity/reuse tests, late independent-mask/empty/sparse/shared RGB tests, exact dense-versus-old combiner and full reducer RGB/map/statistics differentials, cancellation and immutable input checks. Run default/all-feature local format/lint/release plus required actual AVX2/GPU and real FITS gates; require whole 32/full master/maps and scientific/work/I/O/memory/source equality against stored controls. Record uncommitted source/binary fingerprints and historical-performance limitations, including adverse unchanged-phase observations.


Shared RGB rejection preparation locally verified: format/default-all-feature lint, release suites 112/112 and 114/114, required real GPU/AVX2 PASS. One source-order RGB/coverage gather, shared weights with exact independent-mask fallback, dense per-channel clipping/Neumaier and unchanged scientific rules. Reserved payload 44 bytes/source/band fits existing 64-byte scratch allowance; plans unchanged. Entire 32/full master/maps match stored accepted CPU/M4, all source/reference hashes and science/work/I/O/memory fields exact. Reused stored baseline without reexecution: full processing 282.965/275.913 s (2.492% shorter), preparation/rejection/maps 89.296/82.613 s (7.485% shorter), RSS 3399.676/3417.559 MiB. Historical comparison with OS conditions uncontrolled; preserve unchanged-phase/adverse variation, no isolated causal/portable claim. Cache 0, no crop, only new-version stacks, all local/uncommitted, no pushes or remote CI. See [shared RGB review](M5_SHARED_RGB_REJECTION_REVIEW.md); M5 remains open.


## ADR-035 — Bounded Parallel External Registration (2026-10-22)

Reuse the stored shared-RGB result (275.9134115 s) and accepted FITS/report/source fingerprints; execute only the new version on DS-B then full DS-A with RegisteringTemplate200pct.fit, complete reference canvas/no crop, CPU 16 threads, 512-square tiles, 12288 MiB, FITS cache 0. All work stays local/uncommitted; no pushes, PR updates or remote CI. M5 remains open. See ADR-035.

Parallelize only independent per-source external similarity refinements after the unchanged serial native-anchor/external catalog/bridge computation. Zip matching source and report rows in disjoint contiguous Rayon chunks, with at most min(pool threads,16) chunks; never create more concurrent catalog fits than sixteen, including in a larger caller pool. Update original source positions directly without intermediate full images/result queues. Retain the complete original per-frame solver calls, prediction composition, operation order, match ordering, quality/failure decisions, native photometry, metadata and footprint calculations. Skip rows with no native transform exactly as before. Join all tasks on error/cancellation; check cancellation before work and per row. A row-count mismatch fails explicitly. Keep final weight normalization, polygon/canvas planning, exposure/report reductions and all later integration sequential/source-ordered as before. No dependency, image/cache allocation, schema or integration memory-plan change; bounded concurrent catalog scratch fits existing runtime headroom.

Require frozen prior sequential-loop comparisons of all-field row snapshots and explicit transform/residual bits across pool sizes 1/3/16/32, scales/rotations, missing/outlier stars, absent native transforms, quality-rejected rows and external failures. Verify cancellation, malformed lengths and immutable source catalogs. Run all local default/all-feature format/lint/release plus required real GPU/AVX2 and FITS gates. Entire 32/full master/coverage/high FITS, scientific/work/I/O/memory fields and source/reference hashes must match stored controls exactly. Record uncommitted code/binary fingerprints and observed historical processing/registration times, retaining adverse/unmodified-phase variation and no isolated causal/portable speedup claim. Do not run another baseline stack.


Parallel external registration locally verified: format/default-all-feature lint, release suites 114/114 and 116/116, actual GPU/AVX2 required PASS. Disjoint Rayon source-row chunks cap concurrent external fits at sixteen; original solver/prediction/row arithmetic and all serial source-ordered planning/reductions retained. Exact whole 32/full master/maps match stored accepted CPU/M4; all input/reference hashes, scientific/work/I/O/memory fields preserved. No baseline rerun: full processing 275.913/300.919 s (9.063% longer), registration 25.259/26.244 s (3.899% longer), RSS 3417.559/3391.301 MiB. Keep unfavorable small-set and unchanged-phase variations. Historical comparison with OS conditions uncontrolled is not isolated causal/portable acceleration; registration timer includes native/external/catalog/canvas work. Cache 0, full reference/no crop, only new-version stacks, all local/uncommitted, no pushes or remote CI. See [external registration review](M5_EXTERNAL_REGISTRATION_REVIEW.md); M5 remains open.


Interpretation: functional/exactness gates PASS; acceleration is NOT ESTABLISHED. Both stored-baseline comparisons were slower, including total registration. The full run spent 22.792 s more in unchanged reconstruction/rejection phases, so the 25.006 s total increase cannot be assigned to the external-registration change. Existing timing does not isolate the external fit loop; retain that limitation and the requested parallel implementation without claiming a measured speedup. No extra baseline/new-version reruns were performed to seek a favorable result.


## ADR-036 — Registration Phase Diagnosis (2026-10-22)

Measure only the 32-frame DS-B set against RegisteringTemplate200pct.fit with CPU 16 threads, complete reference grid, 512-square tiles, 12288 MiB and FITS cache 0. Keep stored controls; no baseline or full-1140 rerun. Add non-overlapping wall intervals for native selection, geometry/photometry, weight/footprint preparation, both native canvases, external FITS read/validation/catalog, anchor bridge, parallel source refinements, weight preparation, both external canvases and finalization. Preserve every scientific operation and source order. Parent totals include every interval; report outer unattributed overhead separately. Never treat overlapping worker time as wall time or infer full-set phase shares from 32 sources.

Verify default/all-feature local format/lint/release and required GPU/AVX2/FITS gates, exact 32-frame master/maps and strict scientific/work/I/O/memory/source fields against stored products. Permit only explicitly validated phase telemetry, timings/RSS and existing available-RAM snapshot differences. Record local binary/source hashes and adverse evidence. Diagnose the measured parallel scope, unchanged serial work and historical timing limitations before recommending another change. No algorithm optimization, cache, image intermediate, dependency, commit, push or remote CI. M5 remains open; see ADR-036.

The first coarse profile identified the native geometry/photometry envelope as dominant. Refine only its telemetry into session bridges, per-source native fits/methods, direct master correspondence search, photometric ratios and residual bookkeeping, with exact parent partition validation. Retain both 32-source diagnostic runs, without historical baseline or full-set execution.


Registration diagnosis verified locally: 115/115 default and 117/117 all-feature release tests, format/lint and required actual GPU/AVX2/FITS gates PASS. Both coarse and refined 32-source diagnostic stacks preserve exact entire master/maps, source/reference hashes and strict science/work/I/O/memory fields. Refined processing 11.735369 s, inspection 0.237584 s, registration 1.604270 s, peak RSS 668.996 MiB. Thirty native triangle-fallback fits cost 1.0552255 s (65.776%); parallel external refinements only 0.0210790 s (1.314%). All four rectangle proposals cost 0.3443040 s. The sparse subset has a 631.963 s median acquisition gap; independent replay finds fewer than ten initial pairs for 29 fallback rows. Do not extrapolate this frequency to dense full data, infer the former serial-loop duration, or claim exact causality for historical slowdowns. Native fallback is the next measured target; no algorithm/quality change is made by this diagnostic slice. Cache 0, no crop, no baseline/full-set rerun, commits, pushes or remote CI. See [registration profile review](M5_REGISTRATION_PROFILE_REVIEW.md); M5 remains open.


## ADR-037 — Exact Registration Fallback Preparation and Scratch Reuse (2026-10-22)

Optimize native fallback registration without changing any scientific arithmetic, correspondence order, deterministic candidate/ranking ties, transforms, prediction history, quality or photometric decisions. Lazily prepare immutable destination triangle descriptors and their ordered bucket index once per matcher; reuse the master target and at most one session target, releasing them before integration. Store triangle indices in each bucket in the original insertion order. Source descriptors and vote traversal/ranking remain unchanged. Direct predicted successes must not construct fallback descriptors. No global catalog retention, FITS/image cache, dependency or additional intermediate. The bounded catalog structures fit the existing runtime headroom; integration allocation estimates and 16-ROI concurrency remain unchanged.

Reuse bounded consensus inlier storage across hypotheses with two vectors and exact original candidate order, residual sums and best-solution comparisons. Do not change trial count, early-exit rules, precision or error thresholds. Retain the full prior consensus/triangle/fallback implementation as an independent test-only oracle (only test visibility may differ), checking all solution fields and transform/residual bits over rigid/similarity scales, rotations, missing/outlier/duplicate stars, predictions, failures, cancellation, target lifetime/laziness and real CFA catalogs.

Run local default/all-feature format/lint/release plus required actual GPU/AVX2/FITS gates. Compare entire 32-frame master/coverage/high FITS and strict scientific/work/I/O/memory/source reports with the stored refined diagnostic (11.7353692 s; registration 1.604270 s). Then execute only the new full 1,140-frame CPU run against RegisteringTemplate200pct.fit, full 4000-square reference canvas, no crop, 16 threads, 512-square tiles, 12288 MiB ceiling and FITS cache 0. Reuse the last full stored control (300.919385 s; registration 26.2439527 s); it predates detailed phase telemetry. Do not execute either baseline again. Preserve whole-source/reference hashes and exact full products, record uncommitted source/binary fingerprints, retain adverse/unchanged-phase observations and historical-comparison limits. All work local; no commits, pushes, PR updates or remote CI. M5 remains open; see ADR-037.


Fallback optimization locally verified: format/default-all-feature lint and release suites 118/118 and 120/120 PASS, actual local GPU/AVX2/FITS gates PASS. Frozen prior fallback/consensus and real catalogs preserve all solution bits; 32-frame native methods and entire 32/full master/maps, source/reference hashes and strict science/work/I/O/memory-plan fields remain exact. Stored controls were not executed again. DS-B registration 1.604270/1.407678 s; the same 30 fallback fits 1.0552255/0.9328137 s. Full processing 300.919385/247.672711 s (17.695% shorter), registration 26.243953/18.753631 s (28.541% shorter), RSS 3391.301/3397.922 MiB. The optimized full profile contains 368 native fallbacks (13.1752424 s) and 771 direct fits (2.5122917 s); no older full subphase timer exists for comparison. Retain higher RSS and 45.756352 s of full time variation outside registration; this historical comparison does not isolate causality or guarantee a portable speedup. Cache 0, full 4000-square reference/no crop, only new-version dataset runs, all local/uncommitted, no pushes or remote CI. See [fallback review](M5_REGISTRATION_FALLBACK_REVIEW.md); M5 remains open.


## Publication Record (2026-10-22)

The owner explicitly authorized one consolidated commit and push of the completed, locally tested CPU optimization batch on m5-gpu-reconstruction. This lifts the publishing hold for this batch only. Historical reviews retain their measurement-time local/uncommitted statements. GitHub Actions remain disabled, no remote CI is required or claimed, and M5 remains open. The tested Rust/Cargo source is unchanged; this publication adds the aggregate evidence and policy clarification. Source/reference FITS, generated outputs, binaries and Python bytecode remain excluded. See [batch summary](M5_CPU_OPTIMIZATION_SUMMARY.md).

## ADR-038 — Windows Workflow Proposal and Thin Native UI Boundary (2026-10-22)

The owner requested Windows UI design and selected quick stacking with optional quality review. Propose Workspace navigation with Focused as an alternative. New projects retain CPU, accepted chronological-median reference, exact reference canvas without crop, automatic RAM-aware tiles and cache off. Optional review stops after inspection and before registration. Retain explicit GPU selection and previously computed result provenance. STF changes only display; channel-dependent coverage/high-rejection maps use fixed percentage legends.

Use the existing planned eframe/egui native shell over current pipeline APIs. The next implementation slice needs bounded structured progress, cooperative cancellation and a resumable inspection-to-registration handoff; do not parse console text, repeat inspection on navigation or duplicate the scientific engine. Worker-owned large buffers and bounded previews share the memory ceiling. No pause/resume promise or all-output transaction is implied. See [Windows UI design](WINDOWS_UI_DESIGN.md) for scope, validation and acceptance.

This task produces a locally reviewed interactive proposal using downsampled accepted M33 outputs and saved quality measurements. Its job sequence is simulated, not a native executable or new benchmark. No Rust, dependency or scientific-output changes, commits, pushes or remote CI. M5 remains open; native implementation and product acceptance are separate gates.

## ADR-039 — Native Workspace with Continuous Quality Review (2026-10-23)

The owner authorizes the first Windows application using Workspace, continuous Quality scrolling, navigation/settings combined on the left and the selected immutable subexposure on the right. Use pinned eframe/egui 0.31.1 with Glow, AccessKit and native rfd dialogs, compatible with the workspace Rust 1.85 baseline. This does not introduce an external C/C++ processing runtime. Keep CPU as the default and the existing optional GPU engine behind the GUI gpu feature; there is no silent backend substitution.

One background worker owns the scientific job and existing pipeline allocations. Bounded channels carry lightweight Quality summaries and final downsampled previews. A latest progress snapshot reports real phase/complete tile counts; registration and inspection use indeterminate progress until completed. Expose the existing inspection/discovery APIs and a register_inspected continuation so optional human review retains original catalogs, does not rerun measurement and validates source size/modification identities before continuing. Preserve scientific operation order. Add a completed-tile observer to both existing integration backends; unchanged callers retain the original path.

The GUI orchestrates the same reference canvas, reconstruction, rejection, accumulation and no-clobber FITS writers. It does not duplicate the numerical engine or parse console output. Report review wait separately from processing time. Output publication remains per file; cancellation/failure names any final products already published and never removes them. Source previews use at most one temporary native VNG frame in one background task, cap source size at 16 million pixels and join/cancel before integration. Retain only <=640-edge float thumbnails and display textures; no image intermediates or reconstructed-frame cache. Bounded UI state/previews occupy the existing 768 MiB catalog/runtime reserve, not an additional unplanned full-resolution copy.

Native shell verification includes a local explicit --ui-smoke mode for a supplied input/reference and unused output folder. It exercises actual Quality review, source preview, continuation and final master/map display, captures only the application's own framebuffer and exits. Scientific products must match stored accepted 32-frame CLI products exactly. No full-set rerun, commits, pushes or remote CI; M5 stays open and untested platform/native accessibility gates remain explicit.

Local verification PASS: 124 default / 126 all-feature release tests, format/default/all-feature lint and explicitly required real FITS/AVX2/GPU gates. Final native 32-frame flow completed in 11.9451358 s processing plus separately reported 0.5856406 s review wait, 818.1328125 MiB peak working set, 27 accepted / 5 rejected, 4000-square external reference without crop/cache. Entire master and both maps match stored CLI bytes and strict science/work/allocation fields; only automatic tile policy, OS RAM availability and UI/timing/RSS metadata differ. All input/PI/template hashes remain unchanged. Initial failed native smoke evidence is retained; the observer path avoids console writes in window-only processes and worker panics now surface as terminal errors. The final report borrows large catalogs and streams JSON instead of duplicating them in a JSON tree. See [native review](M5_WINDOWS_APP_REVIEW.md); local delivery evidence, binary/source hashes and screenshots are under outputs/M5_GUI_native_review_v3. M5 and owner native product review remain open.

## ADR-040 — Distinguish Review Wait from Active Processing (2026-10-23)

The owner reported an apparent 180-second quality hang. The supplied screenshot shows Quality review, Continue stacking and an explicit pre-registration message: measurement had finished and the worker was waiting for approval. The defect was the running spinner/elapsed display, hidden review checkbox and weak action emphasis. Preserve the requested optional review and existing CPU/GPU arithmetic. Move the review checkbox directly above the main action, keep it off by default, and show a static approval message and prominent Continue stacking in both Quality and Processing. Suppress idle/wait spinners and periodic repaint requests; retain indicators for active background processing and source loading only. Disable ordinary widget transition animation under the owner's animation policy. Exclude the current and accumulated human wait from displayed processing elapsed, as the worker report already does.

Verify a real headless button click resumes the parked worker, a long review wait is excluded from processing time, idle/review screens request no animation heartbeat and the quick path requires no approval. Extend the explicit native QA flow to capture the waiting Processing screen; add a separate quick-mode QA flow. Validate both on the existing 32-frame set against stored CLI master/maps without a new baseline or full-set run. Tests, builds and artifacts stay local without commits, pushes or remote CI. M5 remains open.

## ADR-041 — Percentage Progress and Night-Sky Appearance (2026-10-23)

The owner requests UI-only iteration: display determinate phase completion as a percentage and replace neutral window backgrounds with very dark night-sky blue (#071523). Preserve the actual completed-work fraction and phase names, with no inferred overall time estimate. Apply a fixed dark theme with coordinated panel, window, input, alternating-row and button colors. Keep animation restricted to active background work.

Until the owner revokes the UI/UX policy, skip automatic functional tests, scientific regression runs and benchmarks for UI-only edits. Build the local release application for manual product review. This does not waive scientific verification for future engine changes. No commits, pushes or remote CI; M5 remains open.

## ADR-042 — Real Frame-Based Quality Progress (2026-10-23)

Expose an optional inspection progress observer after each completed frame measurement, with the deduplicated input count as denominator. Serialize counter updates and callbacks so parallel completions cannot publish decreasing progress. Keep existing CLI callers on the unobserved path and retain measurement arithmetic, bounded batching, source/result order and final population classification. Quality-rejected or unreadable files that produce a diagnostic row count as processed inputs; cancellation/errors do not fabricate completion.

Connect the GUI observer for standalone Inspect quality and inspection before stacking. Show percentage progress on the Quality screen and reuse the existing percentage bar in Processing. Before discovery finishes, label preparation with an active-work indicator. No progress animation persists after the worker ends or while waiting for approval. This telemetry-only change follows the owner's manual UI review policy: local release build, no automatic functional tests or dataset runs, commits, pushes or remote CI. M5 remains open.

## ADR-043 — Registration Percentage from Completed Work (2026-10-23)

Add an optional registration observer and connect it to the existing Processing percentage bar. Native registration counts each completed source row and one finalization step. External registration additionally counts the reference read, catalog, anchor bridge, each completed external row (including explicit skips) and finalization. Total work is N+1 for native references and 2N+5 for external references, where N is the deduplicated input count. Serialize callback publication for parallel external fits. Never reset the percentage between passes or report 100% before successful finalization. These percentages measure completed work units, not expected remaining time; preparation can hold a percentage steady.

Preserve solver calls/order, source/result order, selection, photometry, geometry, memory bounds, report schemas and numerical operations. Existing CLI calls retain the unobserved path; errors and cancellation do not synthesize completion. The owner explicitly requests only UI/UX work with local release build and manual review: no automatic functional tests, dataset runs, commits, pushes or remote CI. M5 remains open.

## ADR-044 — Elapsed Time and Approximate Completion Estimate (2026-10-23)

Show Elapsed and Estimated remaining in Processing using HH:MM:SS. Exclude human review wait and freeze elapsed time after the job ends; approval wait has a static paused label and cancellation has no countdown. Store phase start and last completed-progress timestamps only in the GUI worker snapshot, outside scientific reports. Existing numerical engines and their reports remain unchanged.

On a fresh run, future registration/integration costs cannot be inferred from inspection frame counts. Show Calculating with an explanation until at least one integration tile and one second of measured progress are available. Estimate the full integration duration from completed tiles, subtract time already spent and add an initial saving allowance of 5% of integration time, with a two-second minimum. Disclose that heuristic in the tooltip; it includes all output files and is not measured write throughput. During output publication, refine from completed file writes. Unequal tile/file costs, report serialization and background load can move the estimate in either direction. Never show zero while unfinished work is known; successful completion shows zero. No persisted timing profile, new dependency or dataset run is introduced.

The owner requests UI/UX work only: prepare a local release build for manual review, without automatic functional tests, benchmarks, commits, pushes or remote CI. M5 remains open.

## ADR-045 — Modal Processing and Per-Phase Times (2026-10-23)

Use egui's centered Modal with a blocking backdrop for Processing instead of a workspace page. Automatically open for inspection/stacking, retain elapsed/ETA and progress, and list all four phases with live or frozen durations to tenths of a second and clear Running/Complete/Not started/Stopped states. The Processing navigation action reopens the last run summary. Bound modal width/height to the viewport and scroll shorter windows.

Keep phase durations only in the GUI worker snapshot, freezing them on transitions and terminal outcomes. Quality review has no scientific phase clock; dismiss the modal for source review and reopen on continuation. Successful stacks expose Results automatically, standalone inspection returns to Quality, and failure/cancellation retains the final message/timings until dismissed. Explicit Cancel run requests the existing cooperative cancellation and remains visible during cleanup; backdrop/Escape does not dismiss an active job. This is cancellation, not a resumable pause. Block workspace shortcuts behind the modal and make the existing close-during-run confirmation modal as well.

No scientific engine, result pixel, report schema, input or output policy changes. Follow the owner's UI-only manual-review policy: local release build, no automatic functional tests, benchmarks, commits, pushes or remote CI. M5 remains open.

## ADR-046 — Automatic GUI Output Versions and Optional Map Files (2026-10-23)

The owner requests repeated runs without replacing earlier products and independent optional coverage/high-rejection files. Select an available output family before processing, retaining the requested name when free and otherwise trying _v2, _v3, etc. If the requested name already ends in _vN, continue that sequence instead of nesting suffixes. Check the master, report and enabled map destinations plus their .tmp siblings with symlink_metadata; propagate permission errors rather than treating them as collisions. Preserve the existing atomic no-clobber writers for publication, including protection against a late external collision. Display the selected master filename in Processing and Results, and track the actual selected family for partial-publication reporting.

Add independent Output checkboxes, off by default, for saving coverage and high-rejection maps. Master and report are always saved. Write progress counts only the selected products; disabled map filenames are null in the GUI report and disabled files have no Open action. Retain existing bounded integration, maps and display thumbnails in memory; mark unsaved map previews explicitly. This changes publication policy, not numerical processing or a claimed memory optimization. The explicit native QA mode enables both maps to preserve its comparison scope. CLI defaults and atomic writer semantics are unchanged.

Keep the owner's local manual-review workflow: prepare the release application without automatic functional tests, dataset runs, commits, pushes or remote CI. Update the existing collision expectation to the newly authorized versioning policy without executing the test suite. M5 remains open.

## ADR-047 — Local Last-Used Application Settings (2026-10-23)

Store a versioned JSON snapshot of input paths, output folder/base filename, reference mode/path, map-saving flags, review option, engine choice, tile selector and memory ceiling. Use %LOCALAPPDATA%/FulgurStack/settings.json on Windows, the user's Application Support folder on macOS and XDG configuration on Linux. Store absolute selected input/output paths so another launch directory does not reinterpret them; preserve unique manual reference names when they are not existing filesystem paths. Explicit launch inputs override the restored input selection only. Do not automatically inspect, preview or resume work. Catalogs, image pixels, results, timers and modal state remain transient.

Restore bounded JSON with defaults for missing fields and validate UI ranges. Missing files use defaults; unreadable, oversized, malformed or unsupported-version files produce a visible notice and defaults without preventing startup. Merely opening/closing does not replace a failed-to-load file. Debounce actual preference changes by 600 ms, flush before starting/continuing a job and on normal exit, and surface save failures without affecting scientific work. A pending save requests one delayed repaint, with no idle heartbeat or animation. Do not repeatedly save while running with locked configuration controls.

Reuse the existing pure-Rust tempfile dependency for a private temporary JSON file, synchronize and atomically replace only this preferences file. Scientific FITS/report writers retain their separate no-clobber policy. Settings stay outside source data and Git. Explicit native QA uses a transient constructor so it never reads or changes the owner's preferences. The owner requests local release preparation and manual UI review only: no automatic functional tests, dataset runs, commits, pushes or remote CI. M5 remains open.

## ADR-048 — Fixed STOP/Close Actions and Terminal Modal Refresh (2026-10-23)

The owner screenshot shows completed processing with the bottom action clipped. The action was inside the scroll area, making both interruption and dismissal unreliable to discover. Move a prominent STOP button to a fixed modal header and place only details in a separately identified scroll area with an explicit viewport height reserve. STOP uses the existing cooperative cancellation; show disabled Stopping during cleanup. Terminal or idle summaries have Close in the same fixed location, independent of whether the worker handle has finished teardown.

Keep successful completion authoritative: close the automatic modal, switch to Results and explicitly request a repaint after terminal events/worker teardown, even when the processing heartbeat has ended. Prevent legacy Processing page conversion from reopening the automatic modal after success; intentional Processing navigation may still open the frozen summary. Keep backdrop/Escape dismissal available for terminal summaries while active processing requires an explicit STOP. Do not infer an unproven scientific failure from the screenshot; no engine, output or settings behavior is changed.

Prepare a local release application for the owner's manual review only, without automatic functional tests, benchmarks, commits, pushes or remote CI. M5 remains open.

## ADR-049 — Reusable Quality Assessments, Manual Selection and Compact Processing

The owner authorizes reusing completed inspections, actual manual include/exclude decisions and a Processing modal without scrolling. Transfer an immutable Arc of the full inspection from both standalone and in-run inspection to the workspace. Keep at most one baseline assessment; clear it on changed input selection. Before reuse, rediscover/sort/deduplicate canonical inputs and compare exact membership plus each saved size/modification timestamp. No source-image/FITS-byte cache is introduced. Recheck before registration after review, and fail explicitly if sources changed then. A new inspection clears stale overrides. Changing output, maps, reference, backend or tile settings does not invalidate measurements. A valid already inspected selection proceeds directly to registration rather than repeating review/measurement.

Clone only the bounded catalog/report metadata for each registration, retaining the untouched baseline for repeated runs and reset. Derive Clone for existing inspection structures without changing measurement arithmetic. Mark reused Inspect as Reused/zero duration in the new run and record the original measurement duration separately. Do not count a previous run's inspection time as new processing work. Catalog copies contain no raw/RGB frames and remain within existing runtime/catalog headroom; FITS cache stays off. No performance or exact-output test result is claimed for this manual-review iteration.

Add per-frame Use checkboxes and Use automatic selection. Filters only affect visibility; manual state is carried independently into continuation and new stacks. Soft HFR, eccentricity and relative-star-count rejection may be overridden, but incompatible, unreadable or unusable measurements cannot be forced in. Record explicit decisions and original automatic reasons in the GUI report, apply accepted counts/exposure/provisional weights consistently, and let the existing registrar enforce fit/photometric validity. All original deduplicated inputs remain the map denominator. Keep preferences persistence separate: manual decisions and inspection catalogs are retained only in the current application session.

Remove the Processing ScrollArea, use compact local spacing, and keep all four rows plus STOP/Close and timing visible together. Truncate long output/status text with full hover text rather than expanding the dialog. Automatic completion still opens Results; a manual frozen summary remains closable. Follow the owner's explicit local workflow without automatic functional tests, dataset runs, commits, pushes or remote CI. M5 remains open.

Owner manual acceptance (2026-10-23): **PASS** for quality-assessment reuse, manual frame selection and Processing without scrolling. This closes review of these three UI behaviors only. No automatic functional tests, scientific parity runs or benchmarks were added; M5 remains open and further development remains local without commits or pushes.

## ADR-050 — Display-Only Image Navigation and Native Viewport Detail (2026-10-23)

Use one shared image-view component for selected subexposures and results, with cursor-anchored wheel zoom, pointer-delta drag panning, Fit/double-click reset and a 1:1 action. Define 100% as one original image pixel per physical screen pixel, accounting for Windows/egui display scaling. Preserve independent source/result view positions, retain result alignment when switching diagnostic layers, and reset when a different source or newly completed result is loaded. Clip drawing to the image viewport and clamp panning so the image remains reachable. Ordinary input redraws are not idle animation.

Keep the bounded 640-edge overview for immediate navigation. After 120 ms without a viewport change, load only the visible native rectangle in a cancellable single-worker task; cap it at 8,388,608 pixels, 8192 per axis and the actual texture-device limit. Source detail uses the existing read-only phase-preserving CFA ROI reader and accepted cosmetics/VNG with an eight-pixel halo. Result detail reads the existing published float RGB FITS rectangle, validating its top-down unscaled header/dimensions and file identity. Apply the overview's fixed display-only STF parameters to detail, avoiding a brightness change as the viewport moves. Nearest texture sampling preserves visible native pixels. No scientific core, exported sample, cache setting or output file is changed.

Retain at most one ready display crop and one bounded request per view. Cancel/join preview tasks and release detail textures before starting/continuing scientific work. Use overview fallback while navigating/loading or when the viewport exceeds the bound; label it explicitly rather than claiming native detail. Unsaved diagnostic maps retain their overview and cannot enable 1:1 because their full buffers have already been released. Saved maps use the same native reader and fixed 0..100% channel display. Source/result FITS remain read-only; no intermediate images or observing coordinates are written/logged.

Prepare a local release application for manual owner review. Do not run automatic functional tests, scientific stacks or benchmarks, and do not commit/push. This navigation slice awaits manual acceptance; the prior three-behavior UI acceptance remains intact and M5 remains open.

## ADR-051 — Compact Desk, Persistent STOP and Frozen Result Telemetry (2026-10-24)

The owner approves native implementation of the Compact Desk revision with the wider continuous subframe table and narrow right preview, rejecting the intervening enlarged-preview revision. Move navigation to a compact top band and supported settings to their own tabbed page. Keep the virtualized table without pagination; collapse the optional HFR plot initially and allow hiding/resizing the narrow preview. Organize existing settings under Basic, Alignment, Rejection and Performance without silently enabling unimplemented canvas, cache or science-parameter capabilities.

Use native maximization and a minimum client area of 800 x 460 logical points, replacing the oversized 1000 x 650 minimum. The review target is 1366 x 768 physical pixels at 125% (about 1093 x 614 logical screen points before taskbar/chrome). Respect OS DPI and work-area sizing; never force a compensating zoom factor or claim native DPI acceptance from compilation. Preserve readable 13-point body/button text and bounded source/result detail loading.

Keep the non-scrolling Processing modal, with STOP above its complete body. Show all four phase rows with actual worker progress, elapsed phase times and Reused inspection state. Back to workspace/Escape hides the modal without cancelling; a persistent bottom-bar STOP supplies the cancellation action whenever background work is active. STOP retains cooperative cancellation and cleanup; terminal dialogs have Close. A successful completion remains authoritative and opens Results automatically.

Store only a small UI-side snapshot of the completed request and four phase durations, separately from current/new job progress. Retain the inspection-reuse flag with that snapshot. Results and their Phase times summary always describe the saved run, including while another job runs or changes setup. Finalize durations again when the existing worker freezes its progress during teardown. Preserve real accepted/input/exposure counts, exact output grid, no-crop policy and saved/unsaved map paths; never substitute prototype values. Do not change worker/scientific arithmetic, dependencies, output formats or input files. Defer new native detail reads during active integration, retaining existing overview previews.

Owner verification policy remains local release preparation and manual UI review, without automatic functional tests, scientific regression stacks, benchmarks, commits, pushes or remote CI. Historical acceptance is retained but is not a pass for this new layout; M5 remains open.

## ADR-052 — Single-Owner Progress and Run Actions (2026-10-25)

The owner reports simultaneous inspection progress in Quality and Processing and requests an audit of other duplicated UI. Make Processing the sole renderer of scientific phase progress, including unknown-total preparation; remove Quality's copied inspection bar/spinner. Registration, integration and output publication already share Processing's phase rows and need no additional renderer. Keep source-preview loading feedback separate because it describes a different bounded read-only operation.

Preserve the bottom panel's geometry, but leave its contents hidden while Processing or exit confirmation is open. The modal supplies STOP/Close and telemetry; on dismissal the workspace resumes bottom-bar STOP, elapsed time and optional review continuation. Request a repaint on dismissal so those controls return even for terminal/static states. Remove the extra bottom Processing launcher because top navigation already supplies it. Keep the Quality review prompt descriptive; its Continue/STOP controls appear only in the bottom bar. Saved-result phase summaries remain distinct from a later live run.

Exit confirmation temporarily owns the modal surface as well: suppress Processing behind it without clearing the saved open state, so Keep working restores the previous presentation rather than stacking two dialogs.

This is presentation-only: worker events, cancellation, phase measurements, scientific arithmetic, input/output files and settings are unchanged. Prepare a local release with formatting/lint only; automatic functional tests, scientific stacks, benchmarks, commits, pushes and remote CI remain excluded under the owner's policy. Native acceptance remains manual.

## ADR-053 — Owner-Selected Application and Desktop Icon (2026-10-25)

The owner selects the three navy/cyan photographic layers with aligned white stars and a continuous gold lightning bolt entirely in front of the stack. Retain the approved transparent artwork in the GUI assets directory, together with reproducible 256-pixel PNG and multi-resolution ICO exports. Supply 16/20/24/32/40/48/64/96/128/256 pixel representations for Windows desktop and DPI requests, without redrawing the approved composition.

Set the native viewport icon explicitly from the embedded PNG, replacing the framework default. Embed the ICO as the executable's first Windows icon group through an MSVC-only build step using the installed Windows SDK resource compiler. Respect an explicit RC override and locate the installed SDK when building outside a developer shell. No additional crate, external C/C++ runtime, scientific operation or layout change is introduced. Non-Windows builds retain the native viewport PNG and skip Windows resource compilation; macOS packaging remains unverified.

Prepare a local GPU-enabled release and a current-user desktop shortcut pointing to that executable with its embedded icon. The shortcut uses the repository as its working directory. Check formatting, GUI lint and release compilation, then inspect the packaged Windows icon without starting scientific work. Native title-bar/desktop appearance at 1366 x 768 / 125% remains an owner manual review step. No automatic functional tests, stacks, benchmarks, commits, pushes or remote CI; M5 remains open.

## ADR-054 — Centered Application Startup (2026-10-25)

The owner reports top-left startup placement and requests a centered window. Explicitly enable the framework's DPI-aware startup centering on the primary monitor and start in the normal window state, superseding ADR-051's automatic maximization. Keep the Compact Desk 1060 x 520 logical client size, 800 x 460 minimum, OS scaling and normal user-controlled maximize/resize behavior. Do not introduce a hardcoded screen position or change the selected icon, desktop shortcut or processing behavior.

Prepare the updated local GPU-enabled release with formatting and GUI lint. Actual centered placement, frame/taskbar clearance and 1366 x 768 / 125% appearance remain manual owner review. No automatic functional tests, scientific stacks, benchmarks, commits, pushes or remote CI. M5 remains open.

## ADR-055 — Compact Quality Table, Range Highlighting and Batch Use (2026-10-25)

The owner approves the compact Excel/PixInsight-style table proposal. Render 20-logical-point virtualized rows with alternating backgrounds, full-row highlighting and a separate active-preview marker. Keep the header/filter row vertically fixed and use a shared horizontal offset for header and data. Pin Use to the right of the quality data, before the vertical scrollbar. Drag column dividers to resize, double-click to fit content; fit the filename column to the complete measured names initially. Widths, filters and highlighting are local UI state; no additional persistence/schema is introduced for them.

Expose the already-measured background ADU and noise ADU/s in GUI rows without measuring or changing source data. The columns are Frame, Exposure, Captured, HFR, Stars, Eccentricity, Sky, Noise, Weight, Status, Reason and Use. Every column can sort and filter: case-insensitive text, inclusive numeric min/max, status predicates and Use on/off. Combine active filters with AND, accept comma/dot decimal separators, display invalid ranges in red and show active-filter indicators. Missing numeric measurements do not satisfy an active range. Filters only change the view.

Track highlighted paths separately from manual scientific Use decisions and from the single active preview row. Click selects one, Ctrl-click toggles membership, Shift-click selects an inclusive range in the currently sorted/filtered list, Ctrl+Shift adds a range, Ctrl+A selects visible rows and Escape clears highlighting while the table has focus. Keep the anchor by path across sorting; filter edits clear highlighting/anchor. Right-click an unmarked row first selects that row; right-click an already marked row preserves the group. Show separate Shown, Marked and Use counts. The footer's accepted count is labeled explicitly to avoid confusing it with highlighted rows.

Use on/off/toggle and restore-automatic act on highlighted visible paths only, through the existing FrameSelection boundary. Unusable/incompatible frames cannot be enabled; summarize skipped items. Disable scientific-selection edits during active non-review work and beneath modals. Removal records a session-local hidden path and a Use-off override. Keep original inspected rows and source membership for unchanged report/map denominators; never delete/move input files or silently re-enable hidden rows through the all-listed automatic reset. Restore removed returns them to the list with Use still off; a fresh inspection or changed inputs resets this session-local state. Refreshing GUI rows indexes existing choices without changing arithmetic or first-choice precedence. The scientific report-selection/registration/integration paths are unchanged.

Retain Compact Desk sizing, centered startup, bounded preview behavior, persistent bottom STOP and the Processing modal's fixed STOP. The Quality table adds no progress display. Local formatting/lint and GPU-enabled release compilation only; automatic functional tests, scientific stacks, benchmarks, commits, pushes and remote CI remain excluded. Native 1366 x 768 / 125% acceptance is manual; M5 remains open.

## ADR-056 — Folder Column and Tail-Preserving Quality Text (2026-10-25)

The owner requests Folder as the first Quality column and keeping the ends of Folder, filename and Captured readable when narrowed. Derive the full parent directory and standalone filename from each row's existing source path for display, filtering, sorting and content fit. Keep the original report filename/path and scientific identity unchanged. Hide the Windows canonical-path prefix in the displayed folder only; preserve drive/UNC semantics. Folder has the same text filter, resize and double-click fit behavior as the other text columns; initial filename fitting still uses complete names.

Use normal left alignment when these three strings fit. If their measured text width exceeds the padded cell width, anchor the complete text to the cell's right edge and clip its leading portion. Do not slice or rewrite the strings, values or filter inputs. The full source path and Captured value remain in the row tooltip. Keep numbers right aligned and other text unchanged. Identify the shifted structural columns explicitly so pinned Use, filters, sorting and filename fitting stay attached to the correct fields.

This is UI-only, preserving 20-point virtualized rows, selection/Use behavior, 1366 x 768 / 125%, fixed STOP and centered startup. Prepare a local GPU-enabled release with formatting/lint, leaving native interaction/appearance to owner review. No functional tests, scientific runs, benchmarks, commits, pushes or remote CI; M5 remains open.

## ADR-057 — Whole-Second Time Display (2026-10-25)

The owner requests whole seconds without fractional digits. Apply display-only truncation to Captured timestamps, exposure labels and completed processing totals. Keep dates and timezone suffixes intact. Phase clocks now use the same whole-second HH:MM:SS format as elapsed/acquisition clocks. Apply the policy consistently in the Quality table, row tooltip, source preview, Processing, Results and completion status. Do not change non-time quality values, numeric filter inputs or RAM/percentage precision.

Preserve all original timestamps, exposure values, measured durations, sorting/filtering inputs, scientific arithmetic, FITS metadata and JSON/CSV precision. Prepare a local GPU-enabled release with formatting/lint; native appearance remains owner review. No functional tests, scientific runs, benchmarks, commits, pushes or remote CI; M5 remains open.

## ADR-058 — Single Quality Toolbar and Footer-Owned Status (2026-10-25)

The owner identifies duplicated Use on/off/toggle commands in the Quality toolbar and adjacent Marked menu, and requests a compact header with status in the main footer. Keep one Quality control strip: direct Use on/off/toggle, Marked with only automatic-for-marked/remove, Clear filters and More for distinct list-level actions. Preserve the full row context menu as the contextual access path. Remove the repeated Quality heading, inline measurement description, inline counts and review prompt. Preview/HFR plot toggles remain in the existing global View menu rather than appearing twice on the same screen.

On Quality, the existing 50-point main footer owns Shown/listed, Marked, Use and selected acquisition exposure. Its second line identifies CFA measurements, provisional/final weights, active filters, removed rows where applicable and the current status/settings notice. Show a distinct registered acceptance count only when final acceptance differs from input Use choices. Derive displayed/marked counts through the exact same filter predicate as the table, without sorting or modifying selection. Footer labels truncate with full hover text instead of growing into the fixed actions; keep tight local line spacing. Review status is stated once in the footer, with existing Continue/STOP actions to its right. Other pages retain their existing accepted-run summary.

Processing/exit modals continue to own actions/status while open, suppressing the underlying footer contents. STOP remains fixed and reachable during active work. Preserve 20-point rows, filtering, range marking, pinned Use, tail clipping, whole-second display, native sizing and scientific behavior. Local formatting/lint and release preparation only, with native appearance/interaction left to owner review. No functional tests, scientific runs, benchmarks, commits, pushes or remote CI; M5 remains open.

## ADR-059 — Phase Frame Outcomes and Explicit Diagnostic Map Previews (2026-10-26)

The owner requests completed frame counts in Register/Integrate Status and reports a seemingly black High Rejection preview when map saving is off. Read-only diagnosis of the latest stored `outputs/master_v8_report.json`: 1140 inputs, 1140 successful geometric registrations, 981 eligible/accepted integration inputs and 159 quality exclusions. Both optional map filenames are null and the output family contains only master/report. Upper rejection is 725586597 channel samples, a roughly 1.326% RGB-wide mean against the 1140-input map denominator. This establishes nonzero upper rejection and no saved maps, not the exact selected-channel thumbnail appearance. No reference stack or new processing run is performed.

Add small UI-only phase outcomes to live Progress and Completed. Publish Register counts after registration/source preparation and Integrate counts only after successful integration. Registration uses all-input geometric success/failure, including diagnostic registration of quality-rejected frames. Integration reports the submitted source count, zero frame-processing failures after a successful call, and separately excluded inputs. Pixel rejection is never counted as frame failure. Failed/cancelled integration has no fabricated completed totals. Completed summaries retain these counts independently of new jobs. Bound/wrap the Status cell to preserve the non-scrolling modal and fixed STOP. Quality's final eligibility label must not imply that accepted-for-stacking equals all geometric registrations.

Keep the existing map computation and independent FITS-save flags unchanged. State in Output that both maps are always previewed in memory and save flags control additional FITS files only. Give both Results maps the same saved/unsaved wording tied to the completed run. Open map FITS targets the selected map and is disabled if that map was not saved; it must not open the master while a map tab is active.

Improve display only: Coverage defaults to 0–100%, High Rejection to 0–5%, with explicit selectable 0–1/5/10/100% display ranges. Preserve true map fractions, units and original-input denominators. Use the same display transform for the overview and saved native detail, keyed/cancelled with the chosen range. Retain bounded 640-edge previews; the High Rejection overview stores each bin's channel maximum so narrow rejected features do not vanish through point sampling. Label this peak-preserving overview explicitly. Scan the existing in-memory maps once for per-channel full-resolution min/max/mean/nonzero statistics, retaining only these summaries and previews. State when the selected channel is truly zero. Do not allocate or retain another full map/frame or change scientific pixels/writers/reports. Master/source STF remains unchanged.

This remains UI/projection work under the owner's local release/manual-review policy: formatting/lint and GPU-enabled release preparation, no automatic functional tests, scientific stacks, benchmarks, commits, pushes or remote CI. Native 1366 x 768 / 125% acceptance remains manual; M5 remains open.

## ADR-060 — Use-Selected Registration and Isolated Optional Diagnostics (2026-10-26)

The owner authorizes registering only effective Use-on frames by default, with optional diagnostics for excluded inputs, verification of registration accuracy/success and a local release. This supersedes the historical diagnostic-all behavior in the M3 contract and ADR-059's all-input registration outcome denominator. Preserve all deduplicated original rows, original chronological session grouping, accepted anchor selection, manual input guards and all-original-input diagnostic-map denominators. Do not remove rows or reclassify the immutable inspection to implement scheduling.

Add default-false `RegistrationOptions::diagnose_excluded` across the native continuation and CLI register/stack entry points. Persist the GUI preference with a backward-compatible false default, expose it alongside registration reference controls and freeze it during a job (including review). Old public entry points use the selected-only default. Skip excluded native fits before prediction/matching and skip their external refinements. Optional excluded fits never update selected-frame prediction history; both modes therefore produce identical selected solutions and photometry. Solver thresholds, correspondence arithmetic and integration remain unchanged. Sessions containing no selected anchor retain explicit skipped rows and an empty anchor instead of aborting the entire selected stack or choosing a rejected anchor.

Reports retain schema 2 with additive `diagnose_excluded`, attempted/skipped counts and per-row `registration_attempted`; CSV appends the attempted column. A skipped row has no transform, matches, weight or footprint and is not labeled registration_failed. Successful/failed scheduled work and skipped original inputs are separate in native Processing and CLI summaries. Progress counts only scheduled candidate rows and existing reference preparation/finalization steps, stays monotonic and reaches 100% only after finalization.

Center multiline Status in the same row as Time and Progress, with bounded width and a 34-point minimum row height. Preserve non-scrolling Processing, fixed STOP and the 1366 x 768 physical / 125% target. Update the historical review test to the approved footer-owned waiting/Continue UI. Add a focused headless layout gate at 1060 x 520 logical client size; native Windows/DPI appearance remains owner review.

Verification is local: default/all-feature format/lint/release suites, applicable real FITS/SIMD/GPU gates and an opt-in replay of the stored full-run CFA catalogs from master_v8_report.json. Read source headers and external reference read-only. Before measurement require identical selected membership/success, <=0.05 pixel native/reference corner displacement versus stored transforms, <=1.5 reference-pixel inlier RMSE, <=0.001 pixel RMSE increase, <=1e-10 normalization/weight difference relative to max(1,abs(old)), unchanged source metadata, and exact entire selected-row equality between diagnostic modes. Known synthetic transforms, missing-star failures, cancellation, excluded-only sessions, CLI flag behavior and original-input preservation are covered separately. This replay is neither a new full integration nor a performance benchmark. No commits, pushes or remote CI; M5 remains open.

Stored full-catalog replay PASS: selected registration 981/981, skipped 159, optional diagnostic registration 1140. Selected rows are exact between modes. Maximum historical corner differences are 5.533e-12 reference / 1.890e-12 native px; maximum reference RMSE 0.647704 px, increase 1.051e-13 px; normalization/weight scaled difference <=4.548e-13. Historical decimal-report replay is tolerance-verified, not bitwise exact. Source headers/metadata remain unchanged; peak RSS 656.90 MiB. The 65.53 s replay envelope includes loading/validation and both modes, not a performance comparison. Full integration/reference benchmarks were not repeated.

Final local verification for ADR-060: formatting and default/all-feature workspace Clippy PASS; release workspace suites PASS (131/131 default, 133/133 all-feature), with actual real-FITS, AVX2 and GPU hardware gates enabled. The focused headless Processing test verifies Status/Time/Progress center alignment at 1060 x 520 logical client size. Private replay evidence remains excluded from Git. Verification-script profile checks support both historical all-input and new scheduled-only timings and reject inconsistent skipped counts; exact before/after science/FITS comparisons remain strict and were not relabeled as historical bitwise parity. Native DPI appearance and the newly prepared application remain for owner manual review; macOS was not tested.


## ADR-061 — Consolidated Compact Desk Publication (2026-10-26)

The owner authorizes one consolidated commit and push of the current finished batch: Compact Desk navigation/setup/Processing/Results, centered DPI-aware startup, approved embedded/desktop icon assets, compact filterable/resizable Quality table with bulk selection, whole-second display, nonduplicated footer-owned status, phase outcomes, explicit bounded diagnostic-map previews, native preview navigation and default Use-selected registration with isolated optional diagnostics. Include implementation, icon export source/tools, tests and documentation together. Publish to the existing m5-gpu-reconstruction branch without creating a PR or releasing local binaries.

Carry forward the actual local Windows verification: format/default-all-feature lint and release suites PASS (131/131 and 133/133), real FITS/AVX2/GPU gates, selected-only registration 981/981 with 159 skipped and matching selected rows in diagnostic mode. Historical whole-FITS parity does not establish a new full integration comparison for this batch. No new full stack/benchmark, native DPI acceptance or macOS verification is claimed. Exclude local sources/references, outputs, preferences, telemetry and build artifacts. Keep Actions configuration unchanged and use [skip ci] for this publication; do not trigger or poll remote CI. This authorization applies only to this batch; M5 remains open.


## ADR-062 — Measured CPU Integration Concurrency, Bounded Read-Ahead and Windows Identity (2026-10-26)

The owner authorizes profiling and comparisons at 12/16/24 threads, performance changes with unchanged quality, Windows process identity and a local release. Keep all work local without commits, pushes or remote CI. The fixed 16-worker GUI/pipeline limits were confirmed on a 12-core/24-logical-processor Ryzen AI 9 HX 370. Use actual process CPU time instead of interpreting the owner's 34% Task Manager observation as a portable utilization measurement.

Freeze the latest owner's native-grid plan: master_v11_report.json, 1140 original/940 accepted sources, native 2160 x 3840 grid, fixed original order/transforms/normalization/weights, sigma 3.0/2.5, 512-square tiles and cache 0. A release-only profiling example reads this typed plan without recomputing inspection or registration. Central profiling uses exactly four complete existing tile positions at origin (512,1024), size 1024-square, with all 940 selected sources. It is explicitly cropped profiling, not a full-stack timing. Write only final cropped scientific master/maps and JSON. Keep a preserved row-reader control executable, one excluded warmup and three rotated-order rounds per thread count. Require raw-float hashes and whole FITS master/map equality between every run. The complete reference grid is reserved for final quality verification. Decimal JSON replay may differ at a few historical rounding boundaries; use the exact same frozen plan in both binaries rather than claiming bitwise identity to a different in-memory historical plan.

Permit up to 24 CPU reconstruction slots and reduction bands, with memory estimates matching the active count. GPU reconstruction/reduction remains capped at 16. CPU Automatic uses available parallelism up to 24 after the measured comparison; expose remembered Automatic/12/16/24 choices in advanced settings and freeze the request during work. CLI defaults follow CPU/GPU limits. Larger explicit CLI pools still report their actual integration concurrency separately. Preserve bounded continuous refill, cancellation/join behavior and original source-order accumulation.

Evaluate 64 KiB BufReader read-ahead around the existing row decoder. Seek to the first requested row once, read/decode exactly the same row samples and use relative seeks that retain buffered bytes across source-row gaps. The buffer contains only bounded original source bytes and is dropped before reconstruction; cache remains off. Count underlying reader calls and bytes (including gap/read-ahead bytes) truthfully. Add 64 KiB per active reader to conservative memory planning. Compare against a frozen row-reader oracle over odd widths, gaps, buffer crossings and end-of-file, plus exact cropped/full master/maps and input integrity. Preserve any increased byte volume and cold/warm-cache limitations in the result.

Add monotonic Windows GetProcessTimes user/kernel counters around integration and its nonoverlapping reconstruction/reduction/copy phases. These are process-wide CPU seconds and may include GUI overhead; ROI task elapsed sums remain separately labeled and cannot be substituted. On unsupported platforms retain null availability. WPR CPU/FileIO/DiskIO capture was attempted using an isolated named session, but Windows returned 0xc5585011 (profiling privilege unavailable); no trace or stack-sample interpretation is claimed. The session is not recording. Continue the authorized analysis using measured process CPU counters, per-phase wall/CPU intervals and application I/O telemetry; do not change Windows security/power settings.

Embed VERSIONINFO with FileDescription/ProductName FulgurStack, version 0.1.0 and original filename, and set the explicit Windows application ID FulgurStack.Desktop before window creation. File menu shows version/PID for direct lookup. These improve process identity but do not prove the cause of the historical Task Manager omission or visual grouping acceptance. Keep approved icon, 1366 x 768 / 125%, centered startup and fixed STOP.

Measured decision for ADR-062: three rotated regional repetitions per thread count retain 24 CPU workers and bounded read-ahead (medians 12/16/24: 22.531762/20.096449/17.716736 s; original reader 23.362894/20.804144/18.301232 s). Regional whole master/maps and raw samples match in all 20 runs. Full native frozen-plan integration improves 162.791162 s (original reader/16) to 135.044169 s (read-ahead/24), 17.044533% shorter, with exact whole FITS/raw samples/science counters and all 1140 source hashes/metadata unchanged. Peak working set 3267.789/3339.004 MiB; no full external-reference stack is rerun. Read calls 19937074/1294345, application source bytes 20150891642/84728407240: preserve the larger byte volume, higher total CPU seconds at 24-way SMT and uncontrolled OS-cache/load qualifications. This is integration-only timing, not a new complete-GUI speedup claim. See docs/M5_INTEGRATION_PROFILE_REVIEW.md and portable evidence.

Final local gates for ADR-062: formatting, default/all-feature workspace Clippy and release suites PASS (134/134 default, 136/136 all-feature), with real FITS, AVX2 and actual GPU hardware gates enabled. The new gates cover CPU counter monotonicity, buffered-vs-frozen-row reader bits/source integrity and 12/16/24-worker full sample/map/source equality. Complete frozen-reference master/map/raw-sample/science equality and all 1140 source fingerprints PASS. No macOS or native Task Manager grouping acceptance is claimed. Local release preparation only; no commits, pushes or remote CI.

## ADR-063 — Owner-Authorized Integration Performance Publication (2026-10-26)

The owner explicitly authorizes one consolidated commit and push of the completed ADR-062 batch on m5-gpu-reconstruction: measured 12/16/24-thread integration comparison, bounded original-byte read-ahead, accounted 24-worker CPU scheduling and worker preference, process CPU/I/O telemetry, Windows application identity, verification tools and sanitized evidence. Retain the measured integration-only 162.791162 to 135.044169 s result, exact frozen-plan scientific products, unchanged 1140 input fingerprints, local Windows gates (134/134 default, 136/136 all-feature) and all measurement/platform/manual-review limitations. Historical local/uncommitted statements describe verification before this authorization. No new engine changes or benchmark reruns are needed for publication. Exclude private data, raw telemetry, products, preferences and binaries. Keep Actions configuration unchanged, carry [skip ci], and do not trigger or poll remote CI. M5 remains open; subsequent work requires separate publishing authorization.

## ADR-064 — Independent Native Registration Walks (2026-10-26)

The owner authorizes accelerating native registration with unchanged quality and a local release. Use Rayon join for the two independent chronological walks from each session anchor, preserving the complete serial frame order and selected-only prediction history within each walk. Each fit still solves directly to its session anchor. Share the immutable lazily initialized matcher target through its existing OnceLock; retain serial session bridges and at most two live walk scratch sets within existing catalog/runtime headroom. Join both results on cancellation/error, and merge original indexed rows and fit records in historical forward/reverse order. Normalize weights and compute canvases in original source order as before. No new dependency, image buffer, cache, intermediate or input write. Keep fixed STOP and existing progress publication mutex.

Freeze the pre-change serial planner as a test-only scientific oracle. Require exact all-field planner comparisons across worker counts, sessions, manual/missing-time references, diagnostics, invalid/failing catalogs and cancellation/progress. Replay stored native catalogs only to measure registration and compare to the same parsed serial input; retain historical rounding limitations and do not rerun integration. Add explicit overlapping task-elapsed telemetry while keeping phase wall intervals disjoint and verifiable. Run local format/default-all-feature lint/release and applicable real FITS/AVX2/GPU gates, then prepare the local release. No commits, pushes or remote CI. M5 remains open.

ADR-064 local verification PASS: three paired release registration-only replays after excluded warmups give serial/parallel medians 17.0449013/10.1033996 s (40.724798% shorter). Same parsed catalogs yield exact scientific fields, transforms, matches, weights, sessions and canvases; all 940 selected succeed, 200 are skipped, and diagnostics preserve selected rows exactly. All 1140 source SHA256/size/mtime fingerprints match. Historical decimal replay corner difference is at most 2.033692e-12 px, maximum residual 0.534220705 px. Retain increased process CPU cost and uncontrolled local scheduling/thermal conditions; no comparison to a historical GUI time or whole-stack speedup is claimed. Format/default-all-feature lint and release suites PASS 137/137 and 139/139, with real FITS/AVX2/GPU enabled. Existing source-protection hardlink tests require normal local permissions outside the restricted sandbox; no test is weakened. No new full integration, macOS, publication or remote CI. See M5_NATIVE_REGISTRATION_WALKS_REVIEW.md and portable evidence.

## ADR-065 — Validated Optional Geometric Distortion Correction (2026-10-26)

Owner-authorized optional nonlinear alignment, default off. Fit a normalized cubic residual field to source-to-reference coordinates, retaining a similarity seed and native scientific photometry. Estimate on training stars only; validate on deterministic held-out stars with spatial and outer-field coverage. Require at least 20% and 0.15 reference-pixel validation improvement over an independently fitted linear model, <=1 reference-pixel validation RMSE, improved outer validation, and bounded nonfolding geometry. Reject insufficient, unsupported or non-improving models and keep ordinary registration unchanged; record explicit decisions. Optional wider-tolerance matching is a seed only, never an accepted unvalidated registration. Exact existing thresholds/operations remain unchanged when disabled.

Use a fixed-size coefficient model with bounded Newton inverse, one interpolation in the existing tile warp and immutable source catalogs. Derive a conservative source-coordinate displacement envelope from coefficient absolute sums; add it to ROI and memory estimates and use conservative interior footprints for optional crop planning. Keep GPU reconstruction with CPU geometry and all scientific pixel arithmetic. Add GUI/CLI preference off by default. Generate an owner-requested final synthetic geometry-only reference from the original green plane with corner radius increased 3%, expanded canvas and explicit artificial FITS history; no defocus, source modification or pixel cache. Full local scientific/lint/release gates, analytic tests and real derived-reference checks are required. No publication, full historical integration rerun or remote CI. M5 remains open.

ADR-065 local gates PASS: formatting/default-all-feature lint and release suites 148/148 and 151/151 with actual FITS/AVX2/GPU. Preserve held-out independence in the exact applied model, use the accepted anchor only as a correspondence seed, and tighten conservative ROI intervals without losing curved interior/edge extrema. Off-path/fallback, analytic source sampling, tile/source invariance, inverse/global nonfolding, progress and corrected GPU gates pass. The artificial 4000-to-4120-square green-plane reference has 3% corner-radius growth (84.831601 px), explicit synthetic provenance and unchanged original hash. Real 32-source controls integrate 27/32, with 25 validated corrections and two ordinary fallbacks; held-out RMSE improves 6.768110..12.012653 to 0.222504..0.854068 reference pixel for accepted models. Complete off/on processing is 7.6690582/12.7537752 s, +66.301714% on this small single paired observation, not the earlier estimated 10..30% or a full-set claim. Actual peak ~692.3/697.4 MiB; all 34 source/reference hashes and metadata remain unchanged. Retain the measured overhead, stricter skipped cases and quality limits. Final GPU-enabled local release and desktop shortcut verification PASS; no macOS/native visual acceptance, full-1140 rerun, commit, push or remote CI. See M5_GEOMETRIC_CORRECTION_REVIEW.md and portable evidence.
## ADR-066: Absolute field validity before optional-correction fallback

2026-10-26, owner-authorized repair after the master_v15 regression; M5 remains
open. The previous acceptance gate certified improvement of a nonlinear model
but incorrectly treated rejection of that model as certification of a similarity.
The owner run integrated 801 cubic and 139 incompatible similarity rows.

Separate nonlinear improvement from absolute registration validity. With the
option enabled, audit broad fixed identities across native and external anchors,
including matched support, missing expected stars, absolute/held-out/outer errors.
No candidate can discard its own witnesses by tightening an inlier radius.
Try existing independently validated cubic, then calibrated anchor composition
with a newly training-only fitted native rigid pose, then a training-only fitted
similarity. Require >=50 fixed stars, >=30 pose-training stars, >=10 validation
stars, >=80% expected support, broad 35%-axis support, 10/16 field cells and 6/16
validation cells, held-out outer witnesses in all quadrants, <=1 reference-pixel
all/validation/quadrant RMSE and <=3-pixel maximum fixed-identity error. Source
pose recovery reserves deterministic spatial witnesses before fitting. Existing
direct-cubic coefficient validation still uses its original modulo-five split;
never relabel its training stars held-out. The cubic's original stronger training
and improvement gates also remain. Disabled behavior remains exact.

Reject unsafe rows; stop an unconfirmed anchor bridge before integration. Add an
integration-entry invariant and distinguish absolute accuracy failure from no
material improvement. Native broad matching counts expected projected stars as
well as matches, so a small local inlier cluster cannot establish success.
Do not weaken image rejection or silently force nonlinear models. Calibration
composition is only accepted per-source and cannot assume different lenses share
distortion. Preserve source data, native photometry/GPS/exposure, single pixel
interpolation, bounded memory, original map denominator and local-only policy.
