//! Facilities for indexing bitmaps

use crate::ffi::{self};
#[cfg(doc)]
use crate::{
    cpu::cpusets::CpuSet,
    memory::nodesets::NodeSet,
    objects::TopologyObject,
    topology::{builder::BuildFlags, Topology},
};
use derive_more::{Binary, Display, LowerExp, LowerHex, Octal, UpperExp, UpperHex};
#[cfg(any(test, feature = "quickcheck"))]
use quickcheck::{Arbitrary, Gen};
#[cfg(any(test, feature = "quickcheck"))]
use rand::Rng;
use std::{
    borrow::Borrow,
    clone::Clone,
    cmp::Ordering,
    convert::TryFrom,
    ffi::{c_int, c_uint},
    fmt::Debug,
    iter::{Product, Sum},
    num::{ParseIntError, TryFromIntError},
    ops::{
        Add, AddAssign, BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Div,
        DivAssign, Mul, MulAssign, Not, Rem, RemAssign, Shl, ShlAssign, Shr, ShrAssign, Sub,
        SubAssign,
    },
    str::FromStr,
};

/// Bitmap indices can range from 0 to an implementation-defined limit
///
/// The limit is the upper bound of C's int type. On all platforms currently
/// supported by Rust, it is at least 32767 (2^15-1), and outside of exotic
/// 16-bit hardware, it will usually be greater than 2147483647 (2^31-1).
///
/// An alternate way to view `BitmapIndex` is as the intersection of integer
/// values permitted by C's int and unsigned int types.
///
/// # Operator overloads
///
/// All binary operators have an overload with exactly one of isize or usize
/// (depending on whether a negative operand makes sense) in order to allow
/// them to be used with integer literals without the type inference errors that
/// implementations for multiple integer types would bring.
///
/// The exception is left and right shifts: following the example of primitive
/// integer types, we overload these for all integer types and references
/// thereof.
///
/// Like primitive integer types, we overload all arithmetic operators for
/// references and values of each operand type for convenience. This convenience
/// does not extend to other operations like type conversions or comparisons.
///
/// Assuming a binary operator `A op B` is defined for two different types A and
/// B, we also define `B op A` if both operands play a symmetric role. We do
/// not generally do so otherwise as the result may be surprising (e.g. it
/// seems fair to expect `BitmapIndex << usize` to be a `BitmapIndex`, but by
/// the same logic `usize << BitmapIndex` should be an `usize`, not a
/// `BitmapIndex`).
#[derive(
    Binary,
    Clone,
    Copy,
    Debug,
    Default,
    Display,
    Eq,
    Hash,
    LowerExp,
    LowerHex,
    Octal,
    Ord,
    PartialEq,
    PartialOrd,
    UpperExp,
    UpperHex,
)]
pub struct BitmapIndex(c_uint);
//
impl BitmapIndex {
    /// The smallest value that can be used as a bitmap index
    pub const MIN: Self = Self(0);

    /// The zero of this integer type
    pub const ZERO: Self = Self(0);

    /// The 1 of this integer type
    pub const ONE: Self = Self(1);

    /// The largest value that can be used as a bitmap index
    pub const MAX: Self = Self(c_int::MAX as c_uint);

    /// Effective size of this integer type in bits
    ///
    /// The actual storage uses more bits for hardware reasons, which is why
    /// this is not called BITS like the other integer::BITS as that could be
    /// misinterpreted by careless users.
    pub const EFFECTIVE_BITS: u32 = c_uint::BITS - 1;

    /// Converts a string slice in a given base to an integer
    ///
    /// The string is expected to be an optional `+` sign followed by digits.
    /// Leading and trailing whitespace represent an error. Digits are a subset
    /// of these characters, depending on `radix`:
    ///
    /// * `0-9`
    /// * `a-z`
    /// * `A-Z`
    ///
    /// # Panics
    ///
    /// This function panics if `radix` is not in the range from 2 to 36.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(BitmapIndex::from_str_radix("0", 16), Ok(BitmapIndex::ZERO));
    /// ```
    pub fn from_str_radix(src: &str, radix: u32) -> Result<Self, ParseIntError> {
        // This convoluted impl is needed because I can't construct ParseIntError
        c_int::from_str_radix(src, radix).and_then(|_| c_uint::from_str_radix(src, radix).map(Self))
    }

    /// Returns the number of ones in the binary representation of `self`
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(BitmapIndex::ZERO.count_ones(), 0);
    /// assert_eq!(BitmapIndex::ONE.count_ones(), 1);
    /// assert_eq!(BitmapIndex::MAX.count_ones(), BitmapIndex::EFFECTIVE_BITS);
    /// ```
    pub const fn count_ones(self) -> u32 {
        self.0.count_ones()
    }

    /// Returns the number of zeros in the binary representation of `self`
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(BitmapIndex::ZERO.count_zeros(), BitmapIndex::EFFECTIVE_BITS);
    /// assert_eq!(BitmapIndex::ONE.count_zeros(), BitmapIndex::EFFECTIVE_BITS - 1);
    /// assert_eq!(BitmapIndex::MAX.count_zeros(), 0);
    /// ```
    pub const fn count_zeros(self) -> u32 {
        self.0.count_zeros() - 1
    }

    /// Returns the number of leading zeros in the binary representation of
    /// `self`.
    ///
    /// Depending on what you’re doing with the value, you might also be
    /// interested in the `ilog2` function which returns a consistent number,
    /// even if the type widens.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(BitmapIndex::ZERO.leading_zeros(), BitmapIndex::EFFECTIVE_BITS);
    /// assert_eq!(BitmapIndex::ONE.leading_zeros(), BitmapIndex::EFFECTIVE_BITS - 1);
    /// assert_eq!(BitmapIndex::MAX.leading_zeros(), 0);
    /// ```
    pub const fn leading_zeros(self) -> u32 {
        self.0.leading_zeros() - 1
    }

    /// Returns the number of trailing zeros in the binary representation of
    /// `self`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(BitmapIndex::ZERO.trailing_zeros(), BitmapIndex::EFFECTIVE_BITS);
    /// assert_eq!(BitmapIndex::ONE.trailing_zeros(), 0);
    /// assert_eq!(BitmapIndex::MAX.trailing_zeros(), 0);
    /// ```
    pub const fn trailing_zeros(self) -> u32 {
        if self.0 != 0 {
            self.0.trailing_zeros()
        } else {
            Self::EFFECTIVE_BITS
        }
    }

    /// Returns the number of leading ones in the binary representation of
    /// `self`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(BitmapIndex::ZERO.leading_ones(), 0);
    /// assert_eq!(BitmapIndex::ONE.leading_ones(), 0);
    /// assert_eq!(BitmapIndex::MAX.leading_ones(), BitmapIndex::EFFECTIVE_BITS);
    /// ```
    pub const fn leading_ones(self) -> u32 {
        (self.0 << 1).leading_ones()
    }

    /// Returns the number of trailing ones in the binary representation of
    /// `self`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(BitmapIndex::ZERO.trailing_ones(), 0);
    /// assert_eq!(BitmapIndex::ONE.trailing_ones(), 1);
    /// assert_eq!(BitmapIndex::MAX.trailing_ones(), BitmapIndex::EFFECTIVE_BITS);
    /// ```
    pub const fn trailing_ones(self) -> u32 {
        self.0.trailing_ones()
    }

    /// Shifts the bits to the left by a specified amount, `n`, wrapping the
    /// truncated bits to the end of the resulting integer.
    ///
    /// Please note this isn’t the same operation as the `<<` shifting operator!
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.rotate_left(129),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.rotate_left(129),
    ///     BitmapIndex::ONE << (129 % BitmapIndex::EFFECTIVE_BITS)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.rotate_left(129),
    ///     BitmapIndex::MAX
    /// );
    /// ```
    pub const fn rotate_left(self, n: u32) -> Self {
        self.rotate_impl(n, true)
    }

    /// Shifts the bits to the right by a specified amount, `n`, wrapping the
    /// truncated bits to the beginning of the resulting integer.
    ///
    /// Please note this isn’t the same operation as the `>>` shifting operator!
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.rotate_right(129),
    ///     BitmapIndex::ZERO
    /// );
    /// let effective_rotate = 129 % BitmapIndex::EFFECTIVE_BITS;
    /// assert_eq!(
    ///     BitmapIndex::ONE.rotate_right(129),
    ///     BitmapIndex::ONE << (BitmapIndex::EFFECTIVE_BITS - effective_rotate)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.rotate_right(129),
    ///     BitmapIndex::MAX
    /// );
    /// ```
    pub const fn rotate_right(self, n: u32) -> Self {
        self.rotate_impl(n, false)
    }

    // Common preparation of rotate_xyz operations
    #[inline]
    const fn rotate_impl(self, n: u32, left: bool) -> Self {
        // We model a rotation as the boolean OR of two bitshifts going in
        // opposite directions:
        // - The direct shift is applied to bits that are just being shifted in
        //   the direction of the rotation, in said direction.
        // - The opposite shift is applied the bits that are brought to the
        //   opposite side of the binary representation by the rotation process,
        //   pushing them in the opposite direction by the expected amount.
        let direct_shift = n % Self::EFFECTIVE_BITS;
        let opposite_shift = Self::EFFECTIVE_BITS - direct_shift;
        let (left_shift, right_shift) = if left {
            (direct_shift, opposite_shift)
        } else {
            (opposite_shift, direct_shift)
        };

        // Compute and composite the low and high order bits
        // Must mask out the high order bit to honor our expected
        // 15/31/63-bit unsigned integer semantics.
        let high_order_bits = (self.0 << left_shift) & Self::MAX.0;
        let low_order_bits = self.0 >> right_shift;
        Self(high_order_bits | low_order_bits)
    }

    // NOTE: No swap_bytes operation, the modeled integer is not made of an
    //       integral number of bytes so this operation does not make sense.

    /// Reverses the order of bits in the integer. The least significant bit
    /// becomes the most significant bit, second least-significant bit becomes
    /// second most-significant bit, etc.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.reverse_bits(),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.reverse_bits(),
    ///     BitmapIndex::ONE << BitmapIndex::EFFECTIVE_BITS - 1
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.reverse_bits(),
    ///     BitmapIndex::MAX
    /// );
    /// ```
    pub const fn reverse_bits(self) -> Self {
        Self(self.0.reverse_bits() >> 1)
    }

    // NOTE: No (from|to)_(be|le) operation, the modeled integer is not made of
    //       an integral number of bytes so these operations do not make sense.

    /// Checked integer addition. Computes `self + rhs`, returning `None` if
    /// overflow occurred.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_add(BitmapIndex::ZERO),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_add(BitmapIndex::MAX),
    ///     Some(BitmapIndex::MAX)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_add(BitmapIndex::ONE),
    ///     BitmapIndex::try_from(2).ok()
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_add(BitmapIndex::MAX),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_add(BitmapIndex::ZERO),
    ///     Some(BitmapIndex::MAX)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_add(BitmapIndex::ONE),
    ///     None
    /// );
    /// ```
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        let Some(inner) = self.0.checked_add(rhs.0) else {
            return None;
        };
        Self::const_try_from_c_uint(inner)
    }

    /// Checked addition with a signed integer. Computes `self + rhs`, returning
    /// `None` if overflow occurred.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_add_signed(0),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_add_signed(1),
    ///     Some(BitmapIndex::ONE)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MIN.checked_add_signed(-1),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_add_signed(0),
    ///     Some(BitmapIndex::MAX)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_add_signed(1),
    ///     None
    /// );
    /// ```
    pub const fn checked_add_signed(self, rhs: isize) -> Option<Self> {
        let Some(rhs) = Self::try_c_int_from_isize(rhs) else {
            return None;
        };
        let Some(inner) = self.0.checked_add_signed(rhs) else {
            return None;
        };
        Self::const_try_from_c_uint(inner)
    }

    /// Try to convert from isize to c_int
    ///
    /// Will be dropped once TryFrom/TryInto is usable in const fn
    const fn try_c_int_from_isize(x: isize) -> Option<c_int> {
        if x >= c_int::MIN as isize && x <= c_int::MAX as isize {
            Some(x as c_int)
        } else {
            None
        }
    }

    /// Checked integer subtraction. Computes `self - rhs`, returning `None` if
    /// overflow occurred.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_sub(BitmapIndex::ZERO),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MIN.checked_sub(BitmapIndex::ONE),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_sub(BitmapIndex::ZERO),
    ///     Some(BitmapIndex::MAX)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_sub(BitmapIndex::MAX),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// ```
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        if let Some(inner) = self.0.checked_sub(rhs.0) {
            Some(Self(inner))
        } else {
            None
        }
    }

    /// Checked integer multiplication. Computes `self * rhs`, returning `None`
    /// if overflow occurred.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_mul(BitmapIndex::ONE),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_mul(BitmapIndex::MAX),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_mul(BitmapIndex::ONE),
    ///     Some(BitmapIndex::ONE)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_mul(BitmapIndex::MAX),
    ///     Some(BitmapIndex::MAX)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_mul(BitmapIndex::ZERO),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_mul(BitmapIndex::MAX),
    ///     None
    /// );
    /// ```
    pub const fn checked_mul(self, rhs: Self) -> Option<Self> {
        let Some(inner) = self.0.checked_mul(rhs.0) else {
            return None;
        };
        Self::const_try_from_c_uint(inner)
    }

    /// Checked integer division. Computes `self / rhs`, returning `None`
    /// if `rhs == Self::ZERO`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_div(BitmapIndex::ZERO),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_div(BitmapIndex::ONE),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_div(BitmapIndex::ZERO),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_div(BitmapIndex::MAX),
    ///     Some(BitmapIndex::ONE)
    /// );
    /// ```
    pub const fn checked_div(self, rhs: Self) -> Option<Self> {
        if let Some(inner) = self.0.checked_div(rhs.0) {
            Some(Self(inner))
        } else {
            None
        }
    }

    /// Checked Euclidean division. Computes `self / rhs`, returning `None`
    /// if `rhs == Self::ZERO`. Equivalent to integer division for this type.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_div_euclid(BitmapIndex::ZERO),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_div_euclid(BitmapIndex::ONE),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_div_euclid(BitmapIndex::ZERO),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_div_euclid(BitmapIndex::MAX),
    ///     Some(BitmapIndex::ONE)
    /// );
    /// ```
    pub const fn checked_div_euclid(self, rhs: Self) -> Option<Self> {
        self.checked_div(rhs)
    }

    /// Checked integer remainder. Computes `self % rhs`, returning `None`
    /// if `rhs == Self::ZERO`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_rem(BitmapIndex::ZERO),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_rem(BitmapIndex::ONE),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_rem(BitmapIndex::ZERO),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_rem(BitmapIndex::MAX),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// ```
    pub const fn checked_rem(self, rhs: Self) -> Option<Self> {
        if let Some(inner) = self.0.checked_rem(rhs.0) {
            Some(Self(inner))
        } else {
            None
        }
    }

    /// Checked Euclidean remainder. Computes `self % rhs`, returning `None`
    /// if `rhs == Self::ZERO`. Equivalent to integer remainder for this type.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_rem_euclid(BitmapIndex::ZERO),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_rem_euclid(BitmapIndex::ONE),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_rem_euclid(BitmapIndex::ZERO),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_rem_euclid(BitmapIndex::MAX),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// ```
    pub const fn checked_rem_euclid(self, rhs: Self) -> Option<Self> {
        self.checked_rem(rhs)
    }

    /// Returns the logarithm of the number with respect to an arbitrary base,
    /// rounded down.
    ///
    /// This method might not be optimized owing to implementation details;
    /// `ilog2` can produce results more efficiently for base 2, and `ilog10`
    /// can produce results more efficiently for base 10.
    ///
    /// # Panics
    ///
    /// This function will panic if `self` is zero, or if `base` is less than 2.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.ilog(BitmapIndex::MAX),
    ///     0
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.ilog(BitmapIndex::MAX),
    ///     1
    /// );
    /// ```
    pub const fn ilog(self, base: Self) -> u32 {
        self.0.ilog(base.0)
    }

    /// Returns the base 2 logarithm of the number, rounded down.
    ///
    /// # Panics
    ///
    /// This function will panic if `self` is zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.ilog2(),
    ///     0
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.ilog2(),
    ///     BitmapIndex::EFFECTIVE_BITS - 1
    /// );
    /// ```
    pub const fn ilog2(self) -> u32 {
        self.0.ilog2()
    }

    /// Returns the base 10 logarithm of the number, rounded down.
    ///
    /// # Panics
    ///
    /// This function will panic if `self` is zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.ilog10(),
    ///     0
    /// );
    /// assert_eq!(
    ///     BitmapIndex::try_from(100).unwrap().ilog10(),
    ///     2
    /// );
    /// ```
    pub const fn ilog10(self) -> u32 {
        self.0.ilog10()
    }

    /// Returns the logarithm of the number with respect to an arbitrary base,
    /// rounded down.
    ///
    /// Returns `None` if the number is zero, or if the base is not at least 2.
    ///
    /// This method might not be optimized owing to implementation details;
    /// `checked_ilog2` can produce results more efficiently for base 2, and
    /// `checked_ilog10` can produce results more efficiently for base 10.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_ilog(BitmapIndex::ONE),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_ilog(BitmapIndex::MAX),
    ///     Some(0)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_ilog(BitmapIndex::ZERO),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_ilog(BitmapIndex::MAX),
    ///     Some(1)
    /// );
    /// ```
    pub const fn checked_ilog(self, base: Self) -> Option<u32> {
        self.0.checked_ilog(base.0)
    }

    /// Returns the base 2 logarithm of the number, rounded down.
    ///
    /// Returns `None` if the number is zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_ilog2(),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_ilog2(),
    ///     Some(0)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_ilog2(),
    ///     Some(BitmapIndex::EFFECTIVE_BITS - 1)
    /// );
    /// ```
    pub const fn checked_ilog2(self) -> Option<u32> {
        self.0.checked_ilog2()
    }

    /// Returns the base 10 logarithm of the number, rounded down.
    ///
    /// Returns `None` if the number is zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_ilog10(),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_ilog10(),
    ///     Some(0)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::try_from(100).ok().and_then(BitmapIndex::checked_ilog10),
    ///     Some(2)
    /// );
    /// ```
    pub const fn checked_ilog10(self) -> Option<u32> {
        self.0.checked_ilog10()
    }

    /// Checked negation. Computes `-self`, returning `None` unless `self == 0`.
    ///
    /// Note that negating any positive integer will overflow.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_neg(),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_neg(),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_neg(),
    ///     None
    /// );
    /// ```
    pub const fn checked_neg(self) -> Option<Self> {
        if self.0 == 0 {
            Some(Self(0))
        } else {
            None
        }
    }

    /// Checked shift left. Computes `self << rhs`, returning `None` if `rhs` is
    /// larger than or equal to `Self::EFFECTIVE_BITS`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_shl(1),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_shl(BitmapIndex::EFFECTIVE_BITS),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_shl(1),
    ///     BitmapIndex::try_from(2).ok()
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_shl(0),
    ///     Some(BitmapIndex::MAX)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_shl(1),
    ///     Some((BitmapIndex::MAX / 2) * 2)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_shl(BitmapIndex::EFFECTIVE_BITS),
    ///     None
    /// );
    /// ```
    pub const fn checked_shl(self, rhs: u32) -> Option<Self> {
        if rhs < Self::EFFECTIVE_BITS {
            Some(Self((self.0 << rhs) & Self::MAX.0))
        } else {
            None
        }
    }

    /// Checked shift right. Computes `self >> rhs`, returning `None` if `rhs`
    /// is larger than or equal to `Self::EFFECTIVE_BITS`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_shr(1),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_shr(BitmapIndex::EFFECTIVE_BITS),
    ///     None
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_shr(1),
    ///     Some(BitmapIndex::ZERO)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_shr(0),
    ///     Some(BitmapIndex::MAX)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_shr(1),
    ///     Some(BitmapIndex::MAX / 2)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_shr(BitmapIndex::EFFECTIVE_BITS),
    ///     None
    /// );
    /// ```
    pub const fn checked_shr(self, rhs: u32) -> Option<Self> {
        if rhs < Self::EFFECTIVE_BITS {
            Some(Self(self.0 >> rhs))
        } else {
            None
        }
    }

    /// Checked exponentiation. Computes `self.pow(exp)`, returning `None` if
    /// overflow occurred.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_pow(0),
    ///     Some(BitmapIndex::ONE)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_pow(3),
    ///     Some(BitmapIndex::ONE)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_pow(1),
    ///     Some(BitmapIndex::MAX)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_pow(2),
    ///     None
    /// );
    /// ```
    pub const fn checked_pow(self, exp: u32) -> Option<Self> {
        let Some(inner) = self.0.checked_pow(exp) else {
            return None;
        };
        Self::const_try_from_c_uint(inner)
    }

    /// Saturating integer addition. Computes `self + rhs`, saturating at the
    /// numeric bounds instead of overflowing.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MIN.saturating_add(BitmapIndex::ZERO),
    ///     BitmapIndex::MIN
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.saturating_add(BitmapIndex::ZERO),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.saturating_add(BitmapIndex::MAX),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_add(BitmapIndex::MAX),
    ///     BitmapIndex::MAX
    /// );
    /// ```
    pub const fn saturating_add(self, rhs: Self) -> Self {
        let inner = self.0.saturating_add(rhs.0);
        Self::const_sat_from_c_uint(inner)
    }

    /// Saturating addition with a signed integer. Computes `self + rhs`,
    /// saturating at the numeric bounds instead of overflowing.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MIN.saturating_add_signed(0),
    ///     BitmapIndex::MIN
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MIN.saturating_add_signed(-1),
    ///     BitmapIndex::MIN
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_add_signed(0),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_add_signed(1),
    ///     BitmapIndex::MAX
    /// );
    /// ```
    pub const fn saturating_add_signed(self, rhs: isize) -> Self {
        const C_INT_MIN: isize = c_int::MIN as isize;
        const C_INT_MAX: isize = c_int::MAX as isize;
        let rhs = match rhs {
            C_INT_MIN..=C_INT_MAX => rhs as c_int,
            under if under < 0 => c_int::MIN,
            _over => c_int::MAX,
        };
        let inner = self.0.saturating_add_signed(rhs);
        Self::const_sat_from_c_uint(inner)
    }

    /// Saturating integer subtraction. Computes `self - rhs`, saturating at the
    /// numeric bounds instead of overflowing.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MIN.saturating_sub(BitmapIndex::ZERO),
    ///     BitmapIndex::MIN
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MIN.saturating_sub(BitmapIndex::MAX),
    ///     BitmapIndex::MIN
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_sub(BitmapIndex::ZERO),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_sub(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// ```
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    /// Saturating integer multiplication. Computes `self * rhs`, saturating at
    /// the numeric bounds instead of overflowing.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.saturating_mul(BitmapIndex::ZERO),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.saturating_mul(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_mul(BitmapIndex::ONE),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_mul(BitmapIndex::MAX),
    ///     BitmapIndex::MAX
    /// );
    /// ```
    pub const fn saturating_mul(self, rhs: Self) -> Self {
        let inner = self.0.saturating_mul(rhs.0);
        Self::const_sat_from_c_uint(inner)
    }

    /// Saturating integer division. Identical to `self / rhs` for this
    /// unsigned integer type
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.saturating_div(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_div(BitmapIndex::MAX),
    ///     BitmapIndex::ONE
    /// );
    /// ```
    pub const fn saturating_div(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }

    /// Saturating integer exponentiation. Computes `self.pow(rhs)`, saturating
    /// at the numeric bounds instead of overflowing.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.saturating_pow(0),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.saturating_pow(2),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.saturating_pow(3),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_pow(1),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.saturating_pow(2),
    ///     BitmapIndex::MAX
    /// );
    /// ```
    pub const fn saturating_pow(self, exp: u32) -> Self {
        let inner = self.0.saturating_pow(exp);
        Self::const_sat_from_c_uint(inner)
    }

    /// Saturating version of const_try_from_c_uint
    const fn const_sat_from_c_uint(inner: c_uint) -> Self {
        if inner <= Self::MAX.0 {
            Self(inner)
        } else {
            Self::MAX
        }
    }

    /// Wrapping (modular) addition. Computes `self + rhs`, wrapping around at
    /// the boundary of the type.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.wrapping_add(BitmapIndex::ZERO),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.wrapping_add(BitmapIndex::MAX),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.wrapping_add(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_add(BitmapIndex::ZERO),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_add(BitmapIndex::MAX),
    ///     BitmapIndex::MAX - 1
    /// );
    /// ```
    pub const fn wrapping_add(self, rhs: Self) -> Self {
        Self(self.0.wrapping_add(rhs.0) & Self::MAX.0)
    }

    /// Wrapping (modular) addition with a signed integer. Computes `self +
    /// rhs`, wrapping around at the boundary of the type.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MIN.wrapping_add_signed(0),
    ///     BitmapIndex::MIN
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MIN.wrapping_add_signed(-1),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_add_signed(0),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_add_signed(1),
    ///     BitmapIndex::MIN
    /// );
    /// ```
    pub const fn wrapping_add_signed(self, rhs: isize) -> Self {
        Self(self.0.wrapping_add_signed(rhs as _) & Self::MAX.0)
    }

    /// Wrapping (modular) subtraction. Computes `self - rhs`, wrapping around
    /// at the boundary of the type.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MIN.wrapping_sub(BitmapIndex::ZERO),
    ///     BitmapIndex::MIN
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MIN.wrapping_sub(BitmapIndex::ONE),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MIN.wrapping_sub(BitmapIndex::MAX),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_sub(BitmapIndex::ZERO),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_sub(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// ```
    pub const fn wrapping_sub(self, rhs: Self) -> Self {
        Self(self.0.wrapping_sub(rhs.0) & Self::MAX.0)
    }

    /// Wrapping (modular) multiplication. Computes `self * rhs`, wrapping
    /// around at the boundary of the type.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.wrapping_mul(BitmapIndex::ZERO),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.wrapping_mul(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_mul(BitmapIndex::ZERO),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_mul(BitmapIndex::MAX),
    ///     BitmapIndex::ONE
    /// );
    /// ```
    pub const fn wrapping_mul(self, rhs: Self) -> Self {
        Self(self.0.wrapping_mul(rhs.0) & Self::MAX.0)
    }

    /// Wrapping (modular) division. Computes `self / rhs`. Wrapped division on
    /// unsigned types is just normal division. There’s no way wrapping could
    /// ever happen. This function exists, so that all operations are accounted
    /// for in the wrapping operations.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.wrapping_div(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_div(BitmapIndex::MAX),
    ///     BitmapIndex::ONE
    /// );
    /// ```
    pub const fn wrapping_div(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }

    /// Wrapping Euclidean division. Computes `self.div_euclid(rhs)`. Wrapped
    /// division on unsigned types is just normal division. There’s no way
    /// wrapping could ever happen. This function exists, so that all
    /// operations are accounted for in the wrapping operations. Since, for the
    /// positive integers, all common definitions of division are equal, this
    /// is exactly equal to `self.wrapping_div(rhs)`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.wrapping_div_euclid(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_div_euclid(BitmapIndex::MAX),
    ///     BitmapIndex::ONE
    /// );
    /// ```
    pub const fn wrapping_div_euclid(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }

    /// Wrapping (modular) remainder. Computes `self % rhs`. Wrapped remainder
    /// calculation on unsigned types is just the regular remainder
    /// calculation. There’s no way wrapping could ever happen. This function
    /// exists, so that all operations are accounted for in the wrapping
    /// operations.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.wrapping_rem(BitmapIndex::MAX),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_rem(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// ```
    pub const fn wrapping_rem(self, rhs: Self) -> Self {
        Self(self.0 % rhs.0)
    }

    /// Wrapping Euclidean modulo. Computes `self.rem_euclid(rhs)`. Wrapped
    /// modulo calculation on unsigned types is just the regular remainder
    /// calculation. There’s no way wrapping could ever happen. This function
    /// exists, so that all operations are accounted for in the wrapping
    /// operations. Since, for the positive integers, all common definitions of
    /// division are equal, this is exactly equal to `self.wrapping_rem(rhs)`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.wrapping_rem_euclid(BitmapIndex::MAX),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_rem_euclid(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// ```
    pub const fn wrapping_rem_euclid(self, rhs: Self) -> Self {
        Self(self.0 % rhs.0)
    }

    /// Wrapping (modular) negation. Computes `-self`, wrapping around at the
    /// boundary of the type.
    ///
    /// Since unsigned types do not have negative equivalents all applications
    /// of this function will wrap (except for `-0`). For values smaller than
    /// the corresponding signed type’s maximum the result is the same as
    /// casting the corresponding signed value. Any larger values are
    /// equivalent to `MAX + 1 - (val - MAX - 1)` where `MAX` is the
    /// corresponding signed type’s maximum.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.wrapping_neg(),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.wrapping_neg(),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_neg(),
    ///     BitmapIndex::ONE
    /// );
    /// ```
    pub const fn wrapping_neg(self) -> Self {
        Self(self.0.wrapping_neg() & Self::MAX.0)
    }

    /// Panic-free bitwise shift-left; yields `self <<
    /// (rhs % Self::EFFECTIVE_BITS)`.
    ///
    /// Note that this is _not_ the same as a rotate-left; the RHS of a wrapping
    /// shift-left is restricted to the range of the type, rather than the bits
    /// shifted out of the LHS being returned to the other end. This type also
    /// implements a `rotate_left` function, which may be what
    /// you want instead.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_shl(BitmapIndex::EFFECTIVE_BITS - 1),
    ///     BitmapIndex::MAX << (BitmapIndex::EFFECTIVE_BITS - 1)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_shl(BitmapIndex::EFFECTIVE_BITS),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_shl(BitmapIndex::EFFECTIVE_BITS + 1),
    ///     BitmapIndex::MAX << 1
    /// );
    /// ```
    pub const fn wrapping_shl(self, rhs: u32) -> Self {
        let wrapped_rhs = rhs % Self::EFFECTIVE_BITS;
        Self((self.0 << wrapped_rhs) & Self::MAX.0)
    }

    /// Panic-free bitwise shift-right; yields `self >>
    /// (rhs % Self::EFFECTIVE_BITS)`.
    ///
    /// Note that this is _not_ the same as a rotate-right; the RHS of a
    /// wrapping shift-right is restricted to the range of the type, rather
    /// than the bits shifted out of the LHS being returned to the other end.
    /// This type also implements a `rotate_right` function, which may be what
    /// you want instead.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_shr(BitmapIndex::EFFECTIVE_BITS - 1),
    ///     BitmapIndex::MAX >> (BitmapIndex::EFFECTIVE_BITS - 1)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_shr(BitmapIndex::EFFECTIVE_BITS),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_shr(BitmapIndex::EFFECTIVE_BITS + 1),
    ///     BitmapIndex::MAX >> 1
    /// );
    /// ```
    pub const fn wrapping_shr(self, rhs: u32) -> Self {
        Self(self.0 >> (rhs % Self::EFFECTIVE_BITS))
    }

    /// Wrapping (modular) exponentiation. Computes `self.pow(exp)`, wrapping
    /// around at the boundary of the type.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.wrapping_pow(0),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.wrapping_pow(3),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_pow(1),
    ///     BitmapIndex::MAX
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.wrapping_pow(2),
    ///     BitmapIndex::ONE
    /// );
    /// ```
    pub const fn wrapping_pow(self, exp: u32) -> Self {
        Self(self.0.wrapping_pow(exp) & Self::MAX.0)
    }

    /// Calculates `self` + `rhs`
    ///
    /// Returns a tuple of the addition along with a boolean indicating whether
    /// an arithmetic overflow would occur. If an overflow would have occurred
    /// then the wrapped value is returned.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MIN.overflowing_add(BitmapIndex::ZERO),
    ///     (BitmapIndex::MIN, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.overflowing_add(BitmapIndex::MAX),
    ///     (BitmapIndex::MAX, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_add(BitmapIndex::ZERO),
    ///     (BitmapIndex::MAX, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_add(BitmapIndex::ONE),
    ///     (BitmapIndex::MIN, true)
    /// );
    /// ```
    pub const fn overflowing_add(self, rhs: Self) -> (Self, bool) {
        let overflow = rhs.0 > Self::MAX.0 - self.0;
        (self.wrapping_add(rhs), overflow)
    }

    /// Calculates `self` + `rhs` with a signed `rhs`.
    ///
    /// Returns a tuple of the addition along with a boolean indicating whether
    /// an arithmetic overflow would occur. If an overflow would have occurred
    /// then the wrapped value is returned.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MIN.overflowing_add_signed(0),
    ///     (BitmapIndex::MIN, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MIN.overflowing_add_signed(-1),
    ///     (BitmapIndex::MAX, true)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_add_signed(0),
    ///     (BitmapIndex::MAX, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_add_signed(1),
    ///     (BitmapIndex::MIN, true)
    /// );
    /// ```
    pub const fn overflowing_add_signed(self, rhs: isize) -> (Self, bool) {
        let overflow = (rhs > (Self::MAX.0 - self.0) as isize) || (rhs < -(self.0 as isize));
        (self.wrapping_add_signed(rhs), overflow)
    }

    /// Calculates `self` - `rhs`
    ///
    /// Returns a tuple of the addition along with a boolean indicating whether
    /// an arithmetic overflow would occur. If an overflow would have occurred
    /// then the wrapped value is returned.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MIN.overflowing_sub(BitmapIndex::ZERO),
    ///     (BitmapIndex::MIN, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MIN.overflowing_sub(BitmapIndex::ONE),
    ///     (BitmapIndex::MAX, true)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_sub(BitmapIndex::ZERO),
    ///     (BitmapIndex::MAX, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_sub(BitmapIndex::MAX),
    ///     (BitmapIndex::ZERO, false)
    /// );
    /// ```
    pub const fn overflowing_sub(self, rhs: Self) -> (Self, bool) {
        (self.wrapping_sub(rhs), rhs.0 > self.0)
    }

    /// Computes the absolute difference between `self` and `other`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// let big = BitmapIndex::MAX;
    /// let small = BitmapIndex::ONE;
    /// assert_eq!(
    ///     big.abs_diff(small),
    ///     big - small
    /// );
    /// assert_eq!(
    ///     small.abs_diff(big),
    ///     big - small
    /// );
    /// ```
    pub const fn abs_diff(self, other: Self) -> Self {
        Self(self.0.abs_diff(other.0))
    }

    /// Calculates the multiplication of `self` and `rhs`.
    ///
    /// Returns a tuple of the multiplication along with a boolean indicating
    /// whether an arithmetic overflow would occur. If an overflow would have
    /// occurred then the wrapped value is returned.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.overflowing_mul(BitmapIndex::ZERO),
    ///     (BitmapIndex::ZERO, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ZERO.overflowing_mul(BitmapIndex::MAX),
    ///     (BitmapIndex::ZERO, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_mul(BitmapIndex::ZERO),
    ///     (BitmapIndex::ZERO, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_mul(BitmapIndex::MAX),
    ///     (BitmapIndex::ONE, true)
    /// );
    /// ```
    pub const fn overflowing_mul(self, rhs: Self) -> (Self, bool) {
        let (inner, mut overflow) = self.0.overflowing_mul(rhs.0);
        overflow |= inner > Self::MAX.0;
        (Self(inner & Self::MAX.0), overflow)
    }

    /// Calculates the divisor when `self` is divided by `rhs`.
    ///
    /// Returns a tuple of the divisor along with a boolean indicating whether
    /// an arithmetic overflow would occur. Note that for unsigned integers
    /// overflow never occurs, so the second value is always `false`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.overflowing_div(BitmapIndex::MAX),
    ///     (BitmapIndex::ZERO, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_div(BitmapIndex::MAX),
    ///     (BitmapIndex::ONE, false)
    /// );
    /// ```
    pub const fn overflowing_div(self, rhs: Self) -> (Self, bool) {
        (Self(self.0 / rhs.0), false)
    }

    /// Calculates the quotient of Euclidean division `self.div_euclid(rhs)`.
    ///
    /// Returns a tuple of the divisor along with a boolean indicating whether
    /// an arithmetic overflow would occur. Note that for unsigned integers
    /// overflow never occurs, so the second value is always `false`. Since,
    /// for the positive integers, all common definitions of division are
    /// equal, this is exactly equal to `self.overflowing_div(rhs)`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.overflowing_div_euclid(BitmapIndex::MAX),
    ///     (BitmapIndex::ZERO, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_div_euclid(BitmapIndex::MAX),
    ///     (BitmapIndex::ONE, false)
    /// );
    /// ```
    pub const fn overflowing_div_euclid(self, rhs: Self) -> (Self, bool) {
        (Self(self.0 / rhs.0), false)
    }

    /// Calculates the remainder when `self is divided by rhs`.
    ///
    /// Returns a tuple of the remainder along with a boolean indicating whether
    /// an arithmetic overflow would occur. Note that for unsigned integers
    /// overflow never occurs, so the second value is always `false`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.overflowing_rem(BitmapIndex::MAX),
    ///     (BitmapIndex::ONE, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_rem(BitmapIndex::MAX),
    ///     (BitmapIndex::ZERO, false)
    /// );
    /// ```
    pub const fn overflowing_rem(self, rhs: Self) -> (Self, bool) {
        (Self(self.0 % rhs.0), false)
    }

    /// Calculates the remainder `self.rem_euclid(rhs)` as if by Euclidean
    /// division.
    ///
    /// Returns a tuple of the remainder along with a boolean indicating whether
    /// an arithmetic overflow would occur. Note that for unsigned integers
    /// overflow never occurs, so the second value is always `false`. Since,
    /// for the positive integers, all common definitions of division are
    /// equal, this operation is exactly equal to `self.overflowing_rem(rhs)`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.overflowing_rem_euclid(BitmapIndex::MAX),
    ///     (BitmapIndex::ONE, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_rem_euclid(BitmapIndex::MAX),
    ///     (BitmapIndex::ZERO, false)
    /// );
    /// ```
    pub const fn overflowing_rem_euclid(self, rhs: Self) -> (Self, bool) {
        (Self(self.0 % rhs.0), false)
    }

    /// Negates `self in an overflowing fashion.
    ///
    /// Returns `!self + BitmapIndex::ONE` using wrapping operations to return
    /// the value that represents the negation of this unsigned value. Note
    /// that for positive unsigned values overflow always occurs, but negating
    /// `BitmapIndex::ZERO` does not overflow.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.overflowing_neg(),
    ///     (BitmapIndex::ZERO, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.overflowing_neg(),
    ///     (BitmapIndex::MAX, true)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_neg(),
    ///     (BitmapIndex::ONE, true)
    /// );
    /// ```
    pub const fn overflowing_neg(self) -> (Self, bool) {
        (self.wrapping_neg(), self.0 != 0)
    }

    /// Shifts `self` left by `rhs` bits.
    ///
    /// Returns a tuple of the shifted version of `self` along with a boolean
    /// indicating whether the shift value was larger than or equal to the
    /// number of bits. If the shift value is too large, then it is wrapped
    /// around through `rhs % Self::EFFECTIVE_BITS`, and this value is then
    /// used to perform the shift.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_shl(BitmapIndex::EFFECTIVE_BITS - 1),
    ///     (BitmapIndex::MAX << (BitmapIndex::EFFECTIVE_BITS - 1), false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_shl(BitmapIndex::EFFECTIVE_BITS),
    ///     (BitmapIndex::MAX, true)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_shl(BitmapIndex::EFFECTIVE_BITS + 1),
    ///     (BitmapIndex::MAX << 1, true)
    /// );
    /// ```
    pub const fn overflowing_shl(self, rhs: u32) -> (Self, bool) {
        (self.wrapping_shl(rhs), rhs >= Self::EFFECTIVE_BITS)
    }

    /// Shifts `self` right by `rhs` bits.
    ///
    /// Returns a tuple of the shifted version of `self` along with a boolean
    /// indicating whether the shift value was larger than or equal to the
    /// number of bits. If the shift value is too large, then it is wrapped
    /// around through `rhs % Self::EFFECTIVE_BITS`, and this value is then
    /// used to perform the shift.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_shr(BitmapIndex::EFFECTIVE_BITS - 1),
    ///     (BitmapIndex::MAX >> (BitmapIndex::EFFECTIVE_BITS - 1), false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_shr(BitmapIndex::EFFECTIVE_BITS),
    ///     (BitmapIndex::MAX, true)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_shr(BitmapIndex::EFFECTIVE_BITS + 1),
    ///     (BitmapIndex::MAX >> 1, true)
    /// );
    /// ```
    pub const fn overflowing_shr(self, rhs: u32) -> (Self, bool) {
        (self.wrapping_shr(rhs), rhs >= Self::EFFECTIVE_BITS)
    }

    /// Raises `self` to the power of `exp`, using exponentiation by squaring.
    ///
    /// Returns a tuple of the exponentiation along with a `bool` indicating
    /// whether an overflow happened.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.overflowing_pow(0),
    ///     (BitmapIndex::ONE, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.overflowing_pow(3),
    ///     (BitmapIndex::ONE, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_pow(1),
    ///     (BitmapIndex::MAX, false)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.overflowing_pow(2),
    ///     (BitmapIndex::ONE, true)
    /// );
    /// ```
    pub const fn overflowing_pow(self, exp: u32) -> (Self, bool) {
        let (inner, mut overflow) = self.0.overflowing_pow(exp);
        overflow |= inner > Self::MAX.0;
        (Self(inner & Self::MAX.0), overflow)
    }

    /// Raises `self` to the power of `exp`, using exponentiation by squaring.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.pow(0),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.pow(3),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.pow(1),
    ///     BitmapIndex::MAX
    /// );
    /// ```
    pub const fn pow(self, exp: u32) -> Self {
        if cfg!(debug_assertions) {
            match self.checked_pow(exp) {
                Some(res) => res,
                None => panic!("Attempted to call BitmapIndex::pow() with overflow"),
            }
        } else {
            self.wrapping_pow(exp)
        }
    }

    /// Performs Euclidean division.
    ///
    /// Since, for the positive integers, all common definitions of division are
    /// equal, this is exactly equal to `self / rhs`.
    ///
    /// # Panics
    ///
    /// This function will panic if `rhs` is `BitmapIndex::ZERO`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.div_euclid(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.div_euclid(BitmapIndex::MAX),
    ///     BitmapIndex::ONE
    /// );
    /// ```
    pub const fn div_euclid(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }

    /// Calculates the least remainder of `self (mod rhs)`.
    ///
    /// Since, for the positive integers, all common definitions of division are
    /// equal, this is exactly equal to `self % rhs`.
    ///
    /// # Panics
    ///
    /// This function will panic if `rhs` is `BitmapIndex::ZERO`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```rust
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ONE.rem_euclid(BitmapIndex::MAX),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.rem_euclid(BitmapIndex::MAX),
    ///     BitmapIndex::ZERO
    /// );
    /// ```
    pub const fn rem_euclid(self, rhs: Self) -> Self {
        Self(self.0 % rhs.0)
    }

    /// Returns `true` if and only if `self == 2^k` for some `k`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert!(!BitmapIndex::ZERO.is_power_of_two());
    /// assert!(BitmapIndex::ONE.is_power_of_two());
    /// assert!(!BitmapIndex::MAX.is_power_of_two());
    /// ```
    pub const fn is_power_of_two(self) -> bool {
        self.0.is_power_of_two()
    }

    /// Returns the smallest power of two greater than or equal to `self`.
    ///
    /// When return value overflows (i.e., `self > (BitmapIndex::ONE <<
    /// (BitmapIndex::EFFECTIVE_BITS - 1))`, it panics in debug mode and the
    /// return value is wrapped to 0 in release mode (the only situation in
    /// which method can return 0).
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.next_power_of_two(),
    ///     BitmapIndex::ONE
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.next_power_of_two(),
    ///     BitmapIndex::ONE
    /// );
    /// ```
    pub const fn next_power_of_two(self) -> Self {
        if cfg!(debug_assertions) {
            match self.checked_next_power_of_two() {
                Some(res) => res,
                None => panic!("Attempted to compute next power of two with overflow"),
            }
        } else {
            Self(self.0.next_power_of_two() % Self::MAX.0)
        }
    }

    /// Returns the smallest power of two greater than or equal to `self`. If
    /// the next power of two is greater than the type’s maximum value, `None`
    /// is returned, otherwise the power of two is wrapped in `Some`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```
    /// # use hwlocality::bitmaps::BitmapIndex;
    /// assert_eq!(
    ///     BitmapIndex::ZERO.checked_next_power_of_two(),
    ///     Some(BitmapIndex::ONE)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::ONE.checked_next_power_of_two(),
    ///     Some(BitmapIndex::ONE)
    /// );
    /// assert_eq!(
    ///     BitmapIndex::MAX.checked_next_power_of_two(),
    ///     None
    /// );
    /// ```
    pub const fn checked_next_power_of_two(self) -> Option<Self> {
        let Some(inner) = self.0.checked_next_power_of_two() else {
            return None;
        };
        if inner <= Self::MAX.0 {
            Some(Self(inner))
        } else {
            None
        }
    }

    // NOTE: No (from|to)_(be|le)_bytes, the modeled integer is not made of an
    //       integral number of bytes so these operations do not make sense.

    /// Convert from an hwloc-originated c_int
    ///
    /// This is not a TryFrom implementation because that would make Bitmap
    /// indexing accept negative indexing without a compile-time error.
    pub(crate) fn try_from_c_int(x: c_int) -> Result<Self, TryFromIntError> {
        x.try_into().map(Self)
    }

    /// Convert from an hwloc-originated c_uint
    ///
    /// This is not a TryFrom implementation because it would lead integer
    /// type inference to fall back to i32 in bitmap indexing, which is
    /// undesirable. Also, I'd rather avoid having an implementation-specific
    /// type in a public API like TryFrom...
    #[allow(unused)]
    fn try_from_c_uint(x: c_uint) -> Result<Self, TryFromIntError> {
        Self::try_from_c_int(x.try_into()?)
    }

    /// Const version of `try_from_c_uint`
    ///
    /// Will be dropped once TryFrom/TryInto is usable in const fn
    const fn const_try_from_c_uint(x: c_uint) -> Option<Self> {
        if x <= Self::MAX.0 {
            Some(Self(x))
        } else {
            None
        }
    }

    /// Convert into a c_int (okay by construction)
    pub(crate) fn into_c_int(self) -> c_int {
        self.0 as _
    }

    /// Convert into a c_uint (okay by construction)
    pub(crate) fn into_c_uint(self) -> c_uint {
        self.0
    }
}

impl<B: Borrow<BitmapIndex>> Add<B> for BitmapIndex {
    type Output = Self;

    fn add(self, rhs: B) -> Self {
        if cfg!(debug_assertions) {
            self.checked_add(*rhs.borrow())
                .expect("Attempted to add with overflow")
        } else {
            self.wrapping_add(*rhs.borrow())
        }
    }
}

impl Add<isize> for BitmapIndex {
    type Output = Self;

    fn add(self, rhs: isize) -> Self {
        if cfg!(debug_assertions) {
            self.checked_add_signed(rhs)
                .expect("Attempted to add with overflow")
        } else {
            self.wrapping_add_signed(rhs)
        }
    }
}

impl Add<BitmapIndex> for isize {
    type Output = BitmapIndex;

    fn add(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs + self
    }
}

impl Add<&isize> for BitmapIndex {
    type Output = Self;

    fn add(self, rhs: &isize) -> Self {
        self + (*rhs)
    }
}

impl Add<BitmapIndex> for &isize {
    type Output = BitmapIndex;

    fn add(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs + (*self)
    }
}

impl<B: Borrow<BitmapIndex>> Add<B> for &BitmapIndex {
    type Output = BitmapIndex;

    fn add(self, rhs: B) -> BitmapIndex {
        *self + rhs
    }
}

impl Add<isize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn add(self, rhs: isize) -> BitmapIndex {
        *self + rhs
    }
}

impl Add<&BitmapIndex> for isize {
    type Output = BitmapIndex;

    fn add(self, rhs: &BitmapIndex) -> BitmapIndex {
        *rhs + self
    }
}

impl Add<&isize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn add(self, rhs: &isize) -> BitmapIndex {
        *self + (*rhs)
    }
}

impl Add<&BitmapIndex> for &isize {
    type Output = BitmapIndex;

    fn add(self, rhs: &BitmapIndex) -> BitmapIndex {
        *rhs + (*self)
    }
}

impl<Rhs> AddAssign<Rhs> for BitmapIndex
where
    Self: Add<Rhs, Output = Self>,
{
    fn add_assign(&mut self, rhs: Rhs) {
        *self = *self + rhs
    }
}

#[cfg(any(test, feature = "quickcheck"))]
impl Arbitrary for BitmapIndex {
    fn arbitrary(g: &mut Gen) -> Self {
        // Many index-based hwloc APIs exhibit O(n) behavior depending on which
        // index is passed as input, so we enforce that indices used in tests
        // are "not too big", as per the quickcheck size parameter
        let mut rng = rand::thread_rng();
        let max = Self::try_from(g.size()).unwrap_or(Self::MAX);
        let value = rng.gen_range(0..max.0);
        Self(value)
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(
            self.0
                .shrink()
                .filter_map(|x: c_uint| Self::try_from_c_uint(x).ok()),
        )
    }
}

impl<B: Borrow<BitmapIndex>> BitAnd<B> for BitmapIndex {
    type Output = Self;

    fn bitand(self, rhs: B) -> Self {
        Self(self.0 & rhs.borrow().0)
    }
}

impl BitAnd<usize> for BitmapIndex {
    type Output = Self;

    fn bitand(self, rhs: usize) -> Self {
        // This is ok because AND cannot set bits which are unset in self
        Self(self.0 & (rhs as c_uint))
    }
}

impl BitAnd<BitmapIndex> for usize {
    type Output = BitmapIndex;

    fn bitand(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs & self
    }
}

impl BitAnd<&usize> for BitmapIndex {
    type Output = Self;

    fn bitand(self, rhs: &usize) -> Self {
        self & (*rhs)
    }
}

impl BitAnd<BitmapIndex> for &usize {
    type Output = BitmapIndex;

    fn bitand(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs & (*self)
    }
}

impl<B: Borrow<BitmapIndex>> BitAnd<B> for &BitmapIndex {
    type Output = BitmapIndex;

    fn bitand(self, rhs: B) -> BitmapIndex {
        *self & rhs
    }
}

impl BitAnd<usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn bitand(self, rhs: usize) -> BitmapIndex {
        *self & rhs
    }
}

impl BitAnd<&BitmapIndex> for usize {
    type Output = BitmapIndex;

    fn bitand(self, rhs: &BitmapIndex) -> BitmapIndex {
        *rhs & self
    }
}

impl BitAnd<&usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn bitand(self, rhs: &usize) -> BitmapIndex {
        *self & (*rhs)
    }
}

impl BitAnd<&BitmapIndex> for &usize {
    type Output = BitmapIndex;

    fn bitand(self, rhs: &BitmapIndex) -> BitmapIndex {
        *rhs & (*self)
    }
}

impl<Rhs> BitAndAssign<Rhs> for BitmapIndex
where
    Self: BitAnd<Rhs, Output = Self>,
{
    fn bitand_assign(&mut self, rhs: Rhs) {
        *self = *self & rhs
    }
}

impl<B: Borrow<BitmapIndex>> BitOr<B> for BitmapIndex {
    type Output = Self;

    fn bitor(self, rhs: B) -> Self {
        Self(self.0 | rhs.borrow().0)
    }
}

impl BitOr<usize> for BitmapIndex {
    type Output = Self;

    fn bitor(self, rhs: usize) -> Self {
        // This is only ok if rhs is in range because OR can set bits which are
        // unset in self. We go the usual debug-panic/release-truncate way.
        debug_assert!(rhs <= usize::from(Self::MAX), "RHS out of range");
        Self(self.0 | ((rhs as c_uint) & Self::MAX.0))
    }
}

impl BitOr<BitmapIndex> for usize {
    type Output = BitmapIndex;

    fn bitor(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs | self
    }
}

impl BitOr<&usize> for BitmapIndex {
    type Output = Self;

    fn bitor(self, rhs: &usize) -> Self {
        self | (*rhs)
    }
}

impl BitOr<BitmapIndex> for &usize {
    type Output = BitmapIndex;

    fn bitor(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs | (*self)
    }
}

impl<B: Borrow<BitmapIndex>> BitOr<B> for &BitmapIndex {
    type Output = BitmapIndex;

    fn bitor(self, rhs: B) -> BitmapIndex {
        *self | rhs
    }
}

impl BitOr<usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn bitor(self, rhs: usize) -> BitmapIndex {
        *self | rhs
    }
}

impl BitOr<&BitmapIndex> for usize {
    type Output = BitmapIndex;

    fn bitor(self, rhs: &BitmapIndex) -> BitmapIndex {
        *rhs | self
    }
}

impl BitOr<&usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn bitor(self, rhs: &usize) -> BitmapIndex {
        *self | (*rhs)
    }
}

impl BitOr<&BitmapIndex> for &usize {
    type Output = BitmapIndex;

    fn bitor(self, rhs: &BitmapIndex) -> BitmapIndex {
        *rhs | (*self)
    }
}

impl<Rhs> BitOrAssign<Rhs> for BitmapIndex
where
    Self: BitOr<Rhs, Output = Self>,
{
    fn bitor_assign(&mut self, rhs: Rhs) {
        *self = *self | rhs
    }
}

impl<B: Borrow<BitmapIndex>> BitXor<B> for BitmapIndex {
    type Output = Self;

    fn bitxor(self, rhs: B) -> Self {
        Self(self.0 ^ rhs.borrow().0)
    }
}

impl BitXor<usize> for BitmapIndex {
    type Output = Self;

    fn bitxor(self, rhs: usize) -> Self {
        // This is only ok if rhs is in range because XOR can set bits which are
        // unset in self. We go the usual debug-panic/release-truncate way.
        debug_assert!(rhs <= usize::from(Self::MAX), "RHS out of range");
        Self(self.0 ^ ((rhs as c_uint) & Self::MAX.0))
    }
}

impl BitXor<BitmapIndex> for usize {
    type Output = BitmapIndex;

    fn bitxor(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs ^ self
    }
}

impl BitXor<&usize> for BitmapIndex {
    type Output = Self;

    fn bitxor(self, rhs: &usize) -> Self {
        self ^ (*rhs)
    }
}

impl BitXor<BitmapIndex> for &usize {
    type Output = BitmapIndex;

    fn bitxor(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs ^ (*self)
    }
}

impl<B: Borrow<BitmapIndex>> BitXor<B> for &BitmapIndex {
    type Output = BitmapIndex;

    fn bitxor(self, rhs: B) -> BitmapIndex {
        *self ^ rhs
    }
}

impl BitXor<usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn bitxor(self, rhs: usize) -> BitmapIndex {
        *self ^ rhs
    }
}

impl BitXor<&BitmapIndex> for usize {
    type Output = BitmapIndex;

    fn bitxor(self, rhs: &BitmapIndex) -> BitmapIndex {
        *rhs ^ self
    }
}

impl BitXor<&usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn bitxor(self, rhs: &usize) -> BitmapIndex {
        *self ^ (*rhs)
    }
}

impl BitXor<&BitmapIndex> for &usize {
    type Output = BitmapIndex;

    fn bitxor(self, rhs: &BitmapIndex) -> BitmapIndex {
        *rhs ^ (*self)
    }
}

impl<Rhs> BitXorAssign<Rhs> for BitmapIndex
where
    Self: BitXor<Rhs, Output = Self>,
{
    fn bitxor_assign(&mut self, rhs: Rhs) {
        *self = *self ^ rhs
    }
}

impl<B: Borrow<BitmapIndex>> Div<B> for BitmapIndex {
    type Output = Self;

    fn div(self, rhs: B) -> Self {
        Self(self.0 / rhs.borrow().0)
    }
}

impl Div<usize> for BitmapIndex {
    type Output = Self;

    fn div(self, rhs: usize) -> Self {
        Self::try_from(rhs)
            .map(|rhs| self / rhs)
            .unwrap_or(Self::ZERO)
    }
}

impl Div<&usize> for BitmapIndex {
    type Output = Self;

    fn div(self, rhs: &usize) -> Self {
        self / *rhs
    }
}

impl<B: Borrow<BitmapIndex>> Div<B> for &BitmapIndex {
    type Output = BitmapIndex;

    fn div(self, rhs: B) -> BitmapIndex {
        *self / rhs
    }
}

impl Div<usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn div(self, rhs: usize) -> BitmapIndex {
        *self / rhs
    }
}

impl Div<&usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn div(self, rhs: &usize) -> BitmapIndex {
        *self / *rhs
    }
}

impl<Rhs> DivAssign<Rhs> for BitmapIndex
where
    Self: Div<Rhs, Output = Self>,
{
    fn div_assign(&mut self, rhs: Rhs) {
        *self = *self / rhs
    }
}

// NOTE: Guaranteed to succeed because C mandates that int is >=16 bits
//       u16 would not work because it allows u16::MAX > i16::MAX.
//       From<u8> would also be safe to implement, but would break integer type
//       inference when an integer literal is passed to a function that expects
//       T with BitmapIndex: TryFrom<T>.
impl From<bool> for BitmapIndex {
    fn from(x: bool) -> Self {
        Self(x.into())
    }
}

// NOTE: Assumed to work, otherwise the whole premise of allowing users to use
//       usize/isize instead of c_u?int for indexing falls flat.
impl From<BitmapIndex> for isize {
    fn from(x: BitmapIndex) -> isize {
        ffi::expect_isize(x.0 as c_int)
    }
}
//
impl From<BitmapIndex> for usize {
    fn from(x: BitmapIndex) -> usize {
        ffi::expect_usize(x.0)
    }
}

impl FromStr for BitmapIndex {
    type Err = ParseIntError;

    fn from_str(src: &str) -> Result<Self, ParseIntError> {
        Self::from_str_radix(src, 10)
    }
}

impl<B: Borrow<BitmapIndex>> Mul<B> for BitmapIndex {
    type Output = Self;

    fn mul(self, rhs: B) -> Self {
        if cfg!(debug_assertions) {
            self.checked_mul(*rhs.borrow())
                .expect("Attempted to multiply with overflow")
        } else {
            self.wrapping_mul(*rhs.borrow())
        }
    }
}

impl Mul<usize> for BitmapIndex {
    type Output = Self;

    fn mul(self, rhs: usize) -> Self {
        if cfg!(debug_assertions) {
            if self != Self::ZERO {
                Self::try_from(rhs)
                    .ok()
                    .and_then(|rhs| self.checked_mul(rhs))
                    .expect("Attempted to multiply with overflow")
            } else {
                Self::ZERO
            }
        } else {
            let usize_result = usize::from(self) * rhs;
            let truncated = usize_result & (Self::MAX.0 as usize);
            Self(truncated as _)
        }
    }
}

impl Mul<BitmapIndex> for usize {
    type Output = BitmapIndex;

    fn mul(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs * self
    }
}

impl Mul<&usize> for BitmapIndex {
    type Output = Self;

    fn mul(self, rhs: &usize) -> Self {
        self * (*rhs)
    }
}

impl Mul<BitmapIndex> for &usize {
    type Output = BitmapIndex;

    fn mul(self, rhs: BitmapIndex) -> BitmapIndex {
        rhs * (*self)
    }
}

impl<B: Borrow<BitmapIndex>> Mul<B> for &BitmapIndex {
    type Output = BitmapIndex;

    fn mul(self, rhs: B) -> BitmapIndex {
        *self * rhs
    }
}

impl Mul<usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn mul(self, rhs: usize) -> BitmapIndex {
        *self * rhs
    }
}

impl Mul<&BitmapIndex> for usize {
    type Output = BitmapIndex;

    fn mul(self, rhs: &BitmapIndex) -> BitmapIndex {
        *rhs * self
    }
}

impl Mul<&usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn mul(self, rhs: &usize) -> BitmapIndex {
        (*self) * (*rhs)
    }
}

impl Mul<&BitmapIndex> for &usize {
    type Output = BitmapIndex;

    fn mul(self, rhs: &BitmapIndex) -> BitmapIndex {
        (*rhs) * (*self)
    }
}

impl<Rhs> MulAssign<Rhs> for BitmapIndex
where
    Self: Mul<Rhs, Output = Self>,
{
    fn mul_assign(&mut self, rhs: Rhs) {
        *self = *self * rhs
    }
}

impl Not for BitmapIndex {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(self.0 ^ Self::MAX.0)
    }
}
//
impl Not for &BitmapIndex {
    type Output = BitmapIndex;

    fn not(self) -> Self::Output {
        !*self
    }
}

impl PartialEq<usize> for BitmapIndex {
    fn eq(&self, other: &usize) -> bool {
        usize::from(*self) == *other
    }
}

impl PartialEq<BitmapIndex> for usize {
    fn eq(&self, other: &BitmapIndex) -> bool {
        *self == usize::from(*other)
    }
}

impl PartialOrd<usize> for BitmapIndex {
    fn partial_cmp(&self, other: &usize) -> Option<Ordering> {
        usize::from(*self).partial_cmp(other)
    }
}

impl PartialOrd<BitmapIndex> for usize {
    fn partial_cmp(&self, other: &BitmapIndex) -> Option<Ordering> {
        self.partial_cmp(&usize::from(*other))
    }
}

impl<B: Borrow<BitmapIndex>> Product<B> for BitmapIndex {
    fn product<I: Iterator<Item = B>>(iter: I) -> Self {
        iter.fold(BitmapIndex::ONE, |acc, contrib| acc * contrib.borrow())
    }
}

impl<B: Borrow<BitmapIndex>> Rem<B> for BitmapIndex {
    type Output = Self;

    fn rem(self, rhs: B) -> Self {
        Self(self.0 % rhs.borrow().0)
    }
}

impl Rem<usize> for BitmapIndex {
    type Output = Self;

    fn rem(self, rhs: usize) -> Self {
        Self::try_from(rhs).map(|rhs| self % rhs).unwrap_or(self)
    }
}

impl Rem<&usize> for BitmapIndex {
    type Output = Self;

    fn rem(self, rhs: &usize) -> Self {
        self % (*rhs)
    }
}

impl<B: Borrow<BitmapIndex>> Rem<B> for &BitmapIndex {
    type Output = BitmapIndex;

    fn rem(self, rhs: B) -> BitmapIndex {
        *self % rhs
    }
}

impl Rem<usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn rem(self, rhs: usize) -> BitmapIndex {
        *self % rhs
    }
}

impl Rem<&usize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn rem(self, rhs: &usize) -> BitmapIndex {
        *self % (*rhs)
    }
}

impl<Rhs> RemAssign<Rhs> for BitmapIndex
where
    Self: Rem<Rhs, Output = Self>,
{
    fn rem_assign(&mut self, rhs: Rhs) {
        *self = *self % rhs
    }
}

impl Shl<BitmapIndex> for BitmapIndex {
    type Output = Self;

    fn shl(self, rhs: Self) -> Self {
        self << usize::from(rhs)
    }
}

impl Shl<&BitmapIndex> for BitmapIndex {
    type Output = Self;

    fn shl(self, rhs: &Self) -> Self {
        self << *rhs
    }
}

impl Shl<BitmapIndex> for &BitmapIndex {
    type Output = BitmapIndex;

    fn shl(self, rhs: BitmapIndex) -> BitmapIndex {
        *self << rhs
    }
}

impl Shl<&BitmapIndex> for &BitmapIndex {
    type Output = BitmapIndex;

    fn shl(self, rhs: &BitmapIndex) -> BitmapIndex {
        *self << *rhs
    }
}

macro_rules! shl_with_int {
    ( $( $int:ty ),* ) => { $(
        impl Shl<$int> for BitmapIndex {
            type Output = Self;

            fn shl(self, mut rhs: $int) -> Self {
                if cfg!(debug_assertions) {
                    // Debug mode checks if the shift is in range
                    assert_eq!(
                        rhs.div_euclid(Self::EFFECTIVE_BITS as $int), 0,
                        "Attempted to shift left with overflow"
                    );
                } else {
                    // Release mode wraps around the shift operand
                    rhs = rhs.rem_euclid(Self::EFFECTIVE_BITS as $int);
                }
                Self((self.0 << rhs) & Self::MAX.0)
            }
        }

        impl Shl<&$int> for BitmapIndex {
            type Output = Self;

            fn shl(self, rhs: &$int) -> Self {
                self << *rhs
            }
        }

        impl Shl<$int> for &BitmapIndex {
            type Output = BitmapIndex;

            fn shl(self, rhs: $int) -> BitmapIndex {
                *self << rhs
            }
        }

        impl Shl<&$int> for &BitmapIndex {
            type Output = BitmapIndex;

            fn shl(self, rhs: &$int) -> BitmapIndex {
                *self << *rhs
            }
        }
    )* };
}
//
shl_with_int!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

impl<Rhs> ShlAssign<Rhs> for BitmapIndex
where
    Self: Shl<Rhs, Output = Self>,
{
    fn shl_assign(&mut self, rhs: Rhs) {
        *self = *self << rhs
    }
}

impl Shr<BitmapIndex> for BitmapIndex {
    type Output = Self;

    fn shr(self, rhs: Self) -> Self {
        self >> usize::from(rhs)
    }
}

impl Shr<&BitmapIndex> for BitmapIndex {
    type Output = Self;

    fn shr(self, rhs: &Self) -> Self {
        self >> *rhs
    }
}

impl Shr<BitmapIndex> for &BitmapIndex {
    type Output = BitmapIndex;

    fn shr(self, rhs: BitmapIndex) -> BitmapIndex {
        *self >> rhs
    }
}

impl Shr<&BitmapIndex> for &BitmapIndex {
    type Output = BitmapIndex;

    fn shr(self, rhs: &BitmapIndex) -> BitmapIndex {
        *self >> *rhs
    }
}

macro_rules! shr_with_int {
    ( $( $int:ty ),* ) => { $(
        impl Shr<$int> for BitmapIndex {
            type Output = Self;

            fn shr(self, mut rhs: $int) -> Self {
                if cfg!(debug_assertions) {
                    // Debug mode checks if the shift is in range
                    assert_eq!(
                        rhs.div_euclid(Self::EFFECTIVE_BITS as $int), 0,
                        "Attempted to shift right with overflow"
                    );
                } else {
                    // Release mode wraps around the shift operand
                    rhs = rhs.rem_euclid(Self::EFFECTIVE_BITS as $int);
                }
                Self((self.0 >> rhs) & Self::MAX.0)
            }
        }

        impl Shr<&$int> for BitmapIndex {
            type Output = Self;

            fn shr(self, rhs: &$int) -> Self {
                self >> *rhs
            }
        }

        impl Shr<$int> for &BitmapIndex {
            type Output = BitmapIndex;

            fn shr(self, rhs: $int) -> BitmapIndex {
                *self >> rhs
            }
        }

        impl Shr<&$int> for &BitmapIndex {
            type Output = BitmapIndex;

            fn shr(self, rhs: &$int) -> BitmapIndex {
                *self >> *rhs
            }
        }
    )* };
}
//
shr_with_int!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

impl<Rhs> ShrAssign<Rhs> for BitmapIndex
where
    Self: Shr<Rhs, Output = Self>,
{
    fn shr_assign(&mut self, rhs: Rhs) {
        *self = *self >> rhs
    }
}

impl<B: Borrow<BitmapIndex>> Sub<B> for BitmapIndex {
    type Output = Self;

    fn sub(self, rhs: B) -> Self {
        if cfg!(debug_assertions) {
            self.checked_sub(*rhs.borrow())
                .expect("Attempted to subtract with overflow")
        } else {
            self.wrapping_sub(*rhs.borrow())
        }
    }
}

impl Sub<isize> for BitmapIndex {
    type Output = Self;

    fn sub(self, rhs: isize) -> Self {
        if cfg!(debug_assertions) {
            rhs.checked_neg()
                .and_then(|minus_rhs| self.checked_add_signed(minus_rhs))
                .expect("Attempted to subtract with overflow")
        } else if rhs != isize::MIN {
            self.wrapping_add_signed(-rhs)
        } else {
            // isize::MIN is -2^(isize::BITS - 1).
            // So -isize::MIN is 2^(isize::BITS - 1).
            // A core assumption of ours is that (isize::BITS >= c_int::BITS)
            // Thus 2^(isize::BITS - 1) is a power of two greater than or
            // equal to 2^(Self::EFFECTIVE_BITS), and thus adding it does
            // nothing in the presence of wraparound
            self
        }
    }
}

impl Sub<&isize> for BitmapIndex {
    type Output = Self;

    fn sub(self, rhs: &isize) -> Self {
        self - (*rhs)
    }
}

impl<B: Borrow<BitmapIndex>> Sub<B> for &BitmapIndex {
    type Output = BitmapIndex;

    fn sub(self, rhs: B) -> BitmapIndex {
        *self - rhs
    }
}

impl Sub<isize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn sub(self, rhs: isize) -> BitmapIndex {
        *self - rhs
    }
}

impl Sub<&isize> for &BitmapIndex {
    type Output = BitmapIndex;

    fn sub(self, rhs: &isize) -> BitmapIndex {
        *self - (*rhs)
    }
}

impl<Rhs> SubAssign<Rhs> for BitmapIndex
where
    Self: Sub<Rhs, Output = Self>,
{
    fn sub_assign(&mut self, rhs: Rhs) {
        *self = *self - rhs
    }
}

impl<B: Borrow<BitmapIndex>> Sum<B> for BitmapIndex {
    fn sum<I: Iterator<Item = B>>(iter: I) -> Self {
        iter.fold(BitmapIndex::ZERO, |acc, contrib| acc + contrib.borrow())
    }
}

// NOTE: Only implementing TryFrom<usize> for the same reason slices can only be
//       indexed by usize, namely to avoid integer type inference issues
impl TryFrom<usize> for BitmapIndex {
    type Error = TryFromIntError;

    fn try_from(value: usize) -> Result<Self, TryFromIntError> {
        Self::try_from_c_int(value.try_into()?)
    }
}

macro_rules! try_into {
    ( $( $int:ty ),* ) => { $(
        impl TryFrom<BitmapIndex> for $int {
            type Error = TryFromIntError;

            #[allow(clippy::needless_question_mark)]
            fn try_from(value: BitmapIndex) -> Result<Self, TryFromIntError> {
                Ok(value.0.try_into()?)
            }
        }
    )* };
}
//
try_into!(i8, i16, i32, i64, i128, u8, u16, u32, u64, u128);

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck_macros::quickcheck;
    use std::{
        collections::hash_map::DefaultHasher,
        ffi::c_uint,
        hash::{Hash, Hasher},
        num::IntErrorKind,
        panic::{RefUnwindSafe, UnwindSafe},
    };

    /// Inner bits of BitmapIndex that are not used by the implementation
    const UNUSED_BITS: c_uint = 1 << BitmapIndex::EFFECTIVE_BITS;

    /// Number of bits that are not used
    const NUM_UNUSED_BITS: u32 = UNUSED_BITS.count_ones();

    /// Assert that calling some code panics
    #[track_caller]
    fn assert_panics<R: Debug>(f: impl FnOnce() -> R + UnwindSafe) {
        std::panic::catch_unwind(f).expect_err("Operation should have panicked, but didn't");
    }

    /// Assert that calling some code panics in debug builds and does not do so
    /// in release builds
    #[track_caller]
    fn assert_debug_panics<R: Debug + Eq>(f: impl FnOnce() -> R + UnwindSafe, release_result: R) {
        let res = std::panic::catch_unwind(f);
        if cfg!(debug_assertions) {
            res.expect_err("Operation should have panicked, but didn't panic");
        } else {
            let result = res.expect("Operation should not have panicked, but did panic");
            assert_eq!(
                result, release_result,
                "Operation does not produce the expected result in release builds"
            );
        }
    }

    /// Hash something
    fn hash(x: impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        x.hash(&mut hasher);
        hasher.finish()
    }

    /// Test basic properties of our constants
    #[test]
    fn constants() {
        // Constants and conversions to/from bool
        assert_eq!(BitmapIndex::MIN, BitmapIndex::ZERO);
        assert_eq!(BitmapIndex::ZERO, 0);
        assert_eq!(BitmapIndex::ONE, 1);
        assert_eq!(
            BitmapIndex::MAX,
            (1usize << BitmapIndex::EFFECTIVE_BITS) - 1
        );
        assert_eq!(BitmapIndex::default(), 0);
        assert_eq!(BitmapIndex::from(false), 0);
        assert_eq!(BitmapIndex::from(true), 1);

        // Now let's test some properties that are specific to zero
        let zero = BitmapIndex::ZERO;

        // Logarithm fails for zero
        assert_panics(|| zero.ilog2());
        assert_panics(|| zero.ilog10());
        assert_eq!(zero.checked_ilog2(), None);
        assert_eq!(zero.checked_ilog10(), None);

        // Negation succeeds for zero
        assert_eq!(zero.wrapping_neg(), zero);
        assert_eq!(zero.checked_neg(), Some(zero));
        assert_eq!(zero.overflowing_neg(), (zero, false));
    }

    /// Test properties of unary operations on bitmap indices
    #[quickcheck]
    fn unary(idx: BitmapIndex) {
        // Version of idx's payload with the unused bits set
        let set_unused = idx.0 | UNUSED_BITS;

        // Make sure clone is trivial and equality works early on
        assert_eq!(idx.clone(), idx);

        // Bit fiddling
        assert_eq!(idx.count_ones(), idx.0.count_ones());
        assert_eq!(idx.count_zeros(), set_unused.count_zeros());
        assert_eq!(idx.leading_zeros(), idx.0.leading_zeros() - NUM_UNUSED_BITS);
        assert_eq!(idx.trailing_zeros(), set_unused.trailing_zeros());
        assert_eq!(
            idx.leading_ones(),
            set_unused.leading_ones() - NUM_UNUSED_BITS
        );
        assert_eq!(idx.trailing_ones(), idx.0.trailing_ones());
        assert_eq!(
            idx.reverse_bits().0,
            idx.0.reverse_bits() >> NUM_UNUSED_BITS
        );
        assert_eq!(idx.is_power_of_two(), idx.0.is_power_of_two());
        assert_eq!((!idx).0, !(idx.0 | set_unused));

        // Infaillible conversion to isize and usize
        assert_eq!(isize::from(idx), idx.0 as isize);
        assert_eq!(usize::from(idx), idx.0 as usize);

        // Faillible conversions to all primitive integer types
        #[allow(clippy::useless_conversion)]
        {
            assert_eq!(i8::try_from(idx).ok(), i8::try_from(idx.0).ok());
            assert_eq!(u8::try_from(idx).ok(), u8::try_from(idx.0).ok());
            assert_eq!(i16::try_from(idx).ok(), i16::try_from(idx.0).ok());
            assert_eq!(u16::try_from(idx).ok(), u16::try_from(idx.0).ok());
            assert_eq!(i32::try_from(idx).ok(), i32::try_from(idx.0).ok());
            assert_eq!(u32::try_from(idx).ok(), u32::try_from(idx.0).ok());
            assert_eq!(i64::try_from(idx).ok(), i64::try_from(idx.0).ok());
            assert_eq!(u64::try_from(idx).ok(), u64::try_from(idx.0).ok());
            assert_eq!(i128::try_from(idx).ok(), i128::try_from(idx.0).ok());
            assert_eq!(u128::try_from(idx).ok(), u128::try_from(idx.0).ok());
        }

        // Formatting
        assert_eq!(format!("{idx:?}"), format!("BitmapIndex({:})", idx.0));
        assert_eq!(format!("{idx}"), format!("{:}", idx.0));
        assert_eq!(format!("{idx:b}"), format!("{:b}", idx.0));
        assert_eq!(format!("{idx:e}"), format!("{:e}", idx.0));
        assert_eq!(format!("{idx:o}"), format!("{:o}", idx.0));
        assert_eq!(format!("{idx:x}"), format!("{:x}", idx.0));
        assert_eq!(format!("{idx:E}"), format!("{:E}", idx.0));
        assert_eq!(format!("{idx:X}"), format!("{:X}", idx.0));

        // Hashing
        assert_eq!(hash(idx), hash(idx.0));

        // Division and remainder by zero
        #[allow(clippy::op_ref)]
        {
            // With BitmapIndex denominator
            let zero = BitmapIndex::ZERO;
            assert_eq!(idx.checked_div(zero), None);
            assert_panics(|| idx.overflowing_div(zero));
            assert_panics(|| idx.saturating_div(zero));
            assert_panics(|| idx.wrapping_div(zero));
            assert_panics(|| idx / zero);
            assert_panics(|| &idx / zero);
            assert_panics(|| idx / &zero);
            assert_panics(|| &idx / &zero);
            assert_panics(|| {
                let mut tmp = idx;
                tmp /= zero
            });
            assert_panics(|| {
                let mut tmp = idx;
                tmp /= &zero
            });
            assert_eq!(idx.checked_div_euclid(zero), None);
            assert_panics(|| idx.overflowing_div_euclid(zero));
            assert_panics(|| idx.wrapping_div_euclid(zero));
            assert_eq!(idx.checked_rem(zero), None);
            assert_panics(|| idx.overflowing_rem(zero));
            assert_panics(|| idx.wrapping_rem(zero));
            assert_panics(|| idx % zero);
            assert_panics(|| &idx % zero);
            assert_panics(|| idx % &zero);
            assert_panics(|| &idx % &zero);
            assert_panics(|| {
                let mut tmp = idx;
                tmp %= zero
            });
            assert_panics(|| {
                let mut tmp = idx;
                tmp %= &zero
            });
            assert_eq!(idx.checked_rem_euclid(zero), None);
            assert_panics(|| idx.overflowing_rem_euclid(zero));
            assert_panics(|| idx.wrapping_rem_euclid(zero));

            // With usize denominator
            assert_panics(|| idx / 0);
            assert_panics(|| &idx / 0);
            assert_panics(|| idx / &0);
            assert_panics(|| &idx / &0);
            assert_panics(|| {
                let mut tmp = idx;
                tmp /= 0
            });
            assert_panics(|| {
                let mut tmp = idx;
                tmp /= &0
            });
            assert_panics(|| idx % 0);
            assert_panics(|| &idx % 0);
            assert_panics(|| idx % &0);
            assert_panics(|| &idx % &0);
            assert_panics(|| {
                let mut tmp = idx;
                tmp %= 0
            });
            assert_panics(|| {
                let mut tmp = idx;
                tmp %= &0
            });
        }

        // Logarithm of zero should fail/panic
        {
            // ...with base >= 2
            let base_above2 = idx.saturating_add(BitmapIndex(2));
            assert_panics(|| BitmapIndex::ZERO.ilog(base_above2));
            assert_eq!(BitmapIndex::ZERO.checked_ilog(base_above2), None);

            // ...with base < 2
            let base_below2 = idx % 2;
            assert_panics(|| BitmapIndex::ZERO.ilog(base_below2));
            assert_eq!(BitmapIndex::ZERO.checked_ilog(base_below2), None);
        }

        // Considerations specific to positive numbers
        if idx != 0 {
            // Based logarithms succeed for positive numbers
            let expected_log2 = idx.0.ilog2();
            let expected_log10 = idx.0.ilog10();
            assert_eq!(idx.ilog2(), expected_log2);
            assert_eq!(idx.ilog10(), expected_log10);
            assert_eq!(idx.checked_ilog2(), Some(expected_log2));
            assert_eq!(idx.checked_ilog10(), Some(expected_log10));

            // Negation fails or wraps around for positive numbers
            let wrapping_neg = (!idx).wrapping_add(BitmapIndex(1));
            assert_eq!(idx.checked_neg(), None);
            assert_eq!(idx.wrapping_neg(), wrapping_neg);
            assert_eq!(idx.overflowing_neg(), (wrapping_neg, true));
        }

        // Considerations specific to numbers that have a next power of 2
        let last_pow2 = BitmapIndex::ONE << (BitmapIndex::EFFECTIVE_BITS - 1);
        let has_next_pow2 = idx & (!last_pow2);
        let next_pow2 = BitmapIndex(has_next_pow2.0.next_power_of_two());
        assert_eq!(has_next_pow2.next_power_of_two(), next_pow2);
        assert_eq!(has_next_pow2.checked_next_power_of_two(), Some(next_pow2));

        // Considerations specific to numbers that don't have a next power of 2
        let mut no_next_pow2 = idx | last_pow2;
        if no_next_pow2 == last_pow2 {
            no_next_pow2 += 1;
        }
        assert_debug_panics(|| no_next_pow2.next_power_of_two(), BitmapIndex::ZERO);
        assert_eq!(no_next_pow2.checked_next_power_of_two(), None);
    }

    /// Test usize -> BitmapIndex conversion and special index-usize ops
    #[allow(clippy::op_ref)]
    #[quickcheck]
    fn unary_usize(x: usize) {
        // usize -> BitmapIndex conversion
        assert_eq!(
            BitmapIndex::try_from(x),
            c_int::try_from(x).map(|i| BitmapIndex(i as c_uint))
        );

        // Multiplying ZERO by any usize works
        let zero = BitmapIndex::ZERO;
        assert_eq!(zero * x, zero);
        assert_eq!(x * zero, zero);
        assert_eq!(&zero * x, zero);
        assert_eq!(x * (&zero), zero);
        assert_eq!(zero * (&x), zero);
        assert_eq!(&x * zero, zero);
        assert_eq!(&zero * (&x), zero);
        assert_eq!(&x * (&zero), zero);
        let mut tmp = zero;
        tmp *= x;
        assert_eq!(tmp, zero);
        tmp = zero;
        tmp *= &x;
        assert_eq!(tmp, zero);
    }

    /// Test str -> BitmapIndex conversion (common harness)
    fn test_from_str_radix(
        src: &str,
        radix: u32,
        parse: impl FnOnce() -> Result<BitmapIndex, ParseIntError> + UnwindSafe,
    ) {
        // Handle excessive radix
        if !(2..=36).contains(&radix) {
            assert_panics(parse);
            return;
        }
        let result = parse();

        // Use known-good parser to usize
        let as_usize = match usize::from_str_radix(src, radix) {
            Ok(as_usize) => as_usize,
            e @ Err(_) => {
                // If it fails for usize, it should fail similarly for BitmapIndex
                assert_eq!(e, result.map(usize::from));
                return;
            }
        };

        // Handle the fact that valid BitmapIndex is a subset of usize
        const MAX: usize = BitmapIndex::MAX.0 as usize;
        match as_usize {
            0..=MAX => {
                assert_eq!(result.unwrap(), as_usize);
            }
            _overflow => {
                assert_eq!(result.unwrap_err().kind(), &IntErrorKind::PosOverflow);
            }
        }
    }

    /// Test str -> BitmapIndex conversion via the FromStr trait
    #[quickcheck]
    fn from_str(src: String) {
        test_from_str_radix(&src, 10, || BitmapIndex::from_str(&src))
    }

    /// Test str -> BitmapIndex conversion via the from_str_radix() method
    #[quickcheck]
    fn from_str_radix(src: String, radix: u32) {
        let radix = radix % 37;
        test_from_str_radix(&src, radix, || BitmapIndex::from_str_radix(&src, radix))
    }

    /// Test a faillible operation, produce the checked result for verification
    ///
    /// If `release_result` is `Some(result)`, it indicates that in release
    /// builds, the faillible operation will return `result` instead of
    /// panicking. This is the behavior of most built-in arithmetic operations.
    ///
    /// If `release_result` is `None`, it indicates that the faillible operation
    /// should panick even in release mode, as e.g. ilog() does.
    fn test_faillible<Rhs: Copy + RefUnwindSafe, const LEN: usize>(
        index: BitmapIndex,
        rhs: Rhs,
        checked_op: impl FnOnce(BitmapIndex, Rhs) -> Option<BitmapIndex>,
        faillible_ops: [Box<dyn Fn(BitmapIndex, Rhs) -> BitmapIndex + RefUnwindSafe>; LEN],
        release_result: Option<BitmapIndex>,
    ) -> Option<BitmapIndex> {
        let checked_result = checked_op(index, rhs);
        match (checked_result, release_result) {
            (Some(result), _) => {
                for faillible_op in faillible_ops {
                    assert_eq!(faillible_op(index, rhs), result);
                }
            }
            (None, Some(release_result)) => {
                for faillible_op in faillible_ops {
                    assert_debug_panics(|| faillible_op(index, rhs), release_result);
                }
            }
            (None, None) => {
                for faillible_op in faillible_ops {
                    assert_panics(|| faillible_op(index, rhs));
                }
            }
        }
        checked_result
    }

    /// Test an overflowing operation, produce the overflowing result for verification
    fn test_overflowing<Rhs: Copy + RefUnwindSafe, const LEN: usize>(
        index: BitmapIndex,
        rhs: Rhs,
        checked_op: impl Fn(BitmapIndex, Rhs) -> Option<BitmapIndex>,
        overflowing_op: impl Fn(BitmapIndex, Rhs) -> (BitmapIndex, bool),
        wrapping_op: impl Fn(BitmapIndex, Rhs) -> BitmapIndex,
        faillible_ops: [Box<dyn Fn(BitmapIndex, Rhs) -> BitmapIndex + RefUnwindSafe>; LEN],
    ) -> (BitmapIndex, bool) {
        let overflowing_result = overflowing_op(index, rhs);
        let (wrapped_result, overflow) = overflowing_result;
        assert_eq!(wrapping_op(index, rhs), wrapped_result);
        let checked_result =
            test_faillible(index, rhs, checked_op, faillible_ops, Some(wrapped_result));
        if overflow {
            assert_eq!(checked_result, None);
        } else {
            assert_eq!(checked_result, Some(wrapped_result));
        }
        overflowing_result
    }

    /// Predict the result of an overflowing BitmapIndex operation from the
    /// result of the equivalent overflowing usize operation.
    fn predict_overflowing_result<IndexRhs, UsizeRhs: From<IndexRhs>>(
        i1: BitmapIndex,
        i2: IndexRhs,
        usize_op: fn(usize, UsizeRhs) -> (usize, bool),
    ) -> (BitmapIndex, bool) {
        let used_bits_usize = (1usize << BitmapIndex::EFFECTIVE_BITS) - 1;
        let max_usize = usize::from(BitmapIndex::MAX);
        let (wrapped_usize, overflow_usize) = usize_op(usize::from(i1), UsizeRhs::from(i2));
        let expected_wrapped = BitmapIndex::try_from(wrapped_usize & used_bits_usize).unwrap();
        let expected_overflow = overflow_usize || (wrapped_usize > max_usize);
        (expected_wrapped, expected_overflow)
    }

    /// Test index-index binary operations
    #[allow(clippy::op_ref)]
    #[quickcheck]
    fn index_op_index(i1: BitmapIndex, i2: BitmapIndex) {
        // Ordering passes through
        assert_eq!(i1 == i2, i1.0 == i2.0);
        assert_eq!(i1.cmp(&i2), i1.0.cmp(&i2.0));

        // Bitwise AND passes through (no risk of setting high-order bit)
        let expected_and = BitmapIndex(i1.0 & i2.0);
        assert_eq!(i1 & i2, expected_and);
        assert_eq!(&i1 & i2, expected_and);
        assert_eq!(i1 & (&i2), expected_and);
        assert_eq!(&i1 & (&i2), expected_and);
        let mut tmp = i1;
        tmp &= i2;
        assert_eq!(tmp, expected_and);
        tmp = i1;
        tmp &= &i2;
        assert_eq!(tmp, expected_and);

        // Bitwise OR passes through (no risk of setting high-order bit)
        let expected_or = BitmapIndex(i1.0 | i2.0);
        assert_eq!(i1 | i2, expected_or);
        assert_eq!(&i1 | i2, expected_or);
        assert_eq!(i1 | (&i2), expected_or);
        assert_eq!(&i1 | (&i2), expected_or);
        tmp = i1;
        tmp |= i2;
        assert_eq!(tmp, expected_or);
        tmp = i1;
        tmp |= &i2;
        assert_eq!(tmp, expected_or);

        // Bitwise XOR passes through (no risk of setting high-order bit)
        let expected_xor = BitmapIndex(i1.0 ^ i2.0);
        assert_eq!(i1 ^ i2, expected_xor);
        assert_eq!(&i1 ^ i2, expected_xor);
        assert_eq!(i1 ^ (&i2), expected_xor);
        assert_eq!(&i1 ^ (&i2), expected_xor);
        let mut tmp = i1;
        tmp ^= i2;
        assert_eq!(tmp, expected_xor);
        tmp = i1;
        tmp ^= &i2;
        assert_eq!(tmp, expected_xor);

        // Addition
        let (expected_wrapped, expected_overflow) =
            predict_overflowing_result(i1, i2, usize::overflowing_add);
        let (wrapped, overflow) = test_overflowing(
            i1,
            i2,
            BitmapIndex::checked_add,
            BitmapIndex::overflowing_add,
            BitmapIndex::wrapping_add,
            [
                Box::new(|i1, i2| i1 + i2),
                Box::new(|i1, i2| &i1 + i2),
                Box::new(|i1, i2| i1 + &i2),
                Box::new(|i1, i2| &i1 + &i2),
                Box::new(|mut i1, i2| {
                    i1 += i2;
                    i1
                }),
                Box::new(|mut i1, i2| {
                    i1 += &i2;
                    i1
                }),
            ],
        );
        assert_eq!(wrapped, expected_wrapped);
        assert_eq!(overflow, expected_overflow);
        if overflow {
            assert_eq!(i1.saturating_add(i2), BitmapIndex::MAX);
        } else {
            assert_eq!(i1.saturating_add(i2), wrapped);
        }

        // Subtraction
        let (expected_wrapped, expected_overflow) =
            predict_overflowing_result(i1, i2, usize::overflowing_sub);
        let (wrapped, overflow) = test_overflowing(
            i1,
            i2,
            BitmapIndex::checked_sub,
            BitmapIndex::overflowing_sub,
            BitmapIndex::wrapping_sub,
            [
                Box::new(|i1, i2| i1 - i2),
                Box::new(|i1, i2| &i1 - i2),
                Box::new(|i1, i2| i1 - &i2),
                Box::new(|i1, i2| &i1 - &i2),
                Box::new(|mut i1, i2| {
                    i1 -= i2;
                    i1
                }),
                Box::new(|mut i1, i2| {
                    i1 -= &i2;
                    i1
                }),
            ],
        );
        assert_eq!(wrapped, expected_wrapped);
        assert_eq!(overflow, expected_overflow);
        if overflow {
            assert_eq!(i1.saturating_sub(i2), BitmapIndex::MIN);
        } else {
            assert_eq!(i1.saturating_sub(i2), wrapped);
        }

        // Absolute difference
        assert_eq!(i1.abs_diff(i2), BitmapIndex(i1.0.abs_diff(i2.0)));

        // Multiplication
        let (expected_wrapped, expected_overflow) =
            predict_overflowing_result(i1, i2, usize::overflowing_mul);
        let (wrapped, overflow) = test_overflowing(
            i1,
            i2,
            BitmapIndex::checked_mul,
            BitmapIndex::overflowing_mul,
            BitmapIndex::wrapping_mul,
            [
                Box::new(|i1, i2| i1 * i2),
                Box::new(|i1, i2| &i1 * i2),
                Box::new(|i1, i2| i1 * &i2),
                Box::new(|i1, i2| &i1 * &i2),
                Box::new(|mut i1, i2| {
                    i1 *= i2;
                    i1
                }),
                Box::new(|mut i1, i2| {
                    i1 *= &i2;
                    i1
                }),
            ],
        );
        assert_eq!(wrapped, expected_wrapped);
        assert_eq!(overflow, expected_overflow);
        if overflow {
            assert_eq!(i1.saturating_mul(i2), BitmapIndex::MAX);
        } else {
            assert_eq!(i1.saturating_mul(i2), wrapped);
        }

        // Division and remainder by nonzero (zero case tested in unary tests)
        {
            // Division
            let nonzero = i2.saturating_add(BitmapIndex::ONE);
            let (expected_wrapped, expected_overflow) =
                predict_overflowing_result(i1, nonzero, usize::overflowing_div);
            let res1 = test_overflowing(
                i1,
                nonzero,
                BitmapIndex::checked_div,
                BitmapIndex::overflowing_div,
                BitmapIndex::wrapping_div,
                [
                    Box::new(|i1, nonzero| i1 / nonzero),
                    Box::new(|i1, nonzero| &i1 / nonzero),
                    Box::new(|i1, nonzero| i1 / &nonzero),
                    Box::new(|i1, nonzero| &i1 / &nonzero),
                    Box::new(|mut i1, nonzero| {
                        i1 /= nonzero;
                        i1
                    }),
                    Box::new(|mut i1, nonzero| {
                        i1 /= &nonzero;
                        i1
                    }),
                ],
            );
            let res2 = test_overflowing(
                i1,
                nonzero,
                BitmapIndex::checked_div_euclid,
                BitmapIndex::overflowing_div_euclid,
                BitmapIndex::wrapping_div_euclid,
                [Box::new(|i1, nonzero| i1.div_euclid(nonzero))],
            );
            assert_eq!(i1.saturating_div(nonzero), expected_wrapped);
            for (wrapped, overflow) in [res1, res2] {
                assert_eq!(wrapped, expected_wrapped);
                assert_eq!(overflow, expected_overflow);
            }

            // Remainder
            let (expected_wrapped, expected_overflow) =
                predict_overflowing_result(i1, nonzero, usize::overflowing_rem);
            let res1 = test_overflowing(
                i1,
                nonzero,
                BitmapIndex::checked_rem,
                BitmapIndex::overflowing_rem,
                BitmapIndex::wrapping_rem,
                [
                    Box::new(|i1, nonzero| i1 % nonzero),
                    Box::new(|i1, nonzero| &i1 % nonzero),
                    Box::new(|i1, nonzero| i1 % &nonzero),
                    Box::new(|i1, nonzero| &i1 % &nonzero),
                    Box::new(|mut i1, nonzero| {
                        i1 %= nonzero;
                        i1
                    }),
                    Box::new(|mut i1, nonzero| {
                        i1 %= &nonzero;
                        i1
                    }),
                ],
            );
            let res2 = test_overflowing(
                i1,
                nonzero,
                BitmapIndex::checked_rem_euclid,
                BitmapIndex::overflowing_rem_euclid,
                BitmapIndex::wrapping_rem_euclid,
                [Box::new(|i1, nonzero| i1.rem_euclid(nonzero))],
            );
            for (wrapped, overflow) in [res1, res2] {
                assert_eq!(wrapped, expected_wrapped);
                assert_eq!(overflow, expected_overflow);
            }
        }

        // Checked logarithm
        {
            // Should succeed for a number >= 0 with a basis >= 2
            let number_above0 = i1.saturating_add(BitmapIndex::ONE);
            let base_above2 = i2.saturating_add(BitmapIndex(2));
            let expected_ilog = number_above0.0.ilog(base_above2.0);
            assert_eq!(number_above0.ilog(base_above2), expected_ilog);
            assert_eq!(number_above0.checked_ilog(base_above2), Some(expected_ilog));

            // Should fail if the basis is below 2
            let base_below2 = i2 % 2;
            assert_panics(|| BitmapIndex::ZERO.ilog(base_below2));
            assert_eq!(BitmapIndex::ZERO.checked_ilog(base_below2), None);
        }

        // Non-overflowing left shift must keep high-order bit cleared
        let effective_bits = BitmapIndex(BitmapIndex::EFFECTIVE_BITS as _);
        let wrapped_shift = i2 % effective_bits;
        let wrapped_shl = BitmapIndex((i1.0 << wrapped_shift.0) & BitmapIndex::MAX.0);
        assert_eq!(i1 << wrapped_shift, wrapped_shl);
        assert_eq!(&i1 << wrapped_shift, wrapped_shl);
        assert_eq!(i1 << (&wrapped_shift), wrapped_shl);
        assert_eq!(&i1 << (&wrapped_shift), wrapped_shl);
        tmp = i1;
        tmp <<= wrapped_shift;
        assert_eq!(tmp, wrapped_shl);
        tmp = i1;
        tmp <<= &wrapped_shift;
        assert_eq!(tmp, wrapped_shl);

        // Overflowing left shift should panic or wraparound
        let overflown_shift = i2.saturating_add(effective_bits);
        assert_debug_panics(|| i1 << overflown_shift, wrapped_shl);
        assert_debug_panics(|| &i1 << overflown_shift, wrapped_shl);
        assert_debug_panics(|| i1 << (&overflown_shift), wrapped_shl);
        assert_debug_panics(|| &i1 << (&overflown_shift), wrapped_shl);
        assert_debug_panics(
            || {
                let mut tmp = i1;
                tmp <<= overflown_shift;
                tmp
            },
            wrapped_shl,
        );
        assert_debug_panics(
            || {
                let mut tmp = i1;
                tmp <<= &overflown_shift;
                tmp
            },
            wrapped_shl,
        );

        // Non-overflowing right shift can pass through
        let wrapped_shr = BitmapIndex(i1.0 >> wrapped_shift.0);
        assert_eq!(i1 >> wrapped_shift, wrapped_shr);
        assert_eq!(&i1 >> wrapped_shift, wrapped_shr);
        assert_eq!(i1 >> (&wrapped_shift), wrapped_shr);
        assert_eq!(&i1 >> (&wrapped_shift), wrapped_shr);
        tmp = i1;
        tmp >>= wrapped_shift;
        assert_eq!(tmp, wrapped_shr);
        tmp = i1;
        tmp >>= &wrapped_shift;
        assert_eq!(tmp, wrapped_shr);

        // Overflowing right shift should panic or wraparound
        assert_debug_panics(|| i1 >> overflown_shift, wrapped_shr);
        assert_debug_panics(|| &i1 >> overflown_shift, wrapped_shr);
        assert_debug_panics(|| i1 >> (&overflown_shift), wrapped_shr);
        assert_debug_panics(|| &i1 >> (&overflown_shift), wrapped_shr);
        assert_debug_panics(
            || {
                let mut tmp = i1;
                tmp >>= overflown_shift;
                tmp
            },
            wrapped_shr,
        );
        assert_debug_panics(
            || {
                let mut tmp = i1;
                tmp >>= &overflown_shift;
                tmp
            },
            wrapped_shr,
        );
    }

    /// Test index-u32 binary operations
    #[allow(clippy::op_ref)]
    #[quickcheck]
    fn index_op_u32(index: BitmapIndex, rhs: u32) {
        // Elevation to an integer power
        let (expected_wrapped, expected_overflow) =
            predict_overflowing_result(index, rhs, usize::overflowing_pow);
        let (wrapped, overflow) = test_overflowing(
            index,
            rhs,
            BitmapIndex::checked_pow,
            BitmapIndex::overflowing_pow,
            BitmapIndex::wrapping_pow,
            [Box::new(|index, rhs| index.pow(rhs))],
        );
        assert_eq!(wrapped, expected_wrapped);
        assert_eq!(overflow, expected_overflow);
        if overflow {
            assert_eq!(index.saturating_pow(rhs), BitmapIndex::MAX);
        } else {
            assert_eq!(index.saturating_pow(rhs), wrapped);
        }

        // Non-overflowing left shift
        let test_overflowing_shl = |rhs| {
            test_overflowing(
                index,
                rhs,
                BitmapIndex::checked_shl,
                BitmapIndex::overflowing_shl,
                BitmapIndex::wrapping_shl,
                [
                    Box::new(|index, rhs| index << rhs),
                    Box::new(|index, rhs| &index << rhs),
                    Box::new(|index, rhs| index << &rhs),
                    Box::new(|index, rhs| &index << &rhs),
                    Box::new(|mut index, rhs| {
                        index <<= rhs;
                        index
                    }),
                    Box::new(|mut index, rhs| {
                        index <<= &rhs;
                        index
                    }),
                ],
            )
        };
        let wrapped_shift = rhs % BitmapIndex::EFFECTIVE_BITS;
        let expected_wrapped = BitmapIndex((index.0 << wrapped_shift) & BitmapIndex::MAX.0);
        let (wrapped, overflow) = test_overflowing_shl(wrapped_shift);
        assert_eq!(wrapped, expected_wrapped);
        assert!(!overflow);

        // Overflowing left shift
        let overflown_shift = rhs.saturating_add(BitmapIndex::EFFECTIVE_BITS);
        let (wrapped, overflow) = test_overflowing_shl(overflown_shift);
        assert_eq!(wrapped, expected_wrapped);
        assert!(overflow);

        // Non-overflowing right shift
        let test_overflowing_shr = |rhs| {
            test_overflowing(
                index,
                rhs,
                BitmapIndex::checked_shr,
                BitmapIndex::overflowing_shr,
                BitmapIndex::wrapping_shr,
                [
                    Box::new(|index, rhs| index >> rhs),
                    Box::new(|index, rhs| &index >> rhs),
                    Box::new(|index, rhs| index >> &rhs),
                    Box::new(|index, rhs| &index >> &rhs),
                    Box::new(|mut index, rhs| {
                        index >>= rhs;
                        index
                    }),
                    Box::new(|mut index, rhs| {
                        index >>= &rhs;
                        index
                    }),
                ],
            )
        };
        let expected_wrapped = BitmapIndex(index.0 >> wrapped_shift);
        let (wrapped, overflow) = test_overflowing_shr(wrapped_shift);
        assert_eq!(wrapped, expected_wrapped);
        assert!(!overflow);

        // Overflowing right shift
        let (wrapped, overflow) = test_overflowing_shr(overflown_shift);
        assert_eq!(wrapped, expected_wrapped);
        assert!(overflow);

        // Rotate can be expressed in terms of shifts and binary ops
        let expected_rol = (index << wrapped_shift)
            | index.wrapping_shr(BitmapIndex::EFFECTIVE_BITS - wrapped_shift);
        assert_eq!(index.rotate_left(rhs), expected_rol);
        let expected_ror = (index >> wrapped_shift)
            | index.wrapping_shl(BitmapIndex::EFFECTIVE_BITS - wrapped_shift);
        assert_eq!(index.rotate_right(rhs), expected_ror);
    }

    /// Test index-usize binary operations
    #[allow(clippy::op_ref)]
    #[quickcheck]
    fn index_op_usize(index: BitmapIndex, other: usize) {
        // Ordering passes through
        assert_eq!(index == other, usize::from(index) == other);
        assert_eq!(
            index.partial_cmp(&other),
            usize::from(index).partial_cmp(&other)
        );
        assert_eq!(other == usize::from(index), other == usize::from(index));
        assert_eq!(
            other.partial_cmp(&usize::from(index)),
            other.partial_cmp(&usize::from(index))
        );

        // Bitwise AND passes through (no risk of setting high-order bit)
        let expected_and = BitmapIndex(index.0 & (other as c_uint));
        assert_eq!(index & other, expected_and);
        assert_eq!(other & index, expected_and);
        assert_eq!(&index & other, expected_and);
        assert_eq!(&other & index, expected_and);
        assert_eq!(index & (&other), expected_and);
        assert_eq!(other & (&index), expected_and);
        assert_eq!(&index & (&other), expected_and);
        assert_eq!(&other & (&index), expected_and);
        let mut tmp = index;
        tmp &= other;
        assert_eq!(tmp, expected_and);
        tmp = index;
        tmp &= &other;
        assert_eq!(tmp, expected_and);

        // Non-overflowing bitwise OR
        let small_other = other & usize::from(BitmapIndex::MAX);
        let expected_or = BitmapIndex(index.0 | (small_other as c_uint));
        assert_eq!(index | small_other, expected_or);
        assert_eq!(small_other | index, expected_or);
        assert_eq!(&index | small_other, expected_or);
        assert_eq!(small_other | &index, expected_or);
        assert_eq!(index | &small_other, expected_or);
        assert_eq!(&small_other | index, expected_or);
        assert_eq!(&index | &small_other, expected_or);
        assert_eq!(&small_other | &index, expected_or);
        tmp = index;
        tmp |= small_other;
        assert_eq!(tmp, expected_or);
        tmp = index;
        tmp |= &small_other;
        assert_eq!(tmp, expected_or);

        // Overflowing bitwise OR
        let large_other = other.max(1usize << BitmapIndex::EFFECTIVE_BITS);
        assert_debug_panics(|| index | large_other, expected_or);
        assert_debug_panics(|| large_other | index, expected_or);
        assert_debug_panics(|| &index | large_other, expected_or);
        assert_debug_panics(|| large_other | &index, expected_or);
        assert_debug_panics(|| index | &large_other, expected_or);
        assert_debug_panics(|| &large_other | index, expected_or);
        assert_debug_panics(|| &index | &large_other, expected_or);
        assert_debug_panics(|| &large_other | &index, expected_or);
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp |= large_other;
                tmp
            },
            expected_or,
        );
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp |= &large_other;
                tmp
            },
            expected_or,
        );

        // Non-overflowing bitwise XOR
        let expected_xor = BitmapIndex(index.0 ^ (small_other as c_uint));
        assert_eq!(index ^ small_other, expected_xor);
        assert_eq!(small_other ^ index, expected_xor);
        assert_eq!(&index ^ small_other, expected_xor);
        assert_eq!(small_other ^ &index, expected_xor);
        assert_eq!(index ^ &small_other, expected_xor);
        assert_eq!(&small_other ^ index, expected_xor);
        assert_eq!(&index ^ &small_other, expected_xor);
        assert_eq!(&small_other ^ &index, expected_xor);
        tmp = index;
        tmp ^= small_other;
        assert_eq!(tmp, expected_xor);
        tmp = index;
        tmp ^= &small_other;
        assert_eq!(tmp, expected_xor);

        // Overflowing bitwise XOR
        assert_debug_panics(|| index ^ large_other, expected_xor);
        assert_debug_panics(|| large_other ^ index, expected_xor);
        assert_debug_panics(|| &index ^ large_other, expected_xor);
        assert_debug_panics(|| large_other ^ &index, expected_xor);
        assert_debug_panics(|| index ^ &large_other, expected_xor);
        assert_debug_panics(|| &large_other ^ index, expected_xor);
        assert_debug_panics(|| &index ^ &large_other, expected_xor);
        assert_debug_panics(|| &large_other ^ &index, expected_xor);
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp ^= large_other;
                tmp
            },
            expected_xor,
        );
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp ^= &large_other;
                tmp
            },
            expected_xor,
        );

        // Multiplication by an usize in BitmapIndex range works as usual
        let small_other_index = BitmapIndex::try_from(small_other).unwrap();
        let (wrapped, overflow) = index.overflowing_mul(small_other_index);
        if overflow {
            assert_debug_panics(|| index * small_other, wrapped);
            assert_debug_panics(|| small_other * index, wrapped);
            assert_debug_panics(|| &index * small_other, wrapped);
            assert_debug_panics(|| small_other * (&index), wrapped);
            assert_debug_panics(|| index * (&small_other), wrapped);
            assert_debug_panics(|| &small_other * index, wrapped);
            assert_debug_panics(|| &index * (&small_other), wrapped);
            assert_debug_panics(|| &small_other * (&index), wrapped);
            assert_debug_panics(
                || {
                    let mut tmp = index;
                    tmp *= small_other;
                    tmp
                },
                wrapped,
            );
            assert_debug_panics(
                || {
                    let mut tmp = index;
                    tmp *= &small_other;
                    tmp
                },
                wrapped,
            );
        } else {
            assert_eq!(index * small_other, wrapped);
            assert_eq!(small_other * index, wrapped);
            assert_eq!(&index * small_other, wrapped);
            assert_eq!(small_other * (&index), wrapped);
            assert_eq!(index * (&small_other), wrapped);
            assert_eq!(&small_other * index, wrapped);
            assert_eq!(&index * (&small_other), wrapped);
            assert_eq!(&small_other * (&index), wrapped);
            tmp = index;
            tmp *= small_other;
            assert_eq!(tmp, wrapped);
            tmp = index;
            tmp *= &small_other;
            assert_eq!(tmp, wrapped);
        }

        // Multiplication by an out-of-range usize fails for all indices but
        // zero (which is tested elsewhere)
        let zero = BitmapIndex::ZERO;
        if index != zero {
            assert_debug_panics(|| index * large_other, wrapped);
            assert_debug_panics(|| large_other * index, wrapped);
            assert_debug_panics(|| &index * large_other, wrapped);
            assert_debug_panics(|| large_other * (&index), wrapped);
            assert_debug_panics(|| index * (&large_other), wrapped);
            assert_debug_panics(|| &large_other * index, wrapped);
            assert_debug_panics(|| &index * (&large_other), wrapped);
            assert_debug_panics(|| &large_other * (&index), wrapped);
            assert_debug_panics(
                || {
                    let mut tmp = index;
                    tmp *= large_other;
                    tmp
                },
                wrapped,
            );
            assert_debug_panics(
                || {
                    let mut tmp = index;
                    tmp *= &large_other;
                    tmp
                },
                wrapped,
            );
        }

        // Division by an in-range nonzero usize passes through
        if other != 0 {
            let expected = index / small_other_index;
            assert_eq!(index / small_other, expected);
            assert_eq!(&index / small_other, expected);
            assert_eq!(index / &small_other, expected);
            assert_eq!(&index / &small_other, expected);
            tmp = index;
            tmp /= small_other;
            assert_eq!(tmp, expected);
            tmp = index;
            tmp /= &small_other;
            assert_eq!(tmp, expected);
        }

        // Division by an out-of-range usize always returns zero
        assert_eq!(index / large_other, zero);
        assert_eq!(&index / large_other, zero);
        assert_eq!(index / &large_other, zero);
        assert_eq!(&index / &large_other, zero);
        tmp = index;
        tmp /= large_other;
        assert_eq!(tmp, zero);
        tmp = index;
        tmp /= &large_other;
        assert_eq!(tmp, zero);

        // Remainder from an in-range nonzero usize passes through
        if other != 0 {
            let expected = index % small_other_index;
            assert_eq!(index % small_other, expected);
            assert_eq!(&index % small_other, expected);
            assert_eq!(index % &small_other, expected);
            assert_eq!(&index % &small_other, expected);
            tmp = index;
            tmp %= small_other;
            assert_eq!(tmp, expected);
            tmp = index;
            tmp %= &small_other;
            assert_eq!(tmp, expected);
        }

        // Remainder from an out-of-range usize is identity
        assert_eq!(index % large_other, index);
        assert_eq!(&index % large_other, index);
        assert_eq!(index % &large_other, index);
        assert_eq!(&index % &large_other, index);
        tmp = index;
        tmp %= large_other;
        assert_eq!(tmp, index);
        tmp = index;
        tmp %= &large_other;
        assert_eq!(tmp, index);

        // Non-overflowing left shift must keep high-order bit cleared
        let effective_bits = BitmapIndex::EFFECTIVE_BITS as usize;
        let wrapped_shift = other % effective_bits;
        let wrapped_shl = BitmapIndex((index.0 << wrapped_shift) & BitmapIndex::MAX.0);
        assert_eq!(index << wrapped_shift, wrapped_shl);
        assert_eq!(&index << wrapped_shift, wrapped_shl);
        assert_eq!(index << (&wrapped_shift), wrapped_shl);
        assert_eq!(&index << (&wrapped_shift), wrapped_shl);
        tmp = index;
        tmp <<= wrapped_shift;
        assert_eq!(tmp, wrapped_shl);
        tmp = index;
        tmp <<= &wrapped_shift;
        assert_eq!(tmp, wrapped_shl);

        // Overflowing left shift should panic or wraparound
        let overflown_shift = other.saturating_add(effective_bits);
        assert_debug_panics(|| index << overflown_shift, wrapped_shl);
        assert_debug_panics(|| &index << overflown_shift, wrapped_shl);
        assert_debug_panics(|| index << (&overflown_shift), wrapped_shl);
        assert_debug_panics(|| &index << (&overflown_shift), wrapped_shl);
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp <<= overflown_shift;
                tmp
            },
            wrapped_shl,
        );
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp <<= &overflown_shift;
                tmp
            },
            wrapped_shl,
        );

        // Non-overflowing right shift can pass through
        let wrapped_shr = BitmapIndex(index.0 >> wrapped_shift);
        assert_eq!(index >> wrapped_shift, wrapped_shr);
        assert_eq!(&index >> wrapped_shift, wrapped_shr);
        assert_eq!(index >> (&wrapped_shift), wrapped_shr);
        assert_eq!(&index >> (&wrapped_shift), wrapped_shr);
        tmp = index;
        tmp >>= wrapped_shift;
        assert_eq!(tmp, wrapped_shr);
        tmp = index;
        tmp >>= &wrapped_shift;
        assert_eq!(tmp, wrapped_shr);

        // Overflowing right shift should panic or wraparound
        assert_debug_panics(|| index >> overflown_shift, wrapped_shr);
        assert_debug_panics(|| &index >> overflown_shift, wrapped_shr);
        assert_debug_panics(|| index >> (&overflown_shift), wrapped_shr);
        assert_debug_panics(|| &index >> (&overflown_shift), wrapped_shr);
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp >>= overflown_shift;
                tmp
            },
            wrapped_shr,
        );
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp >>= &overflown_shift;
                tmp
            },
            wrapped_shr,
        );
    }

    /// Test index-isize binary operations
    #[allow(clippy::op_ref)]
    #[quickcheck]
    fn index_op_isize(index: BitmapIndex, other: isize) {
        // Addition
        let (expected_wrapped, expected_overflow) =
            predict_overflowing_result(index, other, usize::overflowing_add_signed);
        let (wrapped, overflow) = test_overflowing(
            index,
            other,
            BitmapIndex::checked_add_signed,
            BitmapIndex::overflowing_add_signed,
            BitmapIndex::wrapping_add_signed,
            [
                Box::new(|index, other| index + other),
                Box::new(|index, other| other + index),
                Box::new(|index, other| &index + other),
                Box::new(|index, other| other + &index),
                Box::new(|index, other| index + &other),
                Box::new(|index, other| &other + index),
                Box::new(|index, other| &index + &other),
                Box::new(|index, other| &other + &index),
                Box::new(|mut index, other| {
                    index += other;
                    index
                }),
                Box::new(|mut index, other| {
                    index += &other;
                    index
                }),
            ],
        );
        assert_eq!(wrapped, expected_wrapped);
        assert_eq!(overflow, expected_overflow);
        if overflow {
            if other > 0 {
                assert_eq!(index.saturating_add_signed(other), BitmapIndex::MAX);
            } else {
                assert_eq!(index.saturating_add_signed(other), BitmapIndex::MIN);
            }
        } else {
            assert_eq!(index.saturating_add_signed(other), wrapped);
        }

        // Subtraction
        let (wrapped, overflow) = if other != isize::MIN {
            predict_overflowing_result(index, -other, usize::overflowing_add_signed)
        } else {
            predict_overflowing_result(
                index,
                // iN::MIN is -(1 << (iN::BITS - 1)
                // e.g. i8::MIN is -(1 << (8 - 1)) = -(1 << 7).
                1usize << (isize::BITS - 1),
                usize::overflowing_sub,
            )
        };
        if overflow {
            assert_debug_panics(|| index - other, wrapped);
            assert_debug_panics(|| &index - other, wrapped);
            assert_debug_panics(|| index - &other, wrapped);
            assert_debug_panics(|| &index - &other, wrapped);
            assert_debug_panics(
                || {
                    let mut tmp = index;
                    tmp -= other;
                    tmp
                },
                wrapped,
            );
            assert_debug_panics(
                || {
                    let mut tmp = index;
                    tmp -= &other;
                    tmp
                },
                wrapped,
            );
        } else {
            assert_eq!(index - other, wrapped);
            assert_eq!(&index - other, wrapped);
            assert_eq!(index - &other, wrapped);
            assert_eq!(&index - &other, wrapped);
            let mut tmp = index;
            tmp -= other;
            assert_eq!(tmp, wrapped);
            tmp = index;
            tmp -= &other;
            assert_eq!(tmp, wrapped);
        }

        // Non-overflowing left shift must keep high-order bit cleared
        let effective_bits = BitmapIndex::EFFECTIVE_BITS as isize;
        let wrapped_shift = other.rem_euclid(effective_bits);
        let wrapped_shl = BitmapIndex((index.0 << wrapped_shift) & BitmapIndex::MAX.0);
        assert_eq!(index << wrapped_shift, wrapped_shl);
        assert_eq!(&index << wrapped_shift, wrapped_shl);
        assert_eq!(index << (&wrapped_shift), wrapped_shl);
        assert_eq!(&index << (&wrapped_shift), wrapped_shl);
        let mut tmp = index;
        tmp <<= wrapped_shift;
        assert_eq!(tmp, wrapped_shl);
        tmp = index;
        tmp <<= &wrapped_shift;
        assert_eq!(tmp, wrapped_shl);

        // Overflowing left shift should panic or wraparound
        let overflown_shift = other.saturating_add(effective_bits);
        assert_debug_panics(|| index << overflown_shift, wrapped_shl);
        assert_debug_panics(|| &index << overflown_shift, wrapped_shl);
        assert_debug_panics(|| index << (&overflown_shift), wrapped_shl);
        assert_debug_panics(|| &index << (&overflown_shift), wrapped_shl);
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp <<= overflown_shift;
                tmp
            },
            wrapped_shl,
        );
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp <<= &overflown_shift;
                tmp
            },
            wrapped_shl,
        );

        // Non-overflowing right shift must keep high-order bit cleared as well
        // (with signed operands, a shift of -EFFECTIVE_BITS is possible!)
        let wrapped_shr = BitmapIndex((index.0 >> wrapped_shift) & BitmapIndex::MAX.0);
        assert_eq!(index >> wrapped_shift, wrapped_shr);
        assert_eq!(&index >> wrapped_shift, wrapped_shr);
        assert_eq!(index >> (&wrapped_shift), wrapped_shr);
        assert_eq!(&index >> (&wrapped_shift), wrapped_shr);
        tmp = index;
        tmp >>= wrapped_shift;
        assert_eq!(tmp, wrapped_shr);
        tmp = index;
        tmp >>= &wrapped_shift;
        assert_eq!(tmp, wrapped_shr);

        // Overflowing right shift should panic or wraparound
        assert_debug_panics(|| index >> overflown_shift, wrapped_shr);
        assert_debug_panics(|| &index >> overflown_shift, wrapped_shr);
        assert_debug_panics(|| index >> (&overflown_shift), wrapped_shr);
        assert_debug_panics(|| &index >> (&overflown_shift), wrapped_shr);
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp >>= overflown_shift;
                tmp
            },
            wrapped_shr,
        );
        assert_debug_panics(
            || {
                let mut tmp = index;
                tmp >>= &overflown_shift;
                tmp
            },
            wrapped_shr,
        );
    }

    /// Test iterator reductions
    #[quickcheck]
    fn reductions(indices: Vec<BitmapIndex>) {
        use std::iter::Copied;
        type IndexRefIter<'a> = std::slice::Iter<'a, BitmapIndex>;

        fn test_reduction(
            indices: &[BitmapIndex],
            neutral: BitmapIndex,
            overflowing_op: impl Fn(BitmapIndex, BitmapIndex) -> (BitmapIndex, bool),
            reduce_by_ref: impl Fn(IndexRefIter) -> BitmapIndex + RefUnwindSafe,
            reduce_by_value: impl Fn(Copied<IndexRefIter>) -> BitmapIndex + RefUnwindSafe,
        ) {
            // Perform reduction using the overflowing operator
            let (wrapping_result, overflow) = indices.iter().copied().fold(
                (neutral, false),
                |(wrapping_acc, prev_overflow), elem| {
                    let (wrapping_acc, new_overflow) = overflowing_op(wrapping_acc, elem);
                    (wrapping_acc, prev_overflow || new_overflow)
                },
            );

            // Test the standard reductions accordingly
            let reductions: [&(dyn Fn() -> BitmapIndex + RefUnwindSafe); 2] =
                [&|| reduce_by_ref(indices.iter()), &|| {
                    reduce_by_value(indices.iter().copied())
                }];
            for reduction in reductions {
                if overflow {
                    assert_debug_panics(reduction, wrapping_result);
                } else {
                    assert_eq!(reduction(), wrapping_result);
                }
            }
        }

        test_reduction(
            &indices,
            BitmapIndex::ZERO,
            BitmapIndex::overflowing_add,
            |ref_iter| ref_iter.sum::<BitmapIndex>(),
            |value_iter| value_iter.sum::<BitmapIndex>(),
        );
        test_reduction(
            &indices,
            BitmapIndex::ONE,
            BitmapIndex::overflowing_mul,
            |ref_iter| ref_iter.product::<BitmapIndex>(),
            |value_iter| value_iter.product::<BitmapIndex>(),
        );
    }
}
