use std::{fmt::Debug, pin::Pin, sync::Arc};

#[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
use alloy::primitives::U256;
use alloy::{primitives::Address, sol_types::SolCall};
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::{
    CHAIN_ID,
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails}
};
use eyre::eyre;
use futures::Future;
use pade::PadeEncode;
use revm::{
    Context, InspectEvm, Journal, MainBuilder,
    context::{BlockEnv, CfgEnv, TxEnv},
    database::CacheDB,
    primitives::{TxKind, hardfork::SpecId}
};
use tokio::runtime::Handle;

use crate::{
    common::{TokenPriceGenerator, key_split_threadpool::KeySplitThreadpool},
    order::sim::console_log::CallDataInspector
};

pub mod validator;
pub use validator::*;

pub struct BundleValidator<DB> {
    db:               CacheDB<Arc<DB>>,
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
        Self { db: CacheDB::new(db), angstrom_address, node_address }
    }

    #[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
    fn apply_slot_overrides_for_token(
        db: &mut CacheDB<Arc<DB>>,
        token: Address,
        quantity: U256,
        uniswap: Address,
        angstrom: Address
    ) -> eyre::Result<()>
    where
        <DB as revm::DatabaseRef>::Error: Debug
    {
        use alloy::sol_types::SolValue;
        use revm::primitives::keccak256;

        use crate::order::state::db_state_utils::finders::*;
        // Find the slot for balance and approval for us to take from Uniswap
        let balance_slot = find_slot_offset_for_balance(&db, token)?;
        let approval_slot = find_slot_offset_for_approval(&db, token)?;

        // first thing we will do is setup Uniswap's token balance.
        let uniswap_balance_slot = keccak256((uniswap, balance_slot).abi_encode());
        // let uniswap_approval_slot =
        //     keccak256((angstrom, keccak256((uniswap,
        // approval_slot).abi_encode())).abi_encode());

        // set Uniswap's balance on the token_in
        db.insert_account_storage(token, uniswap_balance_slot.into(), U256::from(2) * quantity)
            .map_err(|e| eyre::eyre!("{e:?}"))?;
        // give angstrom approval
        // db.insert_account_storage(token, uniswap_approval_slot.into(), U256::from(2)
        // * quantity)     .map_err(|e| eyre::eyre!("{e:?}"))?;

        Ok(())
    }

    #[allow(unused_mut)]
    pub fn simulate_bundle(
        &self,
        sender: tokio::sync::oneshot::Sender<eyre::Result<BundleGasDetails>>,
        bundle: AngstromBundle,
        price_gen: &TokenPriceGenerator,
        thread_pool: &mut KeySplitThreadpool<
            Address,
            Pin<Box<dyn Future<Output = ()> + Send + Sync>>,
            Handle
        >,
        metrics: ValidationMetrics,
        number: u64
    ) {
        let node_address = self.node_address;
        let angstrom_address = self.angstrom_address;
        let mut db = self.db.clone();

        tracing::info!("{bundle:#?}");
        let conversion_lookup = price_gen.generate_lookup_map();

        thread_pool.spawn_raw(Box::pin(async move {
            #[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
            {
                tracing::info!("local testnet overrides");
                use angstrom_types::primitive::TESTNET_POOL_MANAGER_ADDRESS;

                let overrides = bundle.fetch_needed_overrides(number + 1);
                for (token, slot, value) in overrides.into_slots_with_overrides(angstrom_address) {
                    tracing::trace!(?token, ?slot, ?value, "Inserting bundle override");
                    db.insert_account_storage(token, slot.into(), value).unwrap();
                }
                for asset in bundle.assets.iter() {
                    tracing::trace!(asset = ?asset.addr, quantity = ?asset.take, uniswap_addr = ?TESTNET_POOL_MANAGER_ADDRESS, ?angstrom_address, "Inserting asset override");
                    Self::apply_slot_overrides_for_token(
                        &mut db,
                        asset.addr,
                        U256::from(asset.take),
                        TESTNET_POOL_MANAGER_ADDRESS,
                        angstrom_address
                    ).unwrap();
                }
            }

            metrics.simulate_bundle(|| {
                let bundle = bundle.pade_encode();
                let mut console_log_inspector = CallDataInspector {};

                 let mut evm = Context {
                        tx: TxEnv::default(),
                        block: BlockEnv::default(),
                        cfg: CfgEnv::<SpecId>::default(), journaled_state: Journal::<CacheDB<Arc<DB>>>::new(SpecId::LATEST, db.clone()),
                        chain: (),
                        error: Ok(()),
                    }
                    .modify_cfg_chained(|cfg| {
                        cfg.disable_balance_check = true;
                        cfg.disable_nonce_check = true;
                    })
                    .modify_block_chained(|block| {
                        block.number = number + 1;
                        tracing::info!(?block.number, "simulating block on");
                    })
                    .modify_tx_chained(|tx| {
                        tx.caller = node_address;
                        tx.kind= TxKind::Call(angstrom_address);
                        tx.data =
                        angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::new((
                            bundle.into(),
                        ))
                        .abi_encode()
                        .into();
                    }).build_mainnet_with_inspector(console_log_inspector);


                // TODO:  Put this on a feature flag so we use `replay()` when not needing debug inspection
                // let result = match evm.inspect_replay()
                //     .map_err(|e| eyre!("failed to transact with revm - {e:?}"))
                // {
                //     Ok(r) => r,
                //     Err(e) => {
                //         let _ = sender.send(Err(eyre!(
                //             "transaction simulation failed - failed to transaction with revm - \
                //              {e:?}"
                //         )));
                //         return;
                //     }
                // };
                //
                // if !result.result.is_success() {
                //     tracing::warn!(?result.result);
                //     let _ = sender.send(Err(eyre!("transaction simulation failed")));
                //     return;
                // }

                let res = BundleGasDetails::new(conversion_lookup,0);
                let _ = sender.send(Ok(res));
            });
        }))
    }
}
