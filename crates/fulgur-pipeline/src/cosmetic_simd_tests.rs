#[cfg(target_arch = "x86_64")]
use super::*;
#[cfg(target_arch = "x86_64")]
use fulgur_core::BayerPattern;

fn require_vector() -> bool {
    #[cfg(target_arch = "x86_64")]
    let available = Kernel::detect().vector;
    #[cfg(not(target_arch = "x86_64"))]
    let available = false;
    if std::env::var("FULGURSTACK_TEST_SIMD").as_deref() == Ok("1") {
        assert!(
            available,
            "Explicitly required local AVX2 cosmetics is unavailable."
        );
    }
    available
}

#[test]
fn cosmetic_vector_rows_match_scalar_bits_counts_tails_and_sources() {
    if !require_vector() {
        return;
    }
    #[cfg(target_arch = "x86_64")]
    for pattern in [
        BayerPattern::GRBG,
        BayerPattern::RGGB,
        BayerPattern::GBRG,
        BayerPattern::BGGR,
    ] {
        for width in [9, 15, 16, 17, 23, 24, 25, 31, 32, 33, 65, 127] {
            for field in 0..10 {
                let mut raw = super::super::cosmetic_tests::frame(pattern, width, 19, field % 6);
                for (i, v) in raw.pixels.iter_mut().enumerate() {
                    match field {
                        6 => *v = if i % 2 == 0 { -0. } else { 0. },
                        7 => {
                            *v = f32::from_bits((i * 37 % 100 + 1) as u32)
                                * if i % 2 == 0 { -1. } else { 1. }
                        }
                        8 => *v = if i % 3 == 0 { f32::MAX } else { -f32::MAX },
                        9 => {
                            *v = [
                                f32::INFINITY,
                                f32::NEG_INFINITY,
                                f32::NAN,
                                -0.,
                                0.,
                                2000.,
                                65535.,
                            ][i % 7]
                        }
                        _ => {}
                    }
                }
                if field < 6 {
                    for x in (4..width - 4).step_by(7) {
                        raw.pixels[9 * width + x] = 65535.;
                    }
                }
                let before: Vec<_> = raw.pixels.iter().map(|v| v.to_bits()).collect();
                let scalar = Kernel { vector: false };
                let vector = Kernel::detect();
                for y in 4..15 {
                    let mut a = raw.pixels[y * width..(y + 1) * width].to_vec();
                    let mut b = a.clone();
                    let ac = scalar.row(&raw, y, &mut a, 4, width - 4);
                    let bc = vector.row(&raw, y, &mut b, 4, width - 4);
                    assert_eq!(ac, bc, "width={width}, field={field}, y={y}");
                    assert_eq!(
                        a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                        b.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                        "width={width}, field={field}, y={y}"
                    );
                    // A column span corrects the same pixels inside it, nothing outside.
                    let from = (4 + y % 7).min(width - 4);
                    let to = (width - 4 - y % 5).max(from);
                    let mut c = raw.pixels[y * width..(y + 1) * width].to_vec();
                    vector.row(&raw, y, &mut c, from, to);
                    for (x, value) in c.iter().enumerate() {
                        let expected = if (from..to).contains(&x) {
                            a[x]
                        } else {
                            raw.pixels[y * width + x]
                        };
                        assert_eq!(value.to_bits(), expected.to_bits(), "x={x}");
                    }
                }
                assert_eq!(
                    raw.pixels.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    before
                );
            }
        }
    }
}

#[test]
fn cosmetic_vector_network_sorts_all_eight_rank_permutations() {
    if !require_vector() {
        return;
    }
    #[cfg(target_arch = "x86_64")]
    {
        #[target_feature(enable = "avx2")]
        unsafe fn check(p: &[i32; 8]) {
            use std::arch::x86_64::*;
            let inputs = p.map(|v| _mm256_set1_epi32(v));
            let actual = vector::sort(inputs);
            for (i, value) in actual.into_iter().enumerate() {
                let mut lanes = [0; 8];
                // SAFETY: destination contains exactly eight i32 lanes.
                unsafe { _mm256_storeu_si256(lanes.as_mut_ptr().cast(), value) };
                assert_eq!(lanes, [i as i32; 8]);
            }
        }
        fn visit(p: &mut [i32; 8], index: usize, count: &mut usize) {
            if index == p.len() {
                // SAFETY: AVX2 explicitly checked by require_vector().
                unsafe { check(p) };
                *count += 1;
            } else {
                for j in index..p.len() {
                    p.swap(index, j);
                    visit(p, index + 1, count);
                    p.swap(index, j);
                }
            }
        }
        let mut count = 0;
        visit(&mut [0, 1, 2, 3, 4, 5, 6, 7], 0, &mut count);
        assert_eq!(count, 40320);
    }
}

#[test]
fn cosmetic_vector_medians_match_total_order_for_float_encodings() {
    if !require_vector() {
        return;
    }
    #[cfg(target_arch = "x86_64")]
    {
        #[target_feature(enable = "avx2")]
        unsafe fn check(values: [[f32; 8]; 8]) {
            use std::arch::x86_64::*;
            // SAFETY: each input and the output contain exactly eight f32 lanes.
            let input = values.map(|v| unsafe { _mm256_loadu_ps(v.as_ptr()) });
            let median = vector::median(input);
            let mut actual = [0.; 8];
            unsafe { _mm256_storeu_ps(actual.as_mut_ptr(), median) };
            for lane in 0..8 {
                let mut samples = values.map(|v| v[lane]);
                assert_eq!(actual[lane].to_bits(), median8(&mut samples).to_bits());
            }
        }
        let mut seed = 0x123456789abcdef_u64;
        for round in 0..1024 {
            let values = std::array::from_fn(|i| {
                std::array::from_fn(|lane| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let bits = (seed >> 32) as u32;
                    match round % 4 {
                        0 => f32::from_bits(bits & 0xfeffffff),
                        1 => f32::from_bits(bits & 0x807fffff),
                        2 => {
                            if (lane + i) % 2 == 0 {
                                -0.
                            } else {
                                0.
                            }
                        }
                        _ => [
                            f32::NEG_INFINITY,
                            -f32::MAX,
                            -1.,
                            -0.,
                            0.,
                            1.,
                            f32::MAX,
                            f32::INFINITY,
                        ][(lane + i) % 8],
                    }
                })
            });
            // SAFETY: AVX2 explicitly checked by require_vector().
            unsafe { check(values) };
        }
    }
}
