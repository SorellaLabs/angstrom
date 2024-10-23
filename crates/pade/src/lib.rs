mod decode;
mod encode;
mod primitives;
// Re-export bitvec so our macro crate can rely on it
pub use bitvec;
pub use decode::*;
pub use encode::*;

pub struct Sequence<const B: usize, T>(std::marker::PhantomData<T>);
impl<const B: usize, T> Sequence<B, T> {}
