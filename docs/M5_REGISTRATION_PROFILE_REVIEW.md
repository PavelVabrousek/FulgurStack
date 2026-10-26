# M5 Registration Phase Profile Review

The parallel external-source fits target a small part of the measured 32-frame registration. The dominant cost is still serial native registration: 30 frames require the triangle fallback. This is a diagnostic delivery; no registration algorithm or quality policy was changed, and M5 remains open.

## Measurement scope

Two current-version release diagnostics were run locally: an initial coarse profile identified the dominant native geometry/photometry envelope; one refined profile separated its operations and recorded each native fit. No historical baseline was executed, and no 1,140-frame processing was run. Stored products and controls were reused.

Both used 32 immutable DS-B inputs, `RegisteringTemplate200pct.fit`, CPU with 16 threads, 512x512 tiles, 12288 MiB ceiling, FITS cache explicitly 0, and the full 4000x4000 reference grid at origin (0,0). The master and both maps remain exactly equal to accepted products. No image intermediates, commits, pushes, PR updates or remote CI.

| Current diagnostic | Processing (s) | Registration (s) | Peak RSS (MiB) |
|---|---:|---:|---:|
| Coarse | 10.697850 | 1.408911 | 647.652 |
| Refined | 11.735369 | 1.604270 | 668.996 |

Differences between these runs are retained, without claiming acceleration from telemetry. The following breakdown belongs entirely to the refined run. Inspection is separate: 0.237584 s. All 32 frames registered successfully; the same 27 are accepted, five quality-rejected, and 270 s exposure contributes.

## Disjoint registration wall intervals

| Scope | Operation | Wall (s) | Registration share |
|---|---|---:|---:|
| native | Reference/session selection | 0.0001941 | 0.012% |
| native | Session bridge geometry | 0.0006027 | 0.038% |
| native | Per-source native geometry | 1.0573528 | 65.909% |
| native | Direct native master correspondences | 0.0115493 | 0.720% |
| native | Photometric flux ratios | 0.0003303 | 0.021% |
| native | Prediction and row bookkeeping | 0.0003061 | 0.019% |
| native | Weight/footprint preparation | 0.0000054 | 0.000% |
| native | Common-intersection rectangle plan | 0.0179072 | 1.116% |
| native | Coverage-optimized rectangle plan | 0.0465333 | 2.901% |
| native | Finalization | 0.0000010 | 0.000% |
| external | External reference FITS read | 0.0325235 | 2.027% |
| external | External target/filter validation | 0.0000028 | 0.000% |
| external | External reference star catalog | 0.0656148 | 4.090% |
| external | Native-anchor to external bridge | 0.0676135 | 4.215% |
| external | Parallel direct external source fits | 0.0210790 | 1.314% |
| external | Weight/footprint preparation | 0.0000164 | 0.001% |
| external | Common-intersection rectangle plan | 0.0904207 | 5.636% |
| external | Coverage-optimized rectangle plan | 0.1894428 | 11.809% |
| external | Finalization | 0.0000144 | 0.001% |
| outer | Outer reference resolution/unattributed overhead | 0.0027599 | 0.172% |
| Total | All registration | 1.6042700 | 100.000% |

The native and external phase lists partition their own parent wall timers. The detailed native intervals replace, rather than add to, the native geometry/photometry envelope in this table. Outer overhead closes the gap to the pre-existing registration timer. The external source-fit timer wraps the joined parallel work: it is elapsed wall time, not summed worker time. The canvas rows measure rectangle proposals retained in the registration report, not an actual crop of the output.

## Native source-fit paths

| Method | Frames | Native fit wall (s) |
|---|---:|---:|
| `sequential_direct` | 1 | 0.0015630 |
| `session_anchor` | 1 | 0.0005643 |
| `triangle_fallback` | 30 | 1.0552255 |

The 30 fallback fits cost 1.0552255 s (65.776% of registration); the complete native source-geometry cost is 1.0573528 s. These durations include any unsuccessful initial prediction attempt before fallback. Photometric ratios cost only 0.0003303 s. The five quality-rejected frames still receive diagnostic native fits, costing 0.1866804 s; their integration weights remain zero.

The selected subset is sparse: median successive header DATE-OBS gap is 631.963 s (10.533 min). An independent read-only replay of the stored one-session prediction, catalogs and native transforms finds fewer than ten unique initial pairs within 12 px for 29 of the 30 fallback rows. The remaining fallback row has 12 initial candidates but still fails the native consensus path. The native method is measured by Rust; Python only checks the predicted-seed geometry and is not a timed solver or a new reference run. Sparse-sample fallback frequency must not be assumed for the dense full set.

## Per-source diagnostic table

Times below cover only the native per-source geometric fit, not its photometry, external refinement or integration. Initial pair counts come from the independent replay. Source order is retained.

| Source | Quality accepted | Native method | Native fit (s) | Initial pairs within 12 px |
|---|---|---|---:|---:|
| `Light_M 33_10.0s_IRCUT_20261006-204534.fit` | yes | `triangle_fallback` | 0.0347161 | 1 |
| `Light_M 33_10.0s_IRCUT_20261006-205221.fit` | yes | `triangle_fallback` | 0.0290071 | 2 |
| `Light_M 33_10.0s_IRCUT_20261006-210159.fit` | no | `triangle_fallback` | 0.0335642 | 4 |
| `Light_M 33_10.0s_IRCUT_20261006-211122.fit` | yes | `triangle_fallback` | 0.0285735 | 4 |
| `Light_M 33_10.0s_IRCUT_20261006-212112.fit` | no | `triangle_fallback` | 0.0301071 | 4 |
| `Light_M 33_10.0s_IRCUT_20261006-213153.fit` | yes | `triangle_fallback` | 0.0304553 | 4 |
| `Light_M 33_10.0s_IRCUT_20261006-214211.fit` | yes | `triangle_fallback` | 0.0236063 | 3 |
| `Light_M 33_10.0s_IRCUT_20261006-215309.fit` | yes | `triangle_fallback` | 0.0390300 | 3 |
| `Light_M 33_10.0s_IRCUT_20261006-220327.fit` | yes | `triangle_fallback` | 0.0312924 | 1 |
| `Light_M 33_10.0s_IRCUT_20261006-221415.fit` | yes | `triangle_fallback` | 0.0416946 | 6 |
| `Light_M 33_10.0s_IRCUT_20261006-222507.fit` | yes | `triangle_fallback` | 0.0313515 | 3 |
| `Light_M 33_10.0s_IRCUT_20261006-223610.fit` | no | `triangle_fallback` | 0.0371567 | 1 |
| `Light_M 33_10.0s_IRCUT_20261006-225204.fit` | yes | `triangle_fallback` | 0.0414782 | 2 |
| `Light_M 33_10.0s_IRCUT_20261006-230212.fit` | yes | `triangle_fallback` | 0.0401743 | 8 |
| `Light_M 33_10.0s_IRCUT_20261006-231414.fit` | yes | `triangle_fallback` | 0.0410683 | 2 |
| `Light_M 33_10.0s_IRCUT_20261006-232807.fit` | yes | `triangle_fallback` | 0.0336480 | 3 |
| `Light_M 33_10.0s_IRCUT_20261006-233727.fit` | yes | `session_anchor` | 0.0005643 | 250 |
| `Light_M 33_10.0s_IRCUT_20261006-234830.fit` | yes | `triangle_fallback` | 0.0488686 | 12 |
| `Light_M 33_10.0s_IRCUT_20261006-235944.fit` | yes | `triangle_fallback` | 0.0393925 | 0 |
| `Light_M 33_10.0s_IRCUT_20261007-001131.fit` | yes | `triangle_fallback` | 0.0357396 | 4 |
| `Light_M 33_10.0s_IRCUT_20261007-002106.fit` | yes | `triangle_fallback` | 0.0257739 | 4 |
| `Light_M 33_10.0s_IRCUT_20261007-003159.fit` | yes | `sequential_direct` | 0.0015630 | 14 |
| `Light_M 33_10.0s_IRCUT_20261007-004225.fit` | yes | `triangle_fallback` | 0.0265151 | 1 |
| `Light_M 33_10.0s_IRCUT_20261007-005149.fit` | yes | `triangle_fallback` | 0.0381557 | 2 |
| `Light_M 33_10.0s_IRCUT_20261007-010137.fit` | yes | `triangle_fallback` | 0.0284398 | 3 |
| `Light_M 33_10.0s_IRCUT_20261007-011221.fit` | yes | `triangle_fallback` | 0.0223091 | 3 |
| `Light_M 33_10.0s_IRCUT_20261007-012253.fit` | yes | `triangle_fallback` | 0.0281316 | 2 |
| `Light_M 33_10.0s_IRCUT_20261007-013244.fit` | yes | `triangle_fallback` | 0.0345334 | 1 |
| `Light_M 33_10.0s_IRCUT_20261007-014249.fit` | no | `triangle_fallback` | 0.0361025 | 1 |
| `Light_M 33_10.0s_IRCUT_20261007-015458.fit` | yes | `triangle_fallback` | 0.0486811 | 1 |
| `Light_M 33_10.0s_IRCUT_20261007-020421.fit` | no | `triangle_fallback` | 0.0497499 | 3 |
| `Light_M 33_10.0s_IRCUT_20261007-021544.fit` | yes | `triangle_fallback` | 0.0459091 | 0 |

## Why the previous parallelization did not establish a speedup

Only the independent external refinements were parallelized. Native chronological prediction/registration, photometry, external catalog preparation, the anchor bridge and all canvas planning remain outside that loop. In this refined run the changed loop takes 0.0210790 s (1.314% of registration), while native fallback takes 1.0552255 s and all four canvas proposals take 0.3443040 s (21.462%). Even eliminating the remaining parallel-loop wall completely would save only 0.021079 s in this run. This is a bound on further improvement of the current loop, not an estimate of its former serial duration or a measured serial-to-parallel speedup.

The stored full-set pair remains unfavorable: processing 275.913411 -> 300.919385 s; registration 25.259039 -> 26.243953 s. Of the total increase, 22.792058 s is in unchanged reconstruction/reduction phases. This rules out attributing all the total slowdown to the registration change. It does not isolate background-load/cache/temperature effects or prove that external fitting became faster. Old reports have no loop timer, so its exact before/after benefit and the exact cause of the historical registration slowdown remain unmeasured. The 32-frame profile identifies the present bottleneck; it is not a full-set phase profile.

## Recommended follow-ups

1. Target the measured native fallback first. Reuse immutable reference-star triangle descriptors and their search index across native fits, then remove redundant geometric matching work while preserving candidate order, ties, deterministic hypotheses and all fit criteria. The current code rebuilds the same target triangles/index on each fallback. This is a proposed optimization, not an implemented or isolated subroutine speedup.
2. Optimize the exact rectangle-planning calculations, which consume a further 21.462% of this registration. In reference-canvas stacking these proposals do not select or crop the scientific grid; any future deferral must explicitly preserve or revise report expectations. No planning output was removed here.
3. Keep bounded parallel external refinements and cache off. Improving only this already short loop is a lower priority. Do not project sparse-subset shares or predict a GPU benefit for the full set without the corresponding evidence.

## Verification and artifacts

- Format, default/all-feature Clippy with warnings denied: PASS.
- Release tests: 115/115 default, 117/117 all-feature; actual local GPU, AVX2, real-FITS and frozen serial/parallel exactness tests: PASS.
- Both diagnostic runs: entire master, coverage and high-rejection FITS bytes equal stored accepted outputs. Source/reference hashes, quality/photometry/registration/scientific counts, I/O/work counters and memory estimates unchanged. Timing/RSS and permitted available-RAM snapshots only may differ.
- Additive timing schemas and partition sums are validated before excluding telemetry from scientific comparisons. Negative controls reject invalid timing/indices/partitions; changed scientific weights remain detectable.
- A restricted-environment atomic-output denial and a transient Windows linker output-open failure were resolved by rerunning the same local gates; the final successful log is retained. No checks were relaxed.

Machine-readable summary: [phase profile](../outputs/M5_registration_phase_profile.json). Detailed raw run: [refined JSON](../outputs/M5_registration_detail_DS-B_after_external200.json). Scientific gate: [equivalence JSON](../outputs/M5_registration_detail_DS-B_equivalence.json). Initial observation: [coarse JSON](../outputs/M5_registration_profile_DS-B_after_external200.json). Independent prediction analysis: [prediction JSON](../outputs/M5_registration_prediction_analysis.json). Binary/source fingerprints are recorded in the summary.

## Reproduce registration-only profiling

Run from the project root; the output report must not exist. This profiles registration without stacking or FITS-block-cache activity.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 register --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --report "outputs/M5_registration_profile_manual.json"
```

Read `phase_profile.native`, `phase_profile.native_fit_details` and `phase_profile.external` in the JSON. The CPU path remains active; enabling the build feature preserves the GPU option but does not select GPU registration.
