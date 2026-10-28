//! Generate constant-index CPU kernels from the shared geometric VNG coefficients.
#[path = "src/vng_stencil.rs"]
mod stencil;

use std::fmt::Write;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/vng_stencil.rs");
    let mut source = String::from(
        "// Generated from vng_stencil.rs; preserve scalar contribution order.\n#[inline(always)]\nfn reconstruct<const KERNEL: usize>(window: &[f32; 25]) -> [f32; 3] {\nmatch KERNEL {\n",
    );
    let directions = [
        (0, -1),
        (1, 0),
        (0, 1),
        (-1, 0),
        (1, -1),
        (1, 1),
        (-1, -1),
        (-1, 1),
    ];
    // The four possible 2x2 channel cells as seen from the current pixel.
    for (kernel, pattern) in [[1, 0, 2, 1], [0, 1, 1, 2], [1, 2, 0, 1], [2, 1, 1, 0]]
        .into_iter()
        .enumerate()
    {
        let plans: Vec<_> = directions
            .into_iter()
            .map(|d| stencil::direction(pattern, 0, d))
            .collect();
        writeln!(source, "{kernel} => {{\nlet gradients: [f32; 8] = [").unwrap();
        for plan in &plans {
            source.push('[');
            for &(a, b, weight) in &plan.differences {
                write!(
                    source,
                    "f32::from_bits({}) * (window[{a}] - window[{b}]).abs(),",
                    weight.to_bits()
                )
                .unwrap();
            }
            source.push_str("].into_iter().sum::<f32>(),\n");
        }
        source.push_str(
            "];\nlet min = gradients.iter().copied().fold(f32::INFINITY, f32::min);\nlet max = gradients.iter().copied().fold(0.0, f32::max);\nlet threshold = min + 0.5 * max;\nlet mut sums = [0.0_f32; 3];\nlet mut count = 0_u32;\n",
        );
        for (i, plan) in plans.iter().enumerate() {
            writeln!(source, "if gradients[{i}] <= threshold {{").unwrap();
            for &(sample, color, weight) in &plan.samples {
                writeln!(
                    source,
                    "sums[{color}] += window[{sample}] * f32::from_bits({});",
                    weight.to_bits()
                )
                .unwrap();
            }
            source.push_str("count += 1;\n}\n");
        }
        writeln!(source, "let mut pixel = std::array::from_fn(|color| window[12] + (sums[color] - sums[{}]) / count as f32);\npixel[{}] = window[12];\npixel\n}},",pattern[0],pattern[0]).unwrap();
    }
    source.push_str("_ => unreachable!(\"Invalid internal VNG kernel.\"),\n}\n}\n");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(output.join("vng_kernels.rs"), source).unwrap();
    std::fs::write(
        output.join("vng_kernels_avx2.rs"),
        vector_kernels(&directions),
    )
    .unwrap();
}

/// Eight-lane copies of the scalar kernels: identical stencils, operations
/// and contribution order per lane; branches become per-lane blends.
fn vector_kernels(directions: &[(isize, isize)]) -> String {
    let mut source = String::from(
        "// Generated from vng_stencil.rs; preserve scalar contribution order per lane.\n#[target_feature(enable = \"avx2\")]\nfn reconstruct<const KERNEL: usize>(window: &[__m256; 25]) -> [__m256; 3] {\nmatch KERNEL {\n",
    );
    for (kernel, pattern) in [[1, 0, 2, 1], [0, 1, 1, 2], [1, 2, 0, 1], [2, 1, 1, 0]]
        .into_iter()
        .enumerate()
    {
        let plans: Vec<_> = directions
            .iter()
            .map(|&d| stencil::direction(pattern, 0, d))
            .collect();
        writeln!(source, "{kernel} => {{\nlet gradients: [__m256; 8] = [").unwrap();
        for plan in &plans {
            // Left fold in the scalar order: ((t0 + t1) + t2) + ...
            let mut sum = String::new();
            for (i, &(a, b, weight)) in plan.differences.iter().enumerate() {
                let term = format!("difference(window, {a}, {b}, {})", weight.to_bits());
                sum = if i == 0 {
                    term
                } else {
                    format!("_mm256_add_ps({sum}, {term})")
                };
            }
            writeln!(source, "{sum},").unwrap();
        }
        source.push_str("];\nlet (threshold, mut sums, mut count) = start(&gradients);\n");
        for (i, plan) in plans.iter().enumerate() {
            writeln!(
                source,
                "let taken = _mm256_cmp_ps(gradients[{i}], threshold, _CMP_LE_OQ);"
            )
            .unwrap();
            for &(sample, color, weight) in &plan.samples {
                writeln!(
                    source,
                    "sums[{color}] = add_if(sums[{color}], window[{sample}], {}, taken);",
                    weight.to_bits()
                )
                .unwrap();
            }
            source.push_str("count = _mm256_sub_epi32(count, _mm256_castps_si256(taken));\n");
        }
        writeln!(source, "finish(window, sums, count, {})\n}},", pattern[0]).unwrap();
    }
    source.push_str("_ => unreachable!(\"Invalid internal VNG kernel.\"),\n}\n}\n");
    source
}
