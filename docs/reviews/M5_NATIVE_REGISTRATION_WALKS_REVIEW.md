# M5: Independent Native Registration Walks (2026-10-26)

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Owner-authorized local quality-preserving registration optimization, ADR-064. Published base 64a5dbe. M5 remains open; no commits, pushes or remote CI are authorized for this batch.

## Change and quality boundary

Run the forward and backward chronological walks from a session anchor concurrently in the existing Rayon pool, with at most two active walks. Keep sessions and their bridge fits serial. Each walk retains its complete original frame order and selected-only prediction history. Every frame is fitted directly to the session anchor; this does not introduce chained registration or new predictions. Solver arithmetic, correspondence/ranking order, thresholds, photometry, source selection and optional excluded-frame diagnostics are unchanged.

Share the existing immutable master/session matcher and its lazy OnceLock target descriptors. No extra target copy is retained. Two catalog scratch sets and indexed row output fit existing runtime headroom; no image buffers, source/frame cache, dependency, source write or image intermediate are introduced. Results and per-fit records are merged in their historical forward/reverse order, then weights, footprints and canvases are finalized in the original source order.

Rayon join waits for both branches before success or error returns. Both retain existing cancellation checks and use the serialized progress observer, so progress remains monotonic. Compact Desk, Windows identity, remembered worker settings and fixed STOP remain unchanged. A one-thread pool executes the same work serially. This step exposes two independent tasks, not 24 independent per-frame fits; additional parallelism would require a separately verified design.

## Timing interpretation

Nonoverlapping phase intervals still partition actual wall time. Native fit details now explicitly label their scope as overlapping walk task elapsed. walk_group_wall_seconds measures the joined groups; walk_task_elapsed_seconds sums both task intervals. Per-fit, direct-match and photometry durations can overlap and must not be treated as additive wall phases. Bookkeeping covers residual task/serial elapsed; detailed sums equal native geometry/photometry wall minus joined-group wall plus summed walk elapsed. The verification script validates this identity and preserves support for historical serial profiles.

## Verification and measurement design

A test-only frozen serial planner is copied exactly from commit 64a5dbe; the original body was checked against Git before measurement. Compare all scientific report fields (including signed-zero serialization) and fit method/order across 1/2/3/12/24/32 worker pools, automatic/manual/edge anchors, multiple sessions, exclusions, mixed exposures, missing timestamps, insufficient-star failures, diagnostics and cancellation/progress. Geometry and integration retain their existing independent scientific gates.

Real-data replay reads only the stored native CFA catalogs from outputs/master_v13_report.json and original source headers: 1140 originals, 940 selected, same automatic reference and 0.80 coverage policy. The original inspections and integration are not recomputed. Both serial control and parallel candidate consume the exact same parsed catalogs in the same 24-thread release test executable. This isolates scheduling while avoiding a repeated reference integration. One excluded warmup per path precedes three paired rounds ordered serial/parallel, parallel/serial, serial/parallel. No builds/tests run concurrently with the measurement. Other applications, OS cache and thermal scheduling remain uncontrolled; report local registration-only observations rather than universal or full-application speedups.

Require exact scientific reports against the frozen serial control on every replay, unchanged membership/success versus the stored report, at most 1e-8 pixel corner displacement and 1e-10 relative normalization differences versus its decimal replay, <=1.5 pixel inlier RMSE, and identical selected rows with excluded diagnostics on. Historical decimal catalogs may differ by floating-point rounding from the original in-memory run; exactness claims apply to the same parsed serial/parallel input. Hash every original FITS before/after and retain sizes and modification times. Raw plans/evidence stay private under outputs/M5_native_walks_20261026; portable summaries exclude source identities and coordinates.

Only local Windows verification is claimed. macOS and native appearance remain unverified. The prior integration performance evidence is unchanged and is not relabeled as a new full integration measurement.

## Measured result

Three measured rounds after excluded warmups, registration seconds:

| Round | Frozen serial planner | Parallel outward walks |
|---|---:|---:|
| 1 | 15.6052102 | 10.0864561 |
| 2 | 17.0449013 | 10.1033996 |
| 3 | 17.1838581 | 10.4190931 |
| Median | 17.0449013 | 10.1033996 |

Median registration is 6.9415017 s / 40.724798% shorter in this replay. This comparison uses a fresh frozen serial planner on today's same parsed input, rather than the historical GUI registration time of 9.6855064 s. Historical timing conditions and executable scope differ; no claim is made that the new 10.10 s replay beats that historical wall time. The independent paired comparison is the relevant local observation.

All 940 selected frames register successfully; 200 Use-off frames are skipped by default. All scientific report fields and fit method/order are exact between serial/parallel execution. There are 602 sequential direct fits, 337 triangle fallbacks and one anchor. Diagnostic mode preserves every selected row exactly. All 1140 source SHA256 hashes, sizes and modification times remain equal. Independent Python profile-schema and exact scientific-plan verification PASS. Maximum historical replay corner difference is 2.033692e-12 pixel, maximum native inlier RMSE 0.534220705 pixel, within the unchanged 1.5 pixel limit.

Median process CPU seconds are 16.718750 serial and 18.765625 parallel: shorter elapsed time does not imply lower total CPU cost. Replay-process peak working set is 1413885952 bytes, including parsed stored JSON/catalogs and verification; it is not full native-stack or integration RAM. No whole image is generated. The replay also exports large private JSON plans after the measured intervals; export/hash/verification time is excluded from registration timings. Final plan export uses a buffered writer for future reproductions; this report-only test change was made after measured intervals and changes no engine operations.

Portable observations are in M5_NATIVE_REGISTRATION_WALKS_EVIDENCE.json. Actual GUI timing remains for manual review; no full stack was rerun to seek a favorable result.

## Final local gates

Formatting and default/all-feature Clippy with warnings denied PASS. Default release suites PASS 137/137; all-feature release suites PASS 139/139, with real FITS, required AVX2 and actual GPU hardware checks enabled. The real catalog replay additionally PASSes all exactness, historical tolerance, diagnostic isolation and source fingerprint checks; the independent Python profile/science comparison PASSes. The frozen oracle body matches the published serial planner exactly.

Initial restricted-sandbox suites could not publish temporary CLI outputs or create the hardlink needed by the existing input-protection test (Windows access denied). Rerun with a repository-local temporary directory and normal local permissions PASSes all gates; no test was disabled or weakened, no application arithmetic changed, and no Windows security setting was changed. No remote CI or macOS verification is claimed.

Local release build PASS: cargo build --release -p fulgur-gui -p fulgur-cli --features gpu --bins. GUI SHA256 59993aafc2eca0a5fac0b1411351970365653a2ea700b841ef32f1b394f7727d; CLI cd235e26ed7c5da3fee0ba11e3b2382f7172293cb1c197deaaf8b38ea0320a6a. Windows metadata still identifies FulgurStack 0.1.0. The existing desktop shortcut targets the updated standard release and its embedded approved icon. Native manual review remains pending. No commit, push or remote CI.
