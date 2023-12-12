pub mod atomics;
pub mod macros;
pub mod poll_ext;

pub use atomics::*;
pub use poll_ext::*;

pub trait GenericExt<T> {
    fn some_if<F>(self, predicate: F) -> Option<T>
    where
        F: FnOnce(&T) -> bool;
}

impl<T> GenericExt<T> for T {
    fn some_if<F>(self, predicate: F) -> Option<T>
    where
        F: FnOnce(&T) -> bool
    {
        predicate(&self).then_some(self)
    }
}
