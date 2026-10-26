"""Exact product/science verification for the frozen-plan integration profiler.

Permits only measured CPU/I/O telemetry, accounted worker counts/read-ahead memory
and OS RAM snapshots. Never relaxes pixel, selection or rejection comparisons.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("before", "after", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args()
    assert not args.output.exists()
    before = json.loads(args.before.read_text(encoding="utf-8"))
    after = json.loads(args.after.read_text(encoding="utf-8"))
    for field in ("scope", "original_frames", "accepted_frames", "width", "height", "origin", "config", "export", "raw_float_sha256", "source_metadata_unchanged"):
        assert before[field] == after[field], field
    spec = importlib.util.spec_from_file_location("gates", Path(__file__).with_name("verify-stack-equivalence.py"))
    gates = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gates)
    stats = []
    for r in (before, after):
        s = gates.science(r["integration"])
        workers = min(r["threads"], 24, r["accepted_frames"])
        assert s.pop("max_concurrent_rois") == workers
        assert s.pop("max_concurrent_reduction_bands") == min(r["threads"], 24, r["height"])
        assert s.pop("rejection_packing_buffer_bytes") == 32 * min(r["threads"], 24) * r["accepted_frames"] * 12
        plan = s.pop("memory_plan")
        planned = s.pop("planned_buffer_bytes")
        assert planned == plan["estimated_buffer_bytes"] == plan["integration_buffer_bytes"]
        assert planned <= plan["effective_budget_bytes"] <= plan["configured_budget_bytes"] <= 12288 * 1024**2
        assert plan["fits_cache_capacity_bytes"] == plan["fits_cache_transient_bytes"] == 0
        assert r["integration"]["peak_rss_bytes"] <= 12288 * 1024**2
        cache = s["fits_cache"]
        assert not cache["enabled"] and cache["capacity_accounted_bytes"] == 0
        assert cache.pop("read_calls") > 0
        assert cache.pop("source_bytes_read") >= cache["requested_roi_bytes"]
        stats.append(s)
    assert stats[0] == stats[1], "Scientific/reconstruction/selection/rejection counters must stay exact"
    products = {}
    for name in ("master.fits", "coverage.fits", "high_rejection.fits"):
        a, b = digest(args.before.parent / name), digest(args.after.parent / name)
        assert a == b, name
        products[name] = a
    summary = {"status": "PASS", "scope": after["scope"], "original_frames": after["original_frames"], "accepted_frames": after["accepted_frames"], "before_threads": before["threads"], "after_threads": after["threads"], "entire_master_maps_exact": True, "raw_floats_exact": True, "scientific_counters_exact": True, "products": products}
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(summary, stream, indent=2)
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
