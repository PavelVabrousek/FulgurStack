const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

// PJSR's preprocessor replaces #__FILE__ with the script filename.
const script = fs.readFileSync(path.join(__dirname, '../OpenFulgurStack.js'), 'utf8')
    .replaceAll('#__FILE__', JSON.stringify('C:/project/scripts/OpenFulgurStack.js'));

function setup({ accepted = false, legacy = false, channels = 3, count = 1 } = {}) {
    const calls = [];
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
        OpenFileDialog: function () {
            this.fileName = 'C:/project/outputs/new.fits';
            this.execute = () => accepted;
        },
        File: { extractDrive: () => 'C:', extractDirectory: () => '/project/scripts' },
        console: { writeln() {} },
    });
    vm.runInContext(script, context);
    return { context, calls };
}

test('opens with explicit unclipped range, preserved orientation and a fresh image', () => {
    const { calls } = setup({ accepted: true });
    assert.equal(calls[0][0], 'C:/project/outputs/new.fits');
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
