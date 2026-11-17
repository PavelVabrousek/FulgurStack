# Windows distribution package

Current as of 2026-11-17. See [development](DEVELOPMENT.md), [current state](CURRENT_STATUS.md) and ADR-089.

## What the package is

The package is a portable ZIP named `FulgurStack-<version>-<commit>-windows-x64.zip`. It contains one folder with these files:

| File | Content |
| --- | --- |
| `fulgurstack-gui.exe` | Desktop application, GPU feature compiled in (GPU use stays an explicit choice) |
| `fulgurstack.exe` | CLI with the same engine and the GPU feature; since ADR-181 it also holds blind plate solving (`solve`, `solve-index`), which package 0.14 shipped as `fulgur-solve.exe` |
| `CTI_ME.txt` | Owner-language guide: contents, news since the previous package, installation, requirements, data and privacy, quick start (GUI and CLI), SHA-256 of the executables and status |
| `FulgurStack_navod_pro_alfa_testery.pdf` | Czech guide for alpha testers with screenshots from real runs |

There is no installer. The user unpacks the ZIP and runs `fulgurstack-gui.exe`. Both executables link the Visual C++ runtime statically, so nothing else has to be installed. Requirements:
- Windows 10/11 x64;
- OpenGL 2.0 for the GUI;
- DirectX 12 or Vulkan only for the optional GPU engine.

AVX2 is detected at run time. Settings are stored in `%LOCALAPPDATA%\FulgurStack`. The executables are not code-signed, so SmartScreen may ask for confirmation on first start.

## Building a package

Build only from a committed, clean `main`, so that the commit in the name identifies the sources. Run the commands from the repository root:

```bash
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p fulgur-gui -p fulgur-cli --features gpu --bins --target-dir target/dist
```

Then check the result:

1. **No CRT DLLs.** The executables must import only system DLLs, and no `VCRUNTIME*`, `MSVCP*` or `api-ms-win-crt-*`. Read the PE import table (a short Python reader is enough; no tool is installed for this).
2. **Exact science.** Stack DS-B with `target/dist/release/fulgurstack.exe stack --input "SeestarS50 Subs" --tile-size 512 …` into a new output folder. Compare the master, coverage map and high-rejection map byte for byte with the same run of the standard release build. `crt-static` must not change a single pixel.
3. **Start check.** Start the GUI from the package folder and confirm that it is still running after a few seconds. Then close only that process.

Then assemble the package:

1. Put the two executables, `CTI_ME.txt` and the guide PDF into `outputs/dist/FulgurStack-<version>-<commit>-windows-x64/`.
2. Update `CTI_ME.txt`: version, commit, date, news since the previous package, CLI examples checked against `--help`, and the SHA-256 of the executables.
3. Create the ZIP with `Compress-Archive` (PowerShell), refusing to overwrite an existing ZIP.
4. Record the ZIP's SHA-256.

`outputs/` and `target/` are excluded from Git, so packages are never committed. Publishing a package (for example as a GitHub release) needs the owner's explicit authorization, like any other publication.

## Current package

| Item | Value |
| --- | --- |
| ZIP | `outputs/dist/FulgurStack-0.15.0-3736674-windows-x64.zip`, 13.2 MB; also on the GitHub pre-release `v0.15.0` |
| ZIP SHA-256 | `c129b389dc342b4e174f835af07ab40a1b074eba680969e7ab0c7bbe906af33f` |
| `fulgurstack-gui.exe` | 17.6 MB, SHA-256 `d7b1b1cba9df0498c680df4090b1908fb181e0eccd94621219a41edc1a430a2d` |
| `fulgurstack.exe` | 12.0 MB, SHA-256 `f238e27e31047a567356a5fb8b8c07ecb01de27a839e21a26c38f7d4848ade78` |
| Checks (2026-11-17) | Release suites PASS: default features 363 passed, all features 367 passed (3 ignored each), format and Clippy PASS. No CRT DLL imports in the two executables: PASS. DS-B (`--tile-size 512`) 27/32 frames in 4.0 s in both builds, peak RAM about 1.1 GB; master, coverage and high-rejection maps byte-identical to the standard build (SHA-256 prefixes 2612AB2A0FE39A79, CCFE7F2E42EEF05B, C7F5F8504B8272D7), with the median DATE-OBS/DATE-AVG of 27 frames in the master and maps: PASS. `fulgurstack.exe --version` reports 0.15.0; the CLI examples in `CTI_ME.txt` were checked against `--help`. GUI start from the package folder: PASS. |

The package was built from commit 3736674 (the release commit); the commit that records this table follows it. It holds two executables: blind plate solving is part of `fulgurstack.exe` since ADR-181, so `fulgur-solve.exe` of package 0.14 is gone. The guide PDF is carried over from package 0.12 (it does not describe the changes since 0.13; `CTI_ME.txt` and the About page do).
