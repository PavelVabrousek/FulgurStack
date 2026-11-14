# Archived experiments

These three programs were feasibility studies. Their work now lives in FulgurStack itself, so they are kept here for reference only (ADR-180). They are not maintained, no script builds them, and they may not compile against the current crates.

| Program | Study | Where it lives now |
| --- | --- | --- |
| `skyreg` | Universal sky registration of mixed cameras (ADR-131) | `--registration-engine sky` in the CLI and Sky registration in the application (ADR-137, ADR-138) |
| `synthref` | Synthetic Gaia reference frame (ADR-127) | Sky registration; the plate solve and Gaia access in `fulgur-pipeline::sky` and `fulgur-catalog` |
| `blindspike` | Blind plate solving (ADR-168) | `fulgur-solve` and the `fulgur-solve` program |

`blindspike --diag` (detections of one frame and which of them are catalogue stars) has no product counterpart; it remains here.

To build one of them, check out the last version they were built with, 0.14 (commit `7317e67`), for example:

```powershell
git worktree add ..\FulgurStack-0.14 7317e67
cargo build --release --manifest-path ..\FulgurStack-0.14\tools\blindspike\Cargo.toml
```
