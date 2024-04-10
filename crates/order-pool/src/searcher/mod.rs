use std::fmt::Debug;

use alloy_primitives::B256;
use angstrom_types::{
    orders::{OrderId, PooledComposableOrder, PooledSearcherOrder, SearcherPriorityData},
    primitive::PoolId
};
use composable::ComposableSearcherPool;

use self::searcher::VanillaSearcherPool;
use crate::common::{SizeTracker, ValidOrder};

mod composable;
mod searcher;

pub const SEARCHER_POOL_MAX_SIZE: usize = 15;
#[allow(non_upper_case_globals)]
pub const V1_LP_POOlS: usize = 5;

pub struct SearcherPool<S: PooledSearcherOrder, CS: PooledComposableOrder + PooledSearcherOrder> {
    /// Holds all non composable searcher order pools
    searcher_orders: VanillaSearcherPool<S>,
    /// Holds all composable searcher order pools
    composable_searcher_orders: ComposableSearcherPool<CS>,
    /// The size of the current transactions.
    _size: SizeTracker
}

impl<S, CS> SearcherPool<S, CS>
where
    S: PooledSearcherOrder<ValidationData = SearcherPriorityData>,
    CS: PooledComposableOrder
        + PooledSearcherOrder<ValidationData = SearcherPriorityData>
        + PooledComposableOrder
{
    pub fn new(ids: &[PoolId], max_size: Option<usize>) -> Self {
        Self {
            searcher_orders: VanillaSearcherPool::new(ids),
            composable_searcher_orders: ComposableSearcherPool::new(ids),
            _size: SizeTracker { max: max_size, current: 0 }
        }
    }

    #[allow(dead_code)]
    pub fn add_searcher_order(&mut self, order: ValidOrder<S>) -> Result<(), SearcherPoolError<S>> {
        let size = order.size();
        if !self._size.has_space(size) {
            return Err(SearcherPoolError::MaxSize(order.order))
        }

        self.searcher_orders.add_order(order)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn add_composable_searcher_order(
        &mut self,
        order: ValidOrder<CS>
    ) -> Result<(), SearcherPoolError<CS>> {
        let size = order.size();
        if !self._size.has_space(size) {
            return Err(SearcherPoolError::MaxSize(order.order))
        }

        self.composable_searcher_orders.add_order(order)?;
        Ok(())
    }

    pub fn remove_searcher_order(
        &mut self,
        id: &OrderId
    ) -> Result<ValidOrder<S>, SearcherPoolError<S>> {
        self.searcher_orders.remove_order(id)
    }

    pub fn remove_composable_searcher_order(
        &mut self,
        id: &OrderId
    ) -> Result<ValidOrder<CS>, SearcherPoolError<CS>> {
        self.composable_searcher_orders.remove_order(id)
    }

    pub fn get_winning_orders_vanilla(&self) -> Vec<ValidOrder<S>> {
        self.searcher_orders.get_winning_orders()
    }

    pub fn get_winning_orders_composable(&self) -> Vec<ValidOrder<CS>> {
        self.composable_searcher_orders.get_winning_orders()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SearcherPoolError<O: Debug> {
    #[error(
        "Pool has reached max size, and order doesn't satisify replacment requirements, Order: \
         {0:#?}"
    )]
    MaxSize(O),
    #[error("No pool was found for address: {0}")]
    NoPool(PoolId),
    #[error("Order Not Found")]
    OrderNotFound(B256)
}
