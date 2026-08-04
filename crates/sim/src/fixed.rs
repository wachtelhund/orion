//! Q16.16 fixed-point arithmetic. The only number type the sim is allowed
//! to do math with. Positions are measured in *tiles* (1.0 = one tile).

use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

pub const FRAC_BITS: u32 = 16;
pub const ONE_RAW: i32 = 1 << FRAC_BITS;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct Fx(pub i32);

impl Fx {
    pub const ZERO: Fx = Fx(0);
    pub const ONE: Fx = Fx(ONE_RAW);
    pub const HALF: Fx = Fx(ONE_RAW / 2);
    pub const MAX: Fx = Fx(i32::MAX);

    #[inline]
    pub const fn from_int(v: i32) -> Fx {
        Fx(v << FRAC_BITS)
    }

    /// `v / d` as fixed. For constants like 3/10.
    #[inline]
    pub const fn from_ratio(v: i32, d: i32) -> Fx {
        Fx(((v as i64 * ONE_RAW as i64) / d as i64) as i32)
    }

    /// Data loading only. f64 decimal parsing + this conversion are exact and
    /// platform-identical, so converting content values at load is safe.
    pub fn from_f64_data(v: f64) -> Fx {
        Fx((v * ONE_RAW as f64).round() as i32)
    }

    /// Truncate toward negative infinity to an integer (tile coordinate).
    #[inline]
    pub const fn floor_int(self) -> i32 {
        self.0 >> FRAC_BITS
    }

    #[inline]
    pub const fn round_int(self) -> i32 {
        (self.0 + ONE_RAW / 2) >> FRAC_BITS
    }

    /// Renderer-side only.
    #[inline]
    pub fn to_f32(self) -> f32 {
        self.0 as f32 / ONE_RAW as f32
    }

    #[inline]
    pub const fn abs(self) -> Fx {
        Fx(self.0.abs())
    }

    #[inline]
    pub fn min(self, o: Fx) -> Fx {
        Fx(self.0.min(o.0))
    }

    #[inline]
    pub fn max(self, o: Fx) -> Fx {
        Fx(self.0.max(o.0))
    }

    #[inline]
    pub fn clamp(self, lo: Fx, hi: Fx) -> Fx {
        Fx(self.0.clamp(lo.0, hi.0))
    }

    /// Deterministic square root. sqrt of Q16.16 = isqrt(raw << 16).
    pub fn sqrt(self) -> Fx {
        debug_assert!(self.0 >= 0, "sqrt of negative fixed");
        if self.0 <= 0 {
            return Fx::ZERO;
        }
        Fx(isqrt_u64((self.0 as u64) << FRAC_BITS) as i32)
    }
}

/// Integer square root, deterministic, no floats.
pub fn isqrt_u64(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    // Newton's method with a good initial guess from leading_zeros.
    let mut x = 1u64 << ((64 - v.leading_zeros()).div_ceil(2));
    loop {
        let nx = (x + v / x) >> 1;
        if nx >= x {
            break;
        }
        x = nx;
    }
    x
}

impl Add for Fx {
    type Output = Fx;
    #[inline]
    fn add(self, o: Fx) -> Fx {
        Fx(self.0 + o.0)
    }
}
impl Sub for Fx {
    type Output = Fx;
    #[inline]
    fn sub(self, o: Fx) -> Fx {
        Fx(self.0 - o.0)
    }
}
impl Neg for Fx {
    type Output = Fx;
    #[inline]
    fn neg(self) -> Fx {
        Fx(-self.0)
    }
}
impl Mul for Fx {
    type Output = Fx;
    #[inline]
    fn mul(self, o: Fx) -> Fx {
        // Truncate toward zero (NOT an arithmetic shift, which floors):
        // (-a)*b must equal -(a*b) exactly, or mirrored halves of the map
        // simulate differently and spawn positions gain a bias.
        Fx(((self.0 as i64 * o.0 as i64) / (1i64 << FRAC_BITS)) as i32)
    }
}
impl Div for Fx {
    type Output = Fx;
    #[inline]
    fn div(self, o: Fx) -> Fx {
        debug_assert!(o.0 != 0, "fixed divide by zero");
        Fx((((self.0 as i64) << FRAC_BITS) / o.0 as i64) as i32)
    }
}
impl AddAssign for Fx {
    #[inline]
    fn add_assign(&mut self, o: Fx) {
        self.0 += o.0;
    }
}
impl SubAssign for Fx {
    #[inline]
    fn sub_assign(&mut self, o: Fx) {
        self.0 -= o.0;
    }
}
impl Mul<i32> for Fx {
    type Output = Fx;
    #[inline]
    fn mul(self, o: i32) -> Fx {
        Fx(((self.0 as i64 * o as i64) as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }
}

impl fmt::Debug for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}fx", self.0 as f64 / ONE_RAW as f64)
    }
}

/// 2D fixed-point vector.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
pub struct FxVec2 {
    pub x: Fx,
    pub y: Fx,
}

impl FxVec2 {
    pub const ZERO: FxVec2 = FxVec2 { x: Fx::ZERO, y: Fx::ZERO };

    #[inline]
    pub const fn new(x: Fx, y: Fx) -> FxVec2 {
        FxVec2 { x, y }
    }

    #[inline]
    pub const fn from_int(x: i32, y: i32) -> FxVec2 {
        FxVec2 { x: Fx::from_int(x), y: Fx::from_int(y) }
    }

    /// Center of a tile.
    #[inline]
    pub fn tile_center(tx: i32, ty: i32) -> FxVec2 {
        FxVec2 { x: Fx::from_int(tx) + Fx::HALF, y: Fx::from_int(ty) + Fx::HALF }
    }

    /// Squared length in raw i64 units (Q32.32). Overflow-safe for map-scale
    /// coordinates. Use for comparisons.
    #[inline]
    pub fn len_sq_raw(self) -> i64 {
        let x = self.x.0 as i64;
        let y = self.y.0 as i64;
        x * x + y * y
    }

    /// Length as fixed. sqrt(Q32.32) == Q16.16.
    #[inline]
    pub fn len(self) -> Fx {
        Fx(isqrt_u64(self.len_sq_raw() as u64) as i32)
    }

    /// Scale to the given length. Zero vector stays zero.
    pub fn scaled_to(self, target: Fx) -> FxVec2 {
        let l = self.len();
        if l.0 == 0 {
            return FxVec2::ZERO;
        }
        FxVec2 { x: self.x * target / l, y: self.y * target / l }
    }

    /// Clamp length to at most `max`.
    pub fn clamped(self, max: Fx) -> FxVec2 {
        let l = self.len();
        if l.0 <= max.0 {
            self
        } else {
            self.scaled_to(max)
        }
    }
}

impl Add for FxVec2 {
    type Output = FxVec2;
    #[inline]
    fn add(self, o: FxVec2) -> FxVec2 {
        FxVec2 { x: self.x + o.x, y: self.y + o.y }
    }
}
impl Sub for FxVec2 {
    type Output = FxVec2;
    #[inline]
    fn sub(self, o: FxVec2) -> FxVec2 {
        FxVec2 { x: self.x - o.x, y: self.y - o.y }
    }
}
impl Mul<Fx> for FxVec2 {
    type Output = FxVec2;
    #[inline]
    fn mul(self, s: Fx) -> FxVec2 {
        FxVec2 { x: self.x * s, y: self.y * s }
    }
}
impl AddAssign for FxVec2 {
    #[inline]
    fn add_assign(&mut self, o: FxVec2) {
        self.x += o.x;
        self.y += o.y;
    }
}

/// Distance between two points.
#[inline]
pub fn dist(a: FxVec2, b: FxVec2) -> Fx {
    (a - b).len()
}

/// Squared distance in raw Q32.32 units — cheap comparisons.
#[inline]
pub fn dist_sq_raw(a: FxVec2, b: FxVec2) -> i64 {
    (a - b).len_sq_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_arith() {
        let a = Fx::from_int(3);
        let b = Fx::from_ratio(1, 2);
        assert_eq!((a * b).floor_int(), 1);
        assert_eq!((a + b).round_int(), 4); // 3.5 rounds up
        assert_eq!((a / Fx::from_int(2)), Fx::from_ratio(3, 2));
    }

    #[test]
    fn sqrt_exact_squares() {
        for i in 0..100i32 {
            let v = Fx::from_int(i * i);
            assert_eq!(v.sqrt(), Fx::from_int(i), "sqrt({})", i * i);
        }
    }

    #[test]
    fn sqrt_monotonic() {
        let mut prev = Fx::ZERO;
        for raw in (0..10_000_000).step_by(9973) {
            let s = Fx(raw).sqrt();
            assert!(s >= prev);
            prev = s;
        }
    }

    #[test]
    fn vec_len() {
        let v = FxVec2::from_int(3, 4);
        assert_eq!(v.len(), Fx::from_int(5));
        let scaled = v.scaled_to(Fx::ONE);
        // |normalized| within 1/256 of 1.0
        assert!((scaled.len() - Fx::ONE).abs().0 < 256);
    }

    #[test]
    fn floor_negative() {
        assert_eq!(Fx::from_ratio(-1, 2).floor_int(), -1);
        assert_eq!(Fx::from_ratio(1, 2).floor_int(), 0);
    }
}
