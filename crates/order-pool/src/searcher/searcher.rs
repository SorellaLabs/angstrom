use std::collections::{BTreeMap, HashMap};

use alloy_primitives::B256;
use angstrom_types::{
    orders::{OrderId, OrderLocation, PooledSearcherOrder, SearcherPriorityData, ValidatedOrder},
    primitive::PoolId
};

use super::{SearcherPoolError, SEARCHER_POOL_MAX_SIZE};
use crate::common::{SizeTracker, ValidOrder};

pub struct VanillaSearcherPool<O: PooledSearcherOrder> {
    sub_pools: HashMap<PoolId, PendingPool<O>>
}

impl<O: PooledSearcherOrder> VanillaSearcherPool<O>
where
    O: PooledSearcherOrder<ValidationData = SearcherPriorityData>
{
    pub fn new(ids: &[PoolId]) -> Self {
        let sub_pools = ids.iter().map(|id| (*id, PendingPool::new())).collect();

        VanillaSearcherPool { sub_pools }
    }

    pub fn add_order(
        &mut self,
        order: ValidOrder<O>
    ) -> Result<OrderLocation, SearcherPoolError<O>> {
        self.sub_pools
            .get_mut(&order.pool_id())
            .ok_or(SearcherPoolError::NoPool(order.pool_id))
            .and_then(|pool| pool.add_order(order))
    }

    pub fn remove_order(
        &mut self,
        order_id: &OrderId
    ) -> Result<ValidOrder<O>, SearcherPoolError<O>> {
        let pool = self
            .sub_pools
            .get_mut(&order_id.pool_id)
            .ok_or(SearcherPoolError::NoPool(order_id.pool_id))?;

        pool.remove_order(order_id.hash)
            .ok_or(SearcherPoolError::OrderNotFound(order_id.hash))
    }

    pub fn get_winning_orders(&self) -> Vec<ValidOrder<O>> {
        self.sub_pools
            .values()
            .filter_map(|pool| pool.winning_order())
            .cloned()
            .collect()
    }
}

pub struct PendingPool<O: PooledSearcherOrder> {
    orders:        HashMap<B256, ValidatedOrder<O>>,
    ordered_arbs:  BTreeMap<O::ValidationData, B256>,
    _size_tracker: SizeTracker
}

impl<O: PooledSearcherOrder> PendingPool<O>
where
    O: PooledSearcherOrder<ValidationData = SearcherPriorityData>
{
    pub fn new() -> Self {
        Self {
            orders:        HashMap::new(),
            ordered_arbs:  BTreeMap::new(),
            _size_tracker: SizeTracker::new(Some(SEARCHER_POOL_MAX_SIZE))
        }
    }

    pub fn add_order(
        &mut self,
        order: ValidOrder<O>
    ) -> Result<OrderLocation, SearcherPoolError<O>> {
        let priority_data = order.priority_data();
        let hash = order.hash();
        self.orders.insert(hash, order);
        self.ordered_arbs.insert(priority_data, hash);

        Ok(OrderLocation::VanillaSearcher)
    }

    pub fn remove_order(&mut self, hash: B256) -> Option<ValidOrder<O>> {
        let order = self.orders.remove(&hash)?;
        //TODO: why fetch when we could pass it as param, given that we do the initial
        // lookup?
        let priority = order.priority_data();

        self.ordered_arbs.remove(&priority);
        Some(order)
    }

    pub fn winning_order(&self) -> Option<&ValidOrder<O>> {
        self.ordered_arbs
            .first_key_value()
            .and_then(|(_, hash)| self.orders.get(hash))
    }
}
