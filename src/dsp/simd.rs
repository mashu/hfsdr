//! SIMD helpers for f32 DSP (preserves Airspy float dynamic range).

use crate::source::Complex32;

#[inline]
pub fn complex_mul(a: Complex32, b: Complex32) -> Complex32 {
    Complex32 {
        re: a.re * b.re - a.im * b.im,
        im: a.re * b.im + a.im * b.re,
    }
}

/// Fused complex multiply for a block: `out[i] = a[i] * b[i]`.
#[inline]
pub fn complex_mul_block(a: &[Complex32], b: &[Complex32], out: &mut [Complex32]) {
    debug_assert_eq!(a.len(), b.len());
    debug_assert_eq!(a.len(), out.len());
    let n = a.len();
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if n >= 4 && std::arch::is_x86_feature_detected!("avx") {
            unsafe {
                while i + 4 <= n {
                    complex_mul_x4(
                        a.as_ptr().add(i),
                        b.as_ptr().add(i),
                        out.as_mut_ptr().add(i),
                    );
                    i += 4;
                }
            }
        } else if n >= 2 && std::arch::is_x86_feature_detected!("sse") {
            unsafe {
                while i + 2 <= n {
                    complex_mul_x2(
                        a.as_ptr().add(i),
                        b.as_ptr().add(i),
                        out.as_mut_ptr().add(i),
                    );
                    i += 2;
                }
            }
        }
    }

    while i < n {
        out[i] = complex_mul(a[i], b[i]);
        i += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
unsafe fn complex_mul_x4(a: *const Complex32, b: *const Complex32, out: *mut Complex32) {
    use std::arch::x86_64::*;

    let va = _mm256_loadu_ps(a.cast());
    let vb = _mm256_loadu_ps(b.cast());

    let ar = _mm256_shuffle_ps(va, va, 0b_10_00_10_00);
    let ai = _mm256_shuffle_ps(va, va, 0b_11_01_11_01);
    let br = _mm256_shuffle_ps(vb, vb, 0b_10_00_10_00);
    let bi = _mm256_shuffle_ps(vb, vb, 0b_11_01_11_01);

    let out_re = _mm256_sub_ps(_mm256_mul_ps(ar, br), _mm256_mul_ps(ai, bi));
    let out_im = _mm256_add_ps(_mm256_mul_ps(ar, bi), _mm256_mul_ps(ai, br));
    let packed = _mm256_unpacklo_ps(out_re, out_im);
    _mm256_storeu_ps(out.cast(), packed);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse")]
unsafe fn complex_mul_x2(a: *const Complex32, b: *const Complex32, out: *mut Complex32) {
    use std::arch::x86_64::*;

    let va = _mm_loadu_ps(a.cast());
    let vb = _mm_loadu_ps(b.cast());

    let ar = _mm_shuffle_ps(va, va, 0b_10_00_10_00);
    let ai = _mm_shuffle_ps(va, va, 0b_11_01_11_01);
    let br = _mm_shuffle_ps(vb, vb, 0b_10_00_10_00);
    let bi = _mm_shuffle_ps(vb, vb, 0b_11_01_11_01);

    let out_re = _mm_sub_ps(_mm_mul_ps(ar, br), _mm_mul_ps(ai, bi));
    let out_im = _mm_add_ps(_mm_mul_ps(ar, bi), _mm_mul_ps(ai, br));
    let packed = _mm_unpacklo_ps(out_re, out_im);
    _mm_storeu_ps(out.cast(), packed);
}

/// Dot product for symmetric FIR accumulation.
#[inline]
pub fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let n = a.len();
    if n == 0 {
        return 0.0;
    }

    let mut sum = 0.0f32;
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if n >= 8 && std::arch::is_x86_feature_detected!("avx") {
            unsafe {
                use std::arch::x86_64::*;
                let mut acc = _mm256_setzero_ps();
                while i + 8 <= n {
                    let va = _mm256_loadu_ps(a.as_ptr().add(i));
                    let vb = _mm256_loadu_ps(b.as_ptr().add(i));
                    acc = _mm256_add_ps(acc, _mm256_mul_ps(va, vb));
                    i += 8;
                }
                let hi = _mm256_extractf128_ps(acc, 1);
                let lo = _mm256_castps256_ps128(acc);
                let sum128 = _mm_add_ps(lo, hi);
                let shuf = _mm_movehdup_ps(sum128);
                let sums = _mm_add_ps(sum128, shuf);
                let shuf2 = _mm_movehl_ps(shuf, sums);
                let summed = _mm_add_ss(sums, shuf2);
                sum = _mm_cvtss_f32(summed);
            }
        } else if n >= 4 && std::arch::is_x86_feature_detected!("sse") {
            unsafe {
                use std::arch::x86_64::*;
                let mut acc = _mm_setzero_ps();
                while i + 4 <= n {
                    let va = _mm_loadu_ps(a.as_ptr().add(i));
                    let vb = _mm_loadu_ps(b.as_ptr().add(i));
                    acc = _mm_add_ps(acc, _mm_mul_ps(va, vb));
                    i += 4;
                }
                let shuf = _mm_movehdup_ps(acc);
                let sums = _mm_add_ps(acc, shuf);
                let shuf2 = _mm_movehl_ps(shuf, sums);
                let summed = _mm_add_ss(sums, shuf2);
                sum = _mm_cvtss_f32(summed);
            }
        }
    }

    while i < n {
        sum += a[i] * b[i];
        i += 1;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complex_mul_block_matches_scalar() {
        let a = [
            Complex32 { re: 0.5, im: -0.2 },
            Complex32 { re: 1.1, im: 0.3 },
            Complex32 { re: -0.4, im: 0.9 },
            Complex32 { re: 2.0, im: -1.0 },
            Complex32 { re: 0.1, im: 0.1 },
        ];
        let b = [
            Complex32 { re: 0.9, im: 0.1 },
            Complex32 { re: -0.2, im: 0.7 },
            Complex32 { re: 0.3, im: -0.5 },
            Complex32 { re: 1.0, im: 0.0 },
            Complex32 { re: -1.0, im: 2.0 },
        ];
        let mut out = vec![Complex32::default(); a.len()];
        complex_mul_block(&a, &b, &mut out);
        for i in 0..a.len() {
            let expect = complex_mul(a[i], b[i]);
            assert!(
                (out[i].re - expect.re).abs() < 1e-6 && (out[i].im - expect.im).abs() < 1e-6,
                "i={i} got {:?} want {:?}",
                out[i],
                expect
            );
        }
    }

    #[test]
    fn dot_f32_matches_scalar() {
        let a: Vec<f32> = (0..64).map(|i| (i as f32 * 0.13).sin()).collect();
        let b: Vec<f32> = (0..64).map(|i| (i as f32 * 0.07).cos()).collect();
        let expect: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let got = dot_f32(&a, &b);
        assert!((got - expect).abs() < 1e-4, "got={got} expect={expect}");
    }
}
