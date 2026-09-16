use std::{fmt::Debug, pin::Pin, sync::Arc};

use alloy::{
    consensus::BlockHeader,
    eips::BlockNumHash,
    primitives::{Address, B256, U256},
    sol_types::SolCall
};
use alloy_evm::{EvmEnv, eth::NextEvmEnvAttributes};
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::{
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails},
    primitive::{CHAIN_ID, ETH_BLOCK_TIME},
    reth_db_wrapper::AtBlock,
    traits::BundleProcessing
};
use eyre::eyre;
use futures::Future;
use pade::PadeEncode;
use reth_chainspec::{EthChainSpec, EthereumHardforks};
use reth_provider::{ChainSpecProvider, HeaderProvider};
use revm::{
    Context, InspectEvm, Journal, MainBuilder,
    context::{JournalTr, LocalContext, TxEnv},
    database::CacheDB,
    primitives::TxKind
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
        + HeaderProvider
        + ChainSpecProvider<ChainSpec: EthereumHardforks>
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

    /// The header of `parent_hash`. This is also the availability check: a
    /// hash we cannot resolve is an error, never a quiet fall back to the
    /// current tip.
    fn get_block(&self, parent_hash: B256) -> eyre::Result<DB::Header> {
        self.db
            .header(parent_hash)
            .map_err(|e| eyre!("failed to resolve parent {parent_hash} - {e:?}"))?
            .ok_or_else(|| eyre!("parent {parent_hash} is not a known block"))
    }

    /// The environment of the block built on `parent`. Its beneficiary and
    /// prevrandao are not known yet, so the parent's stand in.
    fn next_block_env(&self, parent: &DB::Header) -> EvmEnv {
        let chain_spec = self.db.chain_spec();
        let timestamp = parent.timestamp() + ETH_BLOCK_TIME.as_secs();

        EvmEnv::for_eth_next_block(
            parent,
            NextEvmEnvAttributes {
                timestamp,
                suggested_fee_recipient: parent.beneficiary(),
                prev_randao: parent.mix_hash().unwrap_or_default(),
                gas_limit: parent.gas_limit()
            },
            parent
                .next_block_base_fee(chain_spec.base_fee_params_at_timestamp(timestamp))
                .unwrap_or_default(),
            &chain_spec,
            *CHAIN_ID.get().unwrap(),
            chain_spec.blob_params_at_timestamp(timestamp)
        )
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

        let header = match self.get_block(parent_hash) {
            Ok(header) => header,
            Err(e) => {
                let _ = sender.send(Err(e));
                return;
            }
        };
        let EvmEnv { cfg_env, block_env } = self.next_block_env(&header);
        let parent = BlockNumHash::new(header.number(), parent_hash);
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

                tracing::info!(block_number = number + 1, "simulating block on");
                let gas_price = block_env.basefee.into();
                 let mut evm = Context {
                        tx: TxEnv::default(),
                        block: block_env,
                        cfg: cfg_env,
                        journaled_state: Journal::<CacheDB<Arc<DB>>>::new(db.clone()),
                        chain: (),
                        error: Ok(()),
                        local:           LocalContext::default()
                    }
                    .modify_cfg_chained(|cfg| {
                        cfg.disable_nonce_check = true;
                        // The gas limit is the default cap rather than one the node would
                        // send, so the node's balance against it is not what is being tested.
                        cfg.disable_balance_check = true;
                    })
                    .modify_tx_chained(|tx| {
                        tx.caller = node_address;
                        tx.gas_price = gas_price;
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
    use std::{ops::RangeBounds, sync::Mutex};

    use alloy::{
        consensus::Header,
        primitives::{B256, BlockNumber}
    };
    use angstrom_types::{
        primitive::AngstromAddressConfig,
        reth_db_wrapper::{AtBlock, DBError}
    };
    use futures::StreamExt;
    use reth_chainspec::{ChainSpec, MAINNET};
    use reth_primitives_traits::SealedHeader;
    use reth_provider::{BlockHashReader, BlockNumReader, ProviderError, ProviderResult};
    use revm::{bytecode::Bytecode, state::AccountInfo};

    use super::*;

    pub(crate) const PARENT: B256 = B256::repeat_byte(0xa1);
    /// A Prague-era mainnet block and its timestamp, so the chain spec places
    /// it after the merge.
    pub(crate) const PARENT_NUMBER: u64 = 22_750_000;
    const PARENT_TIMESTAMP: u64 = 1_750_000_000;
    const ANGSTROM: Address = Address::repeat_byte(0xaa);
    const PARENT_GAS_LIMIT: u64 = 30_000_000;

    /// State is empty apart from `angstrom_code` and every parent lookup is
    /// answered from `known`, so a test controls exactly which parents exist.
    /// Records every view taken of it and, on each read, the block the view it
    /// went through was pinned to.
    #[derive(Clone, Debug)]
    pub(crate) struct FakeDb {
        known:         Option<u64>,
        errors:        bool,
        angstrom_code: Option<Bytecode>,
        /// The block this view is pinned to; `None` for the source itself.
        pinned:        Option<BlockNumHash>,
        views:         Arc<Mutex<Vec<BlockNumHash>>>,
        reads:         Arc<Mutex<Vec<Option<BlockNumHash>>>>
    }

    impl FakeDb {
        pub(crate) fn knowing(number: u64) -> Self {
            Self {
                known:         Some(number),
                errors:        false,
                angstrom_code: None,
                pinned:        None,
                views:         Arc::default(),
                reads:         Arc::default()
            }
        }

        pub(crate) fn unknown() -> Self {
            Self { known: None, ..Self::knowing(0) }
        }

        pub(crate) fn failing() -> Self {
            Self { errors: true, ..Self::unknown() }
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

        fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
            self.record_read();
            Ok(self
                .angstrom_code
                .clone()
                .filter(|_| address == ANGSTROM)
                .map(AccountInfo::from_bytecode))
        }

        fn code_by_hash_ref(&self, _: B256) -> Result<revm::bytecode::Bytecode, Self::Error> {
            self.record_read();
            Ok(Default::default())
        }

        fn storage_ref(&self, _: Address, _: U256) -> Result<U256, Self::Error> {
            self.record_read();
            Ok(U256::ZERO)
        }

        fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
            self.block_hash(number)?
                .ok_or_else(|| DBError::String(format!("no block hash for {number}")))
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
            unimplemented!()
        }
    }

    impl HeaderProvider for FakeDb {
        type Header = Header;

        fn header(&self, _: B256) -> ProviderResult<Option<Header>> {
            if self.errors {
                return Err(ProviderError::BestBlockNotFound);
            }
            Ok(self.known.map(|number| Header {
                number,
                timestamp: PARENT_TIMESTAMP,
                gas_limit: PARENT_GAS_LIMIT,
                // Above the 15M target, so the next base fee rises.
                gas_used: 20_000_000,
                base_fee_per_gas: Some(1_000_000_000),
                ..Default::default()
            }))
        }

        fn header_by_number(&self, _: u64) -> ProviderResult<Option<Header>> {
            unimplemented!()
        }

        fn headers_range(&self, _: impl RangeBounds<BlockNumber>) -> ProviderResult<Vec<Header>> {
            unimplemented!()
        }

        fn sealed_header(&self, _: BlockNumber) -> ProviderResult<Option<SealedHeader<Header>>> {
            unimplemented!()
        }

        fn sealed_headers_while(
            &self,
            _: impl RangeBounds<BlockNumber>,
            _: impl FnMut(&SealedHeader<Header>) -> bool
        ) -> ProviderResult<Vec<SealedHeader<Header>>> {
            unimplemented!()
        }
    }

    impl ChainSpecProvider for FakeDb {
        type ChainSpec = ChainSpec;

        fn chain_spec(&self) -> Arc<ChainSpec> {
            MAINNET.clone()
        }
    }

    impl BlockHashReader for FakeDb {
        /// Answered as of the pinned block, as the wrapper's is, so a lookup
        /// through the unpinned source errors rather than passing.
        fn block_hash(&self, number: BlockNumber) -> ProviderResult<Option<B256>> {
            self.record_read();
            let pinned = self.pinned.ok_or(ProviderError::BestBlockNotFound)?;
            Ok((number < pinned.number).then(|| B256::from(U256::from(number))))
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

        BundleValidator::new(Arc::new(db.clone()), ANGSTROM, Address::repeat_byte(0xbb))
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
    async fn simulation_runs_in_the_next_block_derived_from_the_parent_header() {
        // Stops only when every block field matches the block after the parent,
        // and reverts otherwise.
        let expected: [(u8, u64); 4] = [
            (0x42, PARENT_TIMESTAMP + 12), // TIMESTAMP
            (0x43, PARENT_NUMBER + 1),     // NUMBER
            (0x45, PARENT_GAS_LIMIT),      // GASLIMIT
            (0x48, 1_041_666_666)          // BASEFEE, raised by the parent's excess gas
        ];
        let mut code = vec![0x60, 0x01]; // PUSH1 1
        for (opcode, value) in expected {
            code.push(opcode);
            code.push(0x67); // PUSH8
            code.extend(value.to_be_bytes());
            code.extend([0x14, 0x16]); // EQ AND
        }
        let jumpdest = code.len() as u8 + 6;
        code.extend([0x60, jumpdest, 0x57, 0x5f, 0x5f, 0xfd, 0x5b, 0x00]); // JUMPI, REVERT, JUMPDEST STOP
        let db = FakeDb {
            angstrom_code: Some(Bytecode::new_raw(code.into())),
            ..FakeDb::knowing(PARENT_NUMBER)
        };

        simulate(&db, PARENT).await.unwrap();
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
