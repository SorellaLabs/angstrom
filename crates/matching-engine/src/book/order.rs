use std::cmp::Ordering;

use angstrom_types::{
    matching::{
        max_t1_for_t0,
        uniswap::{Direction, PoolPriceVec},
        CompositeOrder, Debt, DebtType
    },
    orders::{OrderFillState, OrderId, OrderPrice, OrderVolume},
    sol_bindings::{
        grouped_orders::{
            FlashVariants, GroupedVanillaOrder, OrderWithStorageData, StandingVariants
        },
        RawPoolOrder
    }
};

use super::BookOrder;

/// Definition of the various types of order that we can serve, as well as the
/// outcomes we're able to have for them
#[derive(Clone, Debug)]
pub enum OrderContainer<'a> {
    /// An order from our Book and its current fill state
    BookOrder { order: &'a BookOrder, state: OrderFillState },
    /// An order constructed from the current state of our AMM
    AMM(PoolPriceVec<'a>),
    /// A CompositeOrder built of Debt or AMM or Both
    Composite(CompositeOrder<'a>)
}

impl<'a> From<&'a BookOrder> for OrderContainer<'a> {
    fn from(value: &'a BookOrder) -> Self {
        Self::BookOrder { order: value, state: OrderFillState::Unfilled }
    }
}

impl<'a> From<PoolPriceVec<'a>> for OrderContainer<'a> {
    fn from(value: PoolPriceVec<'a>) -> Self {
        Self::AMM(value)
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

    /// Is `true` when the order in the container includes the AMM, either as a
    /// distinct AMM order or as a Composite order that includes the AMM
    pub fn is_amm(&self) -> bool {
        if let Self::Composite(o) = self {
            o.has_amm()
        } else {
            matches!(self, Self::AMM(_))
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

    /// Represents an applicable order as a debt at its current price
    pub fn as_debt(&self) -> Option<Debt> {
        if self.inverse_order() {
            if let Self::BookOrder { order: o, .. } = self {
                let magnitude = if o.is_bid {
                    DebtType::exact_in(o.max_q())
                } else {
                    DebtType::exact_out(o.max_q())
                };
                return Some(Debt::new(magnitude, o.price()))
            }
        }
        None
    }

    pub fn amm_intersect(&self, debt: Debt) -> eyre::Result<u128> {
        match self {
            Self::AMM(a) => a.start_bound.intersect_with_debt(debt),
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
            Self::AMM(_) => false,
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
        debt: Option<&Debt>
    ) -> Option<u128> {
        // We only have a t1 quantity to report if or order is on the T1 side
        if order.is_bid() == order.exact_in() {
            if let Some(d) = debt {
                if order.is_bid() == d.bid_side() {
                    Some(order.max_q() + d.slack())
                } else {
                    Some(order.max_q().saturating_sub(d.slack()))
                }
            } else {
                Some(order.max_q())
            }
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
                    // Round up is inverted because we use the debt's setting for this
                    let debt_portion = d
                        .price()
                        .inverse_quantity(d.magnitude().min(raw_q), !round_up);
                    // If the debt is greater than the order, our order portion should be zero
                    let order_portion = raw_q
                        .checked_sub(d.magnitude())
                        .map(|q| order.price().inverse_quantity(q, round_up))
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
            Self::AMM(ammo) => ammo.quantity(target_price).0,
            Self::Composite(c) => c.quantity(target_price.into())
        }
    }

    /// Retrieve the quantity of direct t1 match available for this order
    pub fn quantity_t1(&self, debt: Option<&Debt>) -> Option<OrderVolume> {
        match self {
            Self::BookOrder { order, state: OrderFillState::PartialFill(partial_q) } => {
                Self::book_order_q_t1(order, debt).map(|q| q.saturating_sub(*partial_q))
            }
            Self::BookOrder { order, .. } => Self::book_order_q_t1(order, debt),
            Self::Composite(c) => None,
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

    /// Retrieve the starting price bound for a given order.  This price is
    /// always t0/t1 and is flipped if necessary
    pub fn price(&self) -> OrderPrice {
        match self {
            Self::BookOrder { order, .. } => order.price_for_book_side(order.is_bid).into(),
            Self::AMM(o) => (*o.start_bound.price()).into(),
            Self::Composite(o) => o.start_price().into()
        }
    }
}

// Make some tests for book_order_quantity
#[cfg(test)]
mod tests {
    use testing_tools::type_generator::{
        amm::generate_single_position_amm_at_tick, orders::UserOrderBuilder
    };

    use super::OrderContainer;

    #[test]
    fn t1_quantity_calculation() {
        let order = UserOrderBuilder::new().with_storage().build();
        let debt = None;
        let q = OrderContainer::book_order_q_t0(&order, debt);
    }
}
