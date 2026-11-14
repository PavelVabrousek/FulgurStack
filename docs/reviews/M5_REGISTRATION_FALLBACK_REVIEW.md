# M5 Exact Registration Fallback Optimization Review

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

The native fallback now prepares each immutable reference-star triangle/index structure once and reuses it across native fits. Consensus reuses bounded inlier vectors instead of allocating another vector for each hypothesis. Candidate order, tie rules, hypotheses, floating-point arithmetic and all scientific thresholds remain unchanged. M5 remains open.

## Scope and verification

All work and tests ran locally on Windows. No commits, pushes, PR updates or remote CI. Both datasets use `RegisteringTemplate200pct.fit`, CPU 16 threads, 512x512 tiles, 12288 MiB ceiling, FITS cache explicitly 0 and the entire 4000x4000 grid at (0,0). Stored controls were reused and never executed again.

- Format/default-all-feature Clippy with warnings denied: PASS.
- Release tests: 118/118 default and 120/120 all-feature. Actual local GPU, AVX2 and real-FITS checks PASS.
- A frozen complete prior fallback/consensus/triangle implementation verifies all solution fields and explicit transform/residual bits across scales, rotations, predictions, missing/outlier/duplicate stars, failures and real native catalogs. Target preparation is tested for laziness, reuse, scoped ownership, immutability and cancellation.
- No new dependency or image/FITS cache. At most the master and current session target descriptors are retained during native registration, then released before integration. Descriptors retain original triangle order and ordered bucket entries. The direct predicted path does not build descriptors. Two bounded consensus vectors keep original inlier order and residual summation. Integration memory plans remain exact.

## Stored-control performance comparison

Processing times below exclude build/test work. Complete command durations, binary/source hashes, all phase telemetry and adverse observations are retained in the machine-readable report. Historical differences are observed measurements, not isolated causal or portable speedup guarantees.

### DS-B - 32 inputs

Stored report: `outputs\M5_registration_detail_DS-B_after_external200.json`. New report: `outputs\M5_fallback_DS-B_after_external200.json`. Selection remains 27 accepted / 5 rejected; master and both maps have exact entire-file byte equality.

| Metric | Stored | Optimized | Decrease |
|---|---:|---:|---:|
| Processing wall (s) | 11.735369 | 10.460420 | 10.864% |
| Registration wall (s) | 1.604270 | 1.407678 | 12.254% |
| Inspection wall (s) | 0.237584 | 0.227826 | 4.107% |
| Reconstruction wall (s) | 4.102815 | 3.649391 | 11.052% |
| Rejection/accumulation/maps wall (s) | 3.881847 | 3.596489 | 7.351% |
| Write wall (s) | 1.128387 | 0.938756 | 16.806% |
| Peak process RSS (MiB) | 668.996094 | 707.566406 | -5.765% |

Processing difference is 1.274949 s, of which 0.196592 s is in the registration envelope and 1.078357 s in other phases/overhead. The latter must not be presented as an isolated effect of this optimization. The complete command took 10.862355 s.

The same 30 native fallback fits took 1.0552255 -> 0.9328137 s (11.601% shorter). The native method is unchanged for every one of the 32 sources.

### DS-A - 1140 inputs

Stored report: `outputs\M5_external_parallel_DS-A_after_external200.json`. New report: `outputs\M5_fallback_DS-A_after_external200.json`. Selection remains 940 accepted / 200 rejected; master and both maps have exact entire-file byte equality.

| Metric | Stored | Optimized | Decrease |
|---|---:|---:|---:|
| Processing wall (s) | 300.919385 | 247.672711 | 17.695% |
| Registration wall (s) | 26.243953 | 18.753631 | 28.541% |
| Inspection wall (s) | 16.367170 | 14.981352 | 8.467% |
| Reconstruction wall (s) | 156.201794 | 128.447817 | 17.768% |
| Rejection/accumulation/maps wall (s) | 90.325330 | 75.145774 | 16.805% |
| Write wall (s) | 0.974215 | 0.952800 | 2.198% |
| Peak process RSS (MiB) | 3391.300781 | 3397.921875 | -0.195% |

Processing difference is 53.246674 s, of which 7.490322 s is in the registration envelope and 45.756352 s in other phases/overhead. The latter must not be presented as an isolated effect of this optimization. The complete command took 248.187214 s.

The full stored control has no detailed registration profile. The optimized full-run profile below is a new observation, not a before/after comparison of those subphases.

| Current native path | Frames | Wall (s) |
|---|---:|---:|
| `session_anchor` | 1 | 0.0004578 |
| `triangle_fallback` | 368 | 13.1752424 |
| `sequential_direct` | 771 | 2.5122917 |

| Current registration scope | Phase | Wall (s) |
|---|---|---:|
| native | `selection_and_session_setup` | 0.0037424 |
| native | `native_geometry_and_photometry` | 16.1511847 |
| native | `weight_normalization_and_footprints` | 0.0001196 |
| native | `common_intersection_canvas` | 0.0145458 |
| native | `coverage_optimized_canvas` | 0.4579582 |
| native | `finalize` | 0.0000089 |
| external | `reference_fits_read` | 0.0401503 |
| external | `reference_validation` | 0.0000194 |
| external | `reference_star_catalog` | 0.0756085 |
| external | `native_anchor_to_external_bridge` | 0.0953285 |
| external | `parallel_source_refinements` | 0.7299120 |
| external | `weight_normalization_and_footprints` | 0.0002430 |
| external | `common_intersection_canvas` | 0.0857751 |
| external | `coverage_optimized_canvas` | 1.0227281 |
| external | `finalize` | 0.0000122 |

## Interpretation

Keep the exact implementation with cache off. The 32-frame peak RSS increased from 668.996 to 707.566 MiB; full-set RSS increased from 3391.301 to 3397.922 MiB, with unchanged conservative integration allocation estimates. Evaluate the observed registration reduction separately from total-run variation: 45.756352 s of the full processing difference occurred outside registration, so the entire 17.695% total reduction cannot be assigned to this change. The earlier shared-RGB full run was 275.9134115 s and remains contextual evidence rather than the selected latest baseline. No extra run was made to seek a favorable number. Both full and small source/template inventories and accepted scientific products remain unchanged. No integration quality or crop policy was altered.

Machine-readable report: fallback performance (private historical artifact: `outputs/M5_CPU_fallback_performance.json`). Exact gates: 32-frame (private historical artifact: `outputs/M5_fallback_DS-B_equivalence.json`) and full-set (private historical artifact: `outputs/M5_fallback_DS-A_equivalence.json`). The new full outputs are master (private historical artifact: `outputs/M5_fallback_DS-A_after_external200.fits`), coverage (private historical artifact: `outputs/M5_fallback_DS-A_after_external200_coverage.fits`) and high rejection (private historical artifact: `outputs/M5_fallback_DS-A_after_external200_high_rejection.fits`).

## Reproduce

Run from the project root; output destinations must be new. These are optional owner commands, not additional runs executed during this task.

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_fallback_manual32.fits" --report "outputs/M5_fallback_manual32.json"
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:\Astrofoto\2026\10 October\SeeStar50P M33\M33 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_fallback_manual1140.fits" --report "outputs/M5_fallback_manual1140.json"
```

The report exposes registration and native per-source timing. FITS master/map bytes already match the accepted products; no new visual quality claim is needed to establish their identity.
