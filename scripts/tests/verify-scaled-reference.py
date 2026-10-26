"""Independent DS-B 200% geometry/map/photometry/source-integrity regression check.

Requires development-only NumPy. Run after both documented real DS-B runs.
Writes JSON reports only; no images or input modifications.
"""
import importlib.util
import json
import math
from pathlib import Path

import numpy as np

spec = importlib.util.spec_from_file_location("full_check", Path(__file__).with_name("verify-full-stack.py"))
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)
load, digest, header, value = helper.load, helper.digest, helper.header, helper.value


def main():
    prefix = "M4_DS-B_external200_stack"
    report_path = Path("outputs") / (prefix + ".json")
    r = load(report_path)
    native = load("outputs/M4_DS-B_scale_unit_regression.json")
    reg = r["registration"]
    external = reg["external_reference"]
    assert r["width"] == r["height"] == reg["reference_width"] == reg["reference_height"] == 4000
    assert r["origin_x_px"] == r["origin_y_px"] == 0 and r["canvas_mode"] == "reference"
    assert r["map_denominator_frames"] == reg["frame_count"] == 32
    expected_registered = 32 if reg.get("diagnose_excluded", True) else 27
    assert reg["registration_success_count"] == expected_registered and reg["accepted_count"] == 27
    assert reg["accepted_exposure_seconds"] == 270
    assert external["geometry_only"] and external["scale_range"] == [0.33, 3.0]
    assert external["photometric_anchor"] == reg["reference_anchor"] == native["registration"]["reference_anchor"]
    source_rows = {f["file_name"]: f for f in reg["inspection"]["frames"]}
    native_rows = {f["file_name"]: f for f in native["registration"]["frames"]}
    scales, errors, pairs = [], [], []
    for row in reg["frames"]:
        t = row["transform"]
        scale = t["scale_factor"]
        assert abs(scale - 2) < 0.002
        scales.append(scale)
        star_list = source_rows[row["file_name"]]["stars"]
        residuals = []
        for pair in row["matches"]:
            p, q = star_list[pair["source"]], external["stars"][pair["reference"]]
            c, s = math.cos(t["theta_rad"]), math.sin(t["theta_rad"])
            x = scale * (c * p["x_px"] - s * p["y_px"]) + t["tx_px"]
            y = scale * (s * p["x_px"] + c * p["y_px"]) + t["ty_px"]
            error = math.hypot(x - q["x_px"], y - q["y_px"])
            assert abs(error - pair["residual_px"]) < 1e-8 and error <= 1.5
            residuals.append(error)
        rmse = math.sqrt(sum(v * v for v in residuals) / len(residuals))
        assert len(residuals) >= 10 and rmse <= 1.5
        assert abs(rmse - row["inlier_rmse_px"]) < 1e-8
        errors.append(rmse)
        pairs.append(len(residuals))
        old = native_rows[row["file_name"]]
        for key in ("accepted", "photometric_scale", "background_offset_adu", "normalized_noise_adu",
                    "matched_flux_ratio", "exposure_corrected_transparency", "final_weight"):
            assert row[key] == old[key], key
        assert row["source_to_photometric_anchor"] == old["transform"]
    products, expected, images, heads = [], {"files": []}, {}, {}
    for kind, field in (("master", "output_file"), ("coverage", "coverage_file"), ("high_rejection", "high_rejection_file")):
        path = report_path.parent / r[field]
        with path.open("rb") as f:
            h = header(f)
            assert value(h, "BITPIX") == "-32" and value(h, "NAXIS3") == "3"
            assert int(value(h, "NAXIS1")) == int(value(h, "NAXIS2")) == 4000
            a = np.frombuffer(f.read(4000 * 4000 * 12), dtype=">f4").reshape(3, 4000, 4000)
        assert np.isfinite(a).all() and a.min() >= 0 and a.max() <= 1
        images[kind], heads[kind] = a, h
        products.append({"kind": kind, "path": path.as_posix(), "sha256": digest(path),
                         "minimum": float(a.min()), "maximum": float(a.max())})
        expected["files"].append({"path": path.as_posix(), "width": 4000, "height": 4000,
                                  "minimum": float(a.min()), "maximum": float(a.max()),
                                  "probes": [{"x": x, "y": y, "rgb": [float(v) for v in a[:, y, x]]}
                                             for y in (0, 1, 500, 1409, 1999, 3998, 3999)
                                             for x in (0, 1, 500, 1694, 1999, 3998, 3999)]})
    h = heads["master"]
    assert value(h, "REGMODEL") == "SIM(2)"
    assert abs(float(value(h, "GEOMSCL")) - external["anchor_to_reference"]["scale_factor"]) < 1e-10
    assert int(value(h, "NCOMBINE")) == 27 and float(value(h, "TEXPTIME")) == 270
    assert int(value(h, "CROPX")) == int(value(h, "CROPY")) == 0
    assert not any(k in h for k in ("XPIXSZ", "YPIXSZ", "CRPIX1", "CRPIX2", "CRVAL1", "CD1_1"))
    with (Path("SeestarS50 Subs") / reg["reference_anchor"]).open("rb") as f:
        anchor = header(f)
    for key in ("SITELAT", "SITELONG"):
        assert h[key] == anchor[key]
    assert float(value(h, "EXPTIME")) == float(value(anchor, "EXPTIME"))
    for kind, maptype in (("coverage", "COVERAGE"), ("high_rejection", "HIGHREJ")):
        h, a = heads[kind], images[kind]
        assert value(h, "MAPTYPE") == maptype and value(h, "BUNIT") == "fraction"
        assert int(value(h, "NINPUT")) == 32
        assert not any(k in h for k in ("SITELAT", "SITELONG", "FLXDIV", "BSCALE", "BZERO"))
        assert np.array_equal(a * 32, np.rint(a * 32))
    coverage, high = images["coverage"], images["high_rejection"]
    assert (coverage + high <= 27 / 32).all()
    used, upper = int(coverage.sum(dtype=np.float64) * 32), int(high.sum(dtype=np.float64) * 32)
    assert upper == r["integration"]["rejected_high_channel_samples"]
    assert used + upper + r["integration"]["rejected_low_channel_samples"] == r["integration"]["valid_channel_samples"]
    assert (images["master"][coverage == 0] == 0).all()
    parity = {}
    for suffix in (".fits", "_coverage.fits", "_high_rejection.fits"):
        a, b = Path("outputs/M4_DS-B_scale_unit_regression" + suffix), Path("outputs/M4_DS-B_reference_median" + suffix)
        assert digest(a) == digest(b)
        parity[suffix] = digest(a)
    manifest = load("docs/ds-b-manifest.json")
    for entry in manifest["sources"] + manifest["references"]:
        assert digest(entry["path"]) == entry["sha256"]
    assert digest("RegisteringTemplate200pct.fit") == "3d1f0c73f04749391d18f7764912eac07f642589ac4a401f27eb1f9c3a6f6d61"
    assert not list(Path("outputs").glob("*.tmp"))
    assert r["intermediate_images_written"] == r["integration"]["intermediate_images_written"] == 0
    assert r["peak_rss_bytes"] <= r["config"]["memory_budget_mib"] * 1048576
    assert "SITELAT" not in report_path.read_text(encoding="utf-8")
    result = {"status": "PASS", "registered": 32, "accepted": 27, "rejected": 5,
              "dimensions": [4000, 4000], "scale_min": min(scales), "scale_max": max(scales),
              "rmse_min_reference_px": min(errors), "rmse_max_reference_px": max(errors),
              "minimum_inliers": min(pairs), "processing_seconds": r["wall_seconds"],
              "complete_command_seconds": load("outputs/" + prefix + "_timing.json")["complete_command_seconds"],
              "peak_MiB": r["peak_rss_bytes"] / 1048576, "used_samples": used, "high_rejected_samples": upper,
              "native_unit_scale_three_fits_byte_parity": parity,
              "native_photometry_weights_and_acceptance_unchanged": True,
              "source_integrity": "PASS; 32 sources, two PI references and external template unchanged",
              "products": products}
    for suffix, data in (("_validation.json", result), ("_PI_expected.json", expected)):
        with Path("outputs/" + prefix + suffix).open("x", encoding="utf-8") as f:
            json.dump(data, f, indent=2)
    print(json.dumps({k: v for k, v in result.items() if k not in ("products", "native_unit_scale_three_fits_byte_parity")}, indent=2))


if __name__ == "__main__":
    main()
