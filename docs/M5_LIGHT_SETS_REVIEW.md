# M5: Light Sets, Calibration Sets and Declared Sensor Layout (2026-10-28)

Current state: [CURRENT_STATUS.md](CURRENT_STATUS.md). Decision: [ADR-071](ARCHITECTURE_DECISIONS.md). Evidence: [M5_LIGHT_SETS_EVIDENCE.json](M5_LIGHT_SETS_EVIDENCE.json).

## Scope

The owner approved the clickable mockup (four steps, actions in the top bar, one progress bar per run, light/calibration sets) and asked for its implementation in the Rust application, including selection of light and calibration sets. Calibration arithmetic is explicitly deferred until real calibration data exist. Registration and integration without calibration are functional. Each light set's sensor layout is verified from the headers or declared by the owner; monochrome frames skip demosaicing.

## What changed

| Area | Behaviour |
| --- | --- |
| FITS headers | `BAYERPAT` present → colour; missing → **unknown** (never assumed mono). `mosaic` and `mosaic_source` are recorded per frame. Header-only metadata scanning (any BITPIX/NAXIS) for planning sets: IMAGETYP, filter, camera (INSTRUME or Seestar CREATOR), gain, sensor temperature, NCOMBINE, dates. |
| Mono frames | 2×2-mean detection grid, full-plane centroids and HFR quadrature, same isolated hot-pixel correction, no demosaicing (R=G=B). Mono-only masters and maps are written as one plane (`NAXIS=2`, `DEBAYER='NONE'`). GPU refuses mono sources explicitly. |
| Light sets | Quality limits per set; other sets may differ in target name (warning only, ADR-093), filter, gain, exposure and colour layout but must share sensor dimensions (`different_sensor_size_unsupported`). Provisional weights normalized over all sets. One set is bit-identical to the previous single population. |
| Registration | Native and distortion compatibility require only the same target and sensor dimensions. Cross-camera scale for sources is not implemented. |
| CLI | `--sensor auto|mono|RGGB|GRBG|GBRG|BGGR` (stack, register, inspect, debayer-single); `--input-sets` makes each `--input` its own light set. |
| GUI | Frames (rail with All light frames, Light sets, Calibration sets; set overview; light-set detail with layout declaration and calibration choice; calibration-set detail), Settings (one page), Processing (screen), Results. Top-bar primary action/menu/STOP, one-line status bar. Sets persist; old input lists migrate. Reports list light sets, assigned calibration and `calibration_applied: false`. |

## Owner-facing rules in the UI

- A light set whose headers lack a Bayer pattern (or disagree) shows **Choose colour or mono**; Measure and Stack stay disabled until the owner picks *Colour (Bayer mask)* with a pattern or *Mono (no Bayer mask) · skip debayering*.
- Mixed filters among included sets are warned in the overview, set detail and rail, and Stack asks *Stack anyway / Review sets*.
- Mixed mono/colour sets are warned: mono frames feed R, G and B equally.
- A set with another sensor size is flagged as not stackable yet; its frames are rejected with an explicit reason. *Superseded by ADR-110 and ADR-120: such a set now joins the stack as another camera.*
- Calibration compatibility is checked from headers (camera, size, flat filter, flat night, dark exposure/gain/temperature) and Seestar-specific double-correction warnings are shown. Assignment is recorded but not applied.
- Changing set membership, inclusion or layout clears quality measurements; calibration assignment does not.

## Verification (Windows, local, release)

| Gate | Result |
| --- | --- |
| `cargo fmt --check`, Clippy default and all-features | PASS |
| `cargo test --workspace --release` | 175/175 PASS (real FITS, AVX2, GPU environment set) |
| `cargo test --workspace --release --all-features` | 178/178 PASS |
| Mono HFR/centroid/eccentricity, 48 Gaussian phase cases | max relative HFR error < 3%, centroid < 0.05 px |
| Mono full-frame analysis | 25/25 rendered stars, centroid < 0.1 px, HFR within 5% |
| Unknown layout | refused by star measurement, demosaicing and integration; CLI stack without `--sensor` fails without outputs |
| CLI mono stack (`--sensor mono`) | 3/3 frames, one-plane master and maps, `DEBAYER='NONE'`, sources unchanged |
| Per-set classification | different filter/gain/layout set keeps its frames; other sensor and other target rejected; one set bit-identical |
| DS-B (32 Seestar S50 frames) | master, coverage, high-rejection **byte-identical** to ADR-070; registration report differs only by new provenance fields and one description string |
| Real Seestar S30 Pro, two sets (IR 10 s ×4, LP 20 s ×83) | 75/87 stacked in 11.8 s, peak 713 MiB, sources unchanged; LP rejections: 9 eccentricity, 9 HFR |

## Addendum: external reference filter and 30–333% scale

- The external reference's filter is no longer checked (geometry only, no warning); a different target is still refused.
- Scale range widened to 30–333%; synthetic sweeps include both bounds and reject values outside.
- DS-B against `RegisteringTemplate200pct.fit`: master and both maps byte-identical to the ADR-070 control.
- Seestar S30 Pro (160 mm) against the S50 200% template needs 325% (now in range) but still fails to register: the 4000 × 4000 template covers only about 14% of the S30 Pro field, with about 20 detected S30 Pro stars there. Mirror/180° variants and looser tolerances were also tested without success. Practical options: a template made from S30 Pro data (or covering its whole field), or Auto/Chronological median reference.

## Not verified / limits

- No real monochrome data, real calibration data or cross-camera data were available.
- The new screens were checked structurally (accessibility tree) and by automated layout tests; the display was off during this session, so native visual review at 1366 × 768 / 125% remains the owner's manual step.
- Calibration arithmetic, master building and master saving are not implemented.
- Windows only; macOS not run.
