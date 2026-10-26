# M2: DS-B 32-Frame Manual Review

> **Documentation context — 2026-10-26:** This is a dated evidence/design snapshot, included in the owner-authorized consolidated `main` publication. Measurements, gate counts, artifact hashes, acceptance outcomes and measurement-time local-only statements below retain their original scope; they are not current release claims. See [current behavior, verification and delivery](CURRENT_STATUS.md) and the [documentation index](README.md). Later reference/geometry/UI amendments supersede older behavior; M5 remains open and remote CI stays disabled.

Status: automatic selection ready for owner review; no source file was altered or removed.

Release processing: 0.401 s; complete CLI including JSON/CSV: 0.447 s; peak working set 552.3 MiB. 27 accepted / 5 rejected, 320 s input / 270 s accepted exposure.

Source filename: `Light_M 33_10.0s_IRCUT_<Frame ID>.fit` in `SeestarS50 Subs/`. All exposures are 10 s. Stars = uncapped supported detections; measured catalog = 250 stars per frame. HFR and moment FWHM use full-resolution pixels. BG/noise are source ADU on the averaged-green plane. Weight is provisional and excludes matched-star transparency.

| Frame ID | Stars | HFR px | Moment FWHM px | Eccentricity | BG ADU | Noise ADU | Weight | Selection |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 20261006-204534 | 755 | 1.688 | 3.770 | 0.4509 | 2307.75 | 63.38 | 0.6296 | ACCEPT |
| 20261006-205221 | 751 | 1.702 | 3.766 | 0.4376 | 2308.00 | 63.01 | 0.6270 | ACCEPT |
| 20261006-210159 | 772 | 1.823 | 3.927 | 0.6017 | 2297.50 | 62.08 | 0.0000 | **REVIEW / REJECT** |
| 20261006-211122 | 795 | 1.724 | 3.815 | 0.4061 | 2292.00 | 61.53 | 0.6405 | ACCEPT |
| 20261006-212112 | 723 | 2.431 | 4.810 | 0.7760 | 2280.00 | 60.05 | 0.0000 | **REVIEW / REJECT** |
| 20261006-213153 | 831 | 1.734 | 3.801 | 0.5069 | 2270.50 | 58.56 | 0.6992 | ACCEPT |
| 20261006-214211 | 779 | 1.860 | 3.973 | 0.4535 | 2267.88 | 57.82 | 0.6231 | ACCEPT |
| 20261006-215309 | 848 | 1.795 | 3.891 | 0.5551 | 2268.00 | 58.56 | 0.6521 | ACCEPT |
| 20261006-220327 | 865 | 1.764 | 3.870 | 0.5194 | 2257.50 | 57.08 | 0.7114 | ACCEPT |
| 20261006-221415 | 878 | 1.776 | 3.853 | 0.5454 | 2247.00 | 54.86 | 0.7599 | ACCEPT |
| 20261006-222507 | 882 | 1.764 | 3.835 | 0.5401 | 2241.62 | 54.86 | 0.7703 | ACCEPT |
| 20261006-223610 | 756 | 2.195 | 4.409 | 0.7108 | 2236.50 | 54.30 | 0.0000 | **REVIEW / REJECT** |
| 20261006-225204 | 889 | 1.818 | 3.914 | 0.5299 | 2229.38 | 53.37 | 0.7653 | ACCEPT |
| 20261006-230212 | 929 | 1.737 | 3.806 | 0.5107 | 2221.25 | 52.26 | 0.8747 | ACCEPT |
| 20261006-231414 | 953 | 1.723 | 3.774 | 0.4581 | 2220.00 | 51.89 | 0.9018 | ACCEPT |
| 20261006-232807 | 943 | 1.743 | 3.805 | 0.4807 | 2218.00 | 51.15 | 0.9074 | ACCEPT |
| 20261006-233727 | 977 | 1.763 | 3.833 | 0.4424 | 2217.50 | 51.15 | 0.8865 | ACCEPT |
| 20261006-234830 | 971 | 1.718 | 3.788 | 0.5013 | 2215.50 | 50.41 | 0.9617 | ACCEPT |
| 20261006-235944 | 964 | 1.790 | 3.875 | 0.5448 | 2208.50 | 50.22 | 0.8922 | ACCEPT |
| 20261007-001131 | 992 | 1.755 | 3.812 | 0.5078 | 2205.50 | 48.93 | 0.9780 | ACCEPT |
| 20261007-002106 | 950 | 1.812 | 3.906 | 0.4665 | 2200.75 | 48.93 | 0.9173 | ACCEPT |
| 20261007-003159 | 984 | 1.778 | 3.852 | 0.4735 | 2200.50 | 48.93 | 0.9523 | ACCEPT |
| 20261007-004225 | 955 | 1.835 | 3.903 | 0.5372 | 2199.50 | 48.18 | 0.9220 | ACCEPT |
| 20261007-005149 | 992 | 1.749 | 3.802 | 0.4530 | 2199.50 | 48.56 | 1.0000 | ACCEPT |
| 20261007-010137 | 954 | 1.800 | 3.857 | 0.5094 | 2199.00 | 48.37 | 0.9506 | ACCEPT |
| 20261007-011221 | 783 | 1.809 | 3.957 | 0.5273 | 2225.25 | 51.89 | 0.8180 | ACCEPT |
| 20261007-012253 | 684 | 1.862 | 4.028 | 0.5495 | 2238.50 | 54.11 | 0.7103 | ACCEPT |
| 20261007-013244 | 919 | 1.842 | 3.981 | 0.5452 | 2211.00 | 49.67 | 0.8609 | ACCEPT |
| 20261007-014249 | 799 | 2.060 | 4.261 | 0.6742 | 2209.00 | 50.04 | 0.0000 | **REVIEW / REJECT** |
| 20261007-015458 | 910 | 1.844 | 3.987 | 0.5516 | 2199.00 | 48.37 | 0.9060 | ACCEPT |
| 20261007-020421 | 849 | 1.883 | 4.068 | 0.6279 | 2206.00 | 49.30 | 0.0000 | **REVIEW / REJECT** |
| 20261007-021544 | 696 | 1.876 | 4.062 | 0.5122 | 2232.50 | 53.37 | 0.7190 | ACCEPT |

## Flagged Frames to Check in PixInsight

| Frame ID | Reasons | Example star x, y | Example HFR px |
| --- | --- | ---: | ---: |
| 20261006-210159 | excessive_eccentricity | 1570.34, 916.95 | 1.825 |
| 20261006-212112 | excessive_eccentricity, excessive_hfr | 778.84, 3110.95 | 2.432 |
| 20261006-223610 | excessive_eccentricity, excessive_hfr | 609.95, 2204.35 | 2.196 |
| 20261007-014249 | excessive_eccentricity, excessive_hfr | 1529.80, 944.21 | 2.057 |
| 20261007-020421 | excessive_eccentricity | 2023.71, 837.26 | 1.873 |

**Borderline case:** `20261006-210159` is only slightly above the default eccentricity limit of 0.60. Review this frame alongside a nearby accepted frame before deciding whether to keep the default threshold. An automatic rejection is a recommendation; inputs remain untouched.

Open the matching debayered PI reference `PixInsight Files/InterimFiles/dLight_M 33_10.0s_IRCUT_<Frame ID>.xisf` where available. Compare stellar elongation and sharpness at equal zoom/display settings. Coordinates are zero-based original source pixel positions. The example star is close to the frame median HFR; the JSON contains the entire measured catalog. Moment FWHM is not a fitted PI FWHM, and different measurement samples/methods need not agree numerically.

Full precision: `outputs/M2_DS-B_report.json`. Spreadsheet table: `outputs/M2_DS-B_report.csv` (UTF-8 BOM, semicolon delimiter, decimal point). Reproduce with unused output names using the command in `docs/M2_REVIEW.md`. M3 has not started.
