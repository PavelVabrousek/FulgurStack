# Development and local verification

Current as of 2026-10-30. See [current state](CURRENT_STATUS.md), [documentation index](README.md) and [engineering contract](../AGENTS.md).

## Repository and workspace

Use the existing repository at `C:\Users\ASUS\Documents\GitHub\FulgurStack`; do not initialize or clone over observing data. `origin` is `https://github.com/PavelVabrousek/FulgurStack.git`. The owner authorizes each publication batch explicitly (latest: ADR-088, 2026-10-30). Future commits/pushes still require authorization. Never force-push or overwrite remote history.

Workspace members are `fulgur-core`, `fulgur-pipeline`, `fulgur-cli`, `fulgur-gpu` and `fulgur-gui`; CLI is the default member. CPU is default; GPU is an optional feature and explicit runtime choice. No external C/C++ runtime libraries are required. MSVC/Windows SDK are build prerequisites; GUI uses the SDK resource compiler for its embedded icon/version metadata.

## Windows prerequisites and launch

Use Rust stable for `x86_64-pc-windows-msvc`, Visual Studio Build Tools with x64/x86 toolset and Windows SDK. Check the actual environment rather than treating the original setup's tool versions as a current requirement:

```powershell
rustup show
rustc --version
cargo --version
rustup component add rustfmt clippy
```

From the repository root:

```powershell
cargo build --release -p fulgur-gui -p fulgur-cli --features gpu --bins
cargo run --release -p fulgur-gui --features gpu --bin fulgurstack-gui
```

Current local executables are `target/release/fulgurstack-gui.exe` and `target/release/fulgurstack.exe`. The desktop shortcut points to the GUI and its icon. Earlier versioned `target/local-release` paths are historical. The portable package for other computers is built separately with a static Visual C++ runtime into `target/dist`; see [Windows distribution](WINDOWS_DISTRIBUTION.md). A running Windows executable may block Cargo replacement: preserve the owner's running session; do not terminate it to publish a build. Native appearance/DPI review remains manual.

Compact Desk targets 1366×768 physical pixels at 125%, opens centered in a normal window and supports maximizing. Settings persist under `%LOCALAPPDATA%/FulgurStack/settings.json`; startup does not run a job. Input arguments replace restored inputs. Quality catalogs/manual row decisions remain session-local.

## Required local engine gates

All verification runs on the owner's machine, in release mode for tests/benchmarks. **GitHub Actions remain disabled.** Do not enable, trigger, poll or wait for workflows. Retained workflow files are historical; do not modify them during publication. Windows results do not establish macOS verification.

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --release
cargo test --workspace --release --all-features
```

Enable real input/hardware gates in the same session before an engine delivery:

```powershell
$env:FULGURSTACK_TEST_FITS_DIR = Join-Path (Get-Location) "SeestarS50 Subs"
$env:FULGURSTACK_TEST_FITS = Join-Path $env:FULGURSTACK_TEST_FITS_DIR "Light_M 33_10.0s_IRCUT_20261006-204534.fit"
$env:FULGURSTACK_TEST_SIMD = "1"
$env:FULGURSTACK_TEST_GPU = "1"
```

Investigate required hardware failures; never silently skip or relabel them. Latest engine gates passed 210 default / 213 all-feature tests with real FITS, AVX2 and GPU (ADR-087). Private logs are under `outputs/rejection_20261029/`; sanitized [evidence](M5_MASTER_QUALITY_AND_REJECTION_EVIDENCE.json) is versioned.

Light sets and sensor layout from the CLI: `--sensor auto|mono|RGGB|GRBG|GBRG|BGGR` declares the layout (auto never assumes mono when `BAYERPAT` is missing), and `--input-sets` makes each `--input` its own light set. `--rejection auto|winsorized|linear-fit|esd|median` selects pixel rejection; `--accept-derived` and `--exposure` confirm or correct reconstructed header values.

For **UI-only** iterations the owner's exception remains: prepare release for manual review; skip automatic functional/scientific tests and benchmarks. Documentation publication reuses completed engine gates; CLI help is checked against release `--help`. Publishing does not imply new scientific or owner visual acceptance.

## Verification helpers and timing

Node.js 18+ is only needed for `node --test scripts/tests/open-fulgurstack.test.cjs`, the PixInsight adapter contract tests. Host doubles do not verify native PixInsight behavior. Python tools in `scripts/tests` are developer utilities; the geometric-mixture analyzer additionally needs NumPy. Neither Node, Python nor PixInsight is a production Rust runtime dependency.

`verify-stack-equivalence.py` validates exact scientific report/product comparisons and documented timer scopes. Native outward walks overlap: task durations are not additive wall phases. Auto preliminary/scoring timings are nested within registration; a reused pass also appears in the native profile. Frozen integration profiling refuses nonlinear plans rather than dropping their mappings. See [native timing](M5_NATIVE_REGISTRATION_WALKS_REVIEW.md), [Auto scope](M5_AUTO_REFERENCE_REVIEW.md) and [integration profile](M5_INTEGRATION_PROFILE_REVIEW.md).

## Data and benchmark discipline

Never modify/move/overwrite input FITS or supplied PixInsight artifacts. Do not write intermediate images. Use new output names outside source/reference folders. Hash inputs and preserve sizes/mtimes when checking integrity. GPS stays out of default reports/console output. Real images, raw telemetry, preferences, binaries, temporary files and bytecode stay excluded.

Reuse stored comparable performance reports/products for routine controls; do not rerun the full reference stack by default. Full pipeline, frozen integration replay, native catalog replay and artificial-date Auto smoke have distinct scopes. See [benchmark rules](BENCHMARKS.md). Never report debug-build performance. M1–M4 remain accepted; M5, real multi-night validation, latest native visual/PixInsight review and macOS remain open.
