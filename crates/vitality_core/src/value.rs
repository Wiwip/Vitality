/// The number type an attribute folds in. Every fold step runs in this type, so a host type that
/// rounds differently from f64, or outgrows it, gets its own arithmetic back.
///
/// Call the ops as `P::add(a, b)`, so an inherent `add` or `std::ops::Add` can't take the call.
pub trait AttributeValue: Copy + PartialOrd + Send + Sync + 'static {
    fn zero() -> Self;
    fn one() -> Self;
    fn add(self, rhs: Self) -> Self;
    fn sub(self, rhs: Self) -> Self;
    fn mul(self, rhs: Self) -> Self;
    /// stacked `Mul` compounds here, never through a wider type
    fn powi(self, exponent: u32) -> Self;
    fn from_f64(value: f64) -> Self;
    /// lossy: only expressions, def files and debug read through it
    fn to_f64(self) -> f64;
}

macro_rules! attribute_value_floats {
    ($($t:ty),*) => {
        $(
            impl AttributeValue for $t {
                #[inline(always)] fn zero() -> Self { 0.0 }
                #[inline(always)] fn one() -> Self { 1.0 }
                #[inline(always)] fn add(self, rhs: Self) -> Self { self + rhs }
                #[inline(always)] fn sub(self, rhs: Self) -> Self { self - rhs }
                #[inline(always)] fn mul(self, rhs: Self) -> Self { self * rhs }
                // the same powi the erased table used, so an f64 attribute keeps its old bits
                #[inline(always)] fn powi(self, exponent: u32) -> Self {
                    (self as f64).powi(exponent as i32) as $t
                }
                #[inline(always)] fn from_f64(value: f64) -> Self { value as $t }
                #[inline(always)] fn to_f64(self) -> f64 { self as f64 }
            }
        )*
    };
}

macro_rules! attribute_value_signed {
    ($($t:ty),*) => {
        $(
            impl AttributeValue for $t {
                #[inline(always)] fn zero() -> Self { 0 }
                #[inline(always)] fn one() -> Self { 1 }
                #[inline(always)] fn add(self, rhs: Self) -> Self { self.saturating_add(rhs) }
                #[inline(always)] fn sub(self, rhs: Self) -> Self { self.saturating_sub(rhs) }
                #[inline(always)] fn mul(self, rhs: Self) -> Self { self.saturating_mul(rhs) }
                #[inline(always)] fn powi(self, exponent: u32) -> Self {
                    match self.checked_pow(exponent) {
                        Some(value) => value,
                        None => if self < 0 { Self::MIN } else { Self::MAX },
                    }
                }
                #[inline(always)] fn from_f64(value: f64) -> Self { value as $t }
                #[inline(always)] fn to_f64(self) -> f64 { self as f64 }
            }
        )*
    };
}

macro_rules! attribute_value_unsigned {
    ($($t:ty),*) => {
        $(
            impl AttributeValue for $t {
                #[inline(always)] fn zero() -> Self { 0 }
                #[inline(always)] fn one() -> Self { 1 }
                #[inline(always)] fn add(self, rhs: Self) -> Self { self.saturating_add(rhs) }
                #[inline(always)] fn sub(self, rhs: Self) -> Self { self.saturating_sub(rhs) }
                #[inline(always)] fn mul(self, rhs: Self) -> Self { self.saturating_mul(rhs) }
                #[inline(always)] fn powi(self, exponent: u32) -> Self {
                    self.checked_pow(exponent).unwrap_or(Self::MAX)
                }
                #[inline(always)] fn from_f64(value: f64) -> Self { value as $t }
                #[inline(always)] fn to_f64(self) -> f64 { self as f64 }
            }
        )*
    };
}

attribute_value_floats!(f32, f64);
attribute_value_signed!(i8, i16, i32, i64, i128, isize);
attribute_value_unsigned!(u8, u16, u32, u64, u128, usize);

/// The impls a host number type needs on top of [`AttributeValue`] to be a `Property`.
///
/// ```ignore
/// vitality_core::property_number!(Amount);
/// ```
#[macro_export]
macro_rules! property_number {
    ($t:ty) => {
        impl<C: $crate::express_it::expr::Context> $crate::express_it::expr::AsExpression<$t, C>
            for $t
        {
            type Target = $crate::express_it::nodes::LiteralNode<$t>;

            #[inline(always)]
            fn as_expr(self) -> Self::Target {
                $crate::express_it::nodes::LiteralNode { value: self }
            }
        }
    };
}
