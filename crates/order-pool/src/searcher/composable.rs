use std::collections::{BTreeMap, HashMap};

use alloy_primitives::B256;
use angstrom_types::orders::{
    OrderId, PooledComposableOrder, PooledSearcherOrder, SearcherPriorityData, ValidatedOrder
};

use super::{SearcherPoolError, V1_LP_POOlS, SEARCHER_POOL_MAX_SIZE};
use crate::common::{SizeTracker, ValidOrder};
pub struct ComposableSearcherPool<CS: PooledComposableOrder + PooledSearcherOrder> {
    sub_pools: Vec<PendingPool<CS>>
}

impl<CS: PooledComposableOrder + PooledSearcherOrder> ComposableSearcherPool<CS>
where
    CS: PooledSearcherOrder<ValidationData = SearcherPriorityData>
{
    pub fn new(max_size: Option<usize>) -> Self {
        let sub_pools = (0..max_size.unwrap_or(V1_LP_POOlS)) // Default to 15 if None
            .map(|_| PendingPool::new())
            .collect();
        ComposableSearcherPool { sub_pools }
    }

    pub fn add_order(&mut self, order: ValidatedOrder<CS>) -> Result<(), SearcherPoolError<CS>> {
        self.sub_pools
            .get_mut(order.pool_id())
            .ok_or(SearcherPoolError::NoPool(order.pool_id))?
            .add_order(order)
    }

    pub fn remove_order(
        &mut self,
        order_id: &OrderId
    ) -> Result<ValidOrder<CS>, SearcherPoolError<CS>> {
        self.sub_pools
            .get_mut(order_id.pool_id)
            .ok_or(SearcherPoolError::NoPool(order_id.pool_id))?
            .remove_order(order_id.hash)
    }

    #[allow(dead_code)]
    pub fn get_winning_orders(&self) -> Vec<ValidOrder<CS>> {
        self.sub_pools
            .iter()
            .filter_map(|pool| pool.winning_order())
            .cloned()
            .collect()
    }
}

pub struct PendingPool<CS: PooledSearcherOrder + PooledComposableOrder> {
    orders:        HashMap<B256, ValidatedOrder<CS>>,
    ordered_arbs:  BTreeMap<CS::ValidationData, B256>,
    _size_tracker: SizeTracker
}

impl<O: PooledSearcherOrder + PooledComposableOrder> PendingPool<O>
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

    pub fn add_order(&mut self, order: ValidOrder<O>) -> Result<(), SearcherPoolError<O>> {
        let priority_data = order.priority_data();
        let hash = order.hash();

        self.orders.insert(order.hash(), order);
        self.ordered_arbs.insert(priority_data, hash);

        Ok(())
    }

    pub fn remove_order(&mut self, hash: B256) -> Result<ValidOrder<O>, SearcherPoolError<O>> {
        let order = self
            .orders
            .remove(&hash)
            .ok_or(SearcherPoolError::OrderNotFound(hash))?; // If the order is not found, return an error

        let priority = order.priority_data();

        // Check if the priority data exists in ordered_arbs, and if so, remove it
        if self.ordered_arbs.remove(&priority).is_none() {
            return Err(SearcherPoolError::OrderNotFound(order.hash()))
        }

        Ok(order)
    }

    pub fn winning_order(&self) -> Option<&ValidOrder<O>> {
        self.ordered_arbs
            .first_key_value()
            .and_then(|(_, hash)| self.orders.get(hash))
    }
}
