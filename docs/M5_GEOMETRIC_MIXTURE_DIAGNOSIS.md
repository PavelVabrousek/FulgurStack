# M5: Owner-observed geometric mixture regression (2026-10-26)

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

**Product acceptance: FAIL.** This diagnosis supersedes readiness of the optional
geometric-correction delivery. Existing unit/gate passes establish their tested
properties, not safe whole-field fallback selection. No engine change, stack
rerun, release build, commit, push or remote CI is part of this analysis.

## Evidence from the existing owner run

Read the existing `outputs/master_v15_report.json`; do not reconstruct a new
stack or change any source/product FITS. SHA256 of that report is
`6b981ad8aac54d8d311898fb93a3734748270f17289f08754c043a23ca98a102`.
The run used the artificial 4120-square external reference with correction
enabled, 1140 originals, 940 quality-selected/integrated and 200 excluded.
Elapsed processing was 299.0725123 s; recorded peak RSS 4063457280 bytes.
These are the owner's run measurements, not a new benchmark.

| Integrated mapping | Frames | Share of summed frame weights | Fixed-identity star RMSE, reference pixels |
|---|---:|---:|---:|
| Independently validated source cubic | 801 | 85.6878% | 0.308001 |
| Ordinary source similarity after skipped correction | 139 | 14.3122% | 17.170619 |

The majority did receive correction. The minority has sufficient weight to
produce visible secondary star locations. At 114 common star identities with
at least five samples from each group, separation between the two groups'
median predicted positions has median 11.072568 px, 95th percentile 40.754197 px
and maximum 55.079313 px.

The diagnostic joins source-to-native-anchor photometric match identities with
native-anchor-to-external match identities, then evaluates the actual recorded
output mapping without any residual cutoff or nearest-star rematching. All 940
rows declare direct-master photometric identities. There are 79966 such
observations for cubic rows and 13309 for similarity rows. This is evidence of
cross-group disagreement on named common stars, not an independent ground-truth
validation of every cubic coefficient or of unobserved image regions.

Reproduce the read-only catalog analysis to a new JSON destination:

```powershell
python scripts/tests/analyze-geometric-mixture.py outputs/master_v15_report.json outputs/geometric_correction/master_v15_mixture_analysis_new.json
```

The private existing result is
`outputs/geometric_correction/master_v15_mixture_analysis.json`.
The helper requires the locally installed NumPy; it is not a production runtime
dependency. It opens source reports read-only and creates a new aggregate JSON.
No source names, star positions, GPS or model coefficients are emitted.

## Why those 139 frames continued

Correction decisions are: 57 insufficient independent spatial/outer coverage,
74 failed outer-quadrant validation, 7 failed the combined held-out acceptance
gate and 1 insufficient/degenerate training field. The report does not distinguish
missing quadrant evidence from a worsening quadrant; do not infer that detail.

The seven `no_confirmed_held_out_improvement` reasons are misleading: their
validation error actually improved from 7.948696..11.367301 px to
1.004383..1.090363 px, but exceeded the required 1 px ceiling. They are failed
absolute-accuracy gates, not evidence that the linear mapping was adequate.
Insufficient evidence is also not proof of no distortion.

In `reference.rs`, an ordinary fit is calculated first. If `solve_seeded` returns
no nonlinear model, that ordinary fit remains accepted. `integrate::sources`
then faithfully supplies `distortion=None` for that row. No nonlinear model is
silently dropped between the accepted report and integration: the incorrect
fallback decision is already present in the registration plan.

Ordinary consensus calculates its small RMSE only from matched stars within
1.5 pixels and requires at least ten. Here the 139 similarities retain only
14..33 inliers (median 27), versus 103..128 (median 119) for cubic rows.
Their reported inlier RMSE is 0.879775 px aggregate, although fixed-identity
whole-overlap error is 17.170619 px. A locally matching subset masks the
whole-field mismatch. The original implementation checked whether the cubic
improved its validation field, but did not require the fallback to pass a
comparable spatial validation before integration.

The resulting master/map patterns are consistent with this measured mixture:
most sources place stellar signal at the distorted reference positions, while
the fallback group places some stars elsewhere. Upper rejection removes some
of those competing bright samples; surviving-sample Coverage consequently
decreases around stars, and residual displaced signal can contaminate the
master. Pixel rejection cannot repair geometric misregistration. Exact
per-pixel attribution of every visible artifact was not replayed.
The map's possible maximum of 940/1140 = 82.456140% is expected under the
all-original-input denominator, independently of this defect.

## Required correction design (not implemented by this analysis)

1. Make whole-field registration validity separate from nonlinear improvement.
   Evaluate linear and nonlinear candidates against a spatially distributed
   fixed identity validation set with explicit unmatched/missing-star accounting.
   A smaller inlier subset must not manufacture a low error. Preserve genuinely
   independent coefficient validation and avoid validation-label leakage.
2. Use ordinary registration after an optional correction is skipped only if
   ordinary mapping itself passes absolute error and spatial support gates in
   the same output reference coordinates. Distinguish `not_needed`,
   `insufficient_evidence`, `absolute_accuracy_failed` and invalid models.
   With a distorted target, unknown or failed correction cannot silently mean
   accepted whole-field linear registration.
3. For this same-optics sequence, a validated anchor-to-external cubic composed
   with a source-to-anchor pose is a possible recovery candidate. Validate that
   complete mapping on each source before applying it, including the native
   pose and support/domain assumptions. Do not blindly share optical models
   across different lenses or extrapolate an unsupported cubic beyond its
   validated overlap. Preserve one pixel interpolation and bounded memory.
4. If no candidate passes, mark the source as a registration failure and exclude
   it from integration, with explicit counts/reasons. Preserve Use decisions,
   original-input map denominators and native scientific provenance. If the
   anchor/reference geometry itself is unconfirmed, fail clearly or use one
   explicitly identified coherent target for the whole run; never silently
   produce mixed incompatible geometry.
5. Add a regression where a convincing small linear inlier cluster coexists
   with broad nonlinear distortion. Assert rejected unsafe fallback, absolute
   residual limits for every integrated source and stable common-star positions
   across mapping methods, sessions, rotations and tile sizes. Reuse the stored
   940-source catalogs for registration-only assessment, then perform a bounded
   diagnostic integration with both previously accepted and skipped cases and
   inspect master/maps. Do not rerun the full reference benchmark by default.

Mixed model types can be valid when both independently reach the same target
geometry within tolerance. Forcing every row to cubic, weakening validation or
changing sigma rejection is not the remedy. The earlier 27-frame smoke already
contained 25 cubic and 2 linear rows; counts and positive cubic fit metrics did
not demonstrate that the fallback rows were safe. Product readiness must remain
FAIL until the new acceptance/recovery gates and image-level regression pass.
