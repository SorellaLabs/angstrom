use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::Arc
};

use angstrom_types::{
    consensus::PreProposal,
    matching::{match_estimate_response::BundleEstimate, uniswap::PoolSnapshot},
    orders::PoolSolution,
    primitive::PoolId,
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use futures::{stream::FuturesUnordered, Future};
use futures_util::FutureExt;
use reth_tasks::TaskSpawner;
use tokio::{
    sync::{
        mpsc::{Receiver, Sender},
        oneshot
    },
    task::JoinSet
};

use crate::{
    book::OrderBook,
    build_book,
    strategy::{MatchingStrategy, SimpleCheckpointStrategy},
    MatchingEngineHandle
};

pub enum MatcherCommand {
    BuildProposal(
        Vec<PreProposal>,
        HashMap<PoolId, PoolSnapshot>,
        oneshot::Sender<Result<Vec<PoolSolution>, String>>
    ),
    EstimateGasPerPool {
        limit:    Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        tx:       oneshot::Sender<BundleEstimate>
    }
}

#[derive(Debug, Clone)]
pub struct MatcherHandle {
    pub sender: Sender<MatcherCommand>
}

impl MatcherHandle {
    async fn send(&self, cmd: MatcherCommand) {
        let _ = self.sender.send(cmd).await;
    }

    async fn send_request<T>(&self, rx: oneshot::Receiver<T>, cmd: MatcherCommand) -> T {
        self.send(cmd).await;
        rx.await.unwrap()
    }
}

impl MatchingEngineHandle for MatcherHandle {
    fn solve_pools(
        &self,
        preproposals: Vec<PreProposal>,
        pools: HashMap<PoolId, PoolSnapshot>
    ) -> futures_util::future::BoxFuture<Result<Vec<PoolSolution>, String>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.send_request(rx, MatcherCommand::BuildProposal(preproposals, pools, tx))
                .await
        })
    }
}

pub struct MatchingManager<TP: TaskSpawner> {
    futures: FuturesUnordered<Pin<Box<dyn Future<Output = ()> + Sync + Send + 'static>>>,
    tp:      Arc<TP>
}

impl<TP: TaskSpawner + 'static> MatchingManager<TP> {
    pub fn new(tp: TP) -> Self {
        Self { tp: tp.into(), futures: FuturesUnordered::default() }
    }

    pub fn spawn(tp: TP) -> MatcherHandle {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let tp = Arc::new(tp);

        let fut = manager_thread(rx, tp.clone()).boxed();
        tp.spawn_critical("matching_engine", fut);

        MatcherHandle { sender: tx }
    }

    pub fn orders_by_pool_id(
        preproposals: &[PreProposal]
    ) -> HashMap<PoolId, HashSet<OrderWithStorageData<GroupedVanillaOrder>>> {
        preproposals
            .iter()
            .flat_map(|p| p.limit.iter())
            .cloned()
            .fold(HashMap::new(), |mut acc, order| {
                acc.entry(order.pool_id).or_default().insert(order);
                acc
            })
    }

    pub fn build_non_proposal_books(
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        mut pool_snapshots: HashMap<PoolId, PoolSnapshot>
    ) -> Vec<OrderBook> {
        let book_sources = Self::orders_sorted_by_pool_id(limit);

        book_sources
            .into_iter()
            .map(|(id, orders)| {
                let amm = pool_snapshots.remove(&id);
                build_book(id, amm, orders)
            })
            .collect()
    }

    pub fn build_books(
        preproposals: &[PreProposal],
        mut pool_snapshots: HashMap<PoolId, PoolSnapshot>
    ) -> Vec<OrderBook> {
        // Pull all the orders out of all the preproposals and build OrderPools out of
        // them.  This is ugly and inefficient right now
        let book_sources = Self::orders_by_pool_id(preproposals);

        book_sources
            .into_iter()
            .map(|(id, orders)| {
                let amm = pool_snapshots.remove(&id);
                build_book(id, amm, orders)
            })
            .collect()
    }

    pub async fn build_proposal(
        preproposals: Vec<PreProposal>,
        pool_snapshots: HashMap<PoolId, PoolSnapshot>
    ) -> Result<Vec<PoolSolution>, String> {
        // Pull all the orders out of all the preproposals and build OrderPools out of
        // them.  This is ugly and inefficient right now
        let books = Self::build_books(&preproposals, pool_snapshots);

        let searcher_orders: HashMap<PoolId, OrderWithStorageData<TopOfBlockOrder>> = preproposals
            .iter()
            .flat_map(|p| p.searcher.iter())
            .fold(HashMap::new(), |mut acc, order| {
                acc.entry(order.pool_id).or_insert(order.clone());
                acc
            });

        let mut solution_set = JoinSet::new();
        books.into_iter().for_each(|b| {
            let searcher = searcher_orders.get(&b.id()).cloned();
            // Using spawn-blocking here is not BAD but it might be suboptimal as it allows
            // us to spawn many more tasks that the CPu has threads.  Better solution is a
            // dedicated threadpool and some suggest the `rayon` crate.  This is probably
            // not a problem while I'm testing, but leaving this note here as it may be
            // important for future efficiency gains
            solution_set.spawn_blocking(move || {
                SimpleCheckpointStrategy::run(&b).map(|s| s.solution(searcher))
            });
        });
        let mut solutions = Vec::new();
        while let Some(res) = solution_set.join_next().await {
            if let Ok(Some(r)) = res {
                solutions.push(r);
            }
        }

        Ok(solutions)
    }

    pub fn orders_sorted_by_pool_id(
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>
    ) -> HashMap<PoolId, HashSet<OrderWithStorageData<GroupedVanillaOrder>>> {
        limit.into_iter().fold(HashMap::new(), |mut acc, order| {
            acc.entry(order.pool_id).or_default().insert(order);
            acc
        })
    }

    async fn estimate_current_fills(
        &self,
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pool_snapshots: HashMap<PoolId, PoolSnapshot>
    ) -> BundleEstimate {
        let books = Self::build_non_proposal_books(limit, pool_snapshots);

        let searcher_orders: HashMap<PoolId, OrderWithStorageData<TopOfBlockOrder>> =
            searcher.into_iter().fold(HashMap::new(), |mut acc, order| {
                acc.entry(order.pool_id).or_insert(order);
                acc
            });

        let mut solution_set = JoinSet::new();
        books.into_iter().for_each(|b| {
            let searcher = searcher_orders.get(&b.id()).cloned();
            // Using spawn-blocking here is not BAD but it might be suboptimal as it allows
            // us to spawn many more tasks that the CPu has threads.  Better solution is a
            // dedicated threadpool and some suggest the `rayon` crate.  This is probably
            // not a problem while I'm testing, but leaving this note here as it may be
            // important for future efficiency gains
            solution_set.spawn_blocking(move || {
                SimpleCheckpointStrategy::run(&b).map(|s| s.solution(searcher))
            });
        });
        let mut solutions = Vec::new();
        while let Some(res) = solution_set.join_next().await {
            if let Ok(Some(r)) = res {
                solutions.push(r);
            }
        }

        todo!()
        // Ok(solutions)
    }
}

pub async fn manager_thread<TP: TaskSpawner + 'static>(
    mut input: Receiver<MatcherCommand>,
    tp: Arc<TP>
) {
    let manager = MatchingManager { futures: FuturesUnordered::default(), tp };

    while let Some(c) = input.recv().await {
        match c {
            MatcherCommand::BuildProposal(p, snapshot, r) => {
                r.send(MatchingManager::<TP>::build_proposal(p, snapshot).await)
                    .unwrap();
            }
            MatcherCommand::EstimateGasPerPool { limit, searcher, tx } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use alloy::primitives::FixedBytes;
    use angstrom_types::consensus::PreProposal;
    use reth_tasks::TokioTaskExecutor;
    use testing_tools::type_generator::consensus::preproposal::PreproposalBuilder;

    use super::MatchingManager;

    #[tokio::test]
    async fn can_build_proposal() {
        let preproposals = vec![];
        let _ =
            MatchingManager::<TokioTaskExecutor>::build_proposal(preproposals, HashMap::default())
                .await
                .unwrap();
    }

    #[tokio::test]
    async fn will_combine_preproposals() {
        let task = TokioTaskExecutor::default();
        let manager = MatchingManager::new(task);
        let preproposals: Vec<PreProposal> = (0..3)
            .map(|_| {
                PreproposalBuilder::new()
                    .order_count(10)
                    .for_random_pools(1)
                    .for_block(100)
                    .build()
            })
            .collect();
        let existing_orders: HashSet<FixedBytes<32>> = preproposals
            .iter()
            .flat_map(|p| p.limit.iter().map(|o| o.order_id.hash))
            .collect();

        let res =
            MatchingManager::<TokioTaskExecutor>::build_proposal(preproposals, HashMap::default())
                .await
                .unwrap();
        let orders_in_solution: HashSet<FixedBytes<32>> = res
            .iter()
            .flat_map(|p| p.limit.iter().map(|o| o.id.hash))
            .collect();
        // Check to see if we have differences
        let diff = existing_orders
            .difference(&orders_in_solution)
            .collect::<Vec<_>>();
        if !diff.is_empty() {
            println!("Diff is {}", diff.len())
        }
        assert!(existing_orders == orders_in_solution, "Some orders vanished!");
    }
}
