# M5 CPU Optimization Batch

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

The owner authorized one consolidated commit and push on 2026-10-22 after local development and verification. This batch contains the completed CPU optimizations since Git base `b836b93`, their independent differential tests, registration profiling, exact-output verification tooling and measured reviews. M5 remains open; GPU acceleration remains an explicit user option and FITS caching stays off.

## Result and measurement scope

On the local Windows reference computer, the full 1,140-frame set with `RegisteringTemplate200pct.fit` completed in **247.6727114 s**. Registration took **18.7536311 s** and peak process RSS was **3397.921875 MiB**. The same **940 frames** contribute **9400 s** accepted exposure; **200 frames** remain rejected. Master and both diagnostic maps retain the full **4000x4000** reference grid at origin (0,0).

The earlier accepted M4 run took **632.991 s**: the latest observation is 39.13% of that time (60.87% shorter, about 2.56x throughput). This is a historical endpoint comparison across multiple changes and uncontrolled machine conditions, not a controlled attribution of every saved second. The immediate stored control is **300.919385 s**, with **26.2439527 s** registration. Against that control, registration is 28.541% shorter, while **45.756352 s** of the total 53.246674 s difference occurred outside registration and must not be assigned to the last optimization.

The final 32-frame check measured **10.4604201 s**, with **1.4076777 s** registration, 27 accepted/five rejected frames and exact master/map parity. Peak RSS rose from 668.996 to 707.566 MiB for the small set and from 3391.301 to 3397.922 MiB for the full set; conservative integration allocation estimates remain unchanged.

## Included changes

- Runtime-detected AVX2 inverse resampling, preserving the scalar fallback, coefficient bits, validity masks and original accumulation order.
- Exact median/MAD fixed-point certification to skip redundant scratch winsorization iterations; original clipping remains the fallback.
- AVX2 CFA cosmetic correction with exact scalar confirmation and unchanged bright-star/defect safeguards.
- Shared RGB rejection preparation with independent channel-mask fallback, bounded scratch and unchanged compensated sums/maps.
- Bounded parallel external-reference refinements with original source-order publication and deterministic fits.
- Non-overlapping registration wall profiling, including detailed native per-source fits and validation of timing partitions.
- Lazy reuse of immutable native reference-star triangle/index preparation and bounded consensus inlier buffers.

No precision reduction, changed rejection threshold, altered scientific normalization, new dependency, image cache, automatic crop or intermediate image is introduced. Existing bounded scheduling and GPU selection are preserved.

## Local verification

- Format and default/all-feature Clippy with warnings denied: **PASS**.
- Release tests: **118/118 default**, **120/120 all-feature**, with actual local GPU, AVX2 and real-FITS gates explicitly required: **PASS**.
- Frozen prior implementations and independent scalar/serial oracles verify exact pixels, transforms, residuals, selection, source order, cancellation and bounded ownership.
- Entire master, coverage and high-rejection FITS match accepted products byte for byte on both 32 and 1,140 inputs. Source/reference hashes, photometry, exposure, all scientific counters, work/I/O and memory estimates remain exact. Only validated timing/RSS and available-RAM snapshot differences are excluded from report comparisons.
- The measured release binary and source fingerprints were checked before publication. Publication does not modify tested Rust/Cargo source. No new reference stack or full-set benchmark is executed for the commit.

Tests ran exclusively on the local Windows computer. GitHub Actions remain disabled; no current macOS or remote-CI pass is claimed. The workflow file and triggers are unchanged. Real datasets, FITS/XISF images, observing coordinates, binaries, generated local outputs and bytecode are excluded from this commit.

## Experiment ledger

Each row uses the control documented by its linked review. These are separate experiments rather than a controlled cumulative time series; unfavorable measurements and unchanged-phase variation are retained.

| Change | Full processing before/after (s) | Evidence |
|---|---:|---|
| SIMD CPU resampling | 387.618 / 350.600 | [Review](M5_CPU_SIMD_REVIEW.md) |
| Exact rejection fixed point | 366.351 / 322.559 | [Review](M5_CPU_REJECTION_REVIEW.md) |
| SIMD CPU cosmetics | 322.559 / 282.965 | [Review](M5_CPU_COSMETIC_SIMD_REVIEW.md) |
| Shared RGB preparation | 282.965 / 275.913 | [Review](M5_SHARED_RGB_REJECTION_REVIEW.md) |
| Parallel external registration | 275.913 / 300.919; acceleration not established | [Review](M5_EXTERNAL_REGISTRATION_REVIEW.md) |
| Registration phase diagnosis | 32-frame diagnostics only | [Review](M5_REGISTRATION_PROFILE_REVIEW.md) |
| Native fallback preparation and scratch reuse | 300.919 / 247.673 | [Review](M5_REGISTRATION_FALLBACK_REVIEW.md) |

A portable, coordinate-free summary of final timings and accepted output hashes is included in [machine-readable evidence](M5_CPU_OPTIMIZATION_EVIDENCE.json). Detailed reviews describe the conditions at measurement time; statements about uncommitted/local-only work in those historical records precede this publication authorization. Raw reports and image links point to deliberately excluded local artifacts.

## Reproduce locally

From the project root with the private 32-frame dataset and reference available, choose new destinations:

```powershell
cargo run --release --features gpu --bin fulgurstack -- --threads 16 stack --engine cpu --input "SeestarS50 Subs" --reference "RegisteringTemplate200pct.fit" --canvas reference --tile-size 512 --memory-budget-mib 12288 --fits-cache-mib 0 --output "outputs/M5_cpu_batch_review.fits" --report "outputs/M5_cpu_batch_review.json"
```

The master and both maps open directly in PixInsight. The full-set reproduction command and original scientific limitations are retained in the linked fallback and milestone reviews.
