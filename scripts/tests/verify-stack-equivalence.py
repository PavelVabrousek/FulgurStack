"""Exact before/after FITS and scientific report comparison; read-only image access.

Development verification, Python standard library only. Run from the repository
root. The output JSON must be new. Input/reference coordinates are never emitted.
"""
import argparse
import hashlib
import json
from pathlib import Path


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


TIMING_FIELDS = {"wall_seconds", "inspection_seconds", "registration_seconds",
                 "read_seconds", "measurement_seconds", "frames_per_second",
                 "peak_rss_bytes", "reconstruction_wall_seconds", "reduction_wall_seconds",
                 "output_copy_wall_seconds", "roi_task_elapsed"}


def science(value):
    if isinstance(value, dict):
        return {k: science(v) for k, v in value.items() if k not in TIMING_FIELDS}
    if isinstance(value, list):
        return [science(v) for v in value]
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("before", "after", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--full-input-manifest", type=Path)
    parser.add_argument("--allow-tile-geometry-change", action="store_true",
                        help="Allow only tile configuration/work counters to differ; image bytes and all scientific counts must remain exact")
    args = parser.parse_args()
    assert not args.output.exists()
    before, after = load(args.before), load(args.after)
    assert science(before["registration"]) == science(after["registration"])
    bi, ai = science(before["integration"]), science(after["integration"])
    bc, ac = before["config"].copy(), after["config"].copy()
    if args.allow_tile_geometry_change:
        for config in (bc, ac):
            for key in ("tile_width", "tile_height"):
                config.pop(key)
        for stats in (bi, ai):
            for key in ("tile_width", "tile_height", "tile_count", "planned_buffer_bytes",
                        "decoded_roi_pixels", "corrected_roi_pixels"):
                stats.pop(key)
    assert bi == ai
    assert bc == ac
    for key in ("width", "height", "origin_x_px", "origin_y_px", "canvas_mode",
                "map_denominator_frames", "export", "output_units", "output_bitpix",
                "bias_subtracted", "gps_copied_to_fits", "intermediate_images_written"):
        assert before[key] == after[key], key
    products = []
    for kind, field in (("master", "output_file"), ("coverage", "coverage_file"),
                        ("high_rejection", "high_rejection_file")):
        a, b = args.before.parent / before[field], args.after.parent / after[field]
        ah, bh = digest(a), digest(b)
        assert ah == bh and a.stat().st_size == b.stat().st_size, kind
        products.append({"kind": kind, "before": a.as_posix(), "after": b.as_posix(),
                         "bytes": b.stat().st_size, "before_sha256": ah, "after_sha256": bh,
                         "exact_entire_FITS_byte_equality": True, "different_samples": 0})
    sources = load("docs/ds-b-manifest.json")
    entries = sources["sources"] + sources["references"]
    for entry in entries:
        assert Path(entry["path"]).stat().st_size == entry["size_bytes"]
        assert digest(entry["path"]) == entry["sha256"]
    assert digest("RegisteringTemplate200pct.fit") == "3d1f0c73f04749391d18f7764912eac07f642589ac4a401f27eb1f9c3a6f6d61"
    integrity = "PASS; 32 DS-B sources, two PI references and external template unchanged"
    if args.full_input_manifest:
        manifest = load(args.full_input_manifest)
        root = Path(manifest["root"])
        assert len(manifest["files"]) == after["registration"]["frame_count"] == 1140
        for entry in manifest["files"]:
            path = root / entry["file"]
            assert path.stat().st_size == entry["size"] and digest(path) == entry["sha256"]
        assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*")
                      if p.suffix.lower() in (".fit", ".fits", ".fts")) == sorted(
                          e["file"].replace("\\", "/") for e in manifest["files"])
        integrity += "; all 1140 DS-A source hashes and input inventory unchanged"
    assert after["intermediate_images_written"] == 0
    assert not list(args.after.parent.glob("*.tmp"))
    assert all(k not in args.after.read_text(encoding="utf-8") for k in ("SITELAT", "SITELONG"))
    times = {}
    for label, r in (("before", before), ("after", after)):
        times[label] = {"processing_seconds": r["wall_seconds"],
                        "reconstruction_seconds": r["integration"]["reconstruction_wall_seconds"],
                        "rejection_accumulation_maps_seconds": r["integration"]["reduction_wall_seconds"],
                        "peak_process_MiB": r["peak_rss_bytes"] / 1048576}
    result = {"status": "PASS", "gate": "exact entire master/map FITS byte equality; no tolerance",
              "original_frames": after["registration"]["frame_count"],
              "accepted_frames": after["registration"]["accepted_count"],
              "rejected_frames": after["registration"]["rejected_count"],
              "dimensions": [after["width"], after["height"]],
              "tile_geometry_change_allowed": args.allow_tile_geometry_change,
              "tile_work": {label: {key: r["integration"][key] for key in
                                    ("tile_width", "tile_height", "tile_count", "planned_buffer_bytes",
                                     "decoded_roi_pixels", "corrected_roi_pixels")}
                            for label, r in (("before", before), ("after", after))},
              "scientific_reports": "PASS; registration, quality, photometry, scientific settings, export and sample/rejection counts unchanged; tile configuration/work differences permitted only when explicitly requested",
              "integrity": integrity, "products": products, "telemetry": times,
              "processing_speedup": before["wall_seconds"] / after["wall_seconds"],
              "rejection_phase_speedup": before["integration"]["reduction_wall_seconds"] / after["integration"]["reduction_wall_seconds"]}
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, indent=2)
    print(json.dumps({k: v for k, v in result.items() if k != "products"}, indent=2))


if __name__ == "__main__":
    main()
