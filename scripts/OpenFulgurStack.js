// Legacy importer for historical unbounded normalized exports only.
// Current sensor-bounded exports open normally with PixInsight File > Open.
// Run with PixInsight Script > Execute Script, then select the exported FITS.
// The FITS module's permissive hint preserves negative and >1 float samples.

function openFulgurStack(filePath) {
    var viewId = "FulgurStack_" + File.extractName(filePath).replace(/[^A-Za-z0-9_]/g, "_");
    var windows = ImageWindow.open(
        filePath, viewId, "lower-range 0 upper-range 1 ignore-out-of-range top-down verbosity 0", true
    );
    try {
        if (windows.length !== 1)
            throw new Error("Expected one FulgurStack RGB image.");
        var window = windows[0];
        var image = window.mainView.image;
        var creator = "", divisor = NaN;
        var keywords = window.keywords;
        for (var i = 0; i < keywords.length; ++i) {
            if (keywords[i].name === "CREATOR")
                creator = keywords[i].strippedValue;
            if (keywords[i].name === "FLXDIV")
                divisor = Number(keywords[i].strippedValue);
        }
        if (creator.indexOf("FulgurStack ") !== 0 || divisor !== 65535 ||
            !image.isReal || image.bitsPerSample !== 32 || image.numberOfChannels !== 3)
            throw new Error("Select a normalized Float32 RGB FulgurStack export (FLXDIV=65535). Old ADU exports are unsupported.");
        window.zoomToOptimalFit();
        window.show();
        console.writeln("Legacy import: scientific samples retained. This does not fix display wrapping. Use File > Open for current bounded exports.");
        return window;
    } catch (error) {
        for (var j = 0; j < windows.length; ++j)
            windows[j].forceClose();
        throw error;
    }
}

function main() {
    var dialog = new OpenFileDialog;
    dialog.caption = "Open normalized FulgurStack RGB FITS";
    dialog.multipleSelections = false;
    dialog.filters = [["FITS images", "*.fit", "*.fits", "*.fts"]];
    dialog.initialPath = File.extractDrive(#__FILE__) + File.extractDirectory(#__FILE__) +
        "/../outputs/";
    if (dialog.execute())
        openFulgurStack(dialog.fileName);
}

// Native regression scripts can include this file without opening a dialog.
if (typeof FULGURSTACK_LIBRARY_ONLY === "undefined" || !FULGURSTACK_LIBRARY_ONLY)
    main();
