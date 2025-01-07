pub enum OrderType {
    BookT0,
    BookT1,
    CompositeDebtOnly,
    CompositeAmmOnly,
    CompositeCombo
}

pub trait ComparableOrder {
    fn order_type(&self) -> OrderType;

    fn compare_as_bid<O: ComparableOrder>(&self, ask: &O) -> (u128, u128) {
        match (self.order_type(), ask.order_type()) {
            // Both BookT0 is a t0 match
            (OrderType::BookT0, OrderType::BookT0) => (0, 0),
            // Mixed match is a t0 match
            (OrderType::BookT1, OrderType::BookT0) | (OrderType::BookT0, OrderType::BookT1) => (0, 0),
            // Both BookT1 is a t1 match
            (OrderType::BookT1, OrderType::BookT1) => (0, 0),

            _ => (0, 0)
        }
    }
}
