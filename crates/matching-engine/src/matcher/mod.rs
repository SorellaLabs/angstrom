mod volume;
use angstrom_types::{
    matching::SqrtPriceX96,
    orders::{OrderPrice, OrderVolume}
};
pub use volume::VolumeFillMatcher;

/// Preliminary implementation of a struct that captures all the information
/// we'd want to get out of a finished match for us to use for heurestics and
/// evaluation
#[derive(Clone, Debug, Default)]
pub struct Solution {
    /// Determined Uniform Clearing Price
    pub price:             Option<OrderPrice>,
    /// Total volume moved
    pub total_volume:      OrderVolume,
    /// Volume moved in Partial Fill orders (bids, asks)
    pub partial_volume:    (OrderVolume, OrderVolume),
    /// Which side of the book did the AMM act on
    pub amm_is_bid:        Option<bool>,
    /// Volume moved via the AMM
    pub amm_volume:        OrderVolume,
    /// Final AMM price
    pub amm_final_price:   Option<SqrtPriceX96>,
    /// Final average price of execution for the AMM
    pub amm_average_price: Option<SqrtPriceX96>
}
