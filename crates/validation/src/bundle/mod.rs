use std::{fmt::Debug, pin::Pin, sync::Arc};

use alloy::{primitives::Address, sol_types::SolCall};
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::contract_payloads::angstrom::{AngstromBundle, BundleGasDetails};
use eyre::eyre;
use futures::{Future, FutureExt};
use pade::PadeEncode;
use revm::primitives::{EnvWithHandlerCfg, TxKind};
use tokio::runtime::Handle;

use crate::common::{key_split_threadpool::KeySplitThreadpool, TokenPriceGenerator};

pub mod validator;
pub use validator::*;

pub struct BundleValidator<DB> {
    db:               Arc<DB>,
    angstrom_address: Address,
    /// the address associated with this node.
    /// this will ensure the  node has access and the simulation can pass
    node_address:     Address
}

impl<DB> BundleValidator<DB>
where
    DB: Unpin + Clone + 'static + reth_provider::BlockNumReader + revm::DatabaseRef + Send + Sync,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    pub fn new(db: Arc<DB>, angstrom_address: Address, node_address: Address) -> Self {
        Self { db, angstrom_address, node_address }
    }

    pub fn simulate_bundle(
        &self,
        sender: tokio::sync::oneshot::Sender<eyre::Result<BundleGasDetails>>,
        bundle: AngstromBundle,
        price_gen: &TokenPriceGenerator,
        thread_pool: &mut KeySplitThreadpool<
            Address,
            Pin<Box<dyn Future<Output = ()> + Send>>,
            Handle
        >,
        metrics: ValidationMetrics
    ) {
        let node_address = self.node_address;
        let angstrom_address = self.angstrom_address;
        let db = self.db.clone();

        let conversion_lookup = price_gen.generate_lookup_map();

        thread_pool.spawn_raw(
            async move {
                metrics.simulate_bundle(|| {
                    let bundle = bundle.pade_encode();

                    let mut evm = revm::Evm::builder()
                        .with_ref_db(db.clone())
                        .with_env_with_handler_cfg(EnvWithHandlerCfg::default())
                        .modify_env(|env| {
                            env.cfg.disable_balance_check = true;
                        })
                        .modify_tx_env(|tx| {
                            tx.caller = node_address;
                            tx.transact_to = TxKind::Call(angstrom_address);
                            tx.data =
                        angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::new((
                            bundle.into(),
                        ))
                        .abi_encode()
                        .into();
                        })
                        .build();

                    let result = evm
                        .transact()
                        .map_err(|_| eyre!("failed to transact with revm"))
                        .unwrap();

                    if !result.result.is_success() {
                        let _ = sender.send(Err(eyre!("transaction simulation failed")));
                        return
                    }

                    let res = BundleGasDetails::new(conversion_lookup, result.result.gas_used());
                    let _ = sender.send(Ok(res));
                });
            }
            .boxed()
        )
    }
}
