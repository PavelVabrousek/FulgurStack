// Native regression for ordinary import and direct rendering of bounded FITS.
// No STF, histogram process or image write is performed. Only JSON is saved.
function verifyBoundedFulgurStack() {
    var root = File.extractDrive(#__FILE__) + File.extractDirectory(#__FILE__) + "/../..";
    var reportPath = root + "/outputs/M1_PI_bounded_validation.json";
    if (File.exists(reportPath))
        throw new Error("Archive the previous bounded validation report before rerunning.");
    var report = { status: "FAIL", validation: "Ordinary FITS import and direct native bitmap rendering, without STF or pixel transformations; not a viewport screenshot." };
    var windows = [];
    function require(condition, message) {
        if (!condition) throw new Error(message);
    }
    try {
        var opened = ImageWindow.open(root + "/outputs/M1_M33_vng_bounded.fits", "FulgurBoundedReview", "verbosity 0", true)[0];
        windows.push(opened);
        var before = ImageWindow.open(root + "/outputs/M1_M33_vng.fits", "FulgurUnboundedBaseline", "lower-range 0 upper-range 1 ignore-out-of-range top-down verbosity 0", true)[0];
        windows.push(before);
        var reference = ImageWindow.open(root + "/PixInsight Files/InterimFiles/dLight_M 33_10.0s_IRCUT_20261006-204534.xisf", "FulgurBoundedReference", "verbosity 0", true)[0];
        windows.push(reference);
        var image = opened.mainView.image;
        require(image.width === 2160 && image.height === 3840, "Unexpected test dimensions.");
        require(!opened.isModified, "Imported image was modified.");
        report.stf = opened.mainView.stf;
        for (var st = 0; st < 4; ++st)
            require(report.stf[st][0] === 0.5 && report.stf[st][1] === 0 && report.stf[st][2] === 1,
                "Test requires the default identity display transfer.");
        var n = image.width * image.height;
        var a = new Float32Array(n), b = new Float32Array(n);
        var changed = 0, mismatches = 0, outOfRange = 0;
        for (var c = 0; c < 3; ++c) {
            before.mainView.image.getSamples(a, new Rect(image.width, image.height), c);
            image.getSamples(b, new Rect(image.width, image.height), c);
            for (var i = 0; i < n; ++i) {
                if (a[i] !== b[i]) ++changed;
                if (Math.max(0, Math.min(1, a[i])) !== b[i]) ++mismatches;
                if (!(b[i] >= 0 && b[i] <= 1)) ++outOfRange;
            }
        }
        report.comparedSamples = 3 * n;
        report.changedFromUnbounded = changed;
        report.unexpectedChanges = mismatches;
        report.outOfRange = outOfRange;
        report.minimum = image.minimum();
        report.maximum = image.maximum();
        require(changed === 40 && mismatches === 0 && outOfRange === 0,
            "Ordinary import differs from the fixed-scale sensor-bounded export.");
        var points = [[1694,1407],[1694,1408],[1695,1408],[1693,1409],[1695,1409],[1696,1409],[1694,1410]];
        var bitmap = image.render();
        report.stellarPixels = [];
        for (var p = 0; p < points.length; ++p) {
            var x = points[p][0], y = points[p][1];
            var pixel = bitmap.pixel(x, y);
            var displayRgb8 = [], rgb = [];
            for (var k = 0; k < 3; ++k) {
                var value = image.sample(x, y, k);
                rgb.push(value);
                var ref = reference.mainView.image.sample(x, y, k);
                require(Math.abs(value - ref) <= 1e-6, "Reported stellar pixel differs from PI reference.");
                var rendered = (pixel >>> (8 * (2 - k))) & 255;
                displayRgb8.push(rendered);
                require(rendered === Math.round(255 * ref), "Direct native display byte differs from PI reference.");
            }
            report.stellarPixels.push({ x: x, y: y, rgb: rgb, displayRgb8: displayRgb8 });
        }
        report.correctedStellarPixels = points.length;
        report.status = "PASS";
    } catch (error) {
        report.error = String(error);
    } finally {
        for (var w = windows.length - 1; w >= 0; --w) windows[w].forceClose();
    }
    File.writeTextFile(reportPath, JSON.stringify(report, null, 2));
    require(report.status === "PASS", report.error || "Bounded FITS regression failed.");
}

verifyBoundedFulgurStack();
