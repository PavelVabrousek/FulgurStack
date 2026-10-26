# M4 Quality Comparison Plan

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Defined before measuring the integrated M4 candidate against the supplied PixInsight masters.

## Required Synthetic Gates

- Keys a=-0.5 interpolation: constant/linear fields, identity/rotated sampling and invalid 4x4 sensor support.
- ROI reconstruction agrees with full-frame M1 reconstruction on required pixels, including actual sensor edges, arbitrary even ROI origins and internal tile boundaries.
- Noiseless isolated unsaturated Gaussian star integrated over source subpixel phases and different exposures/transparency retains normalized aperture flux within 1% of the accepted M1 VNG anchor reconstruction; no informational BIAS subtraction.
- A minority satellite trail/outlier population is removed, while clean star cores and original surviving values are retained. Empty coverage produces zero; fewer than five valid samples bypass rejection. Numerator and denominator use Neumaier compensation.
- Changing tile geometry does not change output pixels. Input SHA-256, zero intermediate images, cancellation cleanup, float FITS layout/range, anchor GPS and exposure/crop provenance remain correct.
- Low memory budgets fail before unsafe allocations; configured upper bound is 12288 MiB. Actual Windows peak working set is recorded.

## Real Comparison Conventions

PI references remain immutable. The M4 candidate uses 27/32 M2-accepted DS-B frames; the supplied PI master used 32 frames and ESD rejection. Full DS-A PI accepted input selection is not independently known. Report these differences, not an equivalent-workload speedup.

Compare only the shared fully covered interior, mapping the PI template coordinate system to the automatic M3 anchor. Verify orientation from matched star positions rather than assuming FITS row order. Use unsaturated, isolated matched stars, subtract local sky for stellar flux/moments, and allow one shared scalar to compare master intensity units. Never alter the delivered scientific FITS for a comparison.

Choose spatially distributed isolated stars and sky regions deterministically from the reference/catalog, excluding the galaxy center, edges, bright halos and detectable sources. Record exact selected coordinates and counts in the comparison report. Compare aperture flux (radius 8 px; sky annulus 10..14 px), second-moment equivalent FWHM (radius 6 px), robust sky RMS, and median R/G and B/G stellar flux ratios. Negative sky-subtracted values remain in flux sums; positive flux is used for moments only.

Review thresholds: median flux residual after the single scalar <=1%; median FWHM no more than 5% larger than PI; median stellar color ratios within 5%; median sky RMS no more than 10% larger than PI on the common intensity scale. These are diagnostic thresholds; unequal input counts, weighting and finite-aperture profile differences must be disclosed. A failed threshold is not silently relaxed. Exact-input quality parity and final owner visual approval are separate acceptance conditions.

The owner reviews the ordinary-open 32-bit linear RGB FITS in PixInsight, with display STF as desired: star cores, color artifacts, registration sharpness, galaxy detail/background and changing edge coverage. M4/G4 cannot be signed off automatically from synthetic or numerical checks alone.

## Explicit Flux Diagnostic Finding

The initial test also compared the complete raw-CFA-to-master color flux against an ideal analytic RGB Gaussian. It failed that stronger absolute comparison (approximately +3.89% green, +10.55% blue in the chosen narrow colored synthetic star). Independent M1 VNG reconstruction already has that analytic-profile bias. New M4 resampling/normalization/rejection changes flux relative to the accepted M1 reconstruction by <=0.657% in this case. Keep both results: the integration-relative gate passes, but an absolute analytic CFA-to-RGB flux guarantee is not established and must not be inferred. The real PI master comparison is a separate gate with unchanged thresholds above. No test tolerance was relaxed or failed absolute result relabeled as an absolute pass.
