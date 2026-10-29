# Windows distribution package

Current as of 2026-10-30. See [development](DEVELOPMENT.md), [current state](CURRENT_STATUS.md) and ADR-089.

## What the package is

The package is a portable ZIP named `FulgurStack-<version>-<commit>-windows-x64.zip`. It contains one folder with three files:

| File | Content |
| --- | --- |
| `fulgurstack-gui.exe` | Desktop application, GPU feature compiled in (GPU use stays an explicit choice) |
| `fulgurstack.exe` | CLI with the same engine and the GPU feature |
| `CTI_ME.txt` | Owner-language guide: contents, news since the previous package, installation, requirements, data and privacy, quick start (GUI and CLI), SHA-256 of both executables and status |

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

1. Put the two executables and `CTI_ME.txt` into `outputs/dist/FulgurStack-<version>-<commit>-windows-x64/`.
2. Update `CTI_ME.txt`: version, commit, date, news since the previous package, CLI examples checked against `--help`, and the SHA-256 of both executables.
3. Create the ZIP with `Compress-Archive` (PowerShell), refusing to overwrite an existing ZIP.
4. Record the ZIP's SHA-256.

`outputs/` and `target/` are excluded from Git, so packages are never committed. Publishing a package (for example as a GitHub release) needs the owner's explicit authorization, like any other publication.

## Current package

| Item | Value |
| --- | --- |
| ZIP | `outputs/dist/FulgurStack-0.1.0-97caf3b-windows-x64.zip`, 8.5 MB |
| ZIP SHA-256 | `c4a8aeeedc023f438696c764d1fca6f92d23269d2995ce9a416b6c59013cb104` |
| `fulgurstack-gui.exe` | 12.8 MB, SHA-256 `9fb41a26ed2c5e38cf52ab25247b455c03658ba7b53c5602f502f37a9b400185` |
| `fulgurstack.exe` | 7.5 MB, SHA-256 `1b1d397f5bc01d6ac14aae89f94a98971f3b75d49d1c986bad862f475437eb7c` |
| Checks (2026-10-30) | No CRT DLL imports: PASS. DS-B Auto (ESD) 27/32 frames in 8.6 s, peak 835 MiB; master and maps byte-identical to the standard build: PASS. GUI start: PASS |

The previous package `FulgurStack-0.1.0-ef373b2-windows-x64.zip` (2026-10-29) is superseded. Only Windows 11 x64 was tested; no other Windows version and no machine without a Rust/MSVC toolchain was tried.
