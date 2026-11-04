# skyreg: universal sky-based registration (experiment)

A standalone program beside FulgurStack, with a Cargo workspace of its own. FulgurStack is unchanged; only its FITS/XISF readers and RAM telemetry are used. See ADR-131.

**Idea.** Every frame is plate-solved on its own against Gaia DR3, whatever its camera, optics, filter, colour mosaic, rotation or mirroring. A registration source → target is then the composition *source pixel → sky → target pixel*. Any frame of the set, or any external image (real or a synthetic star field), can be the target. Everything happens in memory; only `registration.log` and `report.json` are written.

**Steps.**

1. **Read.** Pixels are read once.
   - A colour mosaic is recognised from a header card or, without one, from the alternation of neighbouring pixel levels. Its stars are found on 2 × 2 superpixels.
   - Stars come from a local background mesh, 5σ maxima and intensity-weighted centroids, keeping the brightest 1500.
2. **Blind solve near a hint.**
   - Triangle asterisms of the 40 brightest detected and 70 brightest catalogue stars.
   - A ladder of field sizes from 0.06° to 3.2° half height, both parities.
   - The pose must map clearly more catalogue stars onto distinct detected stars than any rival pose.
   - Header RA/Dec, when present, is an extra hint. Frames that fail get a second pass with the centres of solved fields as hints.
   - **Speed.** Only one frame per camera group (instrument, sensor size, mosaic) is searched blind. The others start from a solved sibling's scale, parity and centre: one field size, one parity, scale within 5 %. A pose with 40 votes ends the search. On the M33 test set this takes 6 s instead of 128 s.
3. **Refinement.**
   - Matching with tolerances shrinking from 6 to 2 px.
   - Polynomials of degree 1–3 in both directions; the degree is chosen by every fifth star, which the fit never sees.
   - Positions are moved by proper motion to each frame's date.
4. **End-to-end check of every registration.**
   - The source's point sources are carried into the target and matched with the target's own point sources. Galaxy knots, HII regions and blends are left out by width and elongation.
   - The registration error is the systematic displacement: the mean over the frame and in each cell of a 3 × 3 grid. The scatter of single centroids is not used, because faint narrowband stars scatter even when the mapping is exact.
   - **PASS:** at least 15 pairs; at least 50 % of them within 1 px of the coarser frame; frame offset ≤ 0.15 coarser px; every judged cell ≤ 0.3 coarser px.
5. **Negative control** (`--negative-control`). Every target is displaced on purpose by 0.05–0.5 px, and the check has to catch it.

**Catalogue.** It comes from the ESA Gaia archive, with the GAVO data centre (`gaia.dr3lite`) as fallback.
- A deep cone (1.7°, G < 17.5) and a mosaic of bright cones (G < 13.5) out to 4.8° for wide lenses.
- The answers are cached as CSV in `<out>/catalogue_cache`.

**Build and run** from the repository root:

```bash
cargo build --release --manifest-path tools/skyreg/Cargo.toml
```

```bash
tools/skyreg/target/release/skyreg --input "C:\Astrofoto\M33 Test Sada vyber z hroznu" --out outputs/skyreg/m33_templates --ra 23.4621 --dec 30.6600 --targets-dir "C:\Astrofoto\M33 Test Sada vyber z hroznu\Registratiom templates" --negative-control
```

**Other options.**
- `--target FILE` adds a single target.
- `--all-targets` checks every input frame as a target of all others.
- `--only TEXT` limits the inputs to matching names.
- `--fs-stars` solves with FulgurStack's inspection catalogue (spike S2). `--fs-sky-stars` uses the bright-star list inspection keeps for sky registration (ADR-136) instead.
- `SKYREG_DEBUG=1` prints the blind search and refinement rounds.

No site coordinates are read or written.
