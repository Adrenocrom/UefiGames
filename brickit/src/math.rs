//! Hand-rolled floating-point math.
//!
//! `no_std` `core` does not provide the transcendental and rounding methods
//! that `std` gets from the system libm (`sin`, `cos`, `floor`, `ceil`, ...).
//! The game only needs them for bounce angles and circle rasterization, where
//! a Taylor-series approximation with quadrant reduction is more than
//! accurate enough (max error ~3e-5) and avoids pulling in the `libm` crate.

use core::f32::consts::PI;

const TAU: f32 = 2.0 * PI;

/// Reduce an angle to `[0, 2π)`.
fn reduce(x: f32) -> f32 {
    let mut r = x % TAU;
    if r < 0.0 {
        r += TAU;
    }
    r
}

/// Cosine, evaluated as a Taylor series on `[0, π/2]`.
///
/// Symmetries used for range reduction: cos(2π - x) = cos(x) and
/// cos(π - x) = -cos(x).
pub fn cos(x: f32) -> f32 {
    let mut x = reduce(x);
    if x >= PI {
        x = TAU - x; // mirror [π, 2π) onto [0, π]
    }
    let negate = x > PI / 2.0;
    if negate {
        x = PI - x; // mirror [π/2, π] onto [0, π/2], flipping the sign
    }
    let x2 = x * x;
    // 1 - x²/2! + x⁴/4! - x⁶/6! + x⁸/8! in Horner form.
    let r = 1.0 - x2 * (0.5 - x2 * (1.0 / 24.0 - x2 * (1.0 / 720.0 - x2 / 40320.0)));
    if negate { -r } else { r }
}

/// Sine, evaluated as a Taylor series on `[0, π/2]`.
///
/// Symmetries used for range reduction: sin(2π - x) = -sin(x) and
/// sin(π - x) = sin(x).
pub fn sin(x: f32) -> f32 {
    let mut x = reduce(x);
    let mut negate = false;
    if x >= PI {
        x = TAU - x;
        negate = true;
    }
    if x > PI / 2.0 {
        x = PI - x;
    }
    let x2 = x * x;
    // x - x³/3! + x⁵/5! - x⁷/7! + x⁹/9! in Horner form.
    let r = x * (1.0 - x2 * (1.0 / 6.0 - x2 * (1.0 / 120.0 - x2 * (1.0 / 5040.0 - x2 / 362880.0))));
    if negate { -r } else { r }
}

/// Floor: truncate toward zero, then step down if a fraction was cut off.
pub fn floor(x: f32) -> f32 {
    let t = x as i64 as f32;
    if t > x { t - 1.0 } else { t }
}

/// Ceil: truncate toward zero, then step up if a fraction was cut off.
pub fn ceil(x: f32) -> f32 {
    let t = x as i64 as f32;
    if t < x { t + 1.0 } else { t }
}