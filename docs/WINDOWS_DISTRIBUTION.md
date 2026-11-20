# Windows distribution package

Current as of 2026-11-20. See [development](DEVELOPMENT.md), [current state](CURRENT_STATUS.md) and ADR-089.

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
| ZIP | `outputs/dist/FulgurStack-0.16.0-c3bba6a-windows-x64.zip`, 13.3 MB; local (version 0.16 is not pushed or published yet) |
| ZIP SHA-256 | `f78a73b03bb5208b66c092e8ceb85a852c969759a720b9051183de3cb3f31701` |
| `fulgurstack-gui.exe` | 17.8 MB, SHA-256 `23afd574ef5bb77553f26edb9238f24f4a663fc8f977582fcd1622776b3e0a3e` |
| `fulgurstack.exe` | 12.2 MB, SHA-256 `9900e3f33d70720e46190fb394d4ada3e8702fe5caa071bc762ae7a9358084c0` |
| Checks (2026-11-20) | Release suites PASS: default features 372 passed, all features 376 passed (3 ignored each), format and Clippy PASS. No CRT DLL imports in the two executables: PASS. DS-B (`--tile-size 512`) 27/32 frames: master, coverage and high-rejection maps byte-identical to the standard build (SHA-256 prefixes D9C9C94D0C630857, 3C651738CA1C1C33, 7FB220D883FA4415), pixel data identical to package 0.15 (headers differ only by version); DS-B with `--drizzle` 4320 × 7680 in 41 s, pixels identical to the standard build: PASS. `fulgurstack.exe --version` reports 0.16.0; the new CLI examples in `CTI_ME.txt` were checked against `--help`. GUI start from the package folder: PASS. |

The package was built from commit c3bba6a (the version commit); the commit that records this table follows it. The guide PDF is carried over from package 0.12 (it does not describe the changes since 0.13; `CTI_ME.txt` and the About page do).
