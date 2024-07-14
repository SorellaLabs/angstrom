//! basic book impl so we can benchmark
use alloy_primitives::U256;
use angstrom_types::{
    orders::OrderId,
    primitive::PoolId,
    sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
};
use order::{OrderCoordinate, OrderDirection};

use self::sort::SortStrategy;
use crate::cfmm::uniswap::MarketSnapshot;

pub mod order;
pub mod sort;
pub mod xpool;

pub type BookID = u128;
pub type OrderID = u128;
pub type OrderVolume = U256;
pub type OrderPrice = U256;

#[derive(Default)]
pub struct OrderBook {
    id:   PoolId,
    amm:  Option<MarketSnapshot>,
    bids: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
    asks: Vec<OrderWithStorageData<GroupedVanillaOrder>>
}

impl OrderBook {
    pub fn new(
        id: PoolId,
        amm: Option<MarketSnapshot>,
        mut bids: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        mut asks: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        sort: Option<SortStrategy>
    ) -> Self {
        // Use our sorting strategy to sort our bids and asks
        let strategy = sort.unwrap_or_default();
        strategy.sort_bids(&mut bids);
        strategy.sort_asks(&mut asks);
        Self { id, amm, bids, asks }
    }

    pub fn id(&self) -> PoolId {
        self.id
    }

    pub fn bids(&self) -> &Vec<OrderWithStorageData<GroupedVanillaOrder>> {
        &self.bids
    }

    pub fn asks(&self) -> &Vec<OrderWithStorageData<GroupedVanillaOrder>> {
        &self.asks
    }

    pub fn amm(&self) -> Option<&MarketSnapshot> {
        self.amm.as_ref()
    }

    pub fn find_coordinate(&self, coord: &OrderCoordinate) -> Option<(OrderDirection, usize)> {
        let OrderCoordinate { book, order } = coord;
        if *book != self.id {
            return None;
        }
        self.find_order(*order)
    }

    /// Given an OrderID, find the order with the matching ID and return an
    /// Option, `None` if not found, otherwise we return a tuple containing the
    /// order's direction and its index in the various order arrays
    pub fn find_order(&self, id: OrderId) -> Option<(OrderDirection, usize)> {
        self.bids
            .iter()
            .enumerate()
            .find(|(_, b)| b.order_id == id)
            .map(|(i, _)| (OrderDirection::Bid, i))
            .or_else(|| {
                self.asks
                    .iter()
                    .enumerate()
                    .find(|(_, b)| b.order_id == id)
                    .map(|(i, _)| (OrderDirection::Ask, i))
            })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cfmm::uniswap::SqrtPriceX96;

    #[test]
    fn can_construct_order_book() {
        // Very basic book construction test
        let bids = vec![];
        let asks = vec![];
        let amm = MarketSnapshot::new(vec![], SqrtPriceX96::from_float_price(0.0)).unwrap();
        OrderBook::new(10, Some(amm), bids, asks, None);
    }
}
