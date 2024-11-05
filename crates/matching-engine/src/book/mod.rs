//! basic book impl so we can benchmark
use angstrom_types::{
    matching::uniswap::PoolSnapshot,
    orders::OrderId,
    primitive::PoolId,
    sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
};
use order::OrderCoordinate;

use self::sort::SortStrategy;

pub mod order;
pub mod sort;
pub mod xpool;

#[derive(Debug, Default)]
pub struct OrderBook {
    id:   PoolId,
    amm:  Option<PoolSnapshot>,
    bids: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
    asks: Vec<OrderWithStorageData<GroupedVanillaOrder>>
}

impl OrderBook {
    pub fn new(
        id: PoolId,
        amm: Option<PoolSnapshot>,
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

    pub fn amm(&self) -> Option<&PoolSnapshot> {
        self.amm.as_ref()
    }

    pub fn find_coordinate(&self, coord: &OrderCoordinate) -> Option<(bool, usize)> {
        let OrderCoordinate { book, order } = coord;
        if *book != self.id {
            return None;
        }
        self.find_order(*order)
    }

    /// Given an OrderID, find the order with the matching ID and return an
    /// Option, `None` if not found, otherwise we return a tuple containing the
    /// order's direction (is_bid) and its index in the various order arrays
    pub fn find_order(&self, id: OrderId) -> Option<(bool, usize)> {
        self.bids
            .iter()
            .enumerate()
            .find(|(_, b)| b.order_id == id)
            .map(|(i, _)| (true, i))
            .or_else(|| {
                self.asks
                    .iter()
                    .enumerate()
                    .find(|(_, b)| b.order_id == id)
                    .map(|(i, _)| (false, i))
            })
    }
}

#[cfg(test)]
mod test {
    use alloy::primitives::FixedBytes;
    use angstrom_types::matching::SqrtPriceX96;

    use super::*;

    #[test]
    fn can_construct_order_book() {
        // Very basic book construction test
        let bids = vec![];
        let asks = vec![];
        let amm = PoolSnapshot::new(vec![], SqrtPriceX96::from_float_price(0.0)).unwrap();
        OrderBook::new(FixedBytes::<32>::random(), Some(amm), bids, asks, None);
    }
}
