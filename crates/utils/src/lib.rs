pub mod macros;
pub mod poll_ext;
pub mod sync_pipeline;

pub mod map;
pub mod timer;
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

pub trait FnResultOption<T> {
    fn invert_or_else<F, E>(self, predicate: F) -> Result<Option<T>, E>
    where
        F: FnOnce() -> Result<Option<T>, E>;

    fn invert_map_or_else<F, E>(self, predicate: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>;
}

impl<T> FnResultOption<T> for Option<T> {
    fn invert_or_else<F, E>(self, predicate: F) -> Result<Option<T>, E>
    where
        F: FnOnce() -> Result<Option<T>, E>
    {
        self.map(Ok).or_else(|| predicate().transpose()).transpose()
    }

    fn invert_map_or_else<F, E>(self, predicate: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>
    {
        self.map_or_else(predicate, |v| Ok(v))
    }
}
