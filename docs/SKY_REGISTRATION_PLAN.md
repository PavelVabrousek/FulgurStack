# Plan: sky-based registration in FulgurStack

Status: owner decisions taken on 2026-11-04 (section 4, ADR-132); implementation not started. Nothing in FulgurStack has changed yet. Evidence comes from the standalone experiment `tools/skyreg` (ADR-131).

## 1. Goal

Replace the camera-anchor chain of the current registration (ADR-120 to ADR-130) with a single principle. Every frame is plate-solved against Gaia DR3, and a registration source → reference is source pixel → sky → reference pixel. Mixed cameras, optics, filters, mosaics, rotations, mirroring and nights then need no special cases, and any frame or external image (real or synthetic) can be the reference.

The current star-to-star registration stays as a fallback for frames or projects that the sky method cannot solve.

## 2. Evidence that justifies the work

On the mixed M33 test set (372 frames: ASI1600MM, ASI2600MM, ASI2600MC, ASI071MC, G3-16200 XISF), with a cached catalogue and 24 threads:

| | FulgurStack today | skyreg (after speed-up) |
|---|---|---|
| Frames registered | 245–249 of 330 quality-accepted | 372 of 372, every registration verified |
| Registration time, one reference | 75–87 s (plus 11 s inspection) | 6.1 s solve (plus 10–11 s reading and detection) |
| Each further reference | whole registration again | under 0.2 s |
| Accuracy | per-frame held-out checks | systematic offset median 0.03 px, worst 3 × 3 cell max 0.24 px; a deliberate 0.5 px shift fails for all 6324 registrations |

## 3. How it fits the existing pipeline

The integration stage stays as it is. For every accepted frame it already consumes:
- a similarity `transform` (source → reference);
- an optional cubic `distortion` model (`distortion::Warp`: similarity base plus ten cubic terms, source → reference pixels);
- photometric scale and offsets, derived from `photometric_matches` with the photometric anchor;
- weight, footprint and optional `partial_field`.

The sky engine produces exactly these fields:

1. **Geometry.** The composed map source → sky → reference is sampled on a grid over the part of the source that lands on the reference. It is fitted with the existing `Warp` (base similarity plus cubic). The fit error is measured and reported for every frame; a frame whose map the cubic cannot carry within tolerance is refused or handled by the fallback (see risk R1).
2. **Photometry.** The frame's stars are carried into the photometric anchor through the sky and paired with the anchor's own stars. These pairs feed the existing photometric code unchanged.
3. **Verification.** The end-to-end check of skyreg becomes the acceptance test of every frame: point sources only, systematic displacement over the frame and in 3 × 3 cells. Its result is recorded in `registration_validation`, which integration already requires.
4. **Reports.** Per frame: plate solution (centre, scale, rotation, parity, polynomial degree, held-out RMS against Gaia), how it was found (blind or seeded), the cubic fit error and the end-to-end check. GPS stays out, as now.

Inspection already reads every frame and measures its stars, so the sky engine reads no pixels of its own. Inspection keeps 250 stars per frame today; skyreg used 1500. Whether 250 suffice for blind solving, or whether inspection should keep more (about 25 MB for 372 frames at 1000 stars), is measured in phase 1.

## 4. Owner decisions (2026-11-04, ADR-132)

- **D1. Catalogue source: both.**
  - The PixInsight Gaia DR3 files on disk (XPSD, `C:\PixInsightData`) are read when present: offline and all-sky.
  - Otherwise a cone of Gaia DR3 is downloaded once per target from the public archives (ESA, with the GAVO data centre as fallback) into FulgurStack's data folder and cached.
  - The report names the source used.
- **D2. Target position from four sources.** Precedence, first found wins:
  1. Coordinates of the field centre entered by hand in the registration settings.
  2. RA/Dec cards of the frame headers.
  3. The OBJECT name looked up in a local database of the main deep-sky catalogues built into FulgurStack (Messier, NGC, IC, from OpenNGC).
  4. The OBJECT name looked up on the internet. Services, in order:
     - NED (NASA/IPAC);
     - SIMBAD through its CfA mirror;
     - CDS Sesame.

     On 2026-11-04 the first two answered from the reference PC, while CDS Strasbourg did not.

  The report names the source of the position. Without any position, the star engine registers the stack and the report says so.
- **D3. Rollout.** The sky method is optional, chosen by a switch in the registration settings. The current star registration stays the default and the fallback.
- **D4. Ideal grid (TAN canvas).** Postponed.

**Licence notes.**
- Gaia DR3 is free to use with attribution (ESA/Gaia/DPAC).
- OpenNGC is licensed CC BY-SA 4.0: the built-in object database file keeps that licence and its attribution.
- Both attributions go to the About page.

## 5. Phases and acceptance gates

Each phase ends with the usual local gates: fmt, clippy (default and all features), `cargo test --workspace --release`, and byte-identical masters of the single-camera datasets as long as the star engine is selected.

### Phase 1: core library (no behaviour change), done 2026-11-04 (ADR-133)

Results:
- `fulgur_pipeline::sky` exists with five synthetic tests; the M33 set solves 372/372 through it, with 6324/6324 registrations PASS.
- **S1:** the cubic carrier is within 0.05 px for most targets, but reaches 0.13 px between the two G3 optics. Phase 3 needs a higher-degree or sampled carrier for such cases.
- **S2:** inspection's 250-star catalogue solves only 45/372 because it has no saturated stars and picks stars per cell. Phase 3 needs a dedicated list of the brightest detections, measured during inspection.

Original scope:

- **Work.**
  - New module `fulgur-pipeline::sky`: catalogue trait, Gaia star type with proper motion, gnomonic projection, blind and seeded search, refinement with degree selection by held-out stars, end-to-end check. Ported from skyreg and made deterministic (no hash-order effects).
  - Spike S1: measure the cubic `Warp` fit error of the composed map for all 6324 test registrations.
  - Spike S2: solving with inspection's 250 stars against 1000 stars.
- **Gate.**
  - Synthetic unit tests: recover known solutions under rotation, mirroring, scale 0.5–5″/px, cubic distortion, colour mosaic and 20 % false stars, to 0.05 px.
  - A sky solution for every frame of the M33 test set.
  - S1 and S2 reported with numbers.

### Phase 2: catalogue and position providers (D1, D2), done 2026-11-04 (ADR-134)

Results:
- Crate `fulgur-catalog`: PixInsight XPSD reader, cached ESA/GAVO download, built-in OpenNGC table (14 027 objects), NED, SIMBAD and Sesame lookup, and the position precedence.
- All gates met: 2° cones in 0.04–0.39 s; 3 fields identical to the archive within 0.001 mas and 0.0005 mag; no network when the files cover the request; table positions within 0.005′ of NED for the owner's targets; ordered online fallback with reported failures.

Original scope:

- **Work.**
  - Behind one catalogue trait: the XPSD reader for the PixInsight Gaia DR3 files and the download cache (ESA, then GAVO). Local files are used first, the cache second, the download last. Magnitude limits adapt to the field size.
  - Position sources in the order of D2:
    - manual centre;
    - header RA/Dec;
    - built-in deep-sky database (Messier, NGC, IC from OpenNGC, compiled into a compact table of names, aliases and positions);
    - internet lookup (NED, SIMBAD at CfA, Sesame).
- **Gate.**
  - A cone of 2° read in under 1 s from the PixInsight files.
  - Identical stars (source id, position, magnitude within 0.01) between the PixInsight files and the archive for three test fields.
  - No network access when the PixInsight files cover the field.
  - The built-in database resolves M33, NGC 7000, IC 1396 and the owner's recent targets to within 1′ of NED.
  - Internet lookup falls back in order and reports each failure.

### Phase 3: sky registration engine, done 2026-11-04 behind a CLI switch (ADR-136, ADR-137)

- **Design (2026-11-04).** The engine stays inside `fulgur-pipeline` and never touches the network. The caller passes the field centre (from `fulgur_catalog::position`) and a catalogue provider, a callback that returns the Gaia stars of a cone. The CLI and the application supply the provider from `fulgur_catalog::gaia_field`. Steps:
  - **A. Star list (done, ADR-136).** Inspection's own catalogue leaves saturated stars out and picks stars per cell; spike S2 solved only 45/372 M33 frames with it. `InspectConfig::sky_stars` (off by default) makes inspection keep a separate list of the 1 500 brightest detections, saturated ones included, with colour mosaics measured on 2 × 2 superpixels (`sky::detect`). With it the M33 set solves 372/372 and 16 templates × 372 = 5 952 registrations pass. Inspection of the 388 files takes about 2 s longer (13 s instead of 11 s); with the switch off nothing changes.
  - **B. Solving and the reference.** `sky::batch::solve_all` over the quality-accepted frames, grouped by camera. The reference is the selected frame, an external FITS/XISF (solved the same way) or, for Auto, the frame the star engine would choose.
  - **C. Geometry.** For each frame the composed map source → sky → reference becomes the cubic `distortion::Warp` (`sky::warp::carry`) where it carries the composition within 0.05 px (risk R1) and the stars carried through it pass the end-to-end check. Otherwise (the LBN 437 ASI6200 sensor: 0.46 px) a cubic is fitted on the star pairs the plate solutions identify, judged by five-fold cross-validation. Mirrored rows use the existing camera handling. `registration_validation` is filled from the check, so integration accepts the frame unchanged (ADR-137).
  - **D. Photometry and weights.** Stars matched through the sky map feed the existing photometric scale, background offset, channel offsets and noise normalization, per anchor pixel (area of the source pixel on the anchor's grid). Cameras that share fewer than 10 catalogue stars use the bright-star lists, which carry flux and peak.
  - **E. Fallback and report.** Frames that do not solve, or whose check or carrier fails, are registered by the star engine in the same run; the report says which engine registered each frame and why.
  - **F. Testing switch.** A CLI option selects the engine before the application switch of phase 4.
- **Work.**
  - `RegistrationOptions` gains the engine choice.
  - The sky engine takes the inspection report and produces a `RegistrationReport` that integration accepts unchanged.
  - References: a frame of the stack, an external FITS/XISF, a synthetic reference.
  - Frames the sky engine cannot solve go to the star engine in the same run.
  - Progress reporting and per-frame rejection reasons.
- **Gate (scientific).**
  - M33 test set: every quality-accepted frame registers. Masters on three references are no worse than today's in HFR, FWHM, eccentricity, noise and star count, and the M33 masters are compared in PixInsight by the owner.
  - DS-A (Seestar), LBN 437 and M42: no fewer frames than today, masters no worse; time recorded against the stored references.
  - Negative control (deliberately displaced reference) fails.
- **Gate (time).** Sky registration of the M33 test set ≤ 10 s after inspection on the reference PC.
- **Result (ADR-137).**
  - Every quality-accepted frame of the M33 set (five cameras) registers through the sky: 330 instead of 213, in 7.7–9.8 s instead of 42.6 s.
  - DS-A, LBN 437 and M42 lose no frames; M42 gains 10, whose stars are elongated.
  - On equal frames the masters are sharper. They are no worse in noise, with two exceptions: LBN 437 (+0.3 %) and M33 (+0.9 %). On equal frames eccentricity is no worse, except on M33 (0.282 against 0.266), mostly because the sky run normalizes brightness by pixel area within one sensor size.
  - The owner's PixInsight comparison of the M33 masters is open.

### Phase 4: application and CLI, built 2026-11-04, owner review PASS 2026-11-05 (ADR-138, ADR-143)

- **Work.**
  - A switch in the registration settings: Stars (current, default) or Sky (Gaia).
  - The folder of the local Gaia files as a setting, with a "Test" button that runs the installation check (ADR-135) and shows its verdict and problems.
  - Field-centre input (RA/Dec) and a display of the position found and its source.
  - Catalogue status: PixInsight files found, cache, download.
  - Registration table columns for plate scale, held-out RMS and check result.
  - CLI `--registration-engine sky|stars`.
- **Gate.** UI iterations follow the owner's manual-review policy; the release build is prepared for review.

### Phase 5: later decisions

- **Work.**
  - Whether sky becomes the default stays the owner's decision after reviewing masters (D3 keeps it optional for now).
  - Documentation, README and the Windows package notes are updated.
  - Publishing as an authorized batch.

## 6. Risks

- **R1. Cubic carrier.** (ADR-137: met on M33, DS-A and M42; on LBN 437 a cross-validated cubic on star pairs takes the composition's place.) A wide-field lens frame mapped onto a narrow grid (or the reverse) composes two polynomials and a projection, which is more than a cubic. S1 measures this. If the error exceeds 0.05 px for real cases, integration gets a more general per-frame map (higher degree or a sampled correction grid), which is a contained change in the resampler.
- **R2. Too few catalogue stars or detections.** Narrowband frames, thin clouds, short exposures. The seeded search needs fewer stars than a blind one; frames that still fail go to the star engine, and the report says why.
- **R3. Dense fields.** In the Milky Way a fixed magnitude limit gives too many stars. The magnitude limit has to follow the frame's detected star count, as skyreg's brightness-ordered selection already does.
- **R4. Position unknown.** D2 gives four sources. Without any of them a blind search over the whole sky is out of scope; the engine then uses the star engine and says so.
- **R5. Catalogue size and licence.** See the licence notes in section 4.
  - The PixInsight files belong to the user's PixInsight installation; FulgurStack only reads them.
  - A download costs about 1–3 MB per target.
  - The built-in deep-sky table is a few hundred kilobytes.
- **R6. Behaviour change of masters.** Different registration means different master pixels. The byte-identity gates of the current engine stay valid only while it is selected; the sky engine gets its own reference hashes after the owner's approval.

## 7. Estimated effort (engineering)

| Phase | Estimate |
|---|---|
| Phase 1 | about 2 working sessions |
| Phase 2 | 1 session for the download cache, 2 for the XPSD reader |
| Phase 3 | 3–4 sessions, including the scientific gates |
| Phase 4 | 1–2 sessions |
| Phase 5 | after the owner's review |
