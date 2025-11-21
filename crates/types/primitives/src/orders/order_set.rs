use alloy_primitives::B256;

use crate::{orders::OrderWithStorageData, sol_bindings::grouped_orders::AllOrders};

#[derive(Debug)]
pub struct OrderSet<Limit, Searcher> {
    pub limit:    Vec<OrderWithStorageData<Limit>>,
    pub searcher: Vec<OrderWithStorageData<Searcher>>
}

impl<Limit, Searcher> OrderSet<Limit, Searcher>
where
    Limit: Clone,
    Searcher: Clone,
    AllOrders: From<Searcher> + From<Limit>
{
    pub fn total_orders(&self) -> usize {
        self.limit.len() + self.searcher.len()
    }

    pub fn into_all_orders(&self) -> Vec<AllOrders> {
        self.limit
            .clone()
            .into_iter()
            .map(|o| o.order.into())
            .chain(self.searcher.clone().into_iter().map(|o| o.order.into()))
            .collect()
    }

    pub fn into_book_and_searcher(
        self,
        valid_limit: Vec<B256>,
        valid_searcher: Vec<B256>
    ) -> (Vec<OrderWithStorageData<Limit>>, Vec<OrderWithStorageData<Searcher>>) {
        (
            self.limit
                .into_iter()
                .filter(|order| valid_limit.contains(&order.order_id.hash))
                .collect(),
            self.searcher
                .into_iter()
                .filter(|order| valid_searcher.contains(&order.order_id.hash))
                .collect()
        )
    }
}
