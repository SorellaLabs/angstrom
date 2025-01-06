pub trait ComparableOrder {
    fn compare_as_bid<O: ComparableOrder>(ask: &O) -> (u128, u128) {
        (0, 0)
    }
}
