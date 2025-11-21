use angstrom_types::{
    matching::{CompositeOrder, Debt, Ray},
    orders::{OrderFillState, OrderId, OrderPrice, OrderVolume},
    primitive::Direction,
    sol_bindings::RawPoolOrder
};
use eyre::{OptionExt, eyre};

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

impl OrderContainer<'_> {
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
        if let Self::Composite(o) = self { o.has_amm() } else { false }
    }

    /// Is `true` when the order in the container includes debt, this can only
    /// be true of a Composite order
    pub fn is_debt(&self) -> bool {
        if let Self::Composite(o) = self { o.has_debt() } else { false }
    }

    /// Returns a Debt item covering the partially matched order at its current
    /// price
    pub fn partial_debt(&self, _: u128) -> Option<Debt> {
        None
    }

    /// Represents an applicable book order as a debt at its current price,
    /// taking partial fill into account
    pub fn as_debt(&self, _: Option<u128>, _: bool) -> Option<Debt> {
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
                    angstrom_types::sol_bindings::grouped_orders::AllOrders::PartialFlash(_)
                        | angstrom_types::sol_bindings::grouped_orders::AllOrders::PartialStanding(
                            _
                        )
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

    /// Raw quantity of a book order
    pub fn raw_book_quantity(&self) -> u128 {
        if let Self::BookOrder { order: o, .. } = self { o.amount() } else { 0 }
    }

    pub fn composite_quantities_to_price(&self, target_price: OrderPrice) -> (u128, u128) {
        if let Self::Composite(c) = self { c.calc_quantities(target_price.into()) } else { (0, 0) }
    }

    /// Retrieve the quantity available within the bounds of a given order
    pub fn quantity(&self, _: &OrderContainer, _: Option<&Debt>) -> OrderVolume {
        0
    }

    /// Retrieve the quantity of direct t1 match available for this order.
    /// Right now this is only called when we're matching 2 T1 book orders
    /// against each other
    pub fn quantity_t1(&self, _: Option<&Debt>) -> Option<OrderVolume> {
        None
    }

    /// Get back the maximum amount of T1 out of our bid we can match against
    /// our opposed order for a given amount of T0 matched
    pub fn max_t1_for_t0(&self, _: u128, _: Option<&Debt>) -> Option<OrderVolume> {
        None
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
        if let Self::Composite(c) = self { c.negative_quantity_t1(target_price.into()) } else { 0 }
    }

    /// Retrieve the starting price bound for a given order.  This price is
    /// always t0/t1 and is flipped if necessary
    pub fn price(&self) -> OrderPrice {
        match self {
            Self::BookOrder { .. } => Ray::default().into(),
            Self::Composite(o) => o.start_price().into()
        }
    }
}
