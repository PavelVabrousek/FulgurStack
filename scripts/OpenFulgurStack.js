// Open a normalized FulgurStack FITS without changing pixel values or preferences.
// Run with PixInsight Script > Execute Script, then select the exported FITS.
// The FITS module's permissive hint preserves negative and >1 float samples.

function setFulgurStackDisplay(view) {
    // Identity STF can bypass display-range checks: >1 samples then wrap in
    // PI's 8-bit screen rendition. Force a clipping display path with a linear
    // highlight point just below one (less than one ppm display gain).
    // STF affects the screen only. Never apply it to the scientific image.
    var highlight = 1 - Math.pow(2, -20);
    var display = new ScreenTransferFunction;
    display.STF = [
        // shadows, highlights, midtones, low range, high range
        [0, highlight, 0.5, 0, 1],
        [0, highlight, 0.5, 0, 1],
        [0, highlight, 0.5, 0, 1],
        [0, 1, 0.5, 0, 1]
    ];
    // Unlike assigning view.stf, the process also enables STF if disabled.
    if (!display.executeOn(view))
        throw new Error("Could not enable the overflow-safe display STF.");
}

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
        setFulgurStackDisplay(window.mainView);
        window.zoomToOptimalFit();
        window.show();
        console.writeln("PASS: FulgurStack RGB opened unchanged on the fixed reference scale; overflow-safe display STF enabled.");
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
