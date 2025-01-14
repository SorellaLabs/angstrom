use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::Arc
};

use alloy_primitives::Address;
use angstrom_types::{
    consensus::PreProposal,
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails},
    matching::{match_estimate_response::BundleEstimate, uniswap::PoolSnapshot},
    orders::PoolSolution,
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
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
use validation::bundle::BundleValidatorHandle;

use crate::{
    book::{BookOrder, OrderBook},
    build_book,
    strategy::{MatchingStrategy, SimpleCheckpointStrategy},
    MatchingEngineHandle
};

pub enum MatcherCommand {
    BuildProposal(
        Vec<BookOrder>,
        Vec<OrderWithStorageData<TopOfBlockOrder>>,
        HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>,
        oneshot::Sender<eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>>
    ),
    EstimateGasPerPool {
        limit:    Vec<BookOrder>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pools:    HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>,
        tx:       oneshot::Sender<eyre::Result<BundleEstimate>>
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
        limit: Vec<BookOrder>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pools: HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> futures_util::future::BoxFuture<eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.send_request(rx, MatcherCommand::BuildProposal(limit, searcher, pools, tx))
                .await
        })
    }
}

pub struct MatchingManager<TP: TaskSpawner, V> {
    _futures:          FuturesUnordered<Pin<Box<dyn Future<Output = ()> + Sync + Send + 'static>>>,
    validation_handle: V,
    _tp:               Arc<TP>
}

impl<TP: TaskSpawner + 'static, V: BundleValidatorHandle> MatchingManager<TP, V> {
    pub fn new(tp: TP, validation: V) -> Self {
        Self {
            _futures:          FuturesUnordered::default(),
            validation_handle: validation,
            _tp:               tp.into()
        }
    }

    pub fn spawn(tp: TP, validation: V) -> MatcherHandle {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let tp = Arc::new(tp);

        let fut = manager_thread(rx, tp.clone(), validation).boxed();
        tp.spawn_critical("matching_engine", fut);

        MatcherHandle { sender: tx }
    }

    pub fn orders_by_pool_id(preproposals: &[PreProposal]) -> HashMap<PoolId, HashSet<BookOrder>> {
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
        limit: Vec<BookOrder>,
        pool_snapshots: &HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> Vec<OrderBook> {
        let book_sources = Self::orders_sorted_by_pool_id(limit);

        book_sources
            .into_iter()
            .map(|(id, orders)| {
                let amm = pool_snapshots.get(&id).map(|value| value.2.clone());
                build_book(id, amm, orders)
            })
            .collect()
    }

    pub fn build_books(
        preproposals: &[PreProposal],
        pool_snapshots: &HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> Vec<OrderBook> {
        // Pull all the orders out of all the preproposals and build OrderPools out of
        // them.  This is ugly and inefficient right now
        let book_sources = Self::orders_by_pool_id(preproposals);

        book_sources
            .into_iter()
            .map(|(id, orders)| {
                let amm = pool_snapshots.get(&id).map(|v| v.2.clone());
                build_book(id, amm, orders)
            })
            .collect()
    }

    pub async fn build_proposal(
        &self,
        limit: Vec<BookOrder>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pool_snapshots: HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> eyre::Result<(Vec<PoolSolution>, BundleGasDetails)> {
        tracing::info!("starting to build proposal");
        // Pull all the orders out of all the preproposals and build OrderPools out of
        // them.  This is ugly and inefficient right now
        let books = Self::build_non_proposal_books(limit.clone(), &pool_snapshots);

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

        // generate bundle without final gas known.
        let bundle =
            AngstromBundle::for_gas_finalization(limit, solutions.clone(), &pool_snapshots)?;

        println!("{:#?}", bundle);
        let gas_response = self.validation_handle.fetch_gas_for_bundle(bundle).await?;

        Ok((solutions, gas_response))
    }

    pub fn orders_sorted_by_pool_id(limit: Vec<BookOrder>) -> HashMap<PoolId, HashSet<BookOrder>> {
        limit.into_iter().fold(HashMap::new(), |mut acc, order| {
            acc.entry(order.pool_id).or_default().insert(order);
            acc
        })
    }

    async fn _estimate_current_fills(
        &self,
        limit: Vec<BookOrder>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pool_snapshots: HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> eyre::Result<BundleEstimate> {
        let books = Self::build_non_proposal_books(limit.clone(), &pool_snapshots);

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

        let bundle =
            AngstromBundle::for_gas_finalization(limit, solutions.clone(), &pool_snapshots)?;
        let _gas_response = self.validation_handle.fetch_gas_for_bundle(bundle).await?;

        todo!()
    }
}

pub async fn manager_thread<TP: TaskSpawner + 'static, V: BundleValidatorHandle>(
    mut input: Receiver<MatcherCommand>,
    tp: Arc<TP>,
    validation_handle: V
) {
    let manager =
        MatchingManager { _futures: FuturesUnordered::default(), _tp: tp, validation_handle };

    while let Some(c) = input.recv().await {
        match c {
            MatcherCommand::BuildProposal(limit, searcher, snapshot, r) => {
                r.send(manager.build_proposal(limit, searcher, snapshot).await)
                    .unwrap();
            }
            MatcherCommand::EstimateGasPerPool { .. } => {
                todo!()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use alloy::primitives::FixedBytes;
    use angstrom_types::consensus::PreProposal;
    use reth_tasks::TokioTaskExecutor;
    use testing_tools::{
        mocks::validator::MockValidator, type_generator::consensus::preproposal::PreproposalBuilder
    };

    use super::MatchingManager;

    #[tokio::test]
    async fn can_build_proposal() {
        let manager = MatchingManager::new(TokioTaskExecutor::default(), MockValidator::default());
        let _ = manager
            .build_proposal(vec![], vec![], HashMap::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn will_combine_preproposals() {
        let manager = MatchingManager::new(TokioTaskExecutor::default(), MockValidator::default());
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

        let limit = preproposals
            .iter()
            .flat_map(|p| p.limit.clone())
            .collect::<Vec<_>>();

        let searcher = preproposals
            .iter()
            .flat_map(|p| p.searcher.clone())
            .collect::<Vec<_>>();

        let res = manager
            .build_proposal(limit, searcher, HashMap::default())
            .await
            .unwrap();
        let orders_in_solution: HashSet<FixedBytes<32>> = res
            .0
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
