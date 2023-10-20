use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use ethers_core::types::U256;
use futures::{stream::FuturesUnordered, Stream, StreamExt};
use guard_types::on_chain::{
    CallerInfo, PoolKey, RawBundle, RawLvrSettlement, RawUserSettlement, SearcherOrUser,
    SimmedBundle, SimmedLvrSettlement, SimmedUserSettlement
};
use revm::primitives::B160;
use sim::{
    errors::{SimError, SimResult},
    Simulator
};
use tracing::{debug, info, trace};

#[derive(Debug, Clone)]
pub enum BundleSolverMsg {
    NewBestBundle(Arc<SimmedBundle>),
    NewUserTransaction(Arc<SimmedUserSettlement>),
    NewSearcherTransaction(Arc<SimmedLvrSettlement>)
}

pub type SimFut = Pin<Box<dyn Future<Output = Result<SimResult, SimError>> + Send + 'static>>;

/// Handles what is the currently best bundle and tries
/// to beat it
pub struct BundleSolver<S: Simulator + 'static> {
    all_user_txes:     HashSet<SimmedUserSettlement>,
    // need to know this to properly route multihop transactions
    best_searcher_tx:  HashMap<PoolKey, SimmedLvrSettlement>,
    bytes_to_pool_key: HashMap<[u8; 32], PoolKey>,

    sim:                 S,
    // tmp
    call_info:           CallerInfo,
    pending_simulations: FuturesUnordered<SimFut>
}

impl<S: Simulator + 'static> BundleSolver<S> {
    pub fn new(sim: S, bytes_to_pool_key: Vec<PoolKey>) -> Self {
        Self {
            sim,
            bytes_to_pool_key: bytes_to_pool_key
                .into_iter()
                .map(|key| (key.clone().into(), key))
                .collect(),
            best_searcher_tx: HashMap::default(),
            all_user_txes: HashSet::default(),
            pending_simulations: FuturesUnordered::default(),
            call_info: CallerInfo {
                address:   B160::default(),
                nonce:     69,
                overrides: HashMap::new()
            }
        }
    }

    pub fn new_bundle(&mut self, bundle: RawBundle) {
        let handle = self.sim.clone();
        let call_info = self.call_info.clone();
        self.pending_simulations
            .push(Box::pin(async move { handle.simulate_bundle(call_info, bundle).await }));
    }

    pub fn new_searcher_transaction(&mut self, tx: RawLvrSettlement) {
        let handle = self.sim.clone();
        let call_info = self.call_info.clone();

        self.pending_simulations
            .push(Box::pin(async move { handle.simulate_hooks(tx, call_info).await }));
    }

    pub fn new_user_transaction(&mut self, tx: RawUserSettlement) {
        let handle = self.sim.clone();
        let call_info = self.call_info.clone();

        self.pending_simulations
            .push(Box::pin(async move { handle.simulate_hooks(tx, call_info).await }));
    }

    fn on_sim_res(&mut self, sim_results: Result<SimResult, SimError>) -> Option<BundleSolverMsg> {
        debug!(?sim_results);

        sim_results
            .ok()
            .map(|result| match result {
                SimResult::ExecutionResult(data) => {
                    let sim::BundleOrTransactionResult::HookSimResult {
                        tx,
                        pre_hook_gas,
                        post_hook_gas
                    } = data
                    else {
                        unreachable!()
                    };
                    convert_simmed_results(
                        tx,
                        pre_hook_gas,
                        post_hook_gas,
                        &self.bytes_to_pool_key,
                        &mut self.best_searcher_tx,
                        &mut self.all_user_txes
                    )
                }
                SimResult::SimError(e) => {
                    trace!(?e, "sim error");
                    None
                }
                SimResult::SuccessfulRevmBlockUpdate => {
                    info!("sim sucessfully update to new block");
                    None
                }
            })
            .flatten()
    }
}

impl<S: Simulator + Unpin> Stream for BundleSolver<S> {
    type Item = BundleSolverMsg;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.pending_simulations
            .poll_next_unpin(cx)
            .map(|possible_sim| self.on_sim_res(possible_sim?))
    }
}

fn convert_simmed_results(
    tx: SearcherOrUser,
    pre_hook_gas: U256,
    post_hook_gas: U256,
    bytes_to_pool_key: &HashMap<[u8; 32], PoolKey>,
    best_searcher_tx: &mut HashMap<PoolKey, SimmedLvrSettlement>,
    all_user_tx: &mut HashSet<SimmedUserSettlement>
) -> Option<BundleSolverMsg> {
    match tx {
        guard_types::on_chain::SearcherOrUser::User(user) => {
            let simed_user = SimmedUserSettlement {
                raw:               user,
                amount_out:        U256::zero(),
                amount_gas_actual: pre_hook_gas + post_hook_gas
            };

            if all_user_tx.contains(&simed_user) {
                return None
            }
            all_user_tx.insert(simed_user.clone());

            return Some(BundleSolverMsg::NewUserTransaction(simed_user.into()))
        }
        guard_types::on_chain::SearcherOrUser::Searcher(searcher) => {
            let Some(pool) = bytes_to_pool_key.get(&searcher.order.pool) else { return None };

            let simmed_searcher = SimmedLvrSettlement {
                raw:        searcher,
                gas_actual: pre_hook_gas + post_hook_gas
            };
            match best_searcher_tx.entry(pool.clone()) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(simmed_searcher.clone());
                    return Some(BundleSolverMsg::NewSearcherTransaction(simmed_searcher.into()))
                }
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    if o.get().raw.order.bribe > simmed_searcher.raw.order.bribe {
                        return None
                    }
                    o.insert(simmed_searcher.clone());
                    return Some(BundleSolverMsg::NewSearcherTransaction(simmed_searcher.into()))
                }
            }
        }
    }
}

#[cfg(feature = "test_harness")]
pub mod test_harness {
    use ethers_core::types::H256;
    use guard_types::on_chain::RawBundle;

    use super::*;

    impl<S: Simulator + 'static> UniformClearingSolver<S> {
        /// takes our current best bundle. and then runs it through the sim.
        /// because we don't have a current best. it should then be propagated
        /// through the network since this is our best bundle
        pub fn propagate_bundle(&mut self) {
            let bundle: RawBundle = self.best_simed_bundle.take().unwrap().into();
            self.new_bundle(bundle);
        }

        pub fn propagate_user_transactions(&mut self) {
            let user_txes = self
                .all_user_txes
                .drain()
                .map(Into::into)
                .collect::<Vec<_>>();

            self.new_user_transactions(user_txes);
        }

        pub fn propagate_searcher_transactions(&mut self) {
            let searcher_txes = self
                .best_searcher_tx
                .drain()
                .map(|(_, v)| v)
                .map(Into::into)
                .collect::<Vec<_>>();

            self.new_searcher_transactions(searcher_txes)
        }

        /// does a check on the raw bundle. this is to avoid sim discrepancies
        pub fn check_for_bundle(&self, hash: H256) -> bool {
            // autistic lets not use into trait because it consumes
            let Some(bundle) = self.best_simed_bundle.clone() else { return false };

            return hash == (<SimmedBundle as Into<RawBundle>>::into(bundle)).into()
        }

        pub fn check_for_user_tx(&self, hash: H256) -> bool {
            self.all_user_txes.iter().any(|tx| {
                let raw: RawUserSettlement = tx.clone().into();
                hash == raw.into()
            })
        }

        pub fn check_for_searcher_tx(&self, hash: H256) -> bool {
            self.best_searcher_tx.iter().any(|(_, tx)| {
                let raw: RawLvrSettlement = tx.clone().into();
                hash == raw.into()
            })
        }
    }
}
