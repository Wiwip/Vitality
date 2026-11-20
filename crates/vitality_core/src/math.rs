pub trait AbsDiff<Rhs = Self> {
    type Output;
    fn abs_diff(self, rhs: Rhs) -> Self::Output;
    fn are_different(self, rhs: Rhs) -> bool;
}

macro_rules! impl_abs_diff_for_signed_integers {
    ($($t:ty),*) => {
        $(
            impl AbsDiff for $t {
                type Output = $t;
                fn abs_diff(self, rhs: $t) -> $t {
                    (self - rhs).abs()
                }
                fn are_different(self, rhs: $t) -> bool {
                    self != rhs
                }
            }
        )*
    };
}

macro_rules! impl_abs_diff_for_floats {
    ($($t:ty),*) => {
        $(
            impl AbsDiff for $t {
                type Output = $t;
                fn abs_diff(self, rhs: $t) -> $t {
                    (self - rhs).abs()
                }
                // No epsilon, and `NaN != NaN`, so a NaN resolve always reads as changed and fans out.
                fn are_different(self, rhs: $t) -> bool {
                    self != rhs
                }
            }
        )*
    };
}

macro_rules! impl_abs_diff_for_unsigned {
    ($($t:ty),*) => {
        $(
            impl AbsDiff for $t {
                type Output = $t;
                fn abs_diff(self, rhs: $t) -> $t {
                    self.abs_diff(rhs)
                }
                fn are_different(self, rhs: $t) -> bool {
                    self != rhs
                }
            }
        )*
    };
}

impl_abs_diff_for_signed_integers!(i8, i16, i32, i64, i128, isize);
impl_abs_diff_for_floats!(f32, f64);
impl_abs_diff_for_unsigned!(u8, u16, u32, u64, u128, usize);

pub trait SaturatingAttributes<Rhs = Self> {
    type Output;
    fn saturating_add(self, rhs: Rhs) -> Self::Output;
    fn saturating_sub(self, rhs: Rhs) -> Self::Output;
}

macro_rules! impl_saturating_sub_for_signed_integers {
    ($($t:ty),*) => {
        $(
            impl SaturatingAttributes for $t {
                type Output = $t;
                fn saturating_add(self, rhs: Self::Output) -> Self::Output {
                    self.saturating_add(rhs)
                }
                fn saturating_sub(self, rhs: Self::Output) -> Self::Output {
                    self.saturating_sub(rhs)
                }
            }
        )*
    };
}

macro_rules! impl_saturating_sub_for_floats {
    ($($t:ty),*) => {
        $(
            impl SaturatingAttributes for $t {
                type Output = $t;
                fn saturating_add(self, rhs: Self::Output) -> Self::Output {
                    self + rhs
                }
                fn saturating_sub(self, rhs: Self::Output) -> Self::Output {
                    self - rhs
                }
            }
        )*
    };
}

macro_rules! impl_saturating_sub_for_unsigned {
    ($($t:ty),*) => {
        $(
            impl SaturatingAttributes for $t {
                type Output = $t;
                fn saturating_add(self, rhs: Self::Output) -> Self::Output {
                    self.saturating_add(rhs)
                }
                fn saturating_sub(self, rhs: Self::Output) -> Self::Output {
                    self.saturating_sub(rhs)
                }
            }
        )*
    };
}

impl_saturating_sub_for_signed_integers!(i8, i16, i32, i64, i128, isize);
impl_saturating_sub_for_floats!(f32, f64);
impl_saturating_sub_for_unsigned!(u8, u16, u32, u64, u128, usize);

#[cfg(test)]
mod tests {
    use super::{AbsDiff, SaturatingAttributes};

    #[test]
    fn are_different_is_equality() {
        assert!(i8::MIN.are_different(i8::MAX));
        assert!(i32::MIN.are_different(i32::MAX));
        assert!(!(7u32).are_different(7u32));
        assert!((7u32).are_different(8u32));
        assert!(1.0f32.are_different(f32::NAN));
        assert!(!(1.0f32.are_different(1.0f32)));
        assert!(f32::NAN.are_different(f32::NAN));
    }

    // UFCS, the inherent saturating_add would win method resolution.
    #[test]
    fn saturating_clamps_integers_and_leaves_floats_alone() {
        assert_eq!(
            <i8 as SaturatingAttributes>::saturating_add(i8::MAX, 1),
            i8::MAX
        );
        assert_eq!(
            <i8 as SaturatingAttributes>::saturating_sub(i8::MIN, 1),
            i8::MIN
        );
        assert_eq!(
            <u8 as SaturatingAttributes>::saturating_add(u8::MAX, 1),
            u8::MAX
        );
        assert_eq!(<u8 as SaturatingAttributes>::saturating_sub(u8::MIN, 1), 0);
        assert_eq!(
            <f32 as SaturatingAttributes>::saturating_add(f32::MAX, f32::MAX),
            f32::INFINITY
        );
        assert_eq!(
            <f32 as SaturatingAttributes>::saturating_sub(-f32::MAX, f32::MAX),
            f32::NEG_INFINITY
        );
    }
}
