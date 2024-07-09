use std::sync::Mutex;

use crate::{finalization_pool::FinalizationPool, limit::LimitOrderPool, searcher::SearcherPool};

pub struct OrderStorage {
    limit_orders:                Mutex<LimitOrderPool>,
    searcher_orders:             Mutex<SearcherPool>,
    pending_finalization_orders: Mutex<FinalizationPool>
}
