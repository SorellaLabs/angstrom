pub trait OwnedMap {
    fn owned_map<F, R>(self, f: F) -> Self
    where
        F: FnOnce() -> R;
}

impl<D> OwnedMap for Option<D> {
    fn owned_map<F, R>(self, f: F) -> Self
    where
        F: FnOnce() -> R
    {
        self.inspect(|_| {
            f();
        })
    }
}
