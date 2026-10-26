"""Read-only catalog diagnosis of an existing external-reference stack.

Uses fixed star identities shared through the native photometric anchor, rather
than rematching output predictions with a residual cutoff. No FITS is modified.
This is diagnostic evidence, not a replacement registration acceptance gate.
"""
import argparse
import collections
import hashlib
import json
import math
from pathlib import Path

import numpy as np


def pose(t, points):
    s, c = math.sin(t["theta_rad"]), math.cos(t["theta_rad"])
    return points @ np.array([[c, s], [-s, c]]) * t["scale_factor"] + [t["tx_px"], t["ty_px"]]


def warp(model, points):
    x, y = ((points - model["center"]) / model["normalization"]).T
    b = np.column_stack([np.ones(len(x)), x, y, x*y, x*x, y*y, x**3, x*x*y, x*y*y, y**3])
    return pose(model["base"], points) + b @ np.array(model["coefficients"]).T


def distribution(values):
    a = np.asarray(values)
    if not len(a):
        return {"count": 0}
    return {"count": len(a), "median_px": float(np.median(a)), "p95_px": float(np.percentile(a, 95)),
            "max_px": float(a.max()), "rmse_px": float(np.sqrt(np.mean(a*a))),
            "over_1px": int((a > 1).sum()), "over_3px": int((a > 3).sum()), "over_5px": int((a > 5).sum())}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("report", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    data = json.loads(args.report.read_text(encoding="utf-8-sig"))
    reg = data["registration"]
    ext = reg["external_reference"]
    targets = np.array([[s["x_px"], s["y_px"]] for s in ext["stars"]])
    anchor_labels = {m["source"]: m["reference"] for m in ext["anchor_matches"]}
    groups = collections.defaultdict(lambda: {"frames": 0, "weight": 0., "residuals": [], "frame_rmse": [], "reported_rmse": []})
    reason_counts = collections.Counter()
    reason_weights = collections.Counter()
    star_groups = collections.defaultdict(lambda: collections.defaultdict(list))
    for row, source in zip(reg["frames"], reg["inspection"]["frames"]):
        assert row["file_name"] == source["file_name"]
        if not row["accepted"]:
            continue
        decision = row.get("geometric_correction") or {}
        model = decision.get("model")
        group = "cubic" if model else "similarity"
        stats = groups[group]
        stats["frames"] += 1
        stats["weight"] += row["final_weight"]
        stats["reported_rmse"].append(row["inlier_rmse_px"])
        reason_counts[decision.get("reason")] += 1
        reason_weights[decision.get("reason")] += row["final_weight"]
        assert row["photometric_reference"] == "direct_master_matches"
        pairs = [m for m in row["photometric_matches"] if m["reference"] in anchor_labels]
        points = np.array([[source["stars"][m["source"]]["x_px"], source["stars"][m["source"]]["y_px"]] for m in pairs])
        if not len(points):
            continue
        labels = [anchor_labels[m["reference"]] for m in pairs]
        predicted = warp(model, points) if model else pose(row["transform"], points)
        residuals = np.linalg.norm(predicted - targets[labels], axis=1)
        stats["residuals"].extend(residuals)
        stats["frame_rmse"].append(float(np.sqrt(np.mean(residuals**2))))
        for label, prediction in zip(labels, predicted):
            star_groups[label][group].append(prediction)
    total_weight = sum(v["weight"] for v in groups.values())
    result = {"scope": "Stored catalogs only; fixed source/native-anchor/external star identities; no output residual cutoff or nearest-star rematching. Anchor bridge identities are a diagnostic reference, not independent ground truth for the cubic group.",
              "report_sha256": hashlib.file_digest(args.report.open("rb"), "sha256").hexdigest(),
              "originals": reg["frame_count"], "accepted": reg["accepted_count"], "quality_excluded": reg["frame_count"]-reg["quality_accepted_count"],
              "anchor_cubic": ext.get("anchor_correction") is not None, "anchor_pairs": len(anchor_labels),
              "reasons": dict(reason_counts), "reason_weights": dict(reason_weights), "groups": {}}
    for name, stats in groups.items():
        result["groups"][name] = {"frames": stats["frames"], "weight_fraction": stats["weight"]/total_weight,
            "fixed_identity_residual": distribution(stats["residuals"]), "frame_rmse_distribution": distribution(stats["frame_rmse"]),
            "reported_inlier_rmse_distribution": distribution(stats["reported_rmse"])}
    separation = []
    for label, groups_at_star in star_groups.items():
        if min(len(groups_at_star["cubic"]), len(groups_at_star["similarity"])) < 5:
            continue
        a = np.median(groups_at_star["cubic"], axis=0)
        b = np.median(groups_at_star["similarity"], axis=0)
        separation.append(float(np.linalg.norm(a-b)))
    result["median_group_positions_separation_at_common_stars"] = distribution(separation)
    result["measured_stack_seconds"] = data["wall_seconds"]
    result["peak_rss_bytes"] = data["peak_rss_bytes"]
    with args.output.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(result, stream, indent=2)
        stream.write("\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
