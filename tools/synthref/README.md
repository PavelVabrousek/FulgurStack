# synthref: synthetic Gaia registration reference (experiment)

A separate program beside FulgurStack, with its own Cargo workspace; FulgurStack is used unchanged as a library. See ADR-127.

- `synthref solve` plate-solves one registration frame against Gaia DR3 (ESA Gaia archive, TAP).
  - **Search.** The solve is blind near a target position. It tries a ladder of field sizes and both parities with triangle asterisms, then fits a linear TAN solution with 3-sigma clipping.
  - **Output.** It renders the brightest N catalogue stars inside the frame on the frame's pixel grid. Positions are moved to the frame's DATE-OBS by proper motion. The result is an ideal TAN projection: the frame's own optical distortion is not copied.
  - **Files** in `--out`:
    - `synthetic_reference.fit`: monochrome, BITPIX -32, values 0..1, with a WCS.
    - `synthetic_reference.json`: the solution and every star with Gaia id, RA/Dec, G, x/y and peak.
    - The Gaia answers as CSV, so a rerun queries nothing.
- `synthref register` registers light sets against any reference with FulgurStack's own registration (Maximum coverage, distortion correction on). It prints stacked/usable frames per folder.

Build and run from the repository root:

```bash
cargo build --release --manifest-path tools/synthref/Cargo.toml
```

```bash
tools/synthref/target/release/synthref solve --frame "C:\Astrofoto\M33 all time data\20200921 m33 l\Lb_093.fit" --out outputs/synthref/Lb_093 --object M33 --stars 1000
```

`--object` asks CDS Sesame for the position; give `--ra DEG --dec DEG` when Sesame cannot be reached. Pixel coordinates are zero-based pixel centres, with rows counted top-down, as FulgurStack reads a reference plane. No site coordinates are read or written.
