//! Outward-rounded binary64 interval arithmetic. Trigonometry uses an
//! outward-rounded Taylor enclosure, not the platform libm error estimate.
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive};
use std::ops::{Add, Div, Mul, Neg, Sub};
use std::{str::FromStr, sync::OnceLock};

#[derive(Clone, Copy, Debug)]
pub(super) struct I {
    pub lo: f64,
    pub hi: f64,
}
impl I {
    pub fn point(x: f64) -> Self {
        Self { lo: x, hi: x }
    }
    pub fn new(lo: f64, hi: f64) -> Self {
        Self { lo, hi }
    }
    pub fn entire() -> Self {
        Self::new(f64::NEG_INFINITY, f64::INFINITY)
    }
    pub fn mid(self) -> f64 {
        self.lo * 0.5 + self.hi * 0.5
    }
    pub fn width(self) -> f64 {
        (self.hi - self.lo).next_up()
    }
    pub fn radius(self) -> f64 {
        let m = self.mid();
        (m - self.lo).next_up().max((self.hi - m).next_up())
    }
    pub fn finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite() && self.lo <= self.hi
    }
    pub fn contains_zero(self) -> bool {
        self.lo <= 0. && self.hi >= 0.
    }
    pub fn overlap(self, b: Self) -> bool {
        self.lo <= b.hi && b.lo <= self.hi
    }
    pub fn hull(self, b: Self) -> Self {
        Self::new(self.lo.min(b.lo), self.hi.max(b.hi))
    }
    pub fn absmax(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }
    pub fn square(self) -> Self {
        if self.contains_zero() {
            Self::new(0., (self.absmax() * self.absmax()).next_up())
        } else {
            self * self
        }
    }
    pub fn sqrt(self) -> Self {
        if self.hi < 0. {
            return Self::entire();
        }
        Self::new(
            self.lo.max(0.).sqrt().next_down().max(0.),
            self.hi.max(0.).sqrt().next_up(),
        )
    }
    pub fn sin(self) -> Self {
        self.trig(false)
    }
    pub fn cos(self) -> Self {
        self.trig(true)
    }
    pub fn exp(self) -> Self {
        fn endpoint(x: f64) -> I {
            if !x.is_finite() || x.abs() > 700. {
                return I::entire();
            }
            let k = (x / std::f64::consts::LN_2).round() as i32;
            let logarithm = I::new(
                std::f64::consts::LN_2.next_down(),
                std::f64::consts::LN_2.next_up(),
            );
            let r = I::point(x) - logarithm * I::point(k as f64);
            if r.absmax() > 0.71 {
                return I::entire();
            }
            let mut sum = I::point(1.);
            let mut term = I::point(1.);
            for n in 1..=32 {
                term = term * r / I::point(n as f64);
                sum = sum + term;
            }
            // exp(|r|)|r|^33/33! < 1e-39 for |r|<=0.71.
            (sum + I::new(-1e-38, 1e-38)) * I::point(2f64.powi(k))
        }
        let a = endpoint(self.lo);
        let b = endpoint(self.hi);
        Self::new(a.lo, b.hi)
    }
    fn trig(self, cosine: bool) -> Self {
        if self.lo == 0. && self.hi == 0. {
            return Self::point(if cosine { 1. } else { 0. });
        }
        if !self.finite() || self.width() >= 6. || self.absmax() > 1e9 {
            return Self::new(-1., 1.);
        }
        // Both constants bracket the exact real value of pi. The quotient is
        // merely an integer choice; all reduction error stays in the interval.
        let tau = Self::new(
            std::f64::consts::TAU.next_down(),
            std::f64::consts::TAU.next_up(),
        );
        let turns = (self.mid() / std::f64::consts::TAU).round();
        let x = Self::point(self.mid()) - tau * Self::point(turns);
        if x.absmax() > 3.15 {
            return Self::new(-1., 1.);
        }
        let xx = x * x;
        let mut term = if cosine { Self::point(1.) } else { x };
        let mut sum = term;
        for k in 1..=24 {
            let n = if cosine { 2 * k } else { 2 * k + 1 };
            term = -(term * xx) / Self::point((n * (n - 1)) as f64);
            sum = sum + term;
        }
        // |x|<=3.15, 50! denominator: omitted absolute Taylor remainder
        // < 1e-38. Input radius is propagated using |sin'|,|cos'|<=1.
        let error = (self.radius() + 1e-38).next_up();
        let coarse = Self::new(
            (sum.lo - error).next_down().max(-1.),
            (sum.hi + error).next_up().min(1.),
        );
        if coarse.contains_zero() && self.width() < 1e-10 {
            precise_trig(self, cosine).unwrap_or(coarse)
        } else {
            coarse
        }
    }
}
fn precise_trig(value: I, cosine: bool) -> Option<I> {
    type Q = BigRational;
    static PI: OnceLock<Q> = OnceLock::new();
    let scale = Q::from_integer(10.into()).pow(60);
    let pi = PI.get_or_init(|| {
        Q::from_str("3141592653589793238462643383279502884197169399375105820974944").unwrap()
            / Q::from_integer(10.into()).pow(60)
    });
    let mid = value.mid();
    let turns = (mid / std::f64::consts::TAU).round();
    let x = Q::from_float(mid)? - pi * Q::from_integer(2.into()) * Q::from_float(turns)?;
    if x.abs().to_f64()? > 3.15 {
        return None;
    }
    let xx = &x * &x;
    let mut term = if cosine { Q::one() } else { x.clone() };
    let mut sum = term.clone();
    for k in 1..=40 {
        let n = if cosine { 2 * k } else { 2 * k + 1 };
        term = -term * &xx / Q::from_integer((n * (n - 1)).into());
        sum += &term;
    }
    // The exact real pi lies between the quoted 60-digit decimal and the
    // next decimal unit. The Taylor remainder at |x|<=3.15 is <10^-70.
    let error = Q::from_float(turns.abs())? * Q::from_integer(2.into()) / scale
        + Q::one() / Q::from_integer(10.into()).pow(70);
    let low = &sum - &error;
    let high = sum + error;
    let mut a = low.to_f64()?;
    let mut b = high.to_f64()?;
    for _ in 0..8 {
        if Q::from_float(a)? > low {
            a = a.next_down()
        } else {
            break;
        }
    }
    for _ in 0..8 {
        if Q::from_float(b)? < high {
            b = b.next_up()
        } else {
            break;
        }
    }
    if Q::from_float(a)? > low || Q::from_float(b)? < high {
        return None;
    }
    let radius = value.radius();
    Some(I::new(
        (a - radius).next_down().max(-1.),
        (b + radius).next_up().min(1.),
    ))
}
impl From<f64> for I {
    fn from(x: f64) -> Self {
        Self::point(x)
    }
}
impl Add for I {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        if self.lo == 0. && self.hi == 0. {
            return b;
        }
        if b.lo == 0. && b.hi == 0. {
            return self;
        }
        if self.lo == self.hi && b.lo == b.hi && self.lo == -b.lo {
            return Self::point(0.);
        }
        Self::new((self.lo + b.lo).next_down(), (self.hi + b.hi).next_up())
    }
}
impl Sub for I {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        self + (-b)
    }
}
impl Neg for I {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.hi, -self.lo)
    }
}
impl Mul for I {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        if (self.lo == 0. && self.hi == 0. && b.finite())
            || (b.lo == 0. && b.hi == 0. && self.finite())
        {
            return Self::point(0.);
        }
        if self.lo == 1. && self.hi == 1. {
            return b;
        }
        if b.lo == 1. && b.hi == 1. {
            return self;
        }
        let p = [
            self.lo * b.lo,
            self.lo * b.hi,
            self.hi * b.lo,
            self.hi * b.hi,
        ];
        if p.iter().any(|v| v.is_nan()) {
            return Self::entire();
        }
        Self::new(
            p.iter().copied().fold(f64::INFINITY, f64::min).next_down(),
            p.iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
                .next_up(),
        )
    }
}
impl Div for I {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        if b.lo == 1. && b.hi == 1. {
            return self;
        }
        if b.contains_zero() {
            Self::entire()
        } else {
            self * Self::new((1. / b.hi).next_down(), (1. / b.lo).next_up())
        }
    }
}

/// Value, first derivatives and second derivatives with respect to native UV.
#[derive(Clone, Copy, Debug)]
pub(super) struct J {
    pub x: I,
    pub d: [I; 2],
    pub dd: [[I; 2]; 2],
}
impl J {
    pub fn constant(x: I) -> Self {
        Self {
            x,
            d: [0.0.into(); 2],
            dd: [[0.0.into(); 2]; 2],
        }
    }
    pub fn variable(x: I, axis: usize) -> Self {
        let mut v = Self::constant(x);
        v.d[axis] = 1.0.into();
        v
    }
    pub fn sin(self) -> Self {
        self.compose(self.x.sin(), self.x.cos(), -self.x.sin())
    }
    pub fn cos(self) -> Self {
        self.compose(self.x.cos(), -self.x.sin(), -self.x.cos())
    }
    pub fn exp(self) -> Self {
        let value = self.x.exp();
        self.compose(value, value, value)
    }
    fn compose(self, value: I, first: I, second: I) -> Self {
        Self {
            x: value,
            d: self.d.map(|v| first * v),
            dd: std::array::from_fn(|i| {
                std::array::from_fn(|j| second * self.d[i] * self.d[j] + first * self.dd[i][j])
            }),
        }
    }
    fn reciprocal(self) -> Self {
        let one = I::point(1.);
        self.compose(
            one / self.x,
            -one / (self.x * self.x),
            I::point(2.) / (self.x * self.x * self.x),
        )
    }
}
impl From<f64> for J {
    fn from(x: f64) -> Self {
        Self::constant(x.into())
    }
}
impl Add for J {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self {
            x: self.x + b.x,
            d: std::array::from_fn(|i| self.d[i] + b.d[i]),
            dd: std::array::from_fn(|i| std::array::from_fn(|j| self.dd[i][j] + b.dd[i][j])),
        }
    }
}
impl Neg for J {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            d: self.d.map(|x| -x),
            dd: self.dd.map(|r| r.map(|x| -x)),
        }
    }
}
impl Sub for J {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        self + (-b)
    }
}
#[allow(clippy::suspicious_arithmetic_impl)] // Product rule for a second-order jet.
impl Mul for J {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        Self {
            x: self.x * b.x,
            d: std::array::from_fn(|i| self.d[i] * b.x + self.x * b.d[i]),
            dd: std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    self.dd[i][j] * b.x
                        + self.d[i] * b.d[j]
                        + self.d[j] * b.d[i]
                        + self.x * b.dd[i][j]
                })
            }),
        }
    }
}
#[allow(clippy::suspicious_arithmetic_impl)] // Division is multiplication by the reciprocal jet.
impl Div for J {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        self * b.reciprocal()
    }
}
