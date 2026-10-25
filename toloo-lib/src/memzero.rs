// Minimal zeroize implementation — replaces the `zeroize` crate.
// Provides Zeroize trait for manual implementation on sensitive types.

/// Trait for types that can securely zero their memory.
pub trait Zeroize {
    fn zeroize(&mut self);
}

/// Blanket impl for types that implement `Default` — just assign default.
impl<T: Default> Zeroize for T {
    fn zeroize(&mut self) {
        *self = T::default();
    }
}

/// Extension trait for arrays to zeroize.
pub trait ZeroizeArray {
    fn zeroize(&mut self);
}

impl<T, const N: usize> ZeroizeArray for [T; N]
where
    T: Default + Copy,
{
    fn zeroize(&mut self) {
        self.fill(T::default());
    }
}