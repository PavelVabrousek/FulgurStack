"""Read-only full-run verification and compact profiling report (development NumPy).

Usage: python scripts/tests/verify-full-stack.py --report outputs/run.json
       --input-manifest outputs/before.json --output outputs/validation.json
       --pi-expected outputs/expected.json
All output paths must be new. Never prints observing coordinates.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path

import numpy as np


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        while block := f.read(4194304):
            h.update(block)
    return h.hexdigest()


def header(f):
    result = {}
    while True:
        block = f.read(2880)
        assert len(block) == 2880
        for offset in range(0, 2880, 80):
            card = block[offset:offset + 80]
            if card.startswith(b"END "):
                return result
            if card[8:10] == b"= ":
                result[card[:8].decode().strip()] = card


def value(h, key):
    return h[key][10:].decode().split("/")[0].strip().strip("'").strip()


def date(frame):
    d = datetime.datetime.fromisoformat(frame["frame"]["date_obs"].replace("Z", "+00:00"))
    d = d.replace(tzinfo=datetime.timezone.utc) if d.tzinfo is None else d
    return d.astimezone(datetime.timezone.utc), frame["file_name"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ("report", "input-manifest", "output", "pi-expected"):
        parser.add_argument("--" + key, required=True, type=Path)
    args = parser.parse_args()
    assert not args.output.exists() and not args.pi_expected.exists()
    r = load(args.report)
    reg, stats = r["registration"], r["integration"]
    manifest = load(args.input_manifest)
    root = Path(manifest["root"])
    original = reg["frame_count"]
    accepted = reg["accepted_count"]
    assert original == len(manifest["files"]) == 1140
    candidates = sorted((f for f in reg["inspection"]["frames"] if f["accepted"]), key=date)
    assert candidates[(len(candidates) - 1) // 2]["file_name"] == reg["reference_anchor"]
    assert r["canvas_mode"] == "reference" and r["origin_x_px"] == r["origin_y_px"] == 0
    assert r["map_denominator_frames"] == original
    assert r["intermediate_images_written"] == stats["intermediate_images_written"] == 0
    assert stats["planned_buffer_bytes"] <= r["config"]["memory_budget_mib"] * 1048576
    assert r["peak_rss_bytes"] <= r["config"]["memory_budget_mib"] * 1048576
    with (root / reg["reference_anchor"]).open("rb") as f:
        anchor = header(f)
    assert r["width"] == int(value(anchor, "NAXIS1")) == 2160
    assert r["height"] == int(value(anchor, "NAXIS2")) == 3840
    products, expected = [], {"files": []}
    used = high_count = 0
    empty_mask = master = coverage = None
    for kind, field in (("master", "output_file"), ("coverage", "coverage_file"),
                        ("high_rejection", "high_rejection_file")):
        path = args.report.parent / r[field]
        with path.open("rb") as f:
            h = header(f)
            assert value(h, "BITPIX") == "-32" and value(h, "NAXIS3") == "3"
            assert int(value(h, "NAXIS1")) == r["width"] and int(value(h, "NAXIS2")) == r["height"]
            a = np.frombuffer(f.read(r["width"] * r["height"] * 12), dtype=">f4").reshape(3, r["height"], r["width"])
        assert np.isfinite(a).all() and a.min() >= 0 and a.max() <= 1
        assert path.stat().st_size % 2880 == 0
        if kind == "master":
            master = a
            assert int(value(h, "CROPX")) == int(value(h, "CROPY")) == 0
            assert int(value(h, "NCOMBINE")) == accepted
            assert float(value(h, "TEXPTIME")) == reg["accepted_exposure_seconds"]
            assert float(value(h, "EXPTIME")) == float(value(anchor, "EXPTIME"))
            for key in ("SITELAT", "SITELONG"):
                if key in anchor:
                    assert h[key] == anchor[key]
        else:
            assert value(h, "MAPTYPE") == ("COVERAGE" if kind == "coverage" else "HIGHREJ")
            assert value(h, "BUNIT") == "fraction" and int(value(h, "NINPUT")) == original
            assert float(value(h, "PCTFACT")) == 100
            assert not any(k in h for k in ("FLXDIV", "BSCALE", "BZERO", "SITELAT", "SITELONG"))
            # Non-power-of-two denominators require rounding back to integer counts.
            counts = np.rint(a.astype(np.float64) * original)
            assert np.max(np.abs(a.astype(np.float64) * original - counts)) < 0.0001
            assert counts.max() <= accepted
            if kind == "coverage":
                coverage = counts
                used = int(counts.sum(dtype=np.float64))
                empty_mask = counts == 0
                assert (master[empty_mask] == 0).all()
            else:
                high_count = int(counts.sum(dtype=np.float64))
                assert (coverage + counts <= accepted).all()
        products.append({"kind": kind, "path": path.as_posix(), "sha256": digest(path),
                         "minimum": float(a.min()), "maximum": float(a.max()),
                         "mean_percent": float(a.mean(dtype=np.float64)) * 100 if kind != "master" else None})
        expected["files"].append({"path": path.as_posix(), "width": r["width"], "height": r["height"],
                                  "minimum": float(a.min()), "maximum": float(a.max()),
                                  "probes": [{"x": x, "y": y, "rgb": [float(v) for v in a[:, y, x]]}
                                             for y in (0, 1, 117, 1409, 1919, 3838, 3839)
                                             for x in (0, 1, 500, 1079, 1694, 2158, 2159)]})
    assert high_count == stats["rejected_high_channel_samples"]
    assert used + high_count + stats["rejected_low_channel_samples"] == stats["valid_channel_samples"]
    for entry in manifest["files"]:
        source = root / entry["file"]
        assert source.stat().st_size == entry["size"] and digest(source) == entry["sha256"]
    assert not list(args.report.parent.glob("*.tmp"))
    report_text = args.report.read_text(encoding="utf-8")
    assert "SITELAT" not in report_text and "SITELONG" not in report_text
    partition = stats["reconstruction_wall_seconds"] + stats["reduction_wall_seconds"] + stats["output_copy_wall_seconds"]
    assert 0 < partition <= stats["wall_seconds"]
    phases = {"inspection": reg["inspection_seconds"], "registration": reg["registration_seconds"],
              "reconstruction_batches": stats["reconstruction_wall_seconds"],
              "rejection_accumulation_maps": stats["reduction_wall_seconds"],
              "output_copy": stats["output_copy_wall_seconds"], "three_fits_writes": r["write_seconds"]}
    phases["other_processing"] = r["wall_seconds"] - sum(phases.values())
    assert phases["other_processing"] >= 0
    result = {"status": "PASS", "original_frames": original, "accepted_frames": accepted,
              "rejected_frames": original - accepted, "reference": reg["reference_anchor"],
              "reference_DATE_OBS": reg["reference_date_obs"], "dimensions": [r["width"], r["height"]],
              "processing_seconds": r["wall_seconds"], "peak_MiB": r["peak_rss_bytes"] / 1048576,
              "wall_phases_seconds": phases,
              "roi_task_elapsed_seconds_overlapping_not_cpu": stats["roi_task_elapsed"],
              "used_channel_samples": used, "high_rejected_channel_samples": high_count,
              "low_rejected_channel_samples": stats["rejected_low_channel_samples"],
              "valid_channel_samples": stats["valid_channel_samples"],
              "source_integrity": "PASS; all 1140 SHA256 hashes unchanged",
              "sample_accounting": "PASS; coverage/high maps reconstruct global integer counts",
              "temporary_files": "PASS; no remaining .tmp files", "products": products}
    for path, data in ((args.output, result), (args.pi_expected, expected)):
        with path.open("x", encoding="utf-8") as f:
            json.dump(data, f, indent=2)
    print(json.dumps({k: v for k, v in result.items() if k != "products"}, indent=2))


if __name__ == "__main__":
    main()
