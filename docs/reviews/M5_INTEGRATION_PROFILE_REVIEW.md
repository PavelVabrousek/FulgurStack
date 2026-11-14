# M5: Integration CPU and I/O Profile (2026-10-26)

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Owner-authorized local profiling, performance optimization and Windows process identity, ADR-062. Published base b05d46c; the owner subsequently authorized one consolidated commit and push of this completed batch (ADR-063). Measurement-time statements below describe the local verification before publication. M5 remains open. All scientific calculations, original selection, source immutability, f32/f64 precision, output units, map denominators, reference grid and clipping thresholds are preserved. No remote CI or Windows power/priority/security changes.

## Workload and measurement design

Replay the latest stored native reference plan from outputs/master_v11_report.json: 1140 original sources, 940 accepted, same chronological reference anchor, transforms, scales, offsets, weights and source order. CPU engine, manual 512-square tiles, cache 0, sigma 3.0/2.5, 12288 MiB ceiling. The typed release-only example skips inspection and registration to isolate integration. It writes only final scientific master/coverage/high-rejection FITS and JSON. Private source hashes, raw logs and output products stay under outputs/M5_integration_profile_20261026.

Repeated parameter comparisons use four complete existing central tiles at reference origin (512,1024), width/height 1024. They retain all 940 sources and original denominator 1140. This cropped profiling region is not a full-stack benchmark. Each executable has one excluded warmup, then three rounds ordered 12/16/24, 24/12/16, 16/24/12. No build/test process runs concurrently with measurements. Other applications, OS caches and thermal scheduling are not controlled; do not claim universal performance or formal confidence intervals.

The preserved control executable has the original row-by-row reader plus additive process-CPU profiling and a 24-slot CPU limit to permit a real 24-worker comparison; at 16 workers the source/pixel operations remain unchanged. The candidate adds bounded original-byte read-ahead. SHA256 of the control profiler: 9eecde372c0f428f4f22cbefb26a4483de58a077c0171740e77d90aedd1571fa. Binary fingerprints, all observed values and product hashes are retained locally.

## Repeated central-region measurements

Medians of three measured runs per cell, seconds:

| Threads | Original row reader | Bounded read-ahead |
|---|---:|---:|
| 12 | 23.362894 | 22.531762 |
| 16 | 20.804144 | 20.096449 |
| 24 | 18.301232 | 17.716736 |

Original 16 to candidate 24 is 14.840% shorter in this region. Increasing concurrency alone (16 to 24, row reader) is 12.031% shorter. Read-ahead alone at 24 workers is 3.194% shorter; preserve the modest size/overlap of that difference. Candidate 24 observations are 18.229752, 17.649793 and 17.716736 s; original 24 observations 18.049954, 18.301232 and 19.052712 s. No parameter search beyond the requested thread counts and one bounded reader candidate.

True process user+kernel CPU time divided by wall time and 24 logical processors yields median busy-time utilization 48.23%, 63.98% and 86.75% in the original 12/16/24 runs; candidate 24 is 87.00%. This is a time-based process measurement, not the exact Task Manager display metric, retired instructions, useful work percentage or memory-bandwidth measurement. CPU efficiency is not linear with SMT concurrency. Process counters include every thread in the profiler (or GUI when used there); individual ROI task elapsed sums overlap and remain separately labeled.

Central I/O: 2316008 underlying read calls and 2884684154 requested/source bytes become 151206 read calls and 9909436416 actual source bytes. Logical requested ROI bytes, decoded pixels and all pixel work stay equal. Read-ahead intentionally reads additional original bytes across source-row gaps, increasing application byte volume 3.435x while reducing calls 93.471%; none of these numbers is physical SSD traffic. Median kernel CPU seconds fall 27.265625 to 16.656250 at 16 workers, and 36.437500 to 24.109375 at 24. Retain the increased byte volume and potential storage/cache sensitivity.

All 20 warmup/measured regional outputs have exact raw-float sample hashes and whole master/map FITS equality. This is direct before/after verification using the same frozen decimal plan. Against the historical master, the original row-reader replay itself differs in four regional output samples by at most 3.725291e-9 normalized units due to decimal f64 plan replay; do not misattribute that replay difference to the reader/concurrency change or claim historical bitwise parity.

## Profiling availability

Windows Performance Recorder was attempted with isolated CPU.light/FileIO.light/DiskIO.light recording, but Windows refused profiling privilege (0xc5585011). No trace exists, no session remains recording, and no ETW stack analysis is claimed. Available evidence is real GetProcessTimes user/kernel CPU time, disjoint phase wall/CPU counters and actual application I/O counters. WPR failure does not change the measured products or benchmark. No security/power settings were modified.

## Implementation and review scope

CPU Automatic now uses available logical parallelism capped at 24; remembered Performance choices are Automatic/12/16/24. Both source scheduling and reduction can use 24 CPU workers; GPU retains its 16-worker limit. Memory planning accounts active workers and an additional 64 KiB per source reader. Fixed STOP and Compact Desk sizing remain unchanged.

Read-ahead is a 64 KiB original-byte BufReader, released before reconstruction. The existing signed-big-endian decoder and per-row cancellation are unchanged. Relative seeks retain buffered bytes instead of invalidating them on each row. A frozen row-reader oracle verifies bits, row gaps, odd widths, buffer crossings, EOF and source preservation. Cache remains 0; no frame cache or image intermediate is introduced.

The Windows executable now embeds FileDescription/ProductName FulgurStack and version 0.1.0, with the original filename, approved icon and explicit application ID FulgurStack.Desktop. File menu includes version/PID. This does not retrospectively prove why the earlier Task Manager screenshot omitted the app. Native grouping/DPI appearance remains manual review; macOS has not been run.

## Reproduce a local comparison

Use an existing private report/source root and a new output directory:

```powershell
cargo run --release -p fulgur-pipeline --example profile_integration -- "outputs/master_v11_report.json" "C:/Astrofoto/2026/10 October/SeeStar50P M33/M33 Subs" "outputs/profile_new_24" 24 center
```

Use 12 or 16 for other counts; `full` replaces `center` only for a complete-reference verification. The latter is integration-only with frozen registration, not total application processing. Do not overwrite existing outputs. Raw outputs contain private local data and remain excluded from Git.

## Complete native-reference verification

One sequential requested local full-grid pair, using the same frozen plan and no builds/tests in parallel:

| Measurement | Original row reader, 16 threads | Read-ahead, 24 threads |
|---|---:|---:|
| Integration wall seconds | 162.791162 | 135.044169 |
| Reconstruction wall seconds | 128.490341 | 100.583839 |
| Rejection/maps wall seconds | 29.370039 | 28.593155 |
| Process user CPU seconds | 2062.343750 | 2455.500000 |
| Process kernel CPU seconds | 224.906250 | 244.171875 |
| Peak process working set, MiB | 3267.789063 | 3339.003906 |
| Underlying read calls | 19937074 | 1294345 |
| Application source bytes read | 20150891642 | 84728407240 |

Integration is 27.746993 s / 17.044533% shorter. The full pair changes worker count and reader together; it does not isolate their individual contribution, prove causality under all OS conditions or establish total GUI processing time. Process CPU seconds increase with 24-way SMT even while wall time decreases; retain this resource tradeoff. Actual working set remains well below 12288 MiB and the conservative per-worker plan. Keep increased byte traffic as well as fewer calls.

All three complete FITS files and all unexported raw float sample hashes match exactly, including independent RGB maps. Scientific decoded/corrected/rejected/coverage counters, selection, normalization, dimensions, origin, exposure, export limits and denominator remain equal. Every one of the 1140 source files has the same SHA256, length and modification time before and after the pair. No source/reference/intermediate image is written. The two full outputs use the original 2160 x 3840 reference grid at (0,0), without crop. The only extra images are the owner-authorized final diagnostic maps.

Whole-file SHA256, identical between the full pair:

- Master: a288ee5c5a3871058305dedef81c24b4bb798311a6e5d57d8cacc02d5eb61548
- Coverage: d65c87e4f166da072b90477a029010e448c26b83e25bd0c55ce6a195bf4cd08e
- High rejection: dff8eb3b84be6a3ad84edcf8f50d5c362a0e25744b9fc1351c622903722b444b

Candidate profiler SHA256: d188a089f162fb79385e6bc8a0c5637b6efeeb46e8f6fab1f815e2ddfd0a6d32. Private full equivalence/source verification is in outputs/M5_integration_profile_20261026/full_equivalence.json and input_integrity_result.json. Portable measured summaries are in M5_INTEGRATION_PROFILE_EVIDENCE.json. The verifier permits only CPU/I/O telemetry, accounted concurrency/memory and OS RAM snapshots; it keeps whole-FITS, raw-float and scientific counters strict.

Final local gates for ADR-062: formatting, default/all-feature workspace Clippy and release suites PASS (134/134 default, 136/136 all-feature), with real FITS, AVX2 and actual GPU hardware gates enabled. The new gates cover CPU counter monotonicity, buffered-vs-frozen-row reader bits/source integrity and 12/16/24-worker full sample/map/source equality. Complete frozen-reference master/map/raw-sample/science equality and all 1140 source fingerprints PASS. No macOS or native Task Manager grouping acceptance is claimed. Local release preparation only; no commits, pushes or remote CI.

Local release preparation PASS: cargo build --release -p fulgur-gui -p fulgur-cli --features gpu --bins. GUI SHA256 5e6fcfdc5cb9aa85a97f90b84873e15d38ae64771eda9fed3f04a2e51d4b41d6; root/deps executable hashes agree and the desktop shortcut targets the updated standard release. Windows executable metadata query confirms FileDescription/ProductName FulgurStack, version 0.1.0 and original filename fulgurstack-gui.exe. Actual 32-source CFA inspection at 16 and 24 threads preserves every measurement/catalog/selection field exactly (27 accepted / 5 rejected); only timings, concurrency and memory telemetry differ. No additional performance claim is drawn from this short inspection gate. Native Task Manager grouping/DPI appearance remains manual. No commit, push or remote CI.
