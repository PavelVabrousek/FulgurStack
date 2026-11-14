# M5: Available-RAM Tile Selection

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Date: 2026-10-22. M5 remains in progress. This delivery implements the owner's RAM-aware selection request and reuses the verified 128x128, 256x256 and 512x512 geometries. The supplied 265x256 was interpreted as the established 256x256. No geometry performance search or full DS-A benchmark was repeated.

## Behavior

Default/no tile flags or `--tile-size auto` query available and total physical RAM immediately before integration, after registration. Keep a system reserve of max(2 GiB, ceil(total RAM / 10)), and use the smaller of the configured ceiling and available-minus-reserve. Swap is excluded. The 12288 MiB ceiling and conservative pipeline-buffer model remain, including all three output RGB images, 768 MiB runtime/catalog allowance, accepted source count, bounded ROI workers, inverse scale and CPU/GPU storage. Already resident pipeline memory is not credited back to the OS's available measurement.

Select the first fitting square in preference order 512, 256, 128 pixels, clipped only to reference canvas edges. No fit means an actionable error before image allocation. Unknown OS memory fails auto mode; no invented available-RAM value. RAM snapshots are starting decisions, not OS reservations against later allocations by other applications; geometry remains fixed for this run.

Manual `--tile-size 128|256|512` fixes a square. Existing `--tile-width` and `--tile-height` fix a rectangle, with 512 for an omitted companion. Combining those control families is rejected. Manual requests obey measured headroom and the configured ceiling, and fail rather than silently changing geometry. If OS measurement is unavailable, manual mode explicitly reports configured-ceiling-only enforcement. Windows process peak-RSS ceiling enforcement remains active. GPU stays optional and uses a more conservative ROI-storage estimate.

Console and `integration.memory_plan` record the choice; JSON includes the OS snapshot, system reserve, configured/effective budgets, all three candidates and actual dimensions. Input FITS and scientific pixel math are unchanged. No GPS enters memory telemetry. Requested dimensions/mode stay in config and canvas-capped dimensions in integration statistics.

## Verification

- Formatting and default/all-feature Clippy with denied warnings: PASS.
- Optimized release suites: **86/86** default and **87/87** all features: PASS. GPU hardware gates were explicitly required locally, not skipped.
- New tests cover exact selection boundaries, insufficient RAM, 2-GiB/10% reserve, configured-ceiling precedence, unavailable/invalid telemetry, manual refusal/fallback, GPU/scale/canvas estimates and overflow. CLI checks cover default auto, manual square, incompatible/invalid controls, source integrity and exact synthetic master/map bytes. Existing scientific gates still pass.
- One release CPU functional run on all 32 DS-B inputs against `RegisteringTemplate200pct.fit`: PASS. **27 accepted / 5 rejected**, unchanged **4000x4000** reference grid, origin (0,0), no crop.
- Entire master, coverage and high-rejection FITS bytes match the prior verified CPU products exactly. Registration, selection, photometry, exposure, rejection/export counts and ROI work are unchanged. All 32 sources, two PI references and the external template retain their SHA-256 hashes; no image intermediates or leftover temporary files.
- The exact comparison explicitly permits tile-policy/configuration and RAM telemetry differences only. A CPU-only build omits the optional gpu field, while a GPU-enabled CPU build writes null; the checker normalizes this empty field only for CPU backends. Actual GPU identity remains strict. Ordinary PixInsight import was not repeated for byte-identical files.

## Observed functional-run telemetry

| Item | Value |
|---|---:|
| Physical RAM | 31865.855 MiB |
| Available before integration | 8290.109 MiB |
| Additional system reserve | 3186.586 MiB |
| Configured ceiling | 12288.000 MiB |
| Effective allocation budget | 5103.524 MiB |
| Selected output tile | 512 x 512 px |
| Conservative planned buffers | 1821.929 MiB |
| Measured process peak | 674.359 MiB |
| Processing wall time, including FITS writes | 19.067860 s |

This is one functional smoke run with existing caches, not a new comparative benchmark or speedup claim. It excludes Cargo compilation time. Earlier full-set performance/photometry qualifications remain in force.

## Local evidence and owner commands

- `outputs/M5_auto_RAM_selection_review.json`: compact policy, tests, exact hashes and telemetry.
- `outputs/M5_auto_RAM_DS-B_equivalence.json`: exact products/scientific-report/source-integrity proof.
- `outputs/M5_auto_RAM_DS-B_external200.json`: full scientific and memory-planning report.
- `outputs/M5_auto_RAM_DS-B_external200.fits`, `_coverage.fits`, `_high_rejection.fits`: final unchanged-quality images. These generated files remain local and ignored.

Run from the project root with new filenames:

```powershell
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --tile-size auto --output "outputs/M33_auto_tiles.fits" --report "outputs/M33_auto_tiles.json"
cargo run --release --bin fulgurstack -- stack --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --tile-size 256 --output "outputs/M33_manual_tiles.fits" --report "outputs/M33_manual_tiles.json"
```

Inspect `integration.memory_plan` for RAM and selected dimensions. Open the master/maps normally in PixInsight if desired. Manual control changes only the work partition; no new visual acceptance is needed for the byte-identical smoke products.
