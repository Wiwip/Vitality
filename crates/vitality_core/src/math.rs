pub trait AbsDiff<Rhs = Self> {
    type Output;
    fn abs_diff(self, rhs: Rhs) -> Self::Output;
    fn are_different(self, rhs: Rhs) -> bool;
}

// Macro to implement AbsSub for signed integers
macro_rules! impl_abs_diff_for_signed_integers {
    ($($t:ty),*) => {
        $(
            impl AbsDiff for $t {
                type Output = $t;
                fn abs_diff(self, rhs: $t) -> $t {
                    (self - rhs).abs()
                }
                fn are_different(self, rhs: $t) -> bool {
                    self.abs_diff(rhs) >= 1
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
                fn are_different(self, rhs: $t) -> bool {
                    self.abs_diff(rhs) > Self::Output::MIN
                }
            }
        )*
    };
}

// Macro to implement AbsSub for unsigned integers
macro_rules! impl_abs_diff_for_unsigned {
    ($($t:ty),*) => {
        $(
            impl AbsDiff for $t {
                type Output = $t;
                fn abs_diff(self, rhs: $t) -> $t {
                    self.abs_diff(rhs)
                }
                fn are_different(self, rhs: $t) -> bool {
                    self.abs_diff(rhs) > Self::Output::MIN
                }
            }
        )*
    };
}

// Implement the trait for all the primitive integer types
impl_abs_diff_for_signed_integers!(i8, i16, i32, i64, i128, isize);
impl_abs_diff_for_floats!(f32, f64);
impl_abs_diff_for_unsigned!(u8, u16, u32, u64, u128, usize);

pub trait SaturatingAttributes<Rhs = Self> {
    type Output;
    fn saturating_add(self, rhs: Rhs) -> Self::Output;
    fn saturating_sub(self, rhs: Rhs) -> Self::Output;
}

// Macro to implement AbsSub for signed integers
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

// Macro to implement AbsSub for unsigned integers
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

// Implement the trait for all the primitive integer types
impl_saturating_sub_for_signed_integers!(i8, i16, i32, i64, i128, isize);
impl_saturating_sub_for_floats!(f32, f64);
impl_saturating_sub_for_unsigned!(u8, u16, u32, u64, u128, usize);
