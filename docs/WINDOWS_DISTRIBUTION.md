# Windows distribution package

Current as of 2026-11-09. See [development](DEVELOPMENT.md), [current state](CURRENT_STATUS.md) and ADR-089.

## What the package is

The package is a portable ZIP named `FulgurStack-<version>-<commit>-windows-x64.zip`. It contains one folder with these files:

| File | Content |
| --- | --- |
| `fulgurstack-gui.exe` | Desktop application, GPU feature compiled in (GPU use stays an explicit choice) |
| `fulgurstack.exe` | CLI with the same engine and the GPU feature |
| `fulgur-solve.exe` | Blind plate solver (index build, index info, solve frames); needs the Gaia DR3 files for the index |
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
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p fulgur-gui -p fulgur-cli -p fulgur-solve-cli --features gpu --bins --target-dir target/dist
```

Then check the result:

1. **No CRT DLLs.** The executables must import only system DLLs, and no `VCRUNTIME*`, `MSVCP*` or `api-ms-win-crt-*`. Read the PE import table (a short Python reader is enough; no tool is installed for this).
2. **Exact science.** Stack DS-B with `target/dist/release/fulgurstack.exe stack --input "SeestarS50 Subs" --tile-size 512 …` into a new output folder. Compare the master, coverage map and high-rejection map byte for byte with the same run of the standard release build. `crt-static` must not change a single pixel.
3. **Start check.** Start the GUI from the package folder and confirm that it is still running after a few seconds. Then close only that process.

Then assemble the package:

1. Put the three executables, `CTI_ME.txt` and the guide PDF into `outputs/dist/FulgurStack-<version>-<commit>-windows-x64/`.
2. Update `CTI_ME.txt`: version, commit, date, news since the previous package, CLI examples checked against `--help`, and the SHA-256 of the executables.
3. Create the ZIP with `Compress-Archive` (PowerShell), refusing to overwrite an existing ZIP.
4. Record the ZIP's SHA-256.

`outputs/` and `target/` are excluded from Git, so packages are never committed. Publishing a package (for example as a GitHub release) needs the owner's explicit authorization, like any other publication.

## Current package

| Item | Value |
| --- | --- |
| ZIP | `outputs/dist/FulgurStack-0.13.0-9b7feab-windows-x64.zip`, 12.8 MB |
| ZIP SHA-256 | `120de73b2c56b0df03896fa3d391bac405b9b943fdd519ea7cc8cff45390840f` |
| `fulgurstack-gui.exe` | 17.1 MB, SHA-256 `290749357fb1b5f5cf3503baac072e5587c27cebe73a72395b3f9ece5a268aec` |
| `fulgurstack.exe` | 11.7 MB, SHA-256 `4c9218500c41f2d00a0dbdb31e5479b7a5da2035732bda87d34d4da5e8cacf01` |
| Checks (2026-11-08) | No CRT DLL imports: PASS. DS-B Auto (ESD) 27/32 frames in 5.1 s; master, coverage and high-rejection maps byte-identical to the standard build: PASS. GUI start: PASS. ZIP content and version: PASS |

The guide PDF is carried over from package 0.12 (it does not describe the 0.13 changes; `CTI_ME.txt` does). The previous package `FulgurStack-0.12.0-4d62d72-windows-x64.zip` (2026-11-01) is superseded. Only Windows 11 x64 was tested.
