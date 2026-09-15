use std::{fmt::Debug, pin::Pin, sync::Arc};

use alloy::{
    eips::BlockNumHash,
    primitives::{Address, B256, U256},
    sol_types::SolCall
};
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::{
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails},
    primitive::CHAIN_ID,
    reth_db_wrapper::AtBlock,
    traits::BundleProcessing
};
use eyre::eyre;
use futures::Future;
use pade::PadeEncode;
use revm::{
    Context, InspectEvm, Journal, MainBuilder,
    context::{BlockEnv, CfgEnv, JournalTr, LocalContext, TxEnv},
    database::CacheDB,
    primitives::{TxKind, hardfork::SpecId}
};
use tokio::runtime::Handle;

use crate::{
    common::key_split_threadpool::KeySplitThreadpool, order::sim::console_log::CallDataInspector
};

pub mod validator;
pub use validator::*;

pub struct BundleValidator<DB> {
    /// The state source every simulation takes its own pinned view of.
    db:               Arc<DB>,
    angstrom_address: Address,
    /// the address associated with this node.
    /// this will ensure the  node has access and the simulation can pass
    node_address:     Address
}

impl<DB> BundleValidator<DB>
where
    DB: Unpin
        + Clone
        + 'static
        + reth_provider::BlockNumReader
        + revm::DatabaseRef
        + AtBlock
        + Send
        + Sync,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    pub fn new(db: Arc<DB>, angstrom_address: Address, node_address: Address) -> Self {
        Self { db, angstrom_address, node_address }
    }

    fn apply_slot_overrides_for_token(
        db: &mut CacheDB<Arc<DB>>,
        token: Address,
        quantity: U256,
        uniswap: Address
    ) -> eyre::Result<()>
    where
        <DB as revm::DatabaseRef>::Error: Debug
    {
        use alloy::sol_types::SolValue;
        use revm::primitives::keccak256;

        use crate::order::state::db_state_utils::finders::*;
        // Find the slot for balance and approval for us to take from Uniswap
        let balance_slot = find_slot_offset_for_balance(&db, token)?;

        // first thing we will do is setup Uniswap's token balance.
        let uniswap_balance_slot = keccak256((uniswap, balance_slot).abi_encode());

        // set Uniswap's balance on the token_in
        db.insert_account_storage(token, uniswap_balance_slot.into(), U256::from(2) * quantity)
            .map_err(|e| eyre::eyre!("{e:?}"))?;
        // give angstrom approval

        Ok(())
    }

    /// Places `parent_hash` on the chain. This is also the availability check:
    /// a hash we cannot resolve is an error, never a quiet fall back to the
    /// current tip.
    fn get_block(&self, parent_hash: B256) -> eyre::Result<BlockNumHash> {
        let number = self
            .db
            .block_number(parent_hash)
            .map_err(|e| eyre!("failed to resolve parent {parent_hash} - {e:?}"))?
            .ok_or_else(|| eyre!("parent {parent_hash} is not a known block"))?;

        Ok(BlockNumHash::new(number, parent_hash))
    }

    pub fn simulate_bundle(
        &self,
        sender: tokio::sync::oneshot::Sender<eyre::Result<BundleGasDetails>>,
        bundle: AngstromBundle,
        parent_hash: B256,
        thread_pool: &mut KeySplitThreadpool<
            Address,
            Pin<Box<dyn Future<Output = ()> + Send + Sync>>,
            Handle
        >,
        metrics: ValidationMetrics
    ) {
        let node_address = self.node_address;
        let angstrom_address = self.angstrom_address;

        let parent = match self.get_block(parent_hash) {
            Ok(parent) => parent,
            Err(e) => {
                let _ = sender.send(Err(e));
                return;
            }
        };
        let number = parent.number;

        // A view of the parent that nothing else holds, with a cache built on it and
        // nothing else, so no later request can move what this simulation reads and
        // every read — cache misses included — resolves against `parent`.
        let mut db = CacheDB::new(Arc::new(self.db.at_block(parent)));

        thread_pool.spawn_raw(Box::pin(async move {
            let pool_manager_addr = *angstrom_types::primitive::POOL_MANAGER_ADDRESS.get().unwrap();

            // This is the address that testnet uses
            if alloy::primitives::address!("0x48bC5A530873DcF0b890aD50120e7ee5283E0112") == pool_manager_addr
            {
                tracing::info!("local testnet overrides");

                let overrides = bundle.fetch_needed_overrides(number + 1);
                for (token, slot, value) in overrides.into_slots_with_overrides(angstrom_address) {
                    tracing::trace!(?token, ?slot, ?value, "Inserting bundle override");
                    db.insert_account_storage(token, slot.into(), value).unwrap();
                }
                for asset in bundle.assets.iter() {
                    tracing::trace!(asset = ?asset.addr, quantity = ?asset.take, uniswap_addr = ?pool_manager_addr, ?angstrom_address, "Inserting asset override");
                    Self::apply_slot_overrides_for_token(
                        &mut db,
                        asset.addr,
                        U256::from(asset.take),
                        pool_manager_addr,
                    ).unwrap();

                    Self::apply_slot_overrides_for_token(
                        &mut db,
                        asset.addr,
                        U256::from(asset.settle),
                        angstrom_address,
                    ).unwrap();
                }
            }

            metrics.simulate_bundle(|| {
                let encoded_bundle = bundle.pade_encode();
                let console_log_inspector = CallDataInspector {};

                 let mut evm = Context {
                        tx: TxEnv::default(),
                        block: BlockEnv::default(),
                        cfg: CfgEnv::<SpecId>::default().with_chain_id(*CHAIN_ID.get().unwrap()),
                        journaled_state: Journal::<CacheDB<Arc<DB>>>::new(db.clone()),
                        chain: (),
                        error: Ok(()),
                        local:           LocalContext::default()
                    }
                    .modify_cfg_chained(|cfg| {
                        cfg.disable_nonce_check = true;
                    })
                    .modify_block_chained(|block| {
                        block.number = U256::from(number + 1);
                        tracing::info!(?block.number, "simulating block on");
                    })
                    .modify_tx_chained(|tx| {
                        tx.caller = node_address;
                        tx.kind= TxKind::Call(angstrom_address);
                        tx.chain_id = Some(*CHAIN_ID.get().unwrap());
                        tx.data =
                        angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::new((
                            encoded_bundle.into(),
                        ))
                        .abi_encode()
                        .into();
                    }).build_mainnet_with_inspector(console_log_inspector);

                let tx = std::mem::take(&mut evm.tx);
                // TODO:  Put this on a feature flag so we use `replay()` when not needing debug inspection
                let result = match evm.inspect_one_tx(tx)
                    .map_err(|e| eyre!("failed to transact with revm - {e:?}"))
                {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = sender.send(Err(eyre!(
                            "transaction simulation failed - failed to transaction with revm - \
                             {e:?}"
                        )));
                        return;
                    }
                };

                if !result.is_success() {
                    tracing::error!(?result, block_number=%number + 1);
                    let _ = sender.send(Err(eyre!("transaction simulation failed")));
                    return;
                }

                let res = BundleGasDetails::new(result.gas_used(), parent);
                let _ = sender.send(Ok(res));
            });
        }))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use alloy::primitives::{B256, BlockNumber};
    use angstrom_types::{
        primitive::AngstromAddressConfig,
        reth_db_wrapper::{AtBlock, DBError}
    };
    use futures::StreamExt;
    use reth_provider::{BlockHashReader, BlockNumReader, ProviderError, ProviderResult};
    use revm::state::AccountInfo;

    use super::*;

    pub(crate) const PARENT: B256 = B256::repeat_byte(0xa1);
    pub(crate) const PARENT_NUMBER: u64 = 4_242;

    /// State is empty and every parent lookup is answered from `known`, so a
    /// test controls exactly which parents exist. Records every view taken of
    /// it and, on each read, the block the view it went through was pinned to.
    #[derive(Clone)]
    pub(crate) struct FakeDb {
        known:  Option<u64>,
        errors: bool,
        /// The block this view is pinned to; `None` for the source itself.
        pinned: Option<BlockNumHash>,
        views:  Arc<Mutex<Vec<BlockNumHash>>>,
        reads:  Arc<Mutex<Vec<Option<BlockNumHash>>>>
    }

    impl FakeDb {
        pub(crate) fn knowing(number: u64) -> Self {
            Self {
                known:  Some(number),
                errors: false,
                pinned: None,
                views:  Arc::default(),
                reads:  Arc::default()
            }
        }

        pub(crate) fn unknown() -> Self {
            Self { known: None, ..Self::knowing(0) }
        }

        pub(crate) fn failing() -> Self {
            Self { errors: true, ..Self::unknown() }
        }

        /// The hash `block_hash` answers for `number`.
        pub(crate) fn hash_of(number: u64) -> B256 {
            B256::from(U256::from(number))
        }

        /// The block this view is pinned to.
        pub(crate) fn pinned(&self) -> Option<BlockNumHash> {
            self.pinned
        }

        /// Every view taken of the source, in order.
        pub(crate) fn pinned_to(&self) -> Vec<BlockNumHash> {
            self.views.lock().unwrap().clone()
        }

        /// The pinned block of the view each read went through, in order.
        pub(crate) fn reads(&self) -> Vec<Option<BlockNumHash>> {
            self.reads.lock().unwrap().clone()
        }

        fn record_read(&self) {
            self.reads.lock().unwrap().push(self.pinned);
        }
    }

    impl AtBlock for FakeDb {
        fn at_block(&self, block: BlockNumHash) -> Self {
            self.views.lock().unwrap().push(block);
            Self { pinned: Some(block), ..self.clone() }
        }
    }

    impl revm::DatabaseRef for FakeDb {
        type Error = DBError;

        fn basic_ref(&self, _: Address) -> Result<Option<AccountInfo>, Self::Error> {
            self.record_read();
            Ok(None)
        }

        fn code_by_hash_ref(&self, _: B256) -> Result<revm::bytecode::Bytecode, Self::Error> {
            Ok(Default::default())
        }

        fn storage_ref(&self, _: Address, _: U256) -> Result<U256, Self::Error> {
            self.record_read();
            Ok(U256::ZERO)
        }

        fn block_hash_ref(&self, _: u64) -> Result<B256, Self::Error> {
            Ok(B256::ZERO)
        }
    }

    impl BlockNumReader for FakeDb {
        fn chain_info(&self) -> ProviderResult<reth_chainspec::ChainInfo> {
            unimplemented!()
        }

        fn best_block_number(&self) -> ProviderResult<BlockNumber> {
            unimplemented!()
        }

        fn last_block_number(&self) -> ProviderResult<BlockNumber> {
            unimplemented!()
        }

        fn block_number(&self, _: B256) -> ProviderResult<Option<BlockNumber>> {
            if self.errors {
                return Err(ProviderError::BestBlockNotFound);
            }
            Ok(self.known)
        }
    }

    impl BlockHashReader for FakeDb {
        fn block_hash(&self, number: BlockNumber) -> ProviderResult<Option<B256>> {
            if self.errors {
                return Err(ProviderError::BestBlockNotFound);
            }
            Ok(Some(Self::hash_of(number)))
        }

        fn canonical_hashes_range(
            &self,
            _: BlockNumber,
            _: BlockNumber
        ) -> ProviderResult<Vec<B256>> {
            unimplemented!()
        }
    }

    /// Drives one `simulate_bundle` to completion.
    async fn simulate(db: &FakeDb, parent: B256) -> eyre::Result<BundleGasDetails> {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();

        let mut thread_pool = KeySplitThreadpool::new(Handle::current(), 1);
        let (tx, rx) = tokio::sync::oneshot::channel();

        BundleValidator::new(
            Arc::new(db.clone()),
            Address::repeat_byte(0xaa),
            Address::repeat_byte(0xbb)
        )
        .simulate_bundle(
            tx,
            AngstromBundle::new(vec![], vec![], vec![], vec![], vec![]),
            parent,
            &mut thread_pool,
            ValidationMetrics::new()
        );

        // The work is queued rather than spawned, so it only runs while the pool is
        // polled: a request that failed before queuing resolves without this.
        tokio::select! {
            res = rx => res.unwrap(),
            _ = futures::future::poll_fn(|cx| {
                while let std::task::Poll::Ready(Some(_)) = thread_pool.poll_next_unpin(cx) {}
                std::task::Poll::<()>::Pending
            }) => unreachable!()
        }
    }

    #[tokio::test]
    async fn a_result_carries_the_parent_it_was_produced_against() {
        let db = FakeDb::knowing(PARENT_NUMBER);

        let details = simulate(&db, PARENT).await.unwrap();

        // Both halves of the identity, so a same-height reorg is distinguishable.
        assert_eq!(details.parent(), BlockNumHash::new(PARENT_NUMBER, PARENT));
    }

    #[tokio::test]
    async fn simulation_reads_the_parent_it_was_handed() {
        let db = FakeDb::knowing(PARENT_NUMBER);

        simulate(&db, PARENT).await.unwrap();

        // One view, of the requested parent by number and hash, taken before anything
        // was read — and every read, cache misses included, went through it.
        let parent = BlockNumHash::new(PARENT_NUMBER, PARENT);
        assert_eq!(db.pinned_to(), vec![parent]);
        let reads = db.reads();
        assert!(!reads.is_empty(), "the simulation read nothing");
        assert!(reads.iter().all(|read| *read == Some(parent)), "{reads:?}");
    }

    #[tokio::test]
    async fn an_unavailable_parent_is_an_error() {
        let db = FakeDb::unknown();

        let err = simulate(&db, PARENT).await.unwrap_err();

        assert!(err.to_string().contains("not a known block"), "{err}");
        // It failed before pinning, so nothing was simulated against another state.
        assert!(db.pinned_to().is_empty());
    }

    #[tokio::test]
    async fn a_failed_parent_lookup_is_an_error() {
        let db = FakeDb::failing();

        let err = simulate(&db, PARENT).await.unwrap_err();

        assert!(err.to_string().contains("failed to resolve parent"), "{err}");
        assert!(db.pinned_to().is_empty());
    }
}
