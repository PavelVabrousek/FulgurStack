# Open items

This is the one register of everything still open: owner decisions, owner reviews, unverified requirements, known test gaps and planned clean-up. It was last updated on 2026-11-13. Each row links to the document that holds the details, and items are closed here when they are done. The project uses no issue tracker.

## Owner decisions

| ID | Decision | Details |
| --- | --- | --- |
| O-1 | Linear channel mix of finished masters (SHO, HOO, RGB): start or not | [CURRENT_STATUS](CURRENT_STATUS.md#planned-next) |
| O-2 | Should Auto use ESD below 25 frames? | ADR-102; TESTING U-4 |
| O-6 | Whether the MHC demosaic stays as a diagnostic (it is reachable only from tests) | code review I-08 |
| O-7 | Whether `outputs2/` can be deleted, and where `RegisteringTemplate200pct.fit` should live | code review C-09 |

## Owner reviews pending

| ID | What to check | Details |
| --- | --- | --- |
| R-0 | One application stack after ADR-178/179 (any set): same master as before, Processing phases and Results look right, Settings keep their values after a restart | ADR-178, ADR-179 |
| R-1 | Application fixes of WP-1: "None" calibration kept, files opened with the program, closing during an index build, previews | ADR-173 |
| R-2 | Screens of ADR-157, 159, 161–163, 169–171 on real folders | the ADRs |
| R-3 | Master WCS in PixInsight SPCC (test file `outputs/wcs_test/m45_s30_wcs.fit`) | ADR-155, ADR-158 |
| R-4 | Olyhen L master | ADR-156 |
| R-5 | Native 1366×768 / 125 % interaction review, Task Manager grouping | [CURRENT_STATUS](CURRENT_STATUS.md) |
| R-6 | PixInsight comparison of the rejection methods and the M42 master | [rejection review](reviews/M5_MASTER_QUALITY_AND_REJECTION_REVIEW.md), [header review](reviews/M5_HEADER_RECONSTRUCTION_REVIEW.md) |

## Unverified requirements (waiting for data or hardware)

| ID | Requirement | Details |
| --- | --- | --- |
| U-1 | macOS Apple Silicon compile and tests | TESTING §9 |
| U-2 | Real multi-night and real mixed-exposure data | TESTING §9, ADR-067 |
| U-3 | Full DS-A gates on the current build (stored evidence used per the benchmark policy) | TESTING §9, [BENCHMARKS](BENCHMARKS.md) |
| U-5 | CLI-level tests for `--registration-engine sky`, `gaia-check`, `--accuracy`, `--partial-frames`, `--geometric-correction` | TESTING §9, N-6 |

## Test gaps and proposals

| ID | Item | Details |
| --- | --- | --- |
| D-03 | Real-data and hardware tests pass silently when their variable is unset; `test-full.ps1` mitigates, N-2 would close it | TESTING §9–10 |
| T-1 | Two manual-tile tests fail when free RAM is low, because their outcome depends on what else runs on the PC: `scaled_inverse_warp_matches_full_reconstruction_and_is_tile_invariant` and `hardware_gpu_integration_preserves_flux_masks_and_rejection`. Run them serially or give them a fixed memory budget | ADR-174, code review |
| N-2 … N-11 | Proposed tests: compensated sum, Keys weights, memory independence, golden DS-B hash, interrupt test, report schema snapshot and more | TESTING §10 |
| P-1 … P-5 | Behaviour-neutral test retirements | TESTING §10 |
| G-09 | Tests for non-UI logic in the application (inspection selection, calibration assignment, cache keys) | code review |

## Planned clean-up (code review work packages)

| Package | Status |
| --- | --- |
| WP-1 GUI correctness | Done locally (ADR-173); owner check R-1 |
| WP-2 Robustness, no numeric change | Done locally (ADR-174) |
| WP-3 Documentation refresh | Done locally (ADR-175), including the AGENTS.md trim approved by the owner |
| WP-4 Mechanical engine clean-up | Done locally (ADR-176); shared test fixtures (C-08) moved to WP-7 |
| WP-5 Registration consistency | Done locally (ADR-177): Dwarf3 master differs below one noise unit, 16-bit masters unchanged; shared normalization R-02 not started |
| WP-6 One orchestration for application and CLI | Done locally (ADR-178, ADR-179). Remaining: the other `Workspace` groups after `StackSettings` |
| WP-7 Tools, build and CI hygiene | Done locally (ADR-180, ADR-181) |

Details of every finding are in the [code review](CODE_REVIEW_2026-11-13.md).

## Closed

| ID | Item | Closed |
| --- | --- | --- |
| D-01, D-02 | Truthful `REJECT` and `CREATOR` FITS cards | 2026-11-08 |
| O-3 | Local work ADR-173 to ADR-181 published as one commit (owner authorization) | 2026-11-14 (ADR-182) |
| O-8 | Blind plate solving as `fulgurstack solve` (owner decision) | 2026-11-14 (ADR-181) |
| O-5 | GitHub workflow set to manual start only (owner decision) | 2026-11-14 (ADR-180) |
| O-4 | AGENTS.md §6 trimmed to standing rules and §3.6 restated (owner approval) | 2026-11-13 (ADR-175) |
| U-6 | Documentation drift (spec defaults and XISF, old test counts) | 2026-11-13: owner amendments index in the spec; counts point to TESTING and ADR-174 |
