# M5: Automatic multi-night reference (2026-10-26)

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Owner-authorized local implementation, ADR-067. M5 remains open. No commit, push
or remote CI. The earlier registration/performance/geometry work remains local
in the same checkout.

## Product behavior

Registration reference now offers Auto (default for new preferences), Selected
accepted frame, External FITS and explicit Chronological median. Existing manual
and external preference IDs/path choices are retained. One Use-on observing group
uses the exact historical chronological median without an added catalog pass.
Groups are defined from accepted DATE-OBS gaps >6h, not calendar midnight. Missing
Use-on times require a manual reference. Use-off-only groups do not influence Auto.
Explicit selected/external references take precedence over the Auto strategy.

For multiple Use-on groups, perform a catalog-only provisional registration to
the historical median. Build a bounded geometric count grid from accepted source
footprints; every selected input stays in the denominator, and unmapped inputs
contribute zero. Each mapped source is evaluated as a real reference candidate.
Choose by largest predicted rectangle with >=80% selected-frame count coverage,
then mean count coverage, representative center/orientation, lower HFR, more stars
and deterministic filename. A long night has proportionally more votes; exposure
duration and integration weights do not replace frame counts. If a selected night
cannot be mapped at all, fail clearly instead of quietly omitting it.

The common count grid is at most 1024x1024 (4 MiB); each candidate grid has a
maximum side of 128, retaining the sensor aspect ratio. Pixel-cell centers use
the sensor's true (size-1)/2 center, including equivalent 180-degree framings.
Representative distance ignores numerical differences below 1e-9 normalized
units before quality ties. Candidate nonlinear forward maps are used where
present; conservative source footprints intentionally underestimate unsupported
nonlinear edges. This is an approximate cell-center framing estimate, not an
exact pixel optimum or a post-rejection prediction. It can include a cell whose
center is supported while its edge is not. Results/report label the estimate.

Reuse the provisional native registration if its reference wins; otherwise run
the existing direct-anchor registrar against the selected real source. Every
selected observing group must still have a registered source after the final
pass. Existing per-source scientific eligibility/geometry/photometry gates remain.
The actual chosen source supplies native normalization/provenance through the
existing algorithm; no fake WCS or reference pixels are introduced.

Selection does not crop the output automatically or change its dimensions. The
existing requested reference/common/coverage canvas policy remains. The Auto
count denominator is all Use-on frames; final Coverage/high-rejection maps retain
all original inputs and their existing pixel-rejection definitions. Keep the two
quantities distinct in product review.

## Interface and timing

Stack setup > Registration exposes the mode; the existing saved selection/path
continues to work. Results shows chosen reference, number of groups and estimated
>=80% Use-on rectangle dimensions/mean geometric coverage. JSON adds
`registration.auto_reference`, including both candidate and chronological-median
predictions, counts, grid limits and timing/approximation scopes. It is omitted
on the one-night and explicit-reference paths, preserving scientific report bits.

CLI register/stack expose `--reference-strategy auto|chronological-median`; Auto is
default, while `--reference` still wins. The selected mode is frozen with the job
and can be updated by reference selection at the Quality review Continue step.
One Processing progress bar accounts both catalog passes/scoring/reuse skips.
STOP checks remain active in selection, scoring and reuse. Existing phase elapsed
time includes the entire operation. Detailed native profile records the final or
reused native pass; preliminary/scoring timings are separately reported with
their nesting scope and must not be blindly added to overlapping parent timers.
No extra image reconstruction, resampling, image cache or disk intermediate.
1366x768 at 125%, fixed STOP, icon and Windows identity are retained; native visual
acceptance is still manual.

## Verification scope

Synthetic three-night known-pose catalogs contain 5+1+5 frames, with the middle
night rotated 90 degrees. The chronological median is the minority framing;
Auto chooses the majority framing and the sharper source within that framing.
All eleven register. Additional gates cover one-night midnight crossover and
exact historical science, explicit reference precedence, angle wrapping at
179/-1 degrees, translations, missing observing groups, Use-off/diagnostic
isolation, deterministic pool sizes, progress completion with reuse/refit and
optional geometric correction, plus the unchanged frozen ordinary planner.

Real FITS verification uses the existing 32-source subset and its stored control
products. An additional end-to-end fixture copies those immutable real pixel
payloads, shifts DATE-OBS into three artificial observing groups, and marks the
copies SYNTHET with HISTORY. These are explicitly artificial dates, not evidence
from three real observing nights. Original and fixture payload hashes are checked.
No existing source is changed. Real multi-night observations and macOS have not
been verified; no full reference integration benchmark is rerun.

For a new-destination native reference run on the owner's actual multi-night
folder:

```powershell
cargo run --release -p fulgur-cli --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "C:\path\to\multi-night-lights" --reference-strategy auto --canvas reference --fits-cache-mib 0 --output "outputs/auto_reference_review.fits" --report "outputs/auto_reference_review.json"
```

Inspect the chosen reference, geometric prediction and accepted counts in JSON,
then the new master and Coverage in PixInsight. Geometrically different output
grids intentionally need not match chronological-reference masters pixel-for-pixel.
For the old behavior, change only the strategy to `chronological-median` and use
new output/report filenames.

## Local result and delivery

Format/default-all-feature Clippy with warnings denied PASS. Release suites
160/160 default and 163/163 all-feature PASS, with actual FITS, required AVX2 and
GPU gates. The GUI regression checks restored strategy -> scientific request:
a previously saved manual path cannot override explicit chronological or Auto
mode, while manual/external selections retain their paths. The frozen serial
oracle only adds an omitted-None report metadata initializer; its solver is
unchanged. Legacy comparison tests explicitly request chronological policy;
separate Auto tests exercise the new default.

The known-pose 5+1+5 synthetic control chooses the majority orientation instead
of the middle night's 90-degree orientation, retaining all eleven sources.
Predicted >=80%-count rectangle grows 4,665,600 -> 8,294,400 px; mean geometric
coverage 60.23% -> 96.02%. This is synthetic evidence, not a real field benchmark.

The 32-real-pixel/artificial-date fixture retains all 27 selected sources in
three acquisition groups. Independent integer-pixel scan conversion of final
registered footprints confirms the >=80%-count rectangle grows from
1,251x2,600 = 3,252,600 px under chronological to
1,191x3,519 = 4,191,129 px under Auto, +28.854732% area. Its count mean decreases
88.702017% -> 87.799571%, -0.902446 percentage point: usable rectangle area has
priority over mean coverage, so this adverse secondary metric is intentional
and retained. Coarse predicted areas are 3,328,200 -> 4,280,400 px; they are
approximations, not exact integer-pixel rectangles. This independent check is
geometric and occurs before pixel rejection, unlike the final Coverage map.

Sequential release smoke observations: chronological 4.8123964 s, Auto 5.6044817 s
(see raw/sanitized reports for full precision); Auto preliminary catalog pass
0.5265525 s and scoring 0.0070617 s. Auto peak process RSS approximately 651.3 MiB,
chronological approximately 662.5 MiB. Do not extrapolate this small-set overhead
or infer isolated causality; OS/cache/thermal conditions are uncontrolled. Both
masters/maps are finite 2160x3840 Float32 RGB in 0..1. All 32 original hashes,
sizes and mtimes, and all 32 marked fixture hashes, remain unchanged; original
and fixture pixel payload identity was verified at creation.

The new 32-frame external distorted-reference control preserves every byte of
the prior repaired master and both maps, all scientific report fields and 27/32
selection. Only the dynamic available-RAM snapshot/headroom and normal timings
differ. Exact I/O/work/allocation and cache-off settings remain. The single-night
and manual policies retain their existing direct scientific paths. No whole
1140-source integration/reference benchmark was rerun.

GPU-enabled GUI/CLI release build PASS. The desktop shortcut targets the updated
standard `target/release/fulgurstack-gui.exe`; its prior working directory and
approved icon/Windows identity are preserved. Native visual/DPI/PixInsight review
and real multi-night observations remain pending; macOS was not tested. No commit,
push or remote CI. Raw local products/reports remain under
`outputs/auto_reference_20261026/`; portable evidence is in
`M5_AUTO_REFERENCE_EVIDENCE.json`.
