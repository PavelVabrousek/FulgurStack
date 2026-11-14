# M3: DS-B Registration Review

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](../CURRENT_STATUS.md) and the [documentation index](../README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

All 32 original CFA frames registered successfully. M2 selection is preserved: 27 accepted, five excluded from integration. Rejected frames below were registered for diagnostic review only. No source or PixInsight reference was changed.

Automatic master anchor: `Light_M 33_10.0s_IRCUT_20261007-001131.fit`. This quality-selected reference differs from the manually selected PI registration template. All values below refer to this automatic anchor; transforms from a different reference cannot be compared directly.

Translations are in original full-resolution pixels and rotations are in degrees. Positive rotation follows the top-down sensor coordinate equation `q = R(theta) p + t`; this is not a sky position-angle convention. A translation refers to the sensor origin and includes rotation about that origin. RMSE is the RMS distance between matched star centers, not known absolute ground-truth registration error. The anchor identity row has 250 self-matches.

| Source timestamp suffix | Rotation deg | tx px | ty px | Inliers | RMSE px | Transparency | Final weight | Integration |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 20261006-204534 | 15.33769 | 592.071 | -165.466 | 185 | 0.5391 | 0.82482 | 0.42118 | ACCEPT |
| 20261006-205221 | 15.49205 | 602.404 | -148.018 | 185 | 0.5299 | 0.82152 | 0.41612 | ACCEPT |
| 20261006-210159 | 15.69350 | 593.527 | -203.613 | 182 | 0.4743 | 0.93168 | 0.00000 | EXCLUDE (M2) |
| 20261006-211122 | 15.78050 | 670.596 | -124.588 | 188 | 0.4458 | 0.87759 | 0.48501 | ACCEPT |
| 20261006-212112 | 15.90603 | 580.611 | -143.042 | 185 | 0.5167 | 0.90108 | 0.00000 | EXCLUDE (M2) |
| 20261006-213153 | 15.90452 | 625.977 | -176.019 | 183 | 0.3715 | 0.91705 | 0.57819 | ACCEPT |
| 20261006-214211 | 15.83952 | 631.630 | -220.059 | 180 | 0.3371 | 1.00585 | 0.61989 | ACCEPT |
| 20261006-215309 | 15.65282 | 669.334 | -238.948 | 180 | 0.3164 | 0.95522 | 0.58503 | ACCEPT |
| 20261006-220327 | 15.48723 | 514.685 | -148.078 | 185 | 0.2919 | 0.92579 | 0.59956 | ACCEPT |
| 20261006-221415 | 15.07427 | 594.732 | -211.062 | 177 | 0.2818 | 0.95311 | 0.67879 | ACCEPT |
| 20261006-222507 | 14.58316 | 568.096 | -159.442 | 183 | 0.2696 | 0.95963 | 0.69751 | ACCEPT |
| 20261006-223610 | 13.98373 | 482.249 | -102.860 | 193 | 0.3605 | 1.04949 | 0.00000 | EXCLUDE (M2) |
| 20261006-225204 | 12.81443 | 443.908 | -112.218 | 188 | 0.2130 | 1.02442 | 0.78970 | ACCEPT |
| 20261006-230212 | 11.88696 | 402.495 | -158.040 | 189 | 0.2151 | 0.98220 | 0.82972 | ACCEPT |
| 20261006-231414 | 10.52106 | 377.961 | -59.424 | 190 | 0.2123 | 0.94099 | 0.78513 | ACCEPT |
| 20261006-232807 | 8.67972 | 303.288 | -136.881 | 192 | 0.2098 | 1.00760 | 0.90585 | ACCEPT |
| 20261006-233727 | 7.11731 | 313.734 | -47.052 | 192 | 0.2001 | 0.98717 | 0.84943 | ACCEPT |
| 20261006-234830 | 5.12792 | 229.643 | -47.464 | 196 | 0.1927 | 1.00034 | 0.94630 | ACCEPT |
| 20261006-235944 | 2.78388 | 134.844 | -81.258 | 206 | 0.2086 | 1.00008 | 0.87739 | ACCEPT |
| 20261007-001131 | 0.00000 | 0.000 | 0.000 | 250 | 0.0000 | 1.00000 | 0.96161 | ACCEPT |
| 20261007-002106 | -2.60173 | -88.340 | 83.828 | 214 | 0.1961 | 1.05294 | 1.00000 | ACCEPT |
| 20261007-003159 | -5.91439 | -185.767 | 177.522 | 206 | 0.1930 | 0.99178 | 0.92100 | ACCEPT |
| 20261007-004225 | -9.46212 | -278.954 | 201.529 | 195 | 0.1907 | 0.99673 | 0.90067 | ACCEPT |
| 20261007-005149 | -12.86330 | -402.458 | 307.840 | 194 | 0.2150 | 0.98088 | 0.94604 | ACCEPT |
| 20261007-010137 | -16.80991 | -440.086 | 433.029 | 191 | 0.2223 | 1.02917 | 0.99000 | ACCEPT |
| 20261007-011221 | -21.16691 | -598.420 | 634.634 | 179 | 0.2744 | 0.83514 | 0.56097 | ACCEPT |
| 20261007-012253 | -25.91459 | -614.108 | 735.879 | 174 | 0.2565 | 0.74453 | 0.38713 | ACCEPT |
| 20261007-013244 | -30.27855 | -737.851 | 843.518 | 172 | 0.2752 | 0.89258 | 0.67438 | ACCEPT |
| 20261007-014249 | -34.78786 | -810.861 | 989.011 | 172 | 0.3224 | 0.97092 | 0.00000 | EXCLUDE (M2) |
| 20261007-015458 | -40.06836 | -936.557 | 1146.642 | 165 | 0.2959 | 0.99732 | 0.88609 | ACCEPT |
| 20261007-020421 | -43.96937 | -920.577 | 1393.389 | 159 | 0.3267 | 0.92593 | 0.00000 | EXCLUDE (M2) |
| 20261007-021544 | -48.44038 | -1040.417 | 1501.342 | 157 | 0.3058 | 0.75585 | 0.40388 | ACCEPT |

Full names use `Light_M 33_10.0s_IRCUT_<suffix>.fit`. The 18-column UTF-8 CSV includes full filenames, photometric scale, sky offset, normalized noise and rejection reasons: `outputs/M3_DS-B_report.csv`. Complete source catalogs, local pair indices, residuals and direct master photometric pairs are in `outputs/M3_DS-B_report.json`.

## Canvas Review

- Common intersection rectangle: origin (479, 1250), 1451 x 1579 pixels; all accepted footprints cover every selected pixel center.
- Coverage-optimized rectangle: origin (469, 226), 1324 x 3465 pixels; at least 80% of accepted final weight covers every selected pixel center.

The optimized rectangle is not a guarantee of identical frame count, exposure or rejection at every pixel. M4 will apply exact inverse-resampling validity and per-pixel rejection. Rectangles are planned in anchor coordinates, with strict 4x4 bicubic sensor support.

## Manual Correspondence Examples

These separated matched stars allow coordinate checks in the unregistered source images in PixInsight. Coordinates are zero-based original sensor coordinates, with original top-down row order. Use the corresponding source and automatic anchor; the legacy PI integration reference has another coordinate origin. The transformed position should be near the anchor center listed. The source RGB reconstruction is not needed for these measurements.

| Source suffix | Source x | Source y | Predicted anchor x | Predicted anchor y | Measured anchor x | Measured anchor y | Residual px |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 20261006-204534 | 41.230 | 1704.790 | 180.903 | 1489.511 | 181.019 | 1489.513 | 0.1155 |
| 20261006-204534 | 1131.139 | 296.541 | 1604.486 | 419.709 | 1604.573 | 419.250 | 0.4673 |
| 20261006-204534 | 2116.966 | 3315.728 | 1756.603 | 3592.122 | 1756.420 | 3592.718 | 0.6232 |
| 20261006-231414 | 35.618 | 1219.082 | 190.379 | 1145.666 | 190.408 | 1145.516 | 0.1532 |
| 20261006-231414 | 1152.799 | 3145.320 | 937.053 | 3243.514 | 937.140 | 3243.700 | 0.2053 |
| 20261006-231414 | 2138.938 | 2180.344 | 2082.814 | 2474.827 | 2082.607 | 2474.723 | 0.2318 |
| 20261007-021544 | 26.754 | 2161.786 | 594.922 | 2915.449 | 594.986 | 2915.394 | 0.0844 |
| 20261007-021544 | 992.599 | 1096.425 | 438.490 | 1485.981 | 438.217 | 1485.885 | 0.2890 |
| 20261007-021544 | 2145.541 | 670.142 | 884.377 | 340.479 | 884.452 | 341.126 | 0.6518 |

M3 does not produce registered FITS images. Owner review covers the measured registration plan and proposed canvas. The integrated image and visual stack-quality gate belong to M4, after M3 approval.
