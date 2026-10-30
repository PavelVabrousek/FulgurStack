// Per-pixel pixel rejection and weighted mean (ADR-090).
//
// The CPU uploads one contiguous run of all frames per pixel channel. One
// workgroup owns one output pixel channel: it bitonic-sorts the run in
// workgroup memory, applies the selected rejection method to the sorted
// samples (binary searches and parallel sums wherever the method allows) and
// sums the survivors' weights in f32 around the sample median.
// Placeholders replaced before compilation: {{P}} (power-of-two sort size >=
// frame count), {{WG}} (threads per workgroup), {{METHOD}} (0 winsorized,
// 1 median, 2 linear fit, 3 ESD) and {{SLOTS}} (frame indices carried through
// the sort: {{P}} for linear fit, whose rejections are per sorted position,
// else 1). The other methods keep a value range, so their survivors are found
// again in the unsorted run and the sort moves keys only.

struct Params {
    chunk_pixels: u32,
    frames: u32,
    dispatch_x: u32,
    _pad: u32,
    sigma_low: f32,
    sigma_high: f32,
    _pad2: vec2<f32>,
};

@group(0) @binding(0) var<storage, read> runs: array<f32>;
@group(0) @binding(1) var<storage, read> weights: array<f32>;
@group(0) @binding(2) var<storage, read> lambda: array<f32>;
@group(0) @binding(3) var<storage, read> lambda_offsets: array<u32>;
@group(0) @binding(4) var<storage, read_write> results: array<u32>;
@group(0) @binding(5) var<uniform> params: Params;

const SORT: u32 = {{P}}u;
const THREADS: u32 = {{WG}}u;
const METHOD: u32 = {{METHOD}}u;
// Invalid and padding samples; real samples are within +-1e30 ADU.
const SENTINEL: f32 = 3.0e38;
const LIMIT: f32 = 1.0e30;
const REJECTED: u32 = 0x80000000u;
const NO_FRAME: u32 = 0x7fffffffu;
const MIN_SAMPLES: u32 = 5u;
const ESD_SHARE: f32 = 0.3;
const FIT_LOW: f32 = 5.0;
const FIT_HIGH: f32 = 4.0;
const FIT_ITERATIONS: u32 = 10u;

var<workgroup> keys: array<f32, SORT>;
var<workgroup> frame_of: array<u32, {{SLOTS}}u>;
var<workgroup> critical: array<f32, SORT / 2u>;
var<workgroup> part_a: array<f32, THREADS>;
var<workgroup> part_b: array<f32, THREADS>;
var<workgroup> part_c: array<f32, THREADS>;
var<workgroup> finite_count: u32;
var<workgroup> kept_range: vec2<u32>;
// Survivors' value range (inclusive) for every method except linear fit.
var<workgroup> kept_values: vec2<f32>;

fn is_valid(v: f32) -> bool {
    return v == v && abs(v) < LIMIT;
}

// Value at sorted position `i` after the composed winsorization clamp.
fn clamped(i: u32, lo: f32, hi: f32) -> f32 {
    return clamp(keys[i], lo, hi);
}

// First sorted position whose (clamped) value is >= `value` (or > with `strict`).
fn bound(n: u32, value: f32, strict: bool, lo: f32, hi: f32) -> u32 {
    var a = 0u;
    var b = n;
    while a < b {
        let m = (a + b) / 2u;
        let v = clamped(m, lo, hi);
        if v < value || (strict && v == value) {
            a = m + 1u;
        } else {
            b = m;
        }
    }
    return a;
}

fn median_of(n: u32, lo: f32, hi: f32) -> f32 {
    if n % 2u == 1u {
        return clamped(n / 2u, lo, hi);
    }
    return 0.5 * (clamped(n / 2u - 1u, lo, hi) + clamped(n / 2u, lo, hi));
}

// Deviations from `center` grow outward on both sides of position `r` (the
// first sample >= center): two sorted lists. Their k-th smallest element
// (0-based) by binary search on how many are taken from the left list.
fn left_dev(r: u32, t: u32, center: f32, lo: f32, hi: f32) -> f32 {
    return center - clamped(r - 1u - t, lo, hi);
}
fn right_dev(r: u32, t: u32, center: f32, lo: f32, hi: f32) -> f32 {
    return clamped(r + t, lo, hi) - center;
}
fn kth_deviation(k: u32, n: u32, r: u32, center: f32, lo: f32, hi: f32) -> f32 {
    let take = k + 1u;
    let right = n - r;
    var a = select(0u, take - right, take > right);
    var b = min(take, r);
    while a < b {
        let i = (a + b) / 2u;
        let j = take - i;
        if j >= 1u && left_dev(r, i, center, lo, hi) < right_dev(r, j - 1u, center, lo, hi) {
            a = i + 1u;
        } else {
            b = i;
        }
    }
    let j = take - a;
    var v = 0.0;
    if a > 0u {
        v = left_dev(r, a - 1u, center, lo, hi);
    }
    if j > 0u {
        v = max(v, right_dev(r, j - 1u, center, lo, hi));
    }
    return v;
}

fn mad_of(n: u32, center: f32, lo: f32, hi: f32) -> f32 {
    let r = bound(n, center, false, lo, hi);
    if n % 2u == 1u {
        return kth_deviation(n / 2u, n, r, center, lo, hi);
    }
    return 0.5 * (kth_deviation(n / 2u - 1u, n, r, center, lo, hi) + kth_deviation(n / 2u, n, r, center, lo, hi));
}

fn winsorized(n: u32) {
    var lo = -SENTINEL;
    var hi = SENTINEL;
    var low = -SENTINEL;
    var high = SENTINEL;
    if n >= MIN_SAMPLES {
        for (var iteration = 0u; iteration < 3u; iteration++) {
            let center = median_of(n, lo, hi);
            let mad = mad_of(n, center, lo, hi);
            let sigma = max(1.4826 * mad, 1e-6);
            low = center - params.sigma_low * sigma;
            high = center + params.sigma_high * sigma;
            let changed = clamped(0u, lo, hi) < low || clamped(n - 1u, lo, hi) > high;
            if !changed {
                break;
            }
            lo = clamp(lo, low, high);
            hi = clamp(hi, low, high);
        }
    }
    // Survivors are low <= v <= high on the original samples.
    kept_range = vec2<u32>(bound(n, low, false, -SENTINEL, SENTINEL), bound(n, high, true, -SENTINEL, SENTINEL));
    kept_values = vec2<f32>(low, high);
}

// Generalized ESD on the sorted samples: each step removes the end farther
// from the running mean; running sums arrive precomputed in part_a/part_b.
fn esd(n: u32, shift: f32, s1_total: f32, s2_total: f32) {
    kept_range = vec2<u32>(0u, n);
    let tests = u32(floor(f32(n) * ESD_SHARE));
    if n < MIN_SAMPLES || tests == 0u {
        return;
    }
    let steps = min(tests, n - 3u);
    var s1 = s1_total;
    var s2 = s2_total;
    var lo = 0u;
    var hi = n - 1u;
    for (var step = 0u; step < steps; step++) {
        let m = f32(n - step);
        let mean = s1 / m;
        let variance = (s2 / m - mean * mean) * m / (m - 1.0);
        if variance <= 0.0 || !is_valid(variance) {
            break;
        }
        let low_dev = mean - (keys[lo] - shift);
        let high_dev = (keys[hi] - shift) - mean;
        let take_high = high_dev >= low_dev;
        let d = select(keys[lo], keys[hi], take_high) - shift;
        s1 -= d;
        s2 -= d * d;
        if take_high {
            hi--;
        } else {
            lo++;
        }
        if max(high_dev, low_dev) / sqrt(variance) > critical[step] {
            kept_range = vec2<u32>(lo, hi + 1u);
        }
        if lo >= hi {
            break;
        }
    }
}

fn linear_fit(n: u32, shift: f32) {
    kept_range = vec2<u32>(0u, n);
    if n < MIN_SAMPLES {
        return;
    }
    for (var iteration = 0u; iteration < FIT_ITERATIONS; iteration++) {
        var m = 0u;
        var sy = 0.0;
        for (var i = 0u; i < n; i++) {
            if (frame_of[i] & REJECTED) == 0u {
                sy += keys[i] - shift;
                m++;
            }
        }
        if m < 3u {
            break;
        }
        let mf = f32(m);
        let x_mean = (mf - 1.0) * 0.5;
        let y_mean = sy / mf;
        // Sum of (rank - mean)^2 over ranks 0..m-1, exactly.
        let sxx = mf * (mf * mf - 1.0) / 12.0;
        var sxy = 0.0;
        var rank = 0u;
        for (var i = 0u; i < n; i++) {
            if (frame_of[i] & REJECTED) == 0u {
                sxy += (f32(rank) - x_mean) * (keys[i] - shift - y_mean);
                rank++;
            }
        }
        let slope = select(0.0, sxy / sxx, sxx > 0.0);
        var absolute = 0.0;
        rank = 0u;
        for (var i = 0u; i < n; i++) {
            if (frame_of[i] & REJECTED) == 0u {
                absolute += abs(keys[i] - shift - (y_mean + slope * (f32(rank) - x_mean)));
                rank++;
            }
        }
        let sigma = 1.2533 * absolute / mf;
        if sigma <= 0.0 || !is_valid(sigma) {
            break;
        }
        var changed = false;
        rank = 0u;
        for (var i = 0u; i < n; i++) {
            if (frame_of[i] & REJECTED) == 0u {
                let r = keys[i] - shift - (y_mean + slope * (f32(rank) - x_mean));
                rank++;
                if r < -FIT_LOW * sigma || r > FIT_HIGH * sigma {
                    frame_of[i] |= REJECTED;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

// Tree sum of part_a/part_b/part_c over the workgroup (result in index 0).
fn workgroup_sums(lid: u32) {
    workgroupBarrier();
    for (var stride = THREADS / 2u; stride > 0u; stride >>= 1u) {
        if lid < stride {
            part_a[lid] += part_a[lid + stride];
            part_b[lid] += part_b[lid + stride];
            part_c[lid] += part_c[lid + stride];
        }
        workgroupBarrier();
    }
}

@compute @workgroup_size(THREADS)
fn reduce(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {
    let job = group.x + group.y * params.dispatch_x;
    if job >= params.chunk_pixels * 3u {
        return;
    }
    let base = job * params.frames;
    for (var i = lid; i < SORT; i += THREADS) {
        var v = SENTINEL;
        var f = NO_FRAME;
        if i < params.frames {
            let s = runs[base + i];
            if is_valid(s) {
                v = s;
                f = i;
            }
        }
        keys[i] = v;
        if METHOD == 2u {
            frame_of[i] = f;
        }
    }
    workgroupBarrier();
    // Bitonic sort, ascending; invalid samples (sentinel) sort to the end.
    for (var k = 2u; k <= SORT; k <<= 1u) {
        for (var j = k >> 1u; j > 0u; j >>= 1u) {
            for (var t = lid; t < SORT / 2u; t += THREADS) {
                // j is a power of two: i = 2j * (t / j) + t % j without division.
                let i = ((t & ~(j - 1u)) << 1u) | (t & (j - 1u));
                let partner = i + j;
                let a = keys[i];
                let b = keys[partner];
                if (a > b) == ((i & k) == 0u) && a != b {
                    keys[i] = b;
                    keys[partner] = a;
                    if METHOD == 2u {
                        let fa = frame_of[i];
                        frame_of[i] = frame_of[partner];
                        frame_of[partner] = fa;
                    }
                }
            }
            workgroupBarrier();
        }
    }
    if lid == 0u {
        finite_count = bound(SORT, LIMIT, false, -SENTINEL, SENTINEL);
    }
    workgroupBarrier();
    let n = finite_count;
    var shift = 0.0;
    if n > 0u {
        shift = keys[n / 2u];
    }
    if METHOD == 3u {
        // ESD: critical values for this sample count and running sums.
        let tests = u32(floor(f32(n) * ESD_SHARE));
        if n >= MIN_SAMPLES && tests > 0u {
            let offset = lambda_offsets[n];
            let count = lambda_offsets[n + 1u] - offset;
            for (var i = lid; i < count; i += THREADS) {
                critical[i] = lambda[offset + i];
            }
        }
        var s1 = 0.0;
        var s2 = 0.0;
        for (var i = lid; i < n; i += THREADS) {
            let d = keys[i] - shift;
            s1 += d;
            s2 += d * d;
        }
        part_a[lid] = s1;
        part_b[lid] = s2;
        part_c[lid] = 0.0;
        workgroup_sums(lid);
        if lid == 0u {
            esd(n, shift, part_a[0], part_b[0]);
        }
    } else if lid == 0u {
        switch METHOD {
            case 0u: { winsorized(n); }
            case 2u: { linear_fit(n, shift); }
            default: { kept_range = vec2<u32>(0u, n); }
        }
    }
    if lid == 0u && METHOD != 0u {
        let r = kept_range;
        kept_values = vec2<f32>(SENTINEL, -SENTINEL);
        if r.y > r.x {
            kept_values = vec2<f32>(keys[r.x], keys[r.y - 1u]);
        }
    }
    workgroupBarrier();
    let kept = kept_range;
    let bounds = kept_values;
    // Compensated per-thread sums of the survivors around the median.
    var num = 0.0;
    var num_c = 0.0;
    var den = 0.0;
    var den_c = 0.0;
    var total = 0.0;
    // Linear fit sums sorted positions; the others sum the unsorted run by value.
    let limit = select(params.frames, n, METHOD == 2u);
    for (var i = lid; i < limit; i += THREADS) {
        var v: f32;
        var w: f32;
        var keep: bool;
        if METHOD == 2u {
            let f = frame_of[i];
            v = keys[i];
            w = weights[f & NO_FRAME];
            keep = i >= kept.x && i < kept.y && (f & REJECTED) == 0u;
        } else {
            v = runs[base + i];
            if !is_valid(v) {
                continue;
            }
            w = weights[i];
            keep = v >= bounds.x && v <= bounds.y;
        }
        total += w;
        if keep {
            let y = w * (v - shift) - num_c;
            let t = num + y;
            num_c = (t - num) - y;
            num = t;
            let z = w - den_c;
            let u = den + z;
            den_c = (u - den) - z;
            den = u;
        }
    }
    part_a[lid] = num;
    part_b[lid] = den;
    part_c[lid] = total;
    workgroup_sums(lid);
    if lid == 0u {
        var value = 0.0;
        var low = 0u;
        var high = 0u;
        var surviving = part_b[0];
        if METHOD == 1u {
            // Unweighted median; nothing is counted as rejected.
            if n > 0u {
                value = median_of(n, -SENTINEL, SENTINEL);
            }
            surviving = part_c[0];
        } else {
            if METHOD == 0u {
                // Winsorized counts by threshold side.
                low = kept.x;
                high = n - kept.y;
            } else if METHOD == 3u {
                // Contiguous survivors: rejected below their median are low.
                if kept.y > kept.x {
                    low = kept.x;
                    high = n - kept.y;
                } else {
                    low = n / 2u;
                    high = n - n / 2u;
                }
            } else {
                var survivors = 0u;
                for (var i = kept.x; i < kept.y; i++) {
                    if (frame_of[i] & REJECTED) == 0u {
                        survivors++;
                    }
                }
                var middle = n / 2u;
                var seen = 0u;
                for (var i = kept.x; i < kept.y; i++) {
                    if (frame_of[i] & REJECTED) == 0u {
                        if seen == survivors / 2u {
                            middle = i;
                            break;
                        }
                        seen++;
                    }
                }
                for (var i = 0u; i < n; i++) {
                    let rejected = i < kept.x || i >= kept.y || (frame_of[i] & REJECTED) != 0u;
                    if rejected {
                        if i < middle {
                            low++;
                        } else {
                            high++;
                        }
                    }
                }
            }
            if surviving > 0.0 {
                value = shift + part_a[0] / surviving;
            } else {
                surviving = 0.0;
            }
        }
        let o = job * 6u;
        results[o] = bitcast<u32>(value);
        results[o + 1u] = low;
        results[o + 2u] = high;
        results[o + 3u] = bitcast<u32>(surviving);
        results[o + 4u] = n;
        results[o + 5u] = bitcast<u32>(part_c[0]);
    }
}
