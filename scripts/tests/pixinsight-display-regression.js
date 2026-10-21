// Optional native PJSR test against the prepared M1 VNG export and PI reference.
// Run in an isolated PI automation instance. Only a JSON report is written.
var FULGURSTACK_LIBRARY_ONLY = true;
#include "../OpenFulgurStack.js"
#include <pjsr/UndoFlag.jsh>

function verifyFulgurStackDisplay() {
    var root = File.extractDrive(#__FILE__) + File.extractDirectory(#__FILE__) + "/../..";
    var reportPath = root + "/outputs/M1_PI_display_validation.json";
    if (File.exists(reportPath))
        throw new Error("Use a new report name or archive the previous display report before rerunning.");
    var report = { status: "FAIL", validation: "Native import, sample preservation and in-memory display-transfer rendering; not a viewport screenshot." };
    var windows = [];
    function require(condition, message) {
        if (!condition)
            throw new Error(message);
    }
    try {
        var input = root + "/outputs/M1_M33_vng.fits";
        var before = ImageWindow.open(input, "FulgurBeforeDisplay", "lower-range 0 upper-range 1 ignore-out-of-range top-down verbosity 0", true)[0];
        windows.push(before);
        var opened = openFulgurStack(input);
        windows.push(opened);
        var image = opened.mainView.image;
        var stf = opened.mainView.stf;
        report.stf = stf;
        for (var c = 0; c < 3; ++c)
            require(stf[c][0] === 0.5 && stf[c][1] === 0 &&
                stf[c][2] === 1 - Math.pow(2, -20) && stf[c][3] === 0 && stf[c][4] === 1,
                "Unexpected native display-transfer parameters.");
        require(!opened.isModified, "Display setup marked scientific pixels as modified.");
        require(image.width === 2160 && image.height === 3840, "Unexpected test-frame dimensions.");
        var n = image.width * image.height;
        var a = new Float32Array(n), b = new Float32Array(n);
        var changed = 0, aboveOne = 0;
        for (var channel = 0; channel < 3; ++channel) {
            before.mainView.image.getSamples(a, new Rect(image.width, image.height), channel);
            image.getSamples(b, new Rect(image.width, image.height), channel);
            for (var i = 0; i < n; ++i) {
                if (a[i] !== b[i])
                    ++changed;
                if (b[i] > 1)
                    ++aboveOne;
            }
        }
        report.comparedSamples = 3 * n;
        report.changedSamples = changed;
        report.preservedAboveOne = aboveOne;
        report.maximum = image.maximum();
        require(changed === 0 && aboveOne === 40 && report.maximum > 1.17,
            "Scientific image samples were changed or clipped.");

        var reference = ImageWindow.open(root + "/PixInsight Files/InterimFiles/dLight_M 33_10.0s_IRCUT_20261006-204534.xisf",
            "FulgurDisplayReference", "verbosity 0", true)[0];
        windows.push(reference);
        var points = [[1694,1407],[1694,1408],[1695,1408],[1693,1409],[1695,1409],[1696,1409],[1694,1410]];
        var scratch = new ImageWindow(points.length, 1, 3, 32, true, true, "FulgurDisplayScratch");
        windows.push(scratch);
        // All display arithmetic is restricted to this tiny in-memory copy.
        scratch.mainView.beginProcess(UndoFlag_NoSwapFile);
        try {
            for (var ch = 0; ch < 3; ++ch) {
                var values = points.map(function(p) { return image.sample(p[0], p[1], ch); });
                scratch.mainView.image.setSamples(values, new Rect(points.length, 1), ch);
            }
        } finally {
            scratch.mainView.endProcess();
        }
        var rawBitmap = scratch.mainView.image.render();
        var transfer = new HistogramTransformation;
        transfer.H = [
            [stf[0][1],stf[0][0],stf[0][2],0,1],
            [stf[1][1],stf[1][0],stf[1][2],0,1],
            [stf[2][1],stf[2][0],stf[2][2],0,1],
            [0,0.5,1,0,1], [0,0.5,1,0,1]
        ];
        require(transfer.executeOn(scratch.mainView, false), "Native display-transfer test failed.");
        var safeBitmap = scratch.mainView.image.render();
        var wrapReproduced = 0, overshootsVerified = 0;
        report.stellarPixels = [];
        for (var p = 0; p < points.length; ++p) {
            var x = points[p][0], y = points[p][1];
            var raw = rawBitmap.pixel(p, 0), safe = safeBitmap.pixel(p, 0);
            var rendered = [];
            for (var k = 0; k < 3; ++k) {
                var sample = image.sample(x, y, k);
                var shift = 8 * (2 - k);
                var displayByte = (safe >>> shift) & 255;
                rendered.push(displayByte);
                var expected = Math.round(255 * reference.mainView.image.sample(x, y, k));
                require(displayByte === expected, "Native display byte differs from reference at a reported stellar pixel.");
                if (sample > 1) {
                    ++overshootsVerified;
                    require(displayByte === 255, "Native display did not saturate an overshoot.");
                    if (((raw >>> shift) & 255) < 255)
                        ++wrapReproduced;
                }
            }
            report.stellarPixels.push({ x: x, y: y, displayRgb8: rendered });
        }
        report.rawWrapsReproduced = wrapReproduced;
        report.correctedOvershoots = overshootsVerified;
        require(wrapReproduced === 7 && overshootsVerified === 7, "Expected seven reproduced and corrected display wraps.");
        report.status = "PASS";
    } catch (error) {
        report.error = String(error);
    } finally {
        for (var w = windows.length - 1; w >= 0; --w)
            windows[w].forceClose();
    }
    File.writeTextFile(reportPath, JSON.stringify(report, null, 2));
    require(report.status === "PASS", report.error || "Display regression failed.");
}

verifyFulgurStackDisplay();
