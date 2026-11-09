# blindspike: feasibility of blind plate solving for fields of 0.5 to 12 degrees

A standalone spike beside FulgurStack (workspace of its own). It answers one question: can a frame be placed on the sky with no position, no target name and no header help, for fields 0.5 to 12 degrees wide, using only the Gaia DR3 files already on the machine?

**Method (astrometry.net style, own code).**

1. **Stars.** Every Gaia star brighter than G 13 (7.4 million) or G 14 (16.9 million) is read from the PixInsight files.
2. **Index.** Seven bands of quad sizes: 0.1 to 0.25, 0.25 to 0.5, 0.5 to 1, 1 to 2, 2 to 4, 4 to 8 and 8 to 16 degrees (fields of 0.5 to 2, 2 to 6 and 6 to 12 degrees). In each band only the brightest few stars of every cell as wide as the band's smallest quad are kept, so deeper catalogues add stars only where the sky is sparse. A quad is two stars A and B and two more inside the circle on AB. Its code is the position of C and D when A is 0 and B is 1 in the complex plane (a similarity-invariant four-number code).
3. **Query.** The 30 brightest detections of a frame make quads, in both mirror states. Each is looked up in the index with a tolerance that follows the centroid error over the quad's length. A hit gives a similarity, so a field centre, scale and parity.
4. **Verification.** The brightest catalogue stars of the hypothesis are projected into the frame and matched with the 150 brightest detections within 4 px. A hypothesis needs at least 12 matches and a chance probability below 1e-12.
5. **Refinement.** FulgurStack's own solver, with the hypothesis as its hint, gives the final solution and its held-out check.

**Run** (the Gaia folder defaults to `C:\PixInsightData\Gaia DR3`):

```bash
cargo build --release --manifest-path tools/blindspike/Cargo.toml
tools/blindspike/target/release/blindspike --gmax 13 --frames-per-set 3 --synthetic 80 --negatives 100
```

**Results (Windows 11 x64, 24 threads, 2026-11-09).**

| Measurement | G < 13 | G < 14 |
| --- | --- | --- |
| Stars read (files cached) | 7.4 M in 5 to 8 s (55 s cold) | 16.9 M in 5.5 s |
| Index | 41.9 M quads, 1.4 GB RAM, built in 16 s | 51.5 M quads, 2.0 GB RAM, built in 24 s |
| Real lights, no hint (Seestar S50 Pro and S30 Pro, Dwarf, SQA60P, ASI6200, Stellina, ASI071, old M33 sets; fields 1.06 to 4.9 degrees) | 27 of 27 lights solved in the run, agreeing with the known position; 6 darks, flats and bias have no stars and are correctly not solved | Stellina 6 of 6 |
| The two lights not solved at first | Both solved after cleaning the detections (see below) | |
| Time of a real frame | blind search 0.00 to 0.07 s; with refinement 0.2 to 1.7 s (median 0.8 s) | median 1.0 s |
| Synthetic, field 2 to 6 deg | 240 of 240 | 239 of 240 |
| Synthetic, field 6 to 12 deg | 30 detections 74/80; 50: 79/80; 100 and 200: 80/80 | 30: 77/80; 50 and more: 80/80 |
| Synthetic, field 0.5 to 2 deg | 15 detections 76/80; 20: 72; 30: 73; 50: 71; 100: 75 | 15: 71; 20: 78; 30: 78; 50: 79; 100: 75 |
| Wrong solutions | 0 | 0 |
| Random star lists (150 stars) | 0 of 100; 1.5 s to reject one | 0 of 40; 2.4 s |

The synthetic failures of the 0.5 to 2 degree band are frames that hold only 12 to 25 detectable stars (high galactic latitude, 0.5 degree field); 10 detections never solve. A deeper catalogue helped only a little there.

**Limits of this spike.** No persistent index file; star detection is FulgurStack's own; the synthetic frames use Gaia's own stars with modelled noise, not real cameras; the random-list control bounds the false-positive rate only to about 1.5 % at 95 % confidence for 200 lists.

**The two unsolved frames (diagnosed with `--diag FRAME --hint RA,DEC`).**

- **ASI6200 L, LBN 437.** The 1500 detections were dominated by saturated stars (peak 64 000 to 65 535) that break into up to six detections a few pixels apart, so the 30 brightest held about 15 distinct stars and their centroids were unreliable.
- **ASI1600 Ha, M42.** 23 of the 30 brightest detections were knots of the nebula's core, not stars; only 7 were catalogue stars.
- **Fix.** Before quads are made the detections are cleaned: any detection within 0.2 % of the long side of a brighter one is dropped, and so is any of the 300 brightest with four or more neighbours within 2 % of the long side (a crowd, not a star). Both frames then solve in 0.00 s (72 and 111 star matches), agreeing with the solution around the known position.
- **Regression after the fix (all 12 sets, 10 frames each).** 119 frames, 102 solved, 0 disagree with the known position; the 17 not solved are darks, flats and bias frames with 0 to 6 stars. Two lights without a known position (Series2_018, b33ha_010) solved to plausible places (the Horsehead region among them). Synthetic frames: 0.5 to 2 degrees 87 to 97 % depending on the star count, 2 to 6 degrees 100 % from 30 detections, 6 to 12 degrees 97 to 100 % from 30 to 50; no wrong solution. Random lists: 0 of 100 false.
