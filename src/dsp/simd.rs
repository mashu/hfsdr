//! SIMD helpers for f32 DSP (preserves Airspy float dynamic range).

use crate::source::Complex32;

#[inline]
pub fn complex_mul(a: Complex32, b: Complex32) -> Complex32 {
    Complex32 {
        re: a.re * b.re - a.im * b.im,
        im: a.re * b.im + a.im * b.re,
    }
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
        if n >= 4 && std::arch::is_x86_feature_detected!("sse") {
            unsafe {
                use std::arch::x86_64::*;
                let mut acc = _mm_setzero_ps();
                while i + 4 <= n {
                    let va = _mm_loadu_ps(a.as_ptr().add(i));
                    let vb = _mm_loadu_ps(b.as_ptr().add(i));
                    acc = _mm_add_ps(acc, _mm_mul_ps(va, vb));
                    i += 4;
                }
                let mut tmp = [0.0f32; 4];
                _mm_storeu_ps(tmp.as_mut_ptr(), acc);
                sum = tmp[0] + tmp[1] + tmp[2] + tmp[3];
            }
        }
    }

    while i < n {
        sum += a[i] * b[i];
        i += 1;
    }
    sum
}
