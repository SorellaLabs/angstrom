use angstrom_types::{
    matching::{max_t1_for_t0, uniswap::Direction, CompositeOrder, Debt, DebtType},
    orders::{OrderFillState, OrderId, OrderPrice, OrderVolume},
    sol_bindings::{
        grouped_orders::{
            FlashVariants, GroupedVanillaOrder, OrderWithStorageData, StandingVariants
        },
        RawPoolOrder
    }
};
use eyre::{eyre, OptionExt};

use super::BookOrder;

/// Definition of the various types of order that we can serve, as well as the
/// outcomes we're able to have for them
#[derive(Clone, Debug)]
pub enum OrderContainer<'a> {
    /// An order from our Book and its current fill state
    BookOrder { order: &'a BookOrder, state: OrderFillState },
    /// A CompositeOrder built of Debt or AMM or Both
    Composite(CompositeOrder<'a>)
}

impl<'a> From<&'a BookOrder> for OrderContainer<'a> {
    fn from(value: &'a BookOrder) -> Self {
        Self::BookOrder { order: value, state: OrderFillState::Unfilled }
    }
}

impl<'a> From<CompositeOrder<'a>> for OrderContainer<'a> {
    fn from(value: CompositeOrder<'a>) -> Self {
        Self::Composite(value)
    }
}

impl<'a> OrderContainer<'a> {
    pub fn id(&self) -> Option<OrderId> {
        match self {
            Self::BookOrder { order, .. } => Some(order.order_id),
            _ => None
        }
    }

    pub fn is_book(&self) -> bool {
        matches!(self, Self::BookOrder { .. })
    }

    pub fn is_composite(&self) -> bool {
        matches!(self, Self::Composite(_))
    }

    pub fn composite_t0_quantities(
        &self,
        t0_input: u128,
        direction: Direction
    ) -> (Option<u128>, Option<u128>) {
        if let Self::Composite(c) = self {
            c.t0_quantities(t0_input, direction)
        } else {
            (None, None)
        }
    }

    /// Is `true` when the order in the container includes the AMM, either as a
    /// distinct AMM order or as a Composite order that includes the AMM
    pub fn is_amm(&self) -> bool {
        if let Self::Composite(o) = self {
            o.has_amm()
        } else {
            false
        }
    }

    /// Is `true` when the order in the container includes debt, this can only
    /// be true of a Composite order
    pub fn is_debt(&self) -> bool {
        if let Self::Composite(o) = self {
            o.has_debt()
        } else {
            false
        }
    }

    /// Returns a Debt item covering the partially matched order at its current
    /// price
    pub fn partial_debt(&self, matched_t0: u128) -> Option<Debt> {
        if self.inverse_order() {
            if let Self::BookOrder { order: o, .. } = self {
                let magnitude = if o.is_bid {
                    DebtType::exact_in(max_t1_for_t0(matched_t0, Direction::BuyingT0, o.price()))
                } else {
                    DebtType::exact_out(max_t1_for_t0(matched_t0, Direction::SellingT0, o.price()))
                };
                return Some(Debt::new(magnitude, o.price()));
            }
        }
        None
    }

    /// Represents an applicable book order as a debt at its current price,
    /// taking partial fill into account
    pub fn as_debt(&self, limit: Option<u128>, is_bid: bool) -> Option<Debt> {
        if self.inverse_order() {
            if let Self::BookOrder { order: o, state } = self {
                let partial_fill = if let OrderFillState::PartialFill(y) = state { *y } else { 0 };
                let whole_order = o.max_q().saturating_sub(partial_fill);
                // If we have a limit, restrict the debt to that much.  This is for partial
                // fills.
                let debt_q = limit
                    .map(|l| std::cmp::min(l, whole_order))
                    .unwrap_or(whole_order);
                let magnitude = DebtType::new(debt_q, is_bid);
                return Some(Debt::new(magnitude, o.price_for_book_side(is_bid)))
            }
        }
        None
    }

    pub fn amm_intersect(&self, debt: Debt) -> eyre::Result<u128> {
        match self {
            Self::Composite(c) => c
                .amm()
                .map(|a| a.intersect_with_debt(debt))
                .ok_or_eyre(eyre!("No intersection"))?,
            _ => Ok(0)
        }
    }

    /// Is the underlying order a Partial Fill compatible order
    pub fn is_partial(&self) -> bool {
        match self {
            Self::BookOrder { order, .. } => {
                matches!(
                    order.order,
                    GroupedVanillaOrder::Standing(StandingVariants::Partial(_))
                        | GroupedVanillaOrder::KillOrFill(FlashVariants::Partial(_))
                )
            }
            Self::Composite(_) => false
        }
    }

    /// If `true`, this is an inverse order that operates with T1 as a base
    /// quantity instead of T0.  That means this order will cause or react to
    /// debt
    pub fn inverse_order(&self) -> bool {
        if let Self::BookOrder { order, .. } = self {
            order.is_bid() == order.exact_in()
        } else {
            false
        }
    }

    fn book_order_q_t1(
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        _debt: Option<&Debt>
    ) -> Option<u128> {
        // We only have a t1 quantity to report if or order is on the T1 side
        if order.is_bid() == order.exact_in() {
            // Let's short circuit this for now
            Some(order.max_q())
            // If we have a debt and the debt has slack, we add it to what this
            // order can offer
            // if let Some(d) = debt {
            //     if order.is_bid() == d.bid_side() {
            //         return Some(order.max_q() + d.slack());
            //     }
            // }
            // Some(order.max_q())
        } else {
            None
        }
    }

    fn book_order_q_t0(
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        debt: Option<&Debt>
    ) -> u128 {
        // Get the raw max quantity of the order
        let raw_q = order.max_q();

        // Bid exact_in orders and ask exact_out orders are in T1 Context
        if order.is_bid() == order.exact_in() {
            // Exact In bid or Exact Out ask both interact with debt so we calculate them at
            // our target_price which is the price of the Debt if one already exists,
            // otherwise the limit price of this order When calculating T1, we
            // round bids down (you get the minimum out) and we round asks up (you put more
            // in than needed)
            // First we find our target price - target price is always properly flipped to
            // be T1/T0
            let round_up = !order.is_bid();
            if let Some(d) = debt {
                if order.is_bid() == d.bid_side() {
                    // The debt is on the same side, so we're going to add to the debt.  We
                    // calculate how much additional T0 our order's T1 component would add to the
                    // debt and offer that.
                    d.additional_t0_needed(raw_q)
                } else {
                    // The debt is on the opposite side, so we're going to be unfilling that debt.
                    // We can offer an amount of T0 to the book that is equal to the amount of T0
                    // currently filling the opposed debt and then, once we have eliminated all of
                    // that debt's T1, we can start our own T1 debt at our order price and fill T0
                    // from that.
                    let debt_portion = d.freed_t0(raw_q);
                    // If the debt is greater than the order, our order portion should be zero
                    let order_portion = raw_q
                        .checked_sub(d.magnitude())
                        .map(|q| {
                            order
                                .price_for_book_side(order.is_bid())
                                .inverse_quantity(q, round_up)
                        })
                        .unwrap_or_default();
                    debt_portion + order_portion
                }
            } else {
                // With no debt, we just offer as much T0 as we can get at our current price
                order
                    .price_for_book_side(order.is_bid())
                    .inverse_quantity(raw_q, round_up)
            }
        } else {
            // Exact Out bid (normal bid) and Exact In ask (normal ask)
            // These don't cause or interact with debt so they just offer what they offer
            raw_q
        }
    }

    /// Raw quantity of a book order
    pub fn raw_book_quantity(&self) -> u128 {
        if let Self::BookOrder { order: o, .. } = self {
            o.max_q()
        } else {
            0
        }
    }

    pub fn composite_quantities_to_price(&self, target_price: OrderPrice) -> (u128, u128) {
        if let Self::Composite(c) = self {
            c.calc_quantities(target_price.into())
        } else {
            (0, 0)
        }
    }

    /// Retrieve the quantity available within the bounds of a given order
    pub fn quantity(&self, opposed_order: &OrderContainer, debt: Option<&Debt>) -> OrderVolume {
        let target_price = opposed_order.price();
        match self {
            Self::BookOrder { order, state } => {
                if let Some(partial_q) = state.partial_q() {
                    // If we have a partial, subtract that from what's available
                    Self::book_order_q_t0(order, debt).saturating_sub(partial_q)
                } else {
                    Self::book_order_q_t0(order, debt)
                }
            }
            Self::Composite(c) => c.quantity(target_price.into())
        }
    }

    /// Retrieve the quantity of direct t1 match available for this order.
    /// Right now this is only called when we're matching 2 T1 book orders
    /// against each other
    pub fn quantity_t1(&self, debt: Option<&Debt>) -> Option<OrderVolume> {
        match self {
            Self::BookOrder { order, state: OrderFillState::PartialFill(partial_q) } => {
                Self::book_order_q_t1(order, debt).map(|q| q.saturating_sub(*partial_q))
            }
            Self::BookOrder { order, .. } => Self::book_order_q_t1(order, debt),
            Self::Composite(_) => None
        }
    }

    /// Get back the maximum amount of T1 out of our bid we can match against
    /// our opposed order for a given amount of T0 matched
    pub fn max_t1_for_t0(&self, t0: u128, debt: Option<&Debt>) -> Option<OrderVolume> {
        match self {
            Self::BookOrder { order, .. } => {
                // If I'm not an inverse order, I can never produce any T1 for T0
                if !self.inverse_order() {
                    return None
                }
                // If the debt is opposed, we can consume that first
                // if let Some(d) = debt {
                //     //
                // }
                if let Some(d) = debt {
                    let cur_t0 = d.current_t0();
                }
                let (t0_consumed, debt_t1) = debt
                    .map(|d| (std::cmp::min(t0, d.current_t0()), d.freed_t1(t0)))
                    .unwrap_or_default();
                let order_t0 = t0.saturating_sub(t0_consumed);
                let order_t1 = order
                    .price_for_book_side(order.is_bid())
                    .quantity(order_t0, order.is_bid);
                Some(debt_t1 + order_t1)
            }
            _ => None
        }
    }

    /// Gets the amount a composite order needs to self-fill in order to move to
    /// a new target price
    pub fn negative_quantity(&self, target_price: OrderPrice) -> OrderVolume {
        match self {
            Self::Composite(c) => c.negative_quantity(target_price.into()),
            _ => 0
        }
    }

    /// Gets the amount of T1 a composite order needs to self-fill in order to
    /// move to a new target price
    pub fn negative_quantity_t1(&self, target_price: OrderPrice) -> OrderVolume {
        if let Self::Composite(c) = self {
            c.negative_quantity_t1(target_price.into())
        } else {
            0
        }
    }

    /// Retrieve the starting price bound for a given order.  This price is
    /// always t0/t1 and is flipped if necessary
    pub fn price(&self) -> OrderPrice {
        match self {
            Self::BookOrder { order, .. } => order.price_for_book_side(order.is_bid).into(),
            Self::Composite(o) => o.start_price().into()
        }
    }
}

// Make some tests for book_order_quantity
#[cfg(test)]
mod tests {
    use testing_tools::type_generator::orders::UserOrderBuilder;

    use super::OrderContainer;

    #[test]
    fn t1_quantity_calculation() {
        let order = UserOrderBuilder::new().with_storage().build();
        let debt = None;
        let _ = OrderContainer::book_order_q_t0(&order, debt);
    }

    #[test]
    fn max_t1_for_t0() {
        // OrderContainer::max_t1_for_t0(&self, t0, debt)
    }
}
