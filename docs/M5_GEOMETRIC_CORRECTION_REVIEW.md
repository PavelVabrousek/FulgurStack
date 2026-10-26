# M5: Optional Validated Geometric Correction (2026-10-26)

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

**Historical release regression: product acceptance FAIL.** The later owner run exposed unsafe
ordinary fallback after nonlinear rejection (801 cubic plus 139 incompatible
similarity rows). Historical test passes below do not establish safe fallback
behavior. See [diagnosis and required correction](M5_GEOMETRIC_MIXTURE_DIAGNOSIS.md).
The subsequently owner-authorized [acceptance repair](M5_GEOMETRIC_ACCEPTANCE_REPAIR.md)
replaces that unsafe behavior. Its local verification does not retroactively
approve the earlier release; repaired native/PixInsight manual review remains pending.

Owner-authorized local implementation, ADR-065. The prior independent-walk optimization remains local in the same working tree. No commit, push, PR update or remote CI. M5 remains open.

## Product behavior

Stack setup > Registration > Advanced registration > Correct geometric distortion is off by default. Old saved settings restore it as false; an explicit user choice is remembered. CLI register/stack expose --geometric-correction. Turning it on requests an assessment, not unconditional warping. If independent evidence is insufficient or no material improvement is confirmed, integration uses the ordinary mapping for that frame. Registration failures are recovered only by a validated model with usable native photometry. Use-off scheduling and optional excluded diagnostics retain their previous meaning.

Each requested frame records geometric_correction.status/reason, training/validation counts, ordinary/corrected validation and outer-field RMSE and the accepted coefficients in JSON. Results shows corrected/skipped counts for integrated sources. A skipped correction is not a whole-frame rejection. The fixed Processing STOP, phase alignment, single progress owner, Compact Desk dimensions and Windows identity remain unchanged. Native manual review at 1366 x 768 / 125% remains pending.

## Model and acceptance gates

Fit a normalized third-degree source-to-reference residual field around a proper uniform-scale/rotation/translation seed. It handles mild smooth optical distortion, including asymmetric low-order components; it is not an unrestricted deformation or a fisheye/all-camera solution. Native source compatibility guards remain. No reference pixels enter the scientific master and no template photometry, GPS or exposure is imported.

Optional broad-tolerance similarity/triangle matching only initializes correspondences. Never accept its relaxed residual as a registration. Where available, compose the independently validated anchor warp with each native source pose as a cubic correspondence seed; each source still trains/validates its own model. This improves initialization without assuming all lenses have the same distortion.

Source-index modulo five reserves deterministic held-out stars that never fit the applied coefficients. Require at least 60 stars in each catalog, 40 training matches, 10 held-out matches, 6 held-out outer-field matches, 10/16 occupied training cells and 6/16 held-out cells. The common training field must span at least 35% of both sensor dimensions; this permits the owner's cropped external template. Define the common-field grid/outer regions using training coordinates only. Retain at least 80% of initially matched held-out stars and assess them with a wider validation window so poor predictions cannot disappear through the fitting inlier cutoff.

Acceptance requires held-out RMSE <=1 reference pixel, >=20% relative and >=0.15 pixel absolute improvement over a separately fitted similarity on the same training matches, >=20% outer-field improvement, independent outer stars in all four quadrants and no quadrant worsening by >0.1 pixel. Require finite coefficients, supported base scale, a global derivative contraction bound <0.45 and accurate bounded inverse recovery. Apply exactly the training model whose held-out metrics passed; never refit it using validation labels after acceptance. Ordinary 1.5-pixel final correspondence thresholds remain unchanged. A rejected model retains the original row, transform, photometry and weighting, apart from additive assessment metadata.

Native corrections fit directly to the native scientific master and recompute photometry only from their verified native matches. External corrections preserve that native photometry and separately record any native photometric-anchor model. Source-to-external nonlinear coefficients override the reported similarity seed explicitly. FITS NDISTCOR and HISTORY identify actual corrected source count, with coefficients in JSON; disabled/unused correction does not add FITS cards. This preserves surface-brightness sampling and prior photometric limitations; no new absolute stellar-flux guarantee across different optics is claimed.

## Integration and memory

Compose the inverse nonlinear map with the existing source sampling in each output tile. Precompute rotation per tile, solve the bounded inverse in f64 and reuse the accepted SIMD/scalar Keys sampler and original f64 photometric arithmetic. Image pixels are interpolated once. CPU and GPU reconstruction share this CPU geometry step. No source/frame cache, full registered image, new dependency, lower precision or image intermediate.

Interval polynomial bounds enclose inverse source coordinates, beginning with a conservative whole-sensor coefficient envelope and tightening by proven interval intersections for each tile. This includes curved edge/interior extrema rather than checking four corners only. Keep the existing source halo, even Bayer origins and strict sample validity. Global conservative displacement margins remain accounted in every tile memory candidate, with the 12 GiB ceiling/system reserve unchanged. Optional common/coverage crop planning uses a conservative convex interior; reference mode keeps the exact reference canvas and determines actual pixel validity from the inverse mapping. Coverage/rejection definitions and original-input denominator are unchanged.

## Artificial test reference

The requested final artifact is outputs/geometric_correction/RegisteringTemplate200pct_radial3pct.fit, generated from immutable RegisteringTemplate200pct.fit by the release example create_distorted_reference. It is a geometry-only monochrome Float32 copy of the original reference green plane, not a scientific reconstruction or a production intermediate. No GPS/WCS is copied. FITS SYNTHET, RADPCT and HISTORY mark artificial provenance; OBJECT/FILTER are retained for compatibility.

Use r_out=r_in*(1+0.03*r_in^2/R_corner^2) about the original center. Corner radii grow exactly 3%; do not confuse this with a 3% increase of the full image width on each side. The 4000-square input becomes a centered 4120-square canvas retaining the displaced corners. Single cubic interpolation preserves reference units without normalization or intentional defocus. Black areas outside the warped original field are explicit synthetic padding. The accompanying artificial_reference.json stores source/output hashes, dimensions and exact mapping. Production registration does not read this known mapping or its synthetic FITS cards; it must infer geometry from detected stars.

Reproduce to new destinations only:

```powershell
cargo run --release -p fulgur-pipeline --example create_distorted_reference -- RegisteringTemplate200pct.fit outputs/new_radial3pct.fit outputs/new_radial3pct.json
```

## Scientific verification scope

Analytic tests cover known 3% radial mappings, composition under rotation/scale, corner/inverse accuracy, spatial validation, insufficient/degenerate/linear fields, cancellation, folding rejection, malicious held-out displacement and proof that applied coefficients never use held-out labels. Dense mapped points verify interval tile bounds. Corrected integration checks independent radial inverse samples, unchanged sources, whole master/map equality across 32/64/128 tiles and actual CPU/GPU geometry parity. Native registration checks analytic recovery and exact ordinary rows when correction is unnecessary. Existing frozen serial/matcher/scientific gates remain; the serial planner oracle has only a mechanical default-false request-metadata initializer for the new report field.

Use the real 32-source subset and the artificial reference for local registration/integration controls. No full 1140-frame reference integration is rerun. Retain early assessment outcomes (initial 15 corrected/12 skipped, improved anchor seeding 25/2) as development observations, not final performance evidence. A skipped model remains skipped without weakening acceptance thresholds. Local timings on this small, cropped-reference workload must not be extrapolated to the full dataset. macOS and native visual acceptance remain unverified.

## Actual local result and release gates

One sequential same-binary CPU comparison, 16 threads, manual 512-square tiles, cache 0, 12288 MiB ceiling, the complete synthetic 4120-square reference canvas:

| Measurement | Correction off | Correction requested |
|---|---:|---:|
| Complete processing, seconds | 7.6690582 | 12.7537752 |
| Integration, seconds | 5.5297663 | 10.6059364 |
| Selected/stacked original frames | 27 / 32 | 27 / 32 |
| Confirmed corrected sources | 0 | 25 |
| Ordinary geometry retained | 27 | 2 |
| Approximate peak process working set, MiB | 692.3 | 697.4 |
| Planned buffers, MiB | 1856.5 | 2203.4 |

The measured whole-run overhead is 5.084717 s / 66.301714% on this small-set workload, higher than the earlier rough 10..30% estimate. Do not project this multiplier onto the full native dataset or claim isolated causality; OS caches/background load/thermal scheduling are uncontrolled and one pair is not a distribution. Keep the optional default-off control. Rejection statistics/products differ intentionally because sampling geometry changes; exact whole-file equality across enabled/disabled modes is not expected.

For the 25 accepted corrections, independent validation RMSE falls from 6.768110..12.012653 reference pixels under the fitted similarity to 0.222504..0.854068 pixel under the applied model. Both other sources retain ordinary registration: one lacks confirmed quadrant validation, one fails the held-out improvement gate. All 27 still integrate; five original frames remain quality-excluded. Original map denominator is 32. Both complete masters and four diagnostic maps are valid Float32 4120-square FITS; corrected master NDISTCOR=25, with no such card in the ordinary master. All 32 science sources, the immutable original template and artificial reference retain SHA256, sizes and mtimes (34/34). The original template also still matches the generator's original input fingerprint.

Final formatting, default/all-feature Clippy with warnings denied and local release suites PASS: 148/148 default, 151/151 all-feature, with real FITS, required AVX2 and actual GPU enabled. This includes correction validation progress reaching the exact final count monotonically without duplicate UI, source-protection gates and unchanged ordinary arithmetic. A transient intermediate Windows test-EXE linker lock was resolved by retry; no test was weakened. Existing normal local permissions are needed for atomic publication and hardlink protection tests. No Windows security settings were changed. The old frozen-similarity profiling example explicitly refuses nonlinear plans instead of silently discarding coefficients; this helper-only guard passed final all-target lint after the engine suites.

The final progress-accounting-only change follows the measured integration pair; no pixel or fitting arithmetic changed afterwards. Raw benchmark/source/product evidence remains private in outputs/geometric_correction; sanitized metrics and fingerprints are in M5_GEOMETRIC_CORRECTION_EVIDENCE.json. GPU-enabled release GUI/CLI build PASS, desktop shortcut targets the updated executable, Windows FileDescription/ProductName remain FulgurStack 0.1.0. GUI SHA256 b4bbccc929a557a50d29cd769ffcaccd9c5a57b55e59b4f1fcc13e3507e0b745; CLI e2f75add35f0b548f862fd4dc3f746a771b738b9ad3069a602384d328f157d73. Native visual review is pending. No commits, pushes, PR changes or remote CI.

For product review, select the new artificial FITS as the external reference, explicitly enable the advanced correction and inspect the corrected/skipped counts and corner alignment. Private comparison masters are control_linear.fits and corrected_cubic.fits. A direct new-destination CLI run is:

```powershell
cargo run --release -p fulgur-cli --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "outputs/geometric_correction/RegisteringTemplate200pct_radial3pct.fit" --geometric-correction --canvas reference --tile-size 512 --fits-cache-mib 0 --output "outputs/geometric_correction/manual_review.fits" --report "outputs/geometric_correction/manual_review.json"
```
