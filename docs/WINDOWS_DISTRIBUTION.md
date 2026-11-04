# Windows distribution package

Current as of 2026-11-01. See [development](DEVELOPMENT.md), [current state](CURRENT_STATUS.md) and ADR-089.

## What the package is

The package is a portable ZIP named `FulgurStack-<version>-<commit>-windows-x64.zip`. It contains one folder with these files:

| File | Content |
| --- | --- |
| `fulgurstack-gui.exe` | Desktop application, GPU feature compiled in (GPU use stays an explicit choice) |
| `fulgurstack.exe` | CLI with the same engine and the GPU feature |
| `CTI_ME.txt` | Owner-language guide: contents, news since the previous package, installation, requirements, data and privacy, quick start (GUI and CLI), SHA-256 of both executables and status |
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
2. Update `CTI_ME.txt`: version, commit, date, news since the previous package, CLI examples checked against `--help`, and the SHA-256 of both executables.
3. Create the ZIP with `Compress-Archive` (PowerShell), refusing to overwrite an existing ZIP.
4. Record the ZIP's SHA-256.

`outputs/` and `target/` are excluded from Git, so packages are never committed. Publishing a package (for example as a GitHub release) needs the owner's explicit authorization, like any other publication.

## Current package

| Item | Value |
| --- | --- |
| ZIP | `outputs/dist/FulgurStack-0.12.0-4d62d72-windows-x64.zip`, 10.3 MB |
| ZIP SHA-256 | `d148d4967741dbcdc9592368410a98605a1ef1ec3e8f25293e7017ee19eb6e1c` |
| `fulgurstack-gui.exe` | 14.5 MB, SHA-256 `15e006f8a6e29033788304143c9c7001301faef4628d0e71caa1469fcddf5e48` |
| `fulgurstack.exe` | 8.7 MB, SHA-256 `46970c83c5dfca6d1952c79d43b249a7366c14a4c2a98279db5f37d9eadb717b` |
| Checks (2026-11-01) | No CRT DLL imports: PASS. DS-B Auto (ESD) 27/32 frames in 4.7 s; master and maps byte-identical to the standard build: PASS. GUI start: PASS. ZIP content and version: PASS |

The previous package `FulgurStack-0.1.0-e2fb4fd-windows-x64.zip` (2026-10-31) is superseded. Only Windows 11 x64 was tested; no other Windows version and no machine without a Rust/MSVC toolchain was tried.
