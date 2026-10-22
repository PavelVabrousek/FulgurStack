"""Read-only DS-B CPU/GPU parity and source-integrity gate (development NumPy).

Run from the repository root. Report/PI manifest destinations must be new.
No image writes, display transforms or observing-coordinate output.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path

import numpy as np


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def digest(path):
    return hashlib.file_digest(Path(path).open("rb"), "sha256").hexdigest()


def read_fits(path):
    with Path(path).open("rb") as stream:
        cards = {}
        done = False
        while not done:
            block = stream.read(2880)
            assert len(block) == 2880
            for start in range(0, 2880, 80):
                card = block[start:start + 80]
                if card.startswith(b"END "):
                    done = True
                    break
                if card[8:10] == b"= ":
                    cards[card[:8].decode().strip()] = card[10:].decode().split("/")[0].strip().strip("'").strip()
        assert cards["BITPIX"] == "-32" and cards["NAXIS3"] == "3"
        w, h = int(cards["NAXIS1"]), int(cards["NAXIS2"])
        image = np.frombuffer(stream.read(w * h * 12), dtype=">f4").reshape(3, h, w)
        assert np.isfinite(image).all() and 0 <= image.min() <= image.max() <= 1
        return cards, image


def science_only(value):
    if isinstance(value, dict):
        return {k: science_only(v) for k, v in value.items()
                if not k.endswith("seconds") and k not in ("frames_per_second", "peak_rss_bytes")}
    if isinstance(value, list):
        return [science_only(v) for v in value]
    return value


def apertures(registration, cpu, gpu):
    anchor = registration["reference_anchor"]
    inspected = next(f for f in registration["inspection"]["frames"] if f["file_name"] == anchor)
    transform = next(f["transform"] for f in registration["frames"] if f["file_name"] == anchor)
    scale, theta = transform["scale_factor"], transform["theta_rad"]
    radius = 6 * scale
    centers = []
    stars = inspected["stars"]
    for star in stars:
        if any(other is not star and math.hypot(other["x_px"] - star["x_px"], other["y_px"] - star["y_px"]) < 22 for other in stars):
            continue
        x = scale * (math.cos(theta) * star["x_px"] - math.sin(theta) * star["y_px"]) + transform["tx_px"]
        y = scale * (math.sin(theta) * star["x_px"] + math.cos(theta) * star["y_px"]) + transform["ty_px"]
        if not (12 * scale <= x < cpu.shape[2] - 12 * scale and 12 * scale <= y < cpu.shape[1] - 12 * scale):
            continue
        xi, yi = int(x), int(y)
        r = math.ceil(11 * scale)
        yy, xx = np.mgrid[yi-r:yi+r+1, xi-r:xi+r+1]
        distances = np.hypot(xx - x, yy - y)
        mask = distances <= radius
        sky = (distances >= 8 * scale) & (distances <= 11 * scale)
        a = cpu[:, yi-r:yi+r+1, xi-r:xi+r+1].astype(np.float64)
        b = gpu[:, yi-r:yi+r+1, xi-r:xi+r+1].astype(np.float64)
        if a.max() >= 0.94:
            continue
        af = (a[:, mask] - np.median(a[:, sky], axis=1)[:, None]).sum(axis=1)
        bf = (b[:, mask] - np.median(b[:, sky], axis=1)[:, None]).sum(axis=1)
        if (af <= 0).any():
            continue
        errors = np.abs((bf - af) / af)
        assert errors.max() <= 0.001, (x, y, errors.tolist())
        centers.append({"x": x, "y": y, "radius_px": radius,
                        "cpu_flux_rgb": af.tolist(), "gpu_flux_rgb": bf.tolist(),
                        "relative_difference_rgb": errors.tolist()})
    assert len(centers) >= 10, "Insufficient isolated unsaturated apertures"
    return centers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("cpu", "gpu", "previous", "output", "pi-expected"):
        parser.add_argument("--" + arg, required=True, type=Path)
    args = parser.parse_args()
    assert not args.output.exists() and not args.pi_expected.exists()
    c, g = load(args.cpu), load(args.gpu)
    assert science_only(c["registration"]) == science_only(g["registration"])
    assert c["registration"]["frame_count"] == 32
    assert c["registration"]["accepted_count"] == 27
    assert c["width"] == g["width"] and c["height"] == g["height"]
    assert c["origin_x_px"] == c["origin_y_px"] == g["origin_x_px"] == g["origin_y_px"] == 0
    for key in ("valid_channel_samples", "decoded_roi_pixels", "corrected_roi_pixels"):
        assert c["integration"][key] == g["integration"][key]
    manifest = load("docs/ds-b-manifest.json")
    for source in manifest["sources"] + manifest["references"]:
        assert Path(source["path"]).stat().st_size == source["size_bytes"]
        assert digest(source["path"]) == source["sha256"]
    assert digest("RegisteringTemplate200pct.fit") == "3d1f0c73f04749391d18f7764912eac07f642589ac4a401f27eb1f9c3a6f6d61"
    expected = {"files": []}
    result = {"status": "PASS", "scope": "DS-B; actual GPU, same binary, default canvas, 32 inputs, 27 accepted",
              "source_integrity": "PASS; 32 sources, two PI references and external template SHA256 unchanged",
              "selection_registration_photometry": "PASS; exact scientific report equality",
              "products": [], "gates": {"normalized_rms": 1e-5, "normalized_max": 1e-3, "aperture_relative": 0.001}}
    used = high = 0
    for kind, field in (("master", "output_file"), ("coverage", "coverage_file"), ("high_rejection", "high_rejection_file")):
        cp, gp = args.cpu.parent / c[field], args.gpu.parent / g[field]
        suffix = "" if kind == "master" else "_" + kind
        old = args.previous.with_name(args.previous.stem + suffix + ".fits")
        assert digest(cp) == digest(old), "CPU refactor changed approved M4 product"
        ch, ca = read_fits(cp)
        gh, ga = read_fits(gp)
        assert ch == gh
        entry = {"kind": kind, "path": gp.as_posix(), "sha256": digest(gp),
                 "cpu_matches_M4_bytes": True, "changed_channel_samples": int(np.count_nonzero(ca != ga)),
                 "minimum": float(ga.min()), "maximum": float(ga.max())}
        if kind == "master":
            squared, maximum = 0., 0.
            for channel in range(3):
                difference = ga[channel].astype(np.float64) - ca[channel]
                squared += float(np.square(difference).sum())
                maximum = max(maximum, float(np.abs(difference).max()))
            entry.update(normalized_rms=math.sqrt(squared / ca.size), normalized_max=maximum)
            assert entry["normalized_rms"] <= 1e-5 and maximum <= 1e-3
            entry["isolated_star_apertures"] = apertures(g["registration"], ca, ga)
        else:
            assert gh["BUNIT"] == "fraction" and gh["NINPUT"] == "32"
            assert not any(k in gh for k in ("SITELAT", "SITELONG", "BSCALE", "BZERO", "FLXDIV"))
            counts = np.rint(ga.astype(np.float64) * 32)
            total = int(counts.sum())
            entry["integer_count_total"] = total
            entry["integer_count_delta_GPU_minus_CPU"] = int(total - np.rint(ca.astype(np.float64) * 32).sum())
            if kind == "coverage":
                used = total
            else:
                high = total
        expected["files"].append({"path": gp.as_posix(), "width": g["width"], "height": g["height"],
                                  "minimum": entry["minimum"], "maximum": entry["maximum"],
                                  "probes": [{"x": x, "y": y, "rgb": ga[:, y, x].astype(float).tolist()}
                                             for x in (0, 1, 500, 1694, g["width"]-2, g["width"]-1)
                                             for y in (0, 1, 117, 1409, g["height"]-2, g["height"]-1)]})
        result["products"].append(entry)
    assert high == g["integration"]["rejected_high_channel_samples"]
    assert used + high + g["integration"]["rejected_low_channel_samples"] == g["integration"]["valid_channel_samples"]
    assert c["intermediate_images_written"] == g["intermediate_images_written"] == 0
    assert not list(args.gpu.parent.glob("*.tmp"))
    assert all(k not in args.gpu.read_text(encoding="utf-8") for k in ("SITELAT", "SITELONG"))
    result.update(cpu_seconds=c["wall_seconds"], gpu_seconds=g["wall_seconds"],
                  speedup=c["wall_seconds"] / g["wall_seconds"], gpu_peak_MiB=g["peak_rss_bytes"] / 1048576,
                  gpu=g["integration"]["gpu"], low_rejection_delta=g["integration"]["rejected_low_channel_samples"] - c["integration"]["rejected_low_channel_samples"])
    for path, value in ((args.output, result), (args.pi_expected, expected)):
        with path.open("x", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2)
    print(json.dumps({k: v for k, v in result.items() if k != "products"}, indent=2))


if __name__ == "__main__":
    main()
