use alloy_primitives::B256;
use angstrom_types::orders::PoolOrder;
mod size;

use angstrom_types::orders::ValidatedOrder;
pub use size::*;

pub type ValidOrder<O> = ValidatedOrder<O>;

#[derive(Debug, Clone)]
pub enum Order<L, CL, S, CS>
where
    L: PoolOrder,
    CL: PoolOrder,
    S: PoolOrder,
    CS: PoolOrder
{
    Limit(L),
    ComposableLimit(CL),
    Searcher(S),
    ComposableSearcher(CS)
}

impl<L, CL, S, CS> Order<L, CL, S, CS>
where
    L: PoolOrder,
    CL: PoolOrder,
    S: PoolOrder,
    CS: PoolOrder
{
    pub fn add_limit(order: L) -> Self {
        Self::Limit(order)
    }

    pub fn add_composable_limit(order: CL) -> Self {
        Self::ComposableLimit(order)
    }

    pub fn add_searcher(order: S) -> Self {
        Self::Searcher(order)
    }

    pub fn add_composable_searcher(order: CS) -> Self {
        Self::ComposableSearcher(order)
    }

    pub fn hash(&self) -> B256 {
        match self {
            Order::Limit(l) => l.hash(),
            Order::Searcher(l) => l.hash(),
            Order::ComposableLimit(l) => l.hash(),
            Order::ComposableSearcher(l) => l.hash()
        }
    }
}
