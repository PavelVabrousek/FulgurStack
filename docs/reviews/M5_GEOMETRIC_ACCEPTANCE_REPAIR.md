# M5: Whole-field registration acceptance repair (2026-10-26)

## Owner acceptance update — 2026-10-26

The owner explicitly confirms the repaired geometric correction: **product review PASS for this feature**. No additional run identity, measurements or platform checks were supplied with the confirmation; do not invent them. The acceptance applies to the repaired whole-field behavior, not the older unsafe mixture. The pending-review statements below describe the earlier delivery-time state. M5 remains open; Auto validation on actual multi-night observations awaits suitable owner test data. This status update stays local without a new commit/push.

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Owner-authorized local repair, ADR-066. M5 remains open. This replaces the unsafe
fallback behavior diagnosed in `M5_GEOMETRIC_MIXTURE_DIAGNOSIS.md`. Do not interpret
the older correction-delivery gate passes as acceptance of that older behavior.

## Behavior

With geometric correction requested, optional improvement and absolute mapping
validity are separate decisions. External registration joins source/native-anchor
and native-anchor/reference identities before evaluating candidates. It never
removes witnesses because the output candidate predicts them badly. Require
broad spatial support, at least 80% of expected anchor identities within the
source, absolute error limits and validation in all four outer quadrants.

First try the independently trained source cubic. If it fails, try the validated
anchor/reference calibration composed with a newly fitted native source pose.
Reserve deterministic spatial source witnesses before fitting that pose. For
the calibration image itself, retain its original modulo-five held-out split.
Sharing a calibration never bypasses per-source accuracy checks; different
optics can still require their own directly validated cubic.

An ordinary similarity fallback is fitted on training identities and must pass
its own absolute spatial validation. If no candidate passes, exclude the frame
geometrically and report why. If the anchor/reference bridge lacks broad accuracy,
stop before integration. Native optional corrections also have broad absolute
support checks; an external complete mapping can recover a native geometry
failure only with usable native direct-match photometry. Re-establish every
enabled external row's inverse-noise/HFR weight in the same native units before
the final normalization, avoiding mixed normalized/raw weights during recovery.

Absolute acceptance requires >=50 fixed identities, >=30 pose-training and >=10
validation stars; >=80% expected support; >=35% of both sensor axes; >=10/16
occupied spatial cells and >=6/16 validation cells; outer held-out witnesses in
all quadrants; <=1 reference-pixel RMS for the complete fixed set, validation
set and every outer validation quadrant; <=3 px maximum individual error. The
direct cubic retains its existing >=40 training stars and independent nonlinear
improvement gates as well. Native ordinary mapping receives a broad absolute
sanity check; this is not a new claim that its historical pose fit was trained
without those stars. Recovery and external similarity poses exclude their
reserved validation stars explicitly.

JSON adds `registration_validation` with counts, expected support, errors and
acceptance reason; `initial_correction_reason` preserves the source-fit outcome
when recovery replaces it. Failure above the absolute precision limit is now
distinguished from lack of material improvement. The integration entry point
refuses an accepted opt-in row without a successful field assessment. Results
shows corrected, validated linear and failed geometry counts. Use choices,
original-input map denominators, native photometry/exposure/GPS, scientific
pixel rejection, the single interpolation, memory ceiling, default-off option,
fixed STOP and Compact Desk layout are preserved. Disabled registration still
matches the frozen ordinary implementation.

## Real-catalog regression

Reuse the stored `master_v15_report.json`, its original 1140 source memberships,
940 Use-on selections, the same anchor and artificial 4120-square reference.
Read stored catalogs and source headers; do not reinspect images or integrate
the full dataset. Of the previous 139 incompatible linear mappings, 136 recover
through validated anchor composition and 3 fail safely. No incompatible linear
mapping remains accepted.

Final counts: 937 accepted, 3 geometric failures, 200 quality excluded. Accepted
methods are 766 directly validated cubic and 171 calibrated anchor compositions;
the latter also replace 35 direct cubics which fail the stricter shared-identity
audit. The three failures are two absolute accuracy failures and one insufficient
field-support case. Eight initial native-support failures are recovered by a
confirmed external mapping with usable native photometry.

Across accepted rows, complete fixed-identity RMS ranges 0.227364..0.642004 px;
median 0.311609 px. Maximum held-out RMS is 0.719047 px and maximum individual
fixed-star error 2.748355 px. These errors cover the catalog common field, not
independent truth for every image pixel or arbitrary unsupported extrapolation.
The earlier run's 17.170619 px fallback-group RMS is no longer admitted.

Raw local plans/evidence remain in `outputs/geometric_correction/repair_*`.
Development replay and final verification timings are catalog-only telemetry,
not comparable full-stack speed claims. No full reference integration benchmark
was repeated. See the accompanying sanitized evidence JSON for final verification
and release fingerprints. Native manual appearance/PixInsight acceptance and
macOS remain unverified.

## Manual review

The old GUI remains running and Windows locks
`target/release/fulgurstack-gui.exe`. Both release binaries have been compiled
and linked. Package the linked GUI and CLI without replacing that running file:
`target/local-release/geometry-acceptance-20261026/`. The existing desktop shortcut
now targets this new GUI with its embedded approved icon, preserving its prior
working directory and arguments. No user process was stopped, and the running
old application has not acquired the fix. After finishing any current work,
close it and start the desktop shortcut again. The standard Cargo GUI
publication step remains blocked by that old process; the versioned local
release is the review delivery. No security/ACL workaround was used.

In the new GUI, select the artificial reference and explicitly enable **Correct
geometric distortion**. Check corrected/linear/failed counts and inspect the
master, Coverage and High Rejection at stars and corners. A failure count means
those frames did not contribute; changing setup does not recompute an old result.
Run a new stack to inspect the repaired behavior.

For a bounded new-destination CLI review:

```powershell
cargo run --release -p fulgur-cli --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "outputs/geometric_correction/RegisteringTemplate200pct_radial3pct.fit" --geometric-correction --canvas reference --tile-size 512 --fits-cache-mib 0 --output "outputs/geometric_correction/repair_manual_review.fits" --report "outputs/geometric_correction/repair_manual_review.json"
```

No commit, push, PR change, remote CI, Windows security/power setting change,
source overwrite or intermediate registered image belongs to this repair.

## Local verification result

Formatting, default and all-feature Clippy with warnings denied: PASS. Local
release suites: 154/154 default and 157/157 all-feature, with real FITS, required
AVX2 and actual GPU gates enabled. Regression coverage includes the convincing
small linear inlier cluster, unsafe fallback rejection, recovery/linear target
coherence, training-only recovery pose, different-optics calibration rejection,
weight recovery and integration-entry refusal of an unvalidated accepted row.
Frozen default/matcher/serial gates and existing precision/tile/source protection
gates remain PASS. One test expectation initially differed by one rounding bit
from algebraic reassociation; it now checks mathematical weight accuracy within
four machine epsilons. Production weight arithmetic was unchanged by that test fix.
Initial relative-path test setup and cached Windows hardlink warnings were
resolved locally; final all-target Clippy uses a nonincremental cache and is clean.

The final all-feature catalog replay accepts 937/940, with 10.4099946 s measured
inside the replay and 789274624-byte recorded process peak. It overlapped other
local test work; do not compare it as an isolated speed result with earlier
serial/native/full-stack timings.

The actual CPU release 32-source smoke integrates all 27 selected sources:
23 direct cubic, 4 validated anchor compositions, no linear fallback and no
geometric failure, five quality-excluded originals. Processing 14.5014043 s,
peak RSS 724.00390625 MiB; complete fixed-star RMS 0.311663..0.497815 px.
The 4120-square Float32 RGB master and two maps are finite, in 0..1, with
`NDISTCOR=27` on the master. Coverage plus upper rejection stays within 27/32
at every channel sample; all-original denominator remains 32. All 34 source and
reference hashes, sizes and mtimes match both the pre-run snapshot and the
previous batch's immutable manifest. No intermediate image was written.
This is a bounded smoke observation, not a performance improvement measurement.

Manual PixInsight products are `outputs/geometric_correction/repair_smoke.fits`,
`repair_smoke_coverage.fits` and `repair_smoke_high_rejection.fits`; the JSON report
is `repair_smoke.json`. GUI SHA256
`edc2d80ffe6d8d38322a9abb55d40f1cd76d76df436146345265a63f4e958851`;
CLI SHA256 `e1a5da5e738cbb86ba811f1b86d79c93db4ab45b6f5884345482b908212c5253`.
Windows FileDescription/ProductName remain FulgurStack. Native display/PixInsight
acceptance and macOS remain unverified. All work is local and unpublished.
