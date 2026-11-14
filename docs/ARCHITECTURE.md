# Architecture overview

Current as of 2026-11-13 (version 0.14 plus local work). This page explains how the code is organized and how data flows through a stack. It is meant for a developer who is new to the project. Why each choice was made is recorded in the [decision log](ARCHITECTURE_DECISIONS.md). The binding scientific rules are in [PROJECT_SPEC.md](../PROJECT_SPEC.md) and the [engineering contract](../AGENTS.md). Build and test commands are in [DEVELOPMENT.md](DEVELOPMENT.md) and [TESTING.md](TESTING.md).

## Guardrails that shape the code

- **Sources are immutable.** Input FITS/XISF files are only read. Tests check this by hashing the inputs before and after a run.
- **No intermediate images on disk.** Calibration, demosaicing and resampling happen in memory, per output tile. The only files written are the final master, its optional maps, master calibration files on request, and reports. They are written through `fulgur_core::output::atomic_write`: a fresh `<name>.tmp`, synced, then moved into place without overwriting.
- **Bounded memory.** Integration is output-tile-major. Each tile reads only the source rectangles it needs, so peak RAM depends on the tile size, not on the frame count. `tile_plan` checks every plan against available RAM, a system reserve and the 12 GiB ceiling.
- **Precision.** Pixels are `f32` and weighted sums are `f64` with Neumaier compensation. `f16` is never used.
- **Pure Rust.** There are no C/C++ runtime libraries. GPU support (`wgpu`) is an optional feature, and the CPU is always the default engine.
- **Privacy.** GPS cards are kept in the master with their provenance and redacted from logs and reports.

## Crates and programs

```text
fulgur-solve ─────────────────────────────┐   (no FulgurStack dependencies)
fulgur-core ── fulgur-gpu (optional) ──┐  │
      └──────── fulgur-pipeline ───────┼──┼── fulgur-catalog
                                       │  │        │
                                       ▼  ▼        ▼
               fulgurstack (fulgur-cli)   fulgurstack-gui (fulgur-gui)
```

| Crate | Program | What it holds |
| --- | --- | --- |
| `fulgur-core` | — | FITS and XISF reading (`fits`, `xisf`) and writing (`output`, `xisf_write`); header reconstruction (`derive`) with the camera catalogue `data/cameras.json`; the opt-in original-byte cache (`fits_cache`); process telemetry (`telemetry`). |
| `fulgur-gpu` | — | `wgpu` kernels for reconstruction (`reconstruct.wgsl`) and pixel rejection (`reduce.wgsl`). Used only with the `gpu` feature and an explicit `--engine gpu`. |
| `fulgur-pipeline` | — | The scientific engine: inspection, calibration, registration, plate solving, integration, and the shared post-registration path `stack` (see below). |
| `fulgur-catalog` | — | Gaia DR3 from PixInsight's XPSD files (`xpsd`) or downloaded from ESA/GAVO (`download`); the built-in deep-sky table (`dso`, `data/deep_sky.csv`); target look-up online (`lookup`) and from file names (`guess`); glue that prepares plate-solving options (`sky`). |
| `fulgur-solve` | — | Blind plate solving: a quad index of bright Gaia stars (`index`, `quad`, `sphere`) and a search that places a frame with no position given (`solve`). It has no FulgurStack dependencies, so the pipeline could use it later without a cycle. |
| `fulgur-cli` | `fulgurstack` | Command line, one file per command in `src/cmd/`: `inspect`, `register`, `stack`, `master-dark`, `gaia-check`, blind plate solving with `solve` and `solve-index`, and more. It is the default workspace member. |
| `fulgur-gui` | `fulgurstack-gui` | The egui Windows application. `app` and `app/*` hold the screens, `job` runs stacks in the background, `sets` holds light and calibration sets, `settings` holds preferences, and `preview`/`image_view` handle images. |

The experiment programs behind ADR-131, ADR-127 and ADR-168 (`skyreg`, `synthref`, `blindspike`) are archived in `tools/archive/` (ADR-180). Their code was productized into `fulgur-pipeline::sky`, `fulgur-catalog` and `fulgur-solve`; they are not built.

## Data flow of one stack

```text
input folders ─▶ survey ─▶ inspect ─▶ register ─────────────▶ integrate ─▶ master + maps + report
 (FITS/XISF)    (files)   (quality,   (stars or sky; per    (per output tile:
                           stars,      frame transform,      read ROI ▶ calibrate ▶ cosmetics ▶
                           sky level)  photometry, weight)   demosaic (VNG) ▶ warp (Keys) ▶
                                                             reject ▶ weighted mean)
```

1. **Survey** (`survey`): collects the input files. Light folders contribute only their own files; calibration folders include sub-folders.
2. **Inspection** (`inspect`, `stars`): reads every frame once. It measures stars (HFR, FWHM, eccentricity), sky level and noise, groups frames by camera, gain and filter (`compatibility_groups`) and gives provisional weights. With calibration, `calibration` builds the masters in RAM first and applies them to each frame.
3. **Registration** (`register` and `register/*`, `reference`, `geometry`, `distortion`): computes a transform from each accepted frame to the reference grid, plus its photometric scale and weight.
   - The **star engine** matches star lists by triangles to a session anchor. It walks outward in time, validates the whole field against held-out witness stars, and optionally fits a geometric-distortion model (`distortion`).
   - The reference is chosen by `register/auto_reference` (Auto), or given as chronological median, manual or external.
   - Several cameras are joined by `register/cameras` and `register/set_photometry`.
   - The **sky engine** (`register/sky_engine`, `sky/*`) plate-solves every frame against Gaia (`sky::solve`, `sky::batch`) and maps source pixel → sky → reference pixel.
   - Both engines produce one `RegistrationReport`.
4. **Integration** (`integrate`, `tile_plan`, `cpu_schedule`, `resample`, `vng*`, `debayer`, `cosmetic*`, `rgb_rejection`, `rejection_methods`, `esd_columns`): processes the canvas tile by tile.
   - For each tile, every source's needed rectangle is read, calibrated, cleaned of cosmetic defects, demosaiced (VNG) and resampled with Bicubic Keys and strict validity masks.
   - Pixels are then rejected (Auto, ESD, Winsorized, linear fit or median) and combined into an `f64` weighted mean.
   - A bounded pool (`cpu_schedule::map_bounded`) keeps at most 24 CPU workers busy. With the GPU engine, rejection and the mean run on the GPU while the CPU reconstructs the next tile.
   - `master_groups` lets one registration produce one master per filter or light set.
5. **Output** (`fulgur_core::output`, `xisf_write`, `astrometry`, `master_quality`): writes the master and its coverage and high-rejection maps as 32-bit float FITS or XISF.
   - The master gets a WCS only when its own stars match Gaia (`sky::fits_wcs`).
   - It is measured like a frame (`master_quality`).
   - A JSON report records every decision.

## Where the orchestration lives

`fulgur-cli` (command line, one file per command in `src/cmd/`) and `fulgur-gui/src/job.rs` (application) drive the steps above; both register through `register::register_sets` or `register::register_inspected` with a `RunContext` (ADR-179). Since ADR-178 everything after registration is one shared module, `fulgur_pipeline::stack`: the grid's plate solution, master planning, and integration plus publication of each master (metadata, plate solution check, master and maps, quality). The front ends keep their own concerns:
- the CLI: console output and its `StackReport`;
- the application: calibration-master caching, inspection reuse, progress for the Processing screen, previews and its report.

Inspection and registration are still called separately by each front end, because the application inspects first and lets the owner review before it registers.

## Decision families

| Topic | Main decisions | Code |
| --- | --- | --- |
| FITS/XISF I/O, export range, atomic writes | ADR-007, ADR-010, ADR-013, ADR-014, ADR-104–106, ADR-142 | `fulgur-core` |
| Reconstruction (cosmetics, VNG, resampling) and its speed | ADR-008, ADR-011, ADR-012, ADR-026–033, ADR-069, ADR-113 | `vng*`, `cosmetic*`, `resample`, `integrate` |
| Tiles, memory and scheduling | ADR-017, ADR-023–025, ADR-030, ADR-062, ADR-118 | `tile_plan`, `cpu_schedule`, `integrate` |
| Pixel rejection and accumulation | ADR-022, ADR-028, ADR-032, ADR-034, ADR-087, ADR-090, ADR-111, ADR-114, ADR-146 | `rgb_rejection`, `rejection_methods`, `esd_columns`, `fulgur-gpu` |
| Quality inspection | ADR-015, ADR-073, ADR-112 | `inspect`, `stars` |
| Star registration, references and accuracy | ADR-016, ADR-018, ADR-020, ADR-035–037, ADR-060, ADR-064–067, ADR-074, ADR-081, ADR-085, ADR-107–110, ADR-116–130, ADR-165 | `register*`, `reference`, `geometry`, `distortion` |
| Sky registration and plate solving | ADR-131–138, ADR-154–156, ADR-158, ADR-162 | `sky/*`, `register/sky_engine`, `fulgur-catalog` |
| Blind solving | ADR-168, ADR-169, ADR-181 | `fulgur-solve`, `fulgur-cli/src/cmd/solve.rs`, `fulgur-gui/src/app/blind.rs` |
| Calibration | ADR-076–082, ADR-163 | `calibration`, `calibration/hot_pixels` |
| Header reconstruction and light sets | ADR-071, ADR-078, ADR-084, ADR-093, ADR-101, ADR-139–140, ADR-161, ADR-166–167 | `fulgur-core::derive`, `inspect`, `master_groups`, `fulgur-gui/src/sets.rs` |
| Windows application | ADR-038–059, ADR-071–072, ADR-091–099, ADR-141, ADR-159, ADR-170–173 | `fulgur-gui` |
| Publication and packages | ADR-061, ADR-063, ADR-068, ADR-075, ADR-083, ADR-088, ADR-089, ADR-100, ADR-115, ADR-143, ADR-153, ADR-164, ADR-172 | — |

## Testing layout

- Unit and synthetic tests sit next to the code: inline `mod tests`, or sibling `*_tests.rs` files and `register/tests/`.
- Command-line tests are in `fulgur-cli/tests/commands.rs`.
- Frozen oracles (`vng_reference`, `cosmetic_reference`, `geometry_fallback_oracle`, `register/tests/serial_oracle.rs`) back the claims that masters stay byte-identical.
- Real-data, AVX2 and GPU tests run only when their `FULGURSTACK_TEST_*` variables are set. `scripts/test-full.ps1` sets them.
- See [TESTING.md](TESTING.md).
