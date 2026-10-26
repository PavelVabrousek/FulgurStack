"""Verify a native GUI stack against stored accepted CLI products; no baseline run.

Read-only FITS/hash access, standard library only. Output JSON must be new.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("before", "after", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    assert not args.output.exists()
    spec = importlib.util.spec_from_file_location(
        "equivalence", Path(__file__).with_name("verify-stack-equivalence.py"))
    gates = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gates)
    before, after = load(args.before), load(args.after)
    assert after["frontend"] == "windows_gui"
    assert gates.registration_science(before["registration"]) == gates.registration_science(after["registration"])
    old_config, new_config = before["config"].copy(), after["config"].copy()
    assert old_config.pop("tile_mode") == "manual"
    assert new_config.pop("tile_mode") == "auto"
    assert old_config == new_config
    old, new = gates.science(before["integration"]), gates.science(after["integration"])
    for stats in (old, new):
        if stats.get("backend") == "cpu" and stats.get("gpu") is None:
            stats.pop("gpu", None)
        plan = stats["memory_plan"]
        assert plan["selected_width"] == plan["selected_height"] == 512
        assert plan["fits_cache_capacity_bytes"] == 0
        assert plan.pop("mode") in ("manual", "auto")
        plan.pop("decision")
        plan["system_memory"].pop("available_bytes")
        plan.pop("effective_budget_bytes")
        for candidate in plan["candidates"]:
            assert candidate.pop("fits_budget") is True
    assert old == new, "Only tile-selection policy and OS RAM availability may change"
    for key in ("width", "height", "origin_x_px", "origin_y_px", "canvas_mode",
                "map_denominator_frames", "export", "output_units", "output_bitpix",
                "bias_subtracted", "gps_copied_to_fits", "intermediate_images_written",
                "coverage_definition", "high_rejection_definition"):
        assert before[key] == after[key], key
    products = []
    for field in ("output_file", "coverage_file", "high_rejection_file"):
        old_path, new_path = args.before.parent / before[field], args.after.parent / after[field]
        old_hash, new_hash = digest(old_path), digest(new_path)
        assert old_hash == new_hash and old_path.stat().st_size == new_path.stat().st_size, field
        products.append({"file": after[field], "sha256": new_hash, "exact_entire_FITS_equality": True})
    sources = load("docs/ds-b-manifest.json")
    for entry in sources["sources"] + sources["references"]:
        assert Path(entry["path"]).stat().st_size == entry["size_bytes"]
        assert digest(entry["path"]) == entry["sha256"]
    assert digest("RegisteringTemplate200pct.fit") == "3d1f0c73f04749391d18f7764912eac07f642589ac4a401f27eb1f9c3a6f6d61"
    assert not list(args.after.parent.glob("*.tmp"))
    assert not any(card in args.after.read_text() for card in ("SITELAT", "SITELONG"))
    result = {"status": "PASS", "original_frames": after["registration"]["frame_count"],
              "accepted_frames": after["registration"]["accepted_count"],
              "rejected_frames": after["registration"]["rejected_count"],
              "dimensions": [after["width"], after["height"]],
              "source_and_reference_integrity": "PASS; all saved source/PI/template hashes unchanged",
              "scientific_reports": "PASS; exact registration/quality/photometry/rejection/export/work fields",
              "permitted_differences": "frontend, processing/review timing, RSS and auto-vs-manual tile policy/RAM snapshot only",
              "processing_seconds": after["wall_seconds"], "review_wait_seconds": after["review_wait_seconds"],
              "peak_process_MiB": after["peak_rss_bytes"] / 1048576,
              "products": products, "baseline_rerun": False, "full_dataset_run": False}
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, indent=2)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
