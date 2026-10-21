# Development Environment Setup

## Current Setup Check (2026-10-21)

- Project directory: `C:\Users\ASUS\Documents\GitHub\FulgurStack`.
- Git for Windows is available: `2.53.0.windows.1`.
- Microsoft Visual Studio Build Tools with the x64/x86 C++ toolset were detected. These are required by the Windows Rust toolchain, not an external C/C++ runtime dependency introduced by FulgurStack.
- Rust, Cargo, and rustup were not found on PATH or in the standard user installation directory.
- No Cargo workspace exists yet. Application implementation and executable verification belong to M1.
- The existing project directory has been initialized as a local Git repository on branch `main`, with nine documentation/configuration files selected for the initial commit and real image data excluded.
- GitHub owner selection, remote repository creation, and remote connection are pending. A connected GitHub account and a local Git author signature can differ; choose the owner explicitly rather than deriving it from the author signature.

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

Validate the Windows linker and SDK with a release build as part of M1. Detection of Visual Studio Build Tools alone is not a successful build check. Do not install GUI or GPU dependencies during M1-M4.

## 5. Verification After M1 Workspace Creation

These commands become valid after the Rust workspace is created:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release
cargo build --release
```

Real-data validation uses local dataset paths and release builds. See `BENCHMARKS.md` for DS-A and DS-B. Cross-platform CI should run the required checks on Windows and macOS once M1 provides a runnable workspace; do not claim these checks pass during documentation-only setup.
