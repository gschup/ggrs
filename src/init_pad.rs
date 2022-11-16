use bytemuck::{AnyBitPattern, Zeroable};

/// Credits to @fu5ha https://github.com/Lokathor/bytemuck/issues/84#issuecomment-1312912158

/// This type adds some `const PAD` number of "explicit" or "manual" padding
/// bytes to the end of a struct.
///
/// This is useful to make a type not have *real* padding bytes,
/// and therefore be able to be marked as [`bytemuck::NoUninit`]. Specifically,
/// it's used in the `ffi_union` macro to equalize the size of all
/// fields of a union and therefore remove any "real" padding bytes from the union, making
/// it safe to store in WASM memory and pass through the ark module host memory utility functions.
/// It may also be useful in other places.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct TransparentPad<T, const PAD: usize>(pub T, [u8; PAD]);

// SAFETY: Since `[u8; N]` is always Zeroable, this is safe
unsafe impl<T: Zeroable, const PAD: usize> Zeroable for TransparentPad<T, PAD> {}

// SAFETY: Since `[u8; N]` is always AnyBitPattern, this is safe
unsafe impl<T: AnyBitPattern, const PAD: usize> AnyBitPattern for TransparentPad<T, PAD> {}

#[doc(hidden)] // it is only for us in this crate
#[macro_export]
macro_rules! impl_checked_bit_pattern_for_transparent_pad {
    ($inner:ident) => {
        // SAFETY: The extra padding is always AnyBitPattern, so implies CheckedBitPattern, and this just passes
        // down the necessary safety checks to the inner type.
        unsafe impl<const PAD: usize> bytemuck::CheckedBitPattern
            for $crate::TransparentPad<$inner, PAD>
        {
            type Bits = $crate::TransparentPad<<$inner as bytemuck::CheckedBitPattern>::Bits, PAD>;

            fn is_valid_bit_pattern(bits: &Self::Bits) -> bool {
                <$inner as bytemuck::CheckedBitPattern>::is_valid_bit_pattern(&bits.0)
            }
        }
    };
}

impl<T, const PAD: usize> TransparentPad<T, PAD> {
    /// Stores argument value alongside a zero-intialized slice `[u8; N]`
    pub fn new(inner: T) -> Self {
        Self(inner, [0u8; PAD])
    }
}

impl<T, const PAD: usize> AsRef<T> for TransparentPad<T, PAD> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T, const PAD: usize> AsMut<T> for TransparentPad<T, PAD> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T, const PAD: usize> core::ops::Deref for TransparentPad<T, PAD> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, const PAD: usize> core::ops::DerefMut for TransparentPad<T, PAD> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// Additionnal code
impl<T: PartialEq, const PAD: usize> PartialEq for TransparentPad<T, PAD> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
