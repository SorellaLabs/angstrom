//! State for Flashblocks.

use std::sync::Arc;

use alloy::primitives::B256;
use alloy_evm::evm::Evm;
use alloy_primitives::map::B256HashMap;
use angstrom_types::{
    flashblocks::{ExecutionPayloadBaseV1, FlashblocksPayloadV1, PendingChain},
    primitive::ChainExt
};
use parking_lot::RwLock;
use reth::{
    api::ConfigureEvm,
    chainspec::EthChainSpec,
    providers::{BlockReaderIdExt, ChainSpecProvider, StateProviderFactory},
    revm::{
        DatabaseCommit, State,
        context::result::ResultAndState,
        database::StateProviderDatabase,
        db::{Cache, CacheDB, TransitionState}
    },
    rpc::types::state::{AccountOverride, StateOverride, StateOverridesBuilder}
};
use reth_optimism_chainspec::OpHardforks;
use reth_optimism_evm::{OpEvmConfig, OpNextBlockEnvAttributes};
use reth_optimism_primitives::OpBlock;
use reth_primitives_traits::{AlloyBlockHeader, Header, RecoveredBlock};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

#[derive(Debug, Clone)]
pub struct PendingStateWriter<P: Clone> {
    /// The canonical chain state provider. Read-only.
    provider: P,
    /// The current pending state.
    pending:  Arc<RwLock<PendingState>>
}

impl<P> PendingStateWriter<P>
where
    P: StateProviderFactory
        + ChainSpecProvider<ChainSpec: EthChainSpec<Header = Header> + OpHardforks>
        + BlockReaderIdExt<Header = Header>
        + Clone
        + 'static
{
    pub fn new(provider: P) -> Self {
        let (tx, _) = broadcast::channel(128);
        let pending = Arc::new(RwLock::new(PendingState {
            sender:           tx,
            pending_chain:    None,
            cache:            None,
            transition_state: None,
            state_overrides:  None
        }));

        Self { pending, provider }
    }

    /// Get a reader for the pending state.
    pub fn reader(&self) -> PendingStateReader<P> {
        PendingStateReader { pending: self.pending.clone(), provider: self.provider.clone() }
    }

    /// Handles a new canonical block.
    pub fn on_canonical_block(&self, block: &RecoveredBlock<OpBlock>) {
        // Reset the pending chain if the block number is greater than the canonical tip
        // number.
        let should_reset = self
            .pending
            .read()
            .pending_chain
            .as_ref()
            .is_some_and(|chain| chain.tip_number() <= block.number());

        if should_reset {
            let mut pending = self.pending.write();
            pending.reset();

            tracing::debug!("Received canonical block {}, reset pending state", block.number(),);
        }
    }

    /// Handles a new flashblock.
    pub fn on_flashblock(&self, flashblock: FlashblocksPayloadV1) -> eyre::Result<()> {
        let is_new = self.pending.read().pending_chain.is_none();

        if is_new {
            if flashblock.base.is_none() {
                tracing::error!(
                    "Received new flashblock without base block, after reset. This means
                     we most likely missed a block, and we won't update the pending chain until \
                     the next canonical block."
                );

                return Err(eyre::eyre!("Received first Flashblock without base block"));
            }

            // NOTE: Create the pending chain before acquiring the write lock for speed™️.
            let new = PendingChain::new(flashblock);
            let mut pending = self.pending.write();
            pending.pending_chain = Some(new);
        } else {
            let mut pending = self.pending.write();
            // SAFETY: We know the pending chain is not `None` because we checked it above.
            pending
                .pending_chain
                .as_mut()
                .unwrap()
                .push_flashblock(flashblock);
        }

        let tip = self.pending.read().tip().unwrap().clone();
        let base = self.pending.read().base().unwrap().clone();

        let mut l1_block_info = reth_optimism_evm::extract_l1_info(tip.body())?;

        let header = tip.clone_sealed_header();

        let mut gas_used = 0;
        let mut next_log_index = 0;

        let prev_block = header.number() - 1;
        let state = self
            .provider
            .state_by_block_number_or_tag(prev_block.into())?;
        let state = StateProviderDatabase::new(state);
        let db = State::builder()
            .with_database(state)
            .with_bundle_update()
            .build();

        let mut db = CacheDB::new(db);

        let mut state_overrides = StateOverridesBuilder::default();
        if let Some(cache) = self.pending.read().cache().cloned() {
            // Set the current cache and transition state.
            db.cache = cache;
            db.db.transition_state = self.pending.read().transition_state().cloned();

            let current_overrides = self
                .pending
                .read()
                .state_overrides()
                .cloned()
                .unwrap_or_default();

            state_overrides = StateOverridesBuilder::new(current_overrides);
        }

        let block_env = OpNextBlockEnvAttributes {
            timestamp:                base.timestamp,
            suggested_fee_recipient:  base.fee_recipient,
            prev_randao:              base.prev_randao,
            gas_limit:                base.gas_limit,
            parent_beacon_block_root: Some(base.parent_beacon_block_root),
            extra_data:               base.extra_data.clone()
        };

        let prev_header = self
            .provider
            .header_by_number(prev_block)?
            .ok_or(eyre::eyre!("Previous block not found"))?;

        let evm_config = OpEvmConfig::optimism(self.provider.chain_spec());
        let evm_env = evm_config.next_evm_env(&prev_header, &block_env)?;

        let mut evm = evm_config.evm_with_env(db, evm_env);

        for tx in tip.transactions_recovered() {
            let ResultAndState { state, .. } = evm.transact(tx)?;

            for (addr, acc) in &state {
                let state_diff = B256HashMap::<B256>::from_iter(
                    acc.storage
                        .iter()
                        .map(|(&key, slot)| (key.into(), slot.present_value.into()))
                );
                let acc_override = AccountOverride {
                    balance:            Some(acc.info.balance),
                    nonce:              Some(acc.info.nonce),
                    code:               acc.info.code.clone().map(|code| code.bytes()),
                    state:              None,
                    state_diff:         Some(state_diff),
                    move_precompile_to: None
                };

                // Append to existing state overrides.
                state_overrides = state_overrides.append(*addr, acc_override);
            }

            // Commit the state to the DB.
            evm.db_mut().commit(state);
        }

        let CacheDB { cache, db } = evm.into_db();

        // Update the pending state after having executed all transactions.
        {
            let mut pending = self.pending.write();
            pending.cache = Some(cache);
            pending.transition_state = db.transition_state;
            pending.state_overrides = Some(state_overrides.build());
        }

        Ok(())
    }
}

/// Contains the overlay state of the pending chain, aka the applied
/// Flashblocks. Read and write.
#[derive(Debug)]
pub struct PendingState {
    sender:           broadcast::Sender<Arc<PendingChain>>,
    pending_chain:    Option<PendingChain>,
    cache:            Option<Cache>,
    transition_state: Option<TransitionState>,
    state_overrides:  Option<StateOverride>
}

impl PendingState {
    pub fn cache(&self) -> Option<&Cache> {
        self.cache.as_ref()
    }

    pub fn transition_state(&self) -> Option<&TransitionState> {
        self.transition_state.as_ref()
    }

    pub fn state_overrides(&self) -> Option<&StateOverride> {
        self.state_overrides.as_ref()
    }

    /// Resets the pending state.
    pub fn reset(&mut self) {
        self.pending_chain = None;
        self.cache = None;
        self.transition_state = None;
        self.state_overrides = None;
    }

    /// Returns the tip of the pending chain.
    pub fn tip(&self) -> Option<&RecoveredBlock<OpBlock>> {
        self.pending_chain.as_ref().map(|chain| chain.tip())
    }

    /// Returns the base of the pending chain.
    pub fn base(&self) -> Option<&ExecutionPayloadBaseV1> {
        self.pending_chain.as_ref().map(|chain| chain.base())
    }
}

/// TODO(mempirate): this should implement a trait so it can be used as a
/// provider.
///
/// Take a look at reth_db_provider.rs
/// Read only access.
#[derive(Debug, Clone)]
pub struct PendingStateReader<P> {
    pub(crate) pending:  Arc<RwLock<PendingState>>,
    pub(crate) provider: P
}

impl<P> PendingStateReader<P> {
    /// Subscribe to [`PendingChain`] updates.
    pub fn subscribe(&self) -> BroadcastStream<Arc<PendingChain>> {
        BroadcastStream::new(self.pending.read().sender.subscribe())
    }
}
