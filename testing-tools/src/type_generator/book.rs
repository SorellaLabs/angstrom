use angstrom_types::{
    matching::{uniswap::PoolSnapshot, SqrtPriceX96},
    primitive::PoolId,
    sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
};
use matching_engine::book::{sort::SortStrategy, OrderBook};
use uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio;

use super::{
    amm::generate_single_position_amm_at_tick,
    orders::{DistributionParameters, OrderDistributionBuilder}
};

// What are the parameters of an order builder?  A set of orders can be from
#[derive(Default)]
pub struct BookBuilder {
    _poolid: Option<PoolId>,
    _amm:    Option<PoolSnapshot>,
    _bids:   Option<Vec<OrderWithStorageData<GroupedVanillaOrder>>>,
    _asks:   Option<Vec<OrderWithStorageData<GroupedVanillaOrder>>>,
    _sort:   Option<SortStrategy>
}

impl BookBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build(self) -> OrderBook {
        let id = self._poolid.unwrap_or_default();
        let amm = self._amm;
        let bids = self._bids.unwrap_or_default();
        let asks = self._asks.unwrap_or_default();
        let sort = self._sort;
        OrderBook::new(id, amm, bids, asks, sort)
    }

    pub fn poolid(mut self, poolid: PoolId) -> Self {
        self._poolid = Some(poolid);
        self
    }

    pub fn amm(mut self, amm: Option<PoolSnapshot>) -> Self {
        self._amm = amm;
        self
    }

    pub fn bids(mut self, bids: Vec<OrderWithStorageData<GroupedVanillaOrder>>) -> Self {
        self._bids = Some(bids);
        self
    }

    pub fn asks(mut self, asks: Vec<OrderWithStorageData<GroupedVanillaOrder>>) -> Self {
        self._asks = Some(asks);
        self
    }

    pub fn sort(mut self, sort: Option<SortStrategy>) -> Self {
        self._sort = sort;
        self
    }
}

pub fn generate_simple_cross_book(pool_id: PoolId, order_count: usize, price: f64) -> OrderBook {
    let valid_block = 10;
    let (bidprice, askprice) = DistributionParameters::crossed_at(price);
    let (bidquant, askquant) = DistributionParameters::fixed_at(100.0);
    let bids = OrderDistributionBuilder::new()
        .bid()
        .order_count(order_count)
        .price_params(bidprice)
        .volume_params(bidquant)
        .pool_id(pool_id)
        .valid_block(valid_block)
        .build()
        .unwrap();
    let asks = OrderDistributionBuilder::new()
        .ask()
        .order_count(order_count)
        .price_params(askprice)
        .volume_params(askquant)
        .pool_id(pool_id)
        .valid_block(valid_block)
        .build()
        .unwrap();
    let amm_tick = get_tick_at_sqrt_ratio(SqrtPriceX96::from_float_price(price).into()).unwrap();
    let amm = generate_single_position_amm_at_tick(amm_tick, 10000, 2e18 as u128);
    BookBuilder::new()
        .poolid(pool_id)
        .bids(bids)
        .asks(asks)
        .amm(Some(amm))
        .build()
}

pub fn generate_one_sided_book(
    bid_side: bool,
    pool_id: PoolId,
    order_count: usize,
    price: f64
) -> OrderBook {
    let valid_block = 10;
    let (bidprice, askprice) = DistributionParameters::crossed_at(price);
    let (bidquant, askquant) = DistributionParameters::fixed_at(100.0);
    let bids = if bid_side {
        OrderDistributionBuilder::new()
            .bid()
            .order_count(order_count)
            .price_params(bidprice)
            .volume_params(bidquant)
            .pool_id(pool_id)
            .valid_block(valid_block)
            .build()
            .unwrap()
    } else {
        Vec::new()
    };
    let asks = if bid_side {
        Vec::new()
    } else {
        OrderDistributionBuilder::new()
            .ask()
            .order_count(order_count)
            .price_params(askprice)
            .volume_params(askquant)
            .pool_id(pool_id)
            .valid_block(valid_block)
            .build()
            .unwrap()
    };
    let amm_tick = get_tick_at_sqrt_ratio(SqrtPriceX96::from_float_price(price).into()).unwrap();
    let amm = generate_single_position_amm_at_tick(amm_tick, 10000, 2e18 as u128);
    BookBuilder::new()
        .poolid(pool_id)
        .bids(bids)
        .asks(asks)
        .amm(Some(amm))
        .build()
}

#[cfg(test)]
mod tests {
    use super::BookBuilder;
    use crate::type_generator::amm::generate_amm_market;

    #[test]
    fn can_be_constructed() {
        BookBuilder::new();
    }

    #[test]
    fn adds_amm_to_book() {
        let snapshot = generate_amm_market(100);
        let book = BookBuilder::new().amm(Some(snapshot.clone())).build();
        assert!(book.amm().is_some(), "No AMM in book");
        assert!(*book.amm().unwrap() == snapshot, "AMM in book isn't equal to what was provided");
    }
}
