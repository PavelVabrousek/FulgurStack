# M4: Tiled CPU Integration and PixInsight Master Review

## Status

M3/G3 was owner-approved on 2026-10-22 and M4 was authorized. M4 is implemented on `m4-tiled-integration`. Local fmt/Clippy, 67 release Rust tests and four legacy import tests pass. Real DS-B output is ready for product inspection. Full DS-A throughput validation is running; final G4 visual and full-reference quality approval remain pending. No GPU/GUI work has begun.

## Owner Review Files

- `outputs/M4_DS-B_master.fits`: linear Float32 RGB, 1324 x 3465 px, 27 accepted of 32 originals, anchor exposure 10 s and accepted acquisition total 270 s.
- `outputs/M4_DS-B_report.json`: complete input selection, registration, photometry, final weights, tile/memory accounting, low/high pixel rejection, coverage and export limiting counts.
- `outputs/M4_DS-B_comparison.json`: read-only numerical comparison against the immutable supplied PI master, with all selected coordinates.
- `docs/M4_TEST_PLAN.md`: thresholds defined before numerical comparison and the explicitly disclosed stronger analytic flux limitation.

The accepted M1/M2/M3 behavior and source files are preserved. G4 is not automatically signed off by a successful command or numerical comparison.
