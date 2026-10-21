const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

// PJSR's preprocessor replaces #__FILE__ with the script filename.
const script = fs.readFileSync(path.join(__dirname, '../OpenFulgurStack.js'), 'utf8')
    .replaceAll('#__FILE__', JSON.stringify('C:/project/scripts/OpenFulgurStack.js'));

function setup({ accepted = false, legacy = false, channels = 3, count = 1, displaySucceeds = true } = {}) {
    const calls = [];
    const displays = [];
    const windows = Array.from({ length: count }, () => ({
        mainView: { image: Object.freeze({ isReal: true, bitsPerSample: 32, numberOfChannels: channels }) },
        keywords: [{ name: 'CREATOR', strippedValue: 'FulgurStack 0.1.0' },
            ...(!legacy ? [{ name: 'FLXDIV', strippedValue: '65535' }] : [])],
        zoomToOptimalFit() { calls.push('fit'); },
        show() { calls.push('show'); },
        forceClose() { calls.push('close'); },
    }));
    const context = vm.createContext({
        ImageWindow: { open(...args) { calls.push(args); return windows; } },
        ScreenTransferFunction: function () {
            this.executeOn = view => {
                displays.push({ process: this, view });
                // Native process-table order differs from the View.stf API.
                view.stf = this.STF.map(([c0, c1, m, r0, r1]) => [m, c0, c1, r0, r1]);
                return displaySucceeds;
            };
        },
        OpenFileDialog: function () {
            this.fileName = 'C:/project/outputs/new.fits';
            this.execute = () => accepted;
        },
        File: { extractDrive: () => 'C:', extractDirectory: () => '/project/scripts', extractName: p => path.basename(p, path.extname(p)) },
        console: { writeln() {} },
    });
    vm.runInContext(script, context);
    return { context, calls, windows, displays };
}

test('opens with explicit unclipped range, preserved orientation and a fresh image', () => {
    const { calls } = setup({ accepted: true });
    assert.equal(calls[0][0], 'C:/project/outputs/new.fits');
    assert.equal(calls[0][1], 'FulgurStack_new');
    assert.equal(calls[0][2], 'lower-range 0 upper-range 1 ignore-out-of-range top-down verbosity 0');
    assert.equal(calls[0][3], true);
    assert.deepEqual(calls.slice(1), ['fit', 'show']);
});

test('rejects old ADU export rather than silently showing it on the wrong scale', () => {
    const { context, calls } = setup({ legacy: true });
    assert.throws(() => context.openFulgurStack('old.fits'), /Old ADU exports are unsupported/);
    assert.equal(calls.at(-1), 'close');
});

test('rejects non-RGB image and closes all images from a multi-image file', () => {
    for (const options of [{ channels: 1 }, { count: 2 }]) {
        const { context, calls } = setup(options);
        assert.throws(() => context.openFulgurStack('invalid.fits'));
        assert.equal(calls.filter(call => call === 'close').length, options.count || 1);
        assert.ok(!calls.includes('show'));
    }
});

test('cancelling the picker leaves images untouched', () => {
    assert.deepEqual(setup().calls, []);
});

test('enables a linked linear display guard without changing image samples', () => {
    const { windows, displays } = setup({ accepted: true });
    const view = windows[0].mainView;
    assert.equal(displays.length, 1);
    assert.equal(displays[0].view, view);
    const expected = [0.5, 0, 1 - 2 ** -20, 0, 1];
    for (let c = 0; c < 3; c++)
        assert.deepEqual(Array.from(view.stf[c]), expected);
    assert.deepEqual(Array.from(view.stf[3]), [0.5, 0, 1, 0, 1]);
    // The frozen image supplies no mutation/process API. Opening must still work.
    assert.ok(Object.isFrozen(view.image));
});

test('closes the imported copy if the display guard cannot be enabled', () => {
    const { context, calls } = setup({ displaySucceeds: false });
    assert.throws(() => context.openFulgurStack('new.fits'), /Could not enable/);
    assert.equal(calls.at(-1), 'close');
    assert.ok(!calls.includes('show'));
});

test('display transfer saturates the seven reported overshoots instead of wrapping', () => {
    const { windows } = setup({ accepted: true });
    // Actual Float32 export values at the seven failing stellar coordinates.
    const samples = [
        [1.04744029045, 0.8158762455, 1],
        [1.06046962738, 0.7470664382, 0.9336499572],
        [1, 0.8476583958, 1.09189999104],
        [1.06685137749, 0.7583428621, 0.8230243325],
        [1.01967501640, 0.7595177889, 0.9636484385],
        [1.11205303669, 0.9487670660, 1],
        [1.06088960171, 0.7721980810, 0.8036224842],
    ];
    // Independent evaluation of PCL's documented linear STF clipping segment.
    // This is a transfer-model test, not a claim of native viewport execution.
    function display(value, row) {
        const [midtones, shadows, highlights, low, high] = row;
        assert.equal(midtones, 0.5);
        assert.equal(low, 0);
        assert.equal(high, 1);
        assert.ok(highlights < 1, 'identity fast path must be bypassed');
        return Math.round(255 * Math.max(0, Math.min(1, (value - shadows) / (highlights - shadows))));
    }
    let overshoots = 0;
    for (const pixel of samples)
        for (let c = 0; c < 3; c++) {
            const actual = display(pixel[c], windows[0].mainView.stf[c]);
            assert.equal(actual, Math.round(255 * Math.min(1, pixel[c])));
            if (pixel[c] > 1) {
                ++overshoots;
                assert.equal(actual, 255);
            }
        }
    assert.equal(overshoots, 7);
    const row = windows[0].mainView.stf[0];
    assert.equal(display(-0.2, row), 0);
    assert.equal(display(1.172, row), 255);
    // Interior display remains linear, to within <1 ppm; data stays untouched.
    assert.ok(1 / row[2] - 1 < 1e-6);
});
