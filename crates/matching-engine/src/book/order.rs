use angstrom_types::{
    matching::{uniswap::PoolPriceVec, CompositeOrder, Debt},
    orders::{OrderFillState, OrderId, OrderPrice, OrderVolume},
    sol_bindings::grouped_orders::{FlashVariants, GroupedVanillaOrder, StandingVariants}
};

use super::BookOrder;

/// Definition of the various types of order that we can serve, as well as the
/// outcomes we're able to have for them
#[derive(Clone, Debug)]
pub enum OrderContainer<'a> {
    /// A complete order from our book
    BookOrder(&'a BookOrder),
    /// A fragment of an order from our book yet to be filled
    BookOrderFragment { order: &'a BookOrder, state: OrderFillState },
    /// An order constructed from the current state of our AMM
    AMM(PoolPriceVec<'a>),
    /// A CompositeOrder built of Debt or AMM or Both
    Composite(CompositeOrder<'a>)
}

impl<'a> OrderContainer<'a> {
    pub fn id(&self) -> Option<OrderId> {
        match self {
            Self::BookOrder(o) => Some(o.order_id),
            Self::BookOrderFragment { order, .. } => Some(order.order_id),
            _ => None
        }
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

    pub fn amm_intersect(&self, debt: Debt) -> eyre::Result<u128> {
        match self {
            Self::AMM(a) => a.start_bound.intersect_with_debt(debt),
            _ => Ok(0)
        }
    }

    /// Is the underlying order a Partial Fill compatible order
    pub fn is_partial(&self) -> bool {
        match self {
            Self::BookOrder(o) => {
                matches!(
                    o.order,
                    GroupedVanillaOrder::Standing(StandingVariants::Partial(_))
                        | GroupedVanillaOrder::KillOrFill(FlashVariants::Partial(_))
                )
            }
            Self::BookOrderFragment { order, .. } => {
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

    /// Retrieve the quantity available within the bounds of a given order
    pub fn quantity(&self, target_price: OrderPrice) -> OrderVolume {
        match self {
            Self::BookOrder(o) => o.quantity(),
            Self::BookOrderFragment { order, state: OrderFillState::PartialFill(partial_q) } => {
                order.quantity().saturating_sub(*partial_q)
            }
            Self::BookOrderFragment { order, .. } => order.quantity(),
            Self::AMM(ammo) => ammo.quantity(target_price).0,
            Self::Composite(c) => c.quantity(target_price.into())
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

    /// Retrieve the starting price bound for a given order
    pub fn price(&self) -> OrderPrice {
        match self {
            Self::BookOrder(o) => o.price_for_book_side(o.is_bid).into(),
            Self::BookOrderFragment { order, .. } => order.price_for_book_side(order.is_bid).into(),
            Self::AMM(o) => (*o.start_bound.price()).into(),
            Self::Composite(o) => o.start_price().into()
        }
    }
}
