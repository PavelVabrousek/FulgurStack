# Windows distribution package

Current as of 2026-11-09. See [development](DEVELOPMENT.md), [current state](CURRENT_STATUS.md) and ADR-089.

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
| ZIP | `outputs/dist/FulgurStack-0.14.0-6cb9167-windows-x64.zip`, 13.8 MB |
| ZIP SHA-256 | `c30553e8f9a90fb4e2958eb5964dfe534f04a9e105f85bf0e21bd1f8b6343389` |
| `fulgurstack-gui.exe` | 17.5 MB, SHA-256 `6b77494fbb159f3971d8961e5351a35f79d69f198c386332e9ac88b4f1406e9d` |
| `fulgurstack.exe` | 11.8 MB, SHA-256 `75cb4709d07cd289fc325d05a417d5fbca11bc8210175d167a1015f8cebeb7f2` |
| `fulgur-solve.exe` | 1.8 MB, SHA-256 `088c7454ea1870ce1aabae2a7d01672982e75191fcdcac904481d0980d7d7753` |
| Checks (2026-11-09) | Quick suite PASS (204 s). No CRT DLL imports in the three executables: PASS. DS-B (`--tile-size 512`) 27/32 frames in 4.6 s (standard build 5.1 s), peak RAM about 1.1 GB; master, coverage and high-rejection maps byte-identical to the standard build (SHA-256 prefixes 62747219C14DB758, 8418F8760EDBF4F8, 4F704B8CA735F4C2): PASS. GUI start from the package folder: PASS. ZIP content and versions (0.14.0): PASS |

The package was built from commit 6cb9167 (the release commit); the commit that records this table follows it. The blind solver was run end to end with the index built from the owner's Gaia DR3 files (1.12 GB, 36 s to build, 2.2 s to read; real frames placed in 0.5 to 2.2 s with refinement). The guide PDF is carried over from package 0.12 (it does not describe the 0.13 and 0.14 changes; `CTI_ME.txt` does). The previous package `FulgurStack-0.13.0-9b7feab-windows-x64.zip` (2026-11-08) is superseded. Only Windows 11 x64 was tested; the full suite with real data, GPU and SIMD tests was not run for this package.
