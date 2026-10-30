//! Times GPU pixel rejection on synthetic stacks (developer tool, ADR-090).
//! `cargo run --release -p fulgur-gpu --example bench_reduce -- <frames> <pixels>`
use fulgur_gpu::reduce::{GpuReducer, Method};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .map(|a| a.parse().unwrap())
        .collect();
    let frames = args.first().copied().unwrap_or(856);
    let pixels = args.get(1).copied().unwrap_or(65536);
    // Deterministic pseudo-noise around a sky level, with a few outliers.
    let mut state = 0x9e3779b97f4a7c15_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let data: Vec<Vec<[f32; 3]>> = (0..frames)
        .map(|_| {
            (0..pixels)
                .map(|_| {
                    std::array::from_fn(|_| {
                        let u = next();
                        if u < 0.01 {
                            9000.
                        } else {
                            2000. + 30. * (next() + next() + next() - 1.5) as f32
                        }
                    })
                })
                .collect()
        })
        .collect();
    let views: Vec<&[[f32; 3]]> = data.iter().map(|f| f.as_slice()).collect();
    let weights = vec![1.0_f32; frames];
    let mut lambda = Vec::new();
    let mut offsets = Vec::new();
    for n in 0..=frames {
        offsets.push(lambda.len() as u32);
        let tests = (n as f64 * 0.3) as usize;
        if n >= 5 && tests > 0 {
            lambda.extend(std::iter::repeat_n(3.0_f32, tests.min(n - 3)));
        }
    }
    offsets.push(lambda.len() as u32);
    let cancel = AtomicBool::new(false);
    let mut gpu = GpuReducer::new().unwrap();
    for method in [
        Method::Median,
        Method::Esd,
        Method::Winsorized {
            sigma_low: 3.,
            sigma_high: 2.5,
        },
    ] {
        gpu.reduce(&views, &weights, method, &lambda, &offsets, &cancel)
            .unwrap();
        let start = Instant::now();
        let before = gpu.telemetry();
        gpu.reduce(&views, &weights, method, &lambda, &offsets, &cancel)
            .unwrap();
        let after = gpu.telemetry();
        let s = start.elapsed().as_secs_f64();
        println!(
            "{method:?}: {frames} frames x {pixels} px: {s:.3} s ({:.1} ns per pixel channel), upload {:.3} s, wait {:.3} s",
            s * 1e9 / (pixels * 3) as f64,
            after.upload_seconds - before.upload_seconds,
            after.gpu_wait_seconds - before.gpu_wait_seconds
        );
    }
}
