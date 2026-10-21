# Development Environment Setup

## Current Setup Check (2026-10-21)

- Project directory: `C:\Users\ASUS\Documents\GitHub\FulgurStack`.
- Git for Windows is available: `2.53.0.windows.1`.
- Microsoft Visual Studio Build Tools with the x64/x86 C++ toolset were detected. These are required by the Windows Rust toolchain, not an external C/C++ runtime dependency introduced by FulgurStack.
- Rust and Cargo `1.99.1` are installed through rustup with the active `stable-x86_64-pc-windows-msvc` toolchain. The installed components include `rustfmt` and `clippy`.
- The M1 Cargo workspace contains `fulgur-core`, `fulgur-pipeline`, and `fulgur-cli`, providing the `fulgurstack` binary. M1 implementation and review evidence are documented in `M1_REVIEW.md`.
- The existing project directory is a local Git repository; `main` tracks GitHub and M1 work is isolated on `m1-fits-debayer`. Real image data and generated outputs remain excluded.
- The owner selected `PavelVabrousek` and created [PavelVabrousek/FulgurStack](https://github.com/PavelVabrousek/FulgurStack). Local `origin` points to `https://github.com/PavelVabrousek/FulgurStack.git`; the initial push succeeded and `main` tracks `origin/main`.
- GitHub access through the local Git credential setup was verified. The connected GitHub app returned HTTP 404 for this repository at setup time; app-level repository access must be checked separately if a later workflow requires connector features.
- Repository setup and local Windows toolchain validation are complete. The M1 application passed release checks and real FITS export. M1 product acceptance remains pending owner visual review; M2 has not started.

### Verified Environment Gates

On 2026-10-21, an isolated, dependency-free scratch package under the ignored `target/environment-check/` directory passed:

- `cargo fmt --all -- --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test --workspace --release`: the harness compiled and ran successfully, with zero product tests because the scratch package contains none.
- `cargo run --release`: printed `FulgurStack environment check: PASS (windows / x86_64)`.
- DS-B integrity verification: all 34 source/reference files matched the recorded sizes and SHA-256 fingerprints.

Cargo checks used the scratch package's explicit manifest path and offline mode where applicable. Scratch source and build files are ignored by Git and are not the FulgurStack workspace. These results establish local environment readiness only, not an M1 acceptance gate or macOS compatibility. No product performance benchmark has been run.

## 1. Local Repository

Initialize Git in the existing project directory on branch `main`. Do not clone another copy over this directory and do not move the observing data.

Before the first commit, check that `.gitignore` excludes real FITS/XISF files, both local reference directories, build outputs, and credentials. Review the exact files staged for the first commit. Use the existing Git author identity unless the owner requests a change; do not change global Git configuration as part of project setup.

## 2. Empty GitHub Repository

Create an empty repository named `FulgurStack` under the chosen owner. Private visibility is the suggested initial setting; the owner selects visibility. Suggested English description:

```text
CPU-first astrophotography stacking for Seestar FITS exposures.
```

Do not initialize the GitHub repository with a README, `.gitignore`, or license: the local project already contains initial files. A software license is a separate owner decision and is not selected automatically.

Copy the new repository's HTTPS URL for the local remote connection. Do not share access tokens or passwords in chat. If Git prompts for authentication, complete it in the normal browser sign-in flow.

## 3. Connect and Upload

Once the repository URL is known, inspect the remote, connect it as `origin`, create the reviewed initial commit, and push `main` with upstream tracking. Do not force-push or overwrite an existing remote history. If the remote already contains commits, inspect and reconcile it before uploading.

After uploading, verify the remote branch and confirm that only documentation and repository configuration were uploaded. Dataset directories and real images must remain excluded.

Official GitHub guidance: [Adding locally hosted code to GitHub](https://docs.github.com/en/migrations/importing-source-code/using-the-command-line-to-import-source-code/adding-locally-hosted-code-to-github).

## 4. Windows Rust Toolchain

After repository connection, install Rust with the official [Windows rustup installer](https://rust-lang.org/tools/install/), selecting the x64 MSVC toolchain. Reopen the terminal after installation and check:

```powershell
rustup show
rustc --version
cargo --version
```

Confirm the required formatting and lint components are present:

```powershell
rustup component add rustfmt clippy
```

The local Windows linker and SDK have passed the isolated release smoke build described above. Repeat validation with the actual application workspace in M1. Do not install GUI or GPU dependencies during M1-M4.

For project commands, open PowerShell in the project directory and optionally select the small dataset for the current session:

```powershell
Set-Location -LiteralPath "C:\Users\ASUS\Documents\GitHub\FulgurStack"
$env:FULGURSTACK_TEST_FITS_DIR = Join-Path (Get-Location) "SeestarS50 Subs"
$env:FULGURSTACK_TEST_FITS = Join-Path $env:FULGURSTACK_TEST_FITS_DIR "Light_M 33_10.0s_IRCUT_20261006-204534.fit"
```

These environment variables apply to the current PowerShell session only. `FULGURSTACK_TEST_FITS` enables the optional immutable real-input decoding test; use `survey` for the dataset I/O gates. Select DS-A explicitly for the required full-dataset gates. Existing shells or apps may need restarting to inherit the Rust installer's PATH update.

## 5. Workspace Verification

Run from the project root:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release
cargo build --release
```

Real-data validation uses local dataset paths and release builds. See `BENCHMARKS.md` for DS-A and DS-B. The `Rust checks` workflow runs formatting, lint, release tests, and release build on Windows x86_64 and macOS Apple Silicon. Synthetic FITS fixtures are generated during tests; private real datasets are never uploaded to CI. Treat a platform as verified only after its workflow passes.
