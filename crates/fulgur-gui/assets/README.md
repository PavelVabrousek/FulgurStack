# FulgurStack application icon

Current packaging context (2026-10-26): included in the consolidated main publication. The desktop shortcut uses `target/release/fulgurstack-gui.exe` and embedded icon index 0. See [current delivery state](../../../docs/CURRENT_STATUS.md) for verification limits.

Owner-selected design: three navy/cyan astrophotography layers, aligned white stars and one continuous golden lightning bolt in front of the entire stack. Selected in the local icon review on 2026-10-25. The artwork has a transparent background.

- `fulgurstack-source.png`: unchanged approved 1280-square artwork, retained as the export source.
- `fulgurstack.png`: 256-square RGBA image embedded in the native window for its title bar and running application icon.
- `fulgurstack.ico`: Windows executable/desktop icon with 16, 20, 24, 32, 40, 48, 64, 96, 128 and 256 pixel representations. The 20/40 pixel sizes cover common 125% scaling requests.
- `windows.rc`: first Windows icon resource group, embedded by `build.rs` using the standard Windows SDK resource compiler for Windows MSVC targets. No additional runtime dependency is introduced. Other targets skip this Windows resource step.

Re-export without changing the approved design, from the repository root:

```powershell
./scripts/Export-AppIcon.ps1
```

The export uses Windows System.Drawing only for proportional downsampling and PNG/ICO encoding; it does not redraw or rearrange the artwork. The source and both exports are versioned together. Regeneration must preserve the approved design. A desktop shortcut should reference the release executable's embedded icon, index 0.

## Artwork provenance

Created and refined with the built-in image generation tool, not the API/CLI fallback. Final generation input was the previously reviewed aligned-star variant; the following edit prompt produced the owner-selected artwork:

> Use case: precise-object-edit. Create a variant of the supplied FulgurStack app icon by changing ONLY the lightning bolt's depth placement and silhouette. Place ONE continuous golden-yellow lightning bolt completely IN FRONT OF ALL THREE stacked astrophotography plates. The bolt overlays and hides any navy plate and cyan rim beneath it. No plate edge may cross in front of or interrupt the bolt. Replace the current repeated zigzag segments passing through individual plates with a SINGLE unmistakable classic lightning-bolt silhouette: one long pointed upper stroke, ONE central angular zigzag bend, and one long pointed lower stroke. Keep the bolt diagonal orientation from upper right toward lower left and its approximate full height from above top plate to below bottom plate. Bold clean geometry, warm yellow face with restrained golden side shading consistent with existing style. The bolt should look like one unified foreground emblem superimposed on the stack, not multiple bolts and not threaded through layers. Preserve all THREE navy isometric photo plates, cyan blue rims, original perspective and spacing, original two top stars, and vertically aligned corresponding white stars on lower plates, including natural occlusion of lower large stars by plates above. Preserve palette, transparency, square framing and margins. No lettering, no added objects, exactly one icon.

## Windows process identity

windows.rc also embeds FileDescription/ProductName FulgurStack, version 0.1.0 and OriginalFilename fulgurstack-gui.exe. Keep version strings/numeric version components synchronized with the package version. Startup sets FulgurStack.Desktop as the explicit Windows application ID; File menu exposes version and PID for Task Manager lookup. This does not imply native Task Manager grouping has been visually accepted.
