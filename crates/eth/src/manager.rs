use std::{
    collections::{HashMap, HashSet},
    ops::RangeInclusive,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::{
    consensus::Transaction,
    eips::BlockNumHash,
    primitives::{Address, B256, Log, U256, aliases::I24},
    sol_types::{SolCall, SolEvent}
};
use angstrom_types::{
    block_sync::BlockSyncProducer,
    contract_bindings::{
        angstrom::Angstrom::{PoolKey, executeCall},
        angstrom_protocol_fee_config::AngstromProtocolFeeConfig::LpDonationSplitsSet,
        controller_v_1::ControllerV1::{NodeAdded, NodeRemoved, PoolConfigured, PoolRemoved}
    },
    contract_payloads::{
        angstrom::{AngPoolConfigEntry, AngstromBundle, AngstromPoolConfigStore},
        protocol_fees::{DonationSplitSnapshot, DonationSplits, PROTOCOL_FEE_CONFIG_SLOT}
    },
    primitive::PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK,
    traits::ChainExt
};
use futures::Future;
use futures_util::{FutureExt, StreamExt};
use itertools::Itertools;
use pade::PadeDecode;
use reth_provider::{
    CanonStateNotification, CanonStateNotifications, StateProvider, StateProviderFactory
};
use reth_tasks::TaskExecutor;
use telemetry_recorder::TelemetryMessage;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::{
    handle::{EthCommand, EthHandle},
    telemetry::EthUpdaterSnapshot
};

alloy::sol!(
    event Transfer(address indexed _from, address indexed _to, uint256 _value);
    event Approval(address indexed _owner, address indexed _spender, uint256 _value);
);

/// The inclusion half of the parent record: the orders that landed in the tip
/// and the tip's parent, the state the bundle actually executed on. Set beside
/// the construction parent the submitting node recorded under the same order
/// hashes, a difference is a bundle that ran on a state it was not built for.
/// Observability only — nothing here rejects, retries or holds anything.
fn record_included_bundle(chain: &impl ChainExt, order_hashes: Vec<B256>) {
    if order_hashes.is_empty() {
        return;
    }
    telemetry_recorder::telemetry_event!(TelemetryMessage::bundle_included(
        chain.tip_number(),
        chain.tip_hash(),
        chain.tip_parent_hash(),
        order_hashes
    ));
}

/// A pinned storage read, for checking the log-derived config against what
/// the chain actually holds.
pub trait ConfigStorage: Send + Sync + 'static {
    /// `slot` of `address` in the post-state of the block `block_hash` — never
    /// the current tip.
    fn storage_at(&self, block_hash: B256, address: Address, slot: U256) -> eyre::Result<U256>;
}

impl<P: StateProviderFactory + Send + Sync + 'static> ConfigStorage for P {
    fn storage_at(&self, block_hash: B256, address: Address, slot: U256) -> eyre::Result<U256> {
        Ok(self
            .state_by_block_hash(block_hash)?
            .storage(address, slot.into())?
            .unwrap_or_default())
    }
}

/// Listens for CanonStateNotifications and sends the appropriate updates to be
/// executed by the order pool
pub struct EthDataCleanser<Sync> {
    pub(crate) angstrom_address: Address,
    pub(crate) periphery_address: Address,
    pub(crate) protocol_fee_config_address: Address,
    /// our command receiver
    pub(crate) commander: ReceiverStream<EthCommand>,
    /// people listening to events
    pub(crate) event_listeners: Vec<UnboundedSender<EthEvent>>,
    /// for rebroadcasting
    pub(crate) cannon_sender: tokio::sync::broadcast::Sender<CanonStateNotification>,
    /// Notifications for Canonical Block updates
    pub(crate) canonical_updates: BroadcastStream<CanonStateNotification>,
    pub(crate) angstrom_tokens: HashMap<Address, usize>,
    /// handles syncing of blocks.
    block_sync: Sync,
    /// updated by periphery contract.
    pub(crate) pool_store: Arc<AngstromPoolConfigStore>,
    /// the set of currently active nodes.
    pub(crate) node_set: HashSet<Address>,
    /// seeded from a pinned read at the init block, then maintained from logs.
    pub(crate) protocol_fee_config: DonationSplitSnapshot,
    /// storage, pinned per block, that the log-derived pair is checked against
    /// before it is published.
    storage: Box<dyn ConfigStorage>
}

impl<Sync> EthDataCleanser<Sync>
where
    Sync: BlockSyncProducer
{
    pub fn spawn(
        angstrom_address: Address,
        periphery_address: Address,
        protocol_fee_config_address: Address,
        canonical_updates: CanonStateNotifications,
        executor: TaskExecutor,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>,
        angstrom_tokens: HashMap<Address, usize>,
        pool_store: Arc<AngstromPoolConfigStore>,
        protocol_fee_config: DonationSplitSnapshot,
        storage: impl ConfigStorage,
        sync: Sync,
        node_set: HashSet<Address>,
        event_listeners: Vec<UnboundedSender<EthEvent>>
    ) -> anyhow::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);
        let (cannon_tx, _) = tokio::sync::broadcast::channel(1000);

        let mut this = Self {
            angstrom_address,
            periphery_address,
            protocol_fee_config_address,
            canonical_updates: BroadcastStream::new(canonical_updates),
            commander: stream,
            angstrom_tokens,
            cannon_sender: cannon_tx,
            block_sync: sync,
            pool_store,
            node_set,
            event_listeners,
            protocol_fee_config,
            storage: Box::new(storage)
        };
        // ensure we broadcast node set. will allow for proper connections
        // on the network side
        for n in &this.node_set {
            this.event_listeners
                .retain(|e| e.send(EthEvent::AddedNode(*n)).is_ok());
        }

        executor.spawn_critical_task("eth handle", this.boxed());

        let handle = EthHandle::new(tx);

        Ok(handle)
    }

    fn subscribe_cannon_notifications(
        &self
    ) -> tokio::sync::broadcast::Receiver<CanonStateNotification> {
        self.cannon_sender.subscribe()
    }

    fn send_events(&mut self, event: EthEvent) {
        self.event_listeners
            .retain(|e| e.send(event.clone()).is_ok());
    }

    fn on_command(&mut self, command: EthCommand) {
        match command {
            EthCommand::SubscribeEthNetworkEvents(tx) => self.event_listeners.push(tx),
            EthCommand::SubscribeCannon(tx) => {
                let _ = tx.send(self.subscribe_cannon_notifications());
            }
        }
    }

    fn on_canon_update(&mut self, canonical_updates: CanonStateNotification) -> eyre::Result<()> {
        tracing::info!(?canonical_updates, "got new block update!!!!!");

        match canonical_updates.clone() {
            CanonStateNotification::Reorg { old, new } => self.handle_reorg(old, new)?,
            CanonStateNotification::Commit { new } => self.handle_commit(new)?
        }

        // Emitted after the handlers, so every field describes the state this
        // notification produced rather than the state it replaced. That is what makes
        // `protocol_fee_config` the pair in force at this tip, and what lets an
        // operator derive change history by diffing consecutive snapshots.
        telemetry_recorder::telemetry_event!(EthUpdaterSnapshot::from((
            &*self,
            canonical_updates.clone()
        )));

        let _ = self.cannon_sender.send(canonical_updates);
        Ok(())
    }

    fn handle_reorg(
        &mut self,
        old: Arc<impl ChainExt>,
        new: Arc<impl ChainExt>
    ) -> eyre::Result<()> {
        // Removing a setter carries no replacement event, so the value it overwrote
        // has to be reinstated from the removed logs before the new chain's own logs
        // land on top of it.
        let reverted_splits = self.reverted_protocol_fee_config(&old);
        self.apply_periphery_logs(&new, reverted_splits)?;

        // notify producer of reorg if one happened. NOTE: reth also calls this
        // on reverts
        let tip = new.tip_number();
        let reorg = old.reorged_range(&new).unwrap_or(tip..=tip);
        self.block_sync.reorg(reorg.clone());

        let mut eoas = self.get_eoa(old.clone());
        eoas.extend(self.get_eoa(new.clone()));

        // get all reorged orders
        let old_filled: HashSet<_> = self.fetch_filled_order(&old).collect();
        // In bundle order, as the commit path records them, so the key reads the
        // same on both paths.
        let landed = self.fetch_filled_order(&new).collect::<Vec<_>>();
        record_included_bundle(&new, landed.clone());
        let new_filled: HashSet<_> = landed.into_iter().collect();

        let difference: Vec<_> = old_filled.difference(&new_filled).copied().collect();
        let reorged_orders =
            EthEvent::ReorgedOrders(difference, reorg, BlockNumHash::new(tip, new.tip_hash()));

        self.send_events(reorged_orders);
        Ok(())
    }

    fn handle_commit(&mut self, new: Arc<impl ChainExt>) -> eyre::Result<()> {
        // handle this first so the newest state is the first available
        self.apply_periphery_logs(&new, None)?;

        let tip = new.tip_number();
        tracing::info!(?self.block_sync);
        self.block_sync.new_block(tip);

        let filled_orders = self.fetch_filled_order(&new).collect::<Vec<_>>();
        tracing::info!(?filled_orders, "filled orders found");
        record_included_bundle(&new, filled_orders.clone());

        let eoas = self.get_eoa(new.clone());

        let transitions = EthEvent::NewBlockTransitions {
            block_number: new.tip_number(),
            filled_orders,
            address_changeset: eoas
        };

        self.send_events(EthEvent::NewBlock(BlockNumHash::new(tip, new.tip_hash())));
        self.send_events(transitions);
        Ok(())
    }

    /// Applies the periphery and protocol-fee-config logs a notification
    /// carries, updating the internal state + sending out info.
    ///
    /// Walks every block, oldest first, so a change in a non-tip block is not
    /// missed. Later blocks win.
    ///
    /// `reverted_splits` is the pre-image of a config change a reorg removed
    /// (see [`Self::reverted_protocol_fee_config`]). It lands first, so a
    /// `chain` carrying its own setter still ends on the replacement.
    ///
    /// Errors if the pair the logs arrive at is not what storage holds at the
    /// tip, in which case nothing is published and nothing is overwritten.
    fn apply_periphery_logs(
        &mut self,
        chain: &impl ChainExt,
        reverted_splits: Option<DonationSplits>
    ) -> eyre::Result<()> {
        let periphery_address = self.periphery_address;
        let protocol_fee_config_address = self.protocol_fee_config_address;
        let mut splits = reverted_splits;

        for log in logs_in_block_order(chain) {
            if log.address == protocol_fee_config_address
                && let Ok(splits_set) = LpDonationSplitsSet::decode_log(log)
            {
                tracing::info!(?splits_set, "protocol fee config updated");
                splits = Some(splits_set.data.into());
                continue;
            }

            if log.address != periphery_address {
                continue;
            }

            // Every arm below is a no-op for a log that was already applied, so a
            // re-delivered block cannot double-count or re-announce anything.
            if let Ok(remove_node) = NodeRemoved::decode_log(log) {
                tracing::info!(?remove_node.node, "node removed from set");
                if self.node_set.remove(&remove_node.node) {
                    self.send_events(EthEvent::RemovedNode(remove_node.node));
                }
                continue;
            }
            if let Ok(added_node) = NodeAdded::decode_log(log) {
                tracing::info!(?added_node.node, "new node added to set");
                if self.node_set.insert(added_node.node) {
                    self.send_events(EthEvent::AddedNode(added_node.node));
                }
                continue;
            }
            if let Ok(removed_pool) = PoolRemoved::decode_log(log) {
                tracing::info!("new pool removed log");

                self.pool_store
                    .remove_pair(removed_pool.asset0, removed_pool.asset1);

                let count = |asset| self.angstrom_tokens.get(asset).copied().unwrap_or_default();
                let t0 = count(&removed_pool.asset0);
                let t1 = count(&removed_pool.asset1);

                if t0 == 1 {
                    self.angstrom_tokens.remove_entry(&removed_pool.asset0);
                }

                if t1 == 1 {
                    self.angstrom_tokens.remove_entry(&removed_pool.asset1);
                }

                let pool_key = PoolKey {
                    currency0:   removed_pool.asset0,
                    currency1:   removed_pool.asset1,
                    fee:         removed_pool.feeInE6,
                    tickSpacing: removed_pool.tickSpacing,
                    hooks:       self.angstrom_address
                };
                self.send_events(EthEvent::RemovedPool { pool: pool_key });
                continue;
            }
            if let Ok(added_pool) = PoolConfigured::decode_log(log) {
                tracing::info!("new pool configured log");
                let asset0 = added_pool.asset0;
                let asset1 = added_pool.asset1;
                // The controller emits this for a reconfiguration too, which the
                // contract applies in place: a known pair keeps its store index and
                // its tokens are not counted again.
                let known = self.pool_store.get_entry(asset0, asset1);
                let entry = AngPoolConfigEntry {
                    pool_partial_key: AngstromPoolConfigStore::derive_store_key(asset0, asset1),
                    tick_spacing:     added_pool.tickSpacing,
                    fee_in_e6:        added_pool.bundleFee.to(),
                    store_index:      known.map_or(self.pool_store.length(), |k| k.store_index)
                };
                if known == Some(entry) {
                    continue;
                }

                let pool_key = PoolKey {
                    currency0:   asset0,
                    currency1:   asset1,
                    fee:         added_pool.bundleFee,
                    tickSpacing: I24::unchecked_from(added_pool.tickSpacing),
                    hooks:       self.angstrom_address
                };

                self.pool_store.new_pool(asset0, asset1, entry);
                if known.is_none() {
                    *self.angstrom_tokens.entry(asset0).or_default() += 1;
                    *self.angstrom_tokens.entry(asset1).or_default() += 1;
                }

                self.send_events(EthEvent::NewPool { pool: pool_key });
            }
        }

        // Whether or not a setter landed, the pair about to be in force is checked
        // against storage before anything is built on it.
        self.reconcile_with_storage(chain, splits.unwrap_or(self.protocol_fee_config.splits))?;

        // One publication per notification, stamped with the tip it is current as of.
        if let Some(splits) = splits {
            let snapshot = DonationSplitSnapshot {
                block_number: chain.tip_number(),
                block_hash: chain.tip_hash(),
                splits
            };
            self.protocol_fee_config = snapshot;
            self.send_events(EthEvent::ProtocolFeeConfigUpdated(snapshot));
        }
        Ok(())
    }

    /// Storage is the source of truth and the logs are only the mechanism: a
    /// pair the logs arrived at that slot 0 does not hold at the tip — a
    /// dropped notification, a receipt that did not resolve, a reorg whose
    /// `old` chain did not carry the setter — is an error, never a value to
    /// publish. Skipped at or before the deployed block, where the config is
    /// the baked-in const and storage is empty.
    fn reconcile_with_storage(
        &self,
        chain: &impl ChainExt,
        splits: DonationSplits
    ) -> eyre::Result<()> {
        let tip = BlockNumHash::new(chain.tip_number(), chain.tip_hash());
        let deployed_block = PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK
            .get()
            .copied()
            .unwrap_or_default();
        if tip.number <= deployed_block {
            return Ok(());
        }

        let word = self.storage.storage_at(
            tip.hash,
            self.protocol_fee_config_address,
            U256::from(PROTOCOL_FEE_CONFIG_SLOT)
        )?;
        let stored = DonationSplits::from_slot0(word)?;
        if stored != splits {
            eyre::bail!(
                "protocol fee config at block {} ({}) is {stored:?} in storage but {splits:?} \
                 from logs",
                tip.number,
                tip.hash
            );
        }
        Ok(())
    }

    fn fetch_filled_order<'a>(
        &'a self,
        chain: &'a impl ChainExt
    ) -> impl Iterator<Item = B256> + 'a {
        chain
            .successful_tip_transactions()
            .filter(|&tx| tx.to() == Some(self.angstrom_address))
            .cloned()
            .filter_map(|transaction| {
                let input: &[u8] = transaction.input();
                let call = executeCall::abi_decode(input).ok()?;

                let mut input = call.encoded.as_ref();
                AngstromBundle::pade_decode(&mut input, None).ok()
            })
            .flat_map(move |bundle| {
                tracing::info!("found angstrom bundle that landed on chain!");
                bundle
                    .get_order_hashes(chain.tip_number())
                    .collect::<Vec<_>>()
            })
    }

    /// fetches all eoa addresses touched
    fn get_eoa(&self, chain: Arc<impl ChainExt>) -> Vec<Address> {
        chain
            .receipts_by_block_hash(chain.tip_hash())
            .unwrap_or_default()
            .into_iter()
            .filter(|receipt| receipt.success)
            .flat_map(|receipt| &receipt.logs)
            .filter(|log| self.angstrom_tokens.contains_key(&log.address))
            .flat_map(|log| {
                Transfer::decode_log(log)
                    .map(|log| [log._from, log._to])
                    .or_else(|_| Approval::decode_log(log).map(|log| [log._owner, log._spender]))
            })
            .flatten()
            .chain({
                let tip_txs = chain.successful_tip_transactions().cloned();
                tip_txs
                    .filter(|tx| tx.to() == Some(self.angstrom_address))
                    .filter_map(|transaction| {
                        let input: &[u8] = transaction.input();
                        let call = executeCall::abi_decode(input).ok()?;

                        let mut input = call.encoded.as_ref();
                        AngstromBundle::pade_decode(&mut input, None).ok()
                    })
                    .flat_map(|bundle| {
                        tracing::info!("found angstrom bundle that landed on chain!");
                        bundle.get_accounts(chain.tip_number()).collect::<Vec<_>>()
                    })
            })
            .unique()
            .collect()
    }

    /// The config in force before a reorged-out range, recovered from the
    /// removed logs alone.
    ///
    /// The setter writes the full pair and the event records both the old and
    /// the new one, so the earliest removed event holds the state as it was
    /// before the whole range — no storage read needed.
    fn reverted_protocol_fee_config(&self, old: &impl ChainExt) -> Option<DonationSplits> {
        let protocol_fee_config_address = self.protocol_fee_config_address;

        logs_in_block_order(old)
            .filter(|log| log.address == protocol_fee_config_address)
            .find_map(|log| LpDonationSplitsSet::decode_log(log).ok())
            .map(|splits_set| {
                tracing::info!(?splits_set, "reverting reorged-out protocol fee config");
                DonationSplits::overwritten_by(&splits_set)
            })
    }
}

/// Every log a notification carries, oldest block first, from successful
/// receipts only.
///
/// A notification can span several blocks, so anything that must not miss a log
/// walks all of them rather than only the tip.
fn logs_in_block_order(chain: &impl ChainExt) -> impl Iterator<Item = &Log> + '_ {
    chain
        .block_hashes()
        .into_iter()
        .flat_map(|block_hash| chain.receipts_by_block_hash(block_hash).unwrap_or_default())
        .filter(|receipt| receipt.success)
        .flat_map(|receipt| &receipt.logs)
}

impl<Sync> Future for EthDataCleanser<Sync>
where
    Sync: BlockSyncProducer
{
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // poll all canonical updates
        while let Poll::Ready(next) = self.canonical_updates.poll_next_unpin(cx) {
            match next {
                // A critical task, so a panic is how the executor is told the node
                // cannot go on: nothing may build on a pair storage disagrees with.
                Some(Ok(update)) => self
                    .on_canon_update(update)
                    .unwrap_or_else(|err| panic!("canonical update not applied: {err:#}")),
                Some(Err(lagged)) => tracing::error!(
                    %lagged,
                    "canonical updates were dropped; the config is reconciled at the next head"
                ),
                None => return Poll::Ready(())
            }
        }

        while let Poll::Ready(Some(command)) = self.commander.poll_next_unpin(cx) {
            self.on_command(command)
        }

        Poll::Pending
    }
}

#[derive(Debug, Clone)]
pub enum EthEvent {
    //TODO: add shit here
    /// The new canonical tip, by number *and* hash. The hash is what lets a
    /// consumer name the exact parent it is building on; a number cannot,
    /// since same-height reorgs exist.
    NewBlock(BlockNumHash),
    NewBlockTransitions {
        block_number:      u64,
        filled_orders:     Vec<B256>,
        address_changeset: Vec<Address>
    },
    /// The orders the reorg dropped, the range it covers, and the tip the new
    /// chain now ends on — carried for the same reason as
    /// [`EthEvent::NewBlock`].
    ReorgedOrders(Vec<B256>, RangeInclusive<u64>, BlockNumHash),
    FinalizedBlock(u64),
    NewPool {
        pool: PoolKey
    },
    RemovedPool {
        pool: PoolKey
    },
    AddedNode(Address),
    RemovedNode(Address),
    ProtocolFeeConfigUpdated(DonationSplitSnapshot)
}

#[cfg(test)]
pub mod test {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering}
    };

    use alloy::{
        consensus::{Header, TxLegacy},
        hex,
        primitives::{BlockHash, BlockNumber, Log, TxKind, aliases::U24, b256},
        signers::{Signature, local::PrivateKeySigner},
        sol_types::SolEvent
    };
    use angstrom_types::{
        block_sync::*,
        contract_bindings::controller_v_1::ControllerV1::{
            NodeAdded, NodeRemoved, PoolConfigured, PoolRemoved
        },
        contract_payloads::{
            Asset, Pair,
            angstrom::{TopOfBlockOrder, UserOrder}
        },
        orders::OrderOutcome,
        primitive::{AngstromAddressConfig, AngstromSigner},
        sol_bindings::grouped_orders::OrderWithStorageData,
        traits::{ChainExt, UserOrderFromInternal}
    };
    use pade::PadeEncode;
    use reth_ethereum_primitives::{Block, Receipt, TransactionSigned};
    use reth_execution_types::{Chain, ExecutionOutcome};
    use reth_primitives_traits::{LogData, RecoveredBlock};
    use testing_tools::type_generator::orders::{ToBOrderBuilder, UserOrderBuilder};

    use super::*;

    /// Slot 0 as the contract packs it: user share in the low 32 bits, tob
    /// share in the next 32.
    fn slot0_word((user, tob): (u32, u32)) -> U256 {
        U256::from(user) | (U256::from(tob) << 32)
    }

    /// The storage the cleanser reconciles against: a word per block hash, and
    /// the deployed pair at any block a test did not set.
    #[derive(Clone, Default)]
    struct FakeStorage {
        words: Arc<Mutex<HashMap<B256, U256>>>,
        fails: Arc<AtomicBool>
    }

    impl FakeStorage {
        fn holds(&self, block_hash: B256, pair: (u32, u32)) {
            self.words
                .lock()
                .unwrap()
                .insert(block_hash, slot0_word(pair));
        }

        fn fails(&self) {
            self.fails.store(true, Ordering::SeqCst);
        }
    }

    impl ConfigStorage for FakeStorage {
        fn storage_at(&self, block_hash: B256, _: Address, _: U256) -> eyre::Result<U256> {
            if self.fails.load(Ordering::SeqCst) {
                eyre::bail!("storage unavailable");
            }
            Ok(self
                .words
                .lock()
                .unwrap()
                .get(&block_hash)
                .copied()
                .unwrap_or(slot0_word((750_000, 1_000_000))))
        }
    }

    #[derive(Default)]
    pub struct MockChain<'a> {
        pub hash:         BlockHash,
        pub parent_hash:  BlockHash,
        pub number:       BlockNumber,
        pub transactions: Vec<TransactionSigned>,
        /// The tip block's receipts.
        pub receipts:     Vec<&'a Receipt>,
        /// Blocks before the tip, oldest first, for multi-block notifications.
        pub ancestors:    Vec<(BlockHash, Vec<&'a Receipt>)>
    }

    impl ChainExt for MockChain<'_> {
        fn tip_number(&self) -> BlockNumber {
            self.number
        }

        fn successful_tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_ {
            self.tip_transactions()
        }

        fn tip_hash(&self) -> BlockHash {
            self.hash
        }

        fn tip_parent_hash(&self) -> BlockHash {
            self.parent_hash
        }

        fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&Receipt>> {
            self.ancestors
                .iter()
                .find(|(hash, _)| *hash == block_hash)
                .map(|(_, receipts)| receipts.clone())
                .or_else(|| (block_hash == self.hash).then(|| self.receipts.clone()))
        }

        fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_ {
            self.transactions.iter()
        }

        fn reorged_range(&self, _: impl ChainExt) -> Option<RangeInclusive<u64>> {
            None
        }

        fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<Block>> + '_ {
            vec![].into_iter()
        }

        fn block_hashes(&self) -> Vec<BlockHash> {
            self.ancestors
                .iter()
                .map(|(hash, _)| *hash)
                .chain(std::iter::once(self.hash))
                .collect()
        }
    }

    fn setup_non_subscription_eth_manager(
        angstrom_address: Option<Address>
    ) -> EthDataCleanser<GlobalBlockSync> {
        let (_command_tx, command_rx) = tokio::sync::mpsc::channel(3);
        let (_cannon_tx, cannon_rx) = tokio::sync::broadcast::channel(3);
        let (tx, _) = tokio::sync::broadcast::channel(3);
        EthDataCleanser {
            commander:                   ReceiverStream::new(command_rx),
            event_listeners:             vec![],
            angstrom_tokens:             HashMap::default(),
            node_set:                    HashSet::default(),
            angstrom_address:            angstrom_address.unwrap_or_default(),
            periphery_address:           Address::default(),
            protocol_fee_config_address: Address::default(),
            canonical_updates:           BroadcastStream::new(cannon_rx),
            block_sync:                  GlobalBlockSync::new(1),
            cannon_sender:               tx,
            pool_store:                  Default::default(),
            protocol_fee_config:         DonationSplitSnapshot {
                block_number: 0,
                block_hash:   BlockHash::ZERO,
                splits:       DonationSplits::new(750_000, 1_000_000).unwrap()
            },
            storage:                     Box::new(FakeStorage::default())
        }
    }

    fn setup_signing_info() -> AngstromSigner<PrivateKeySigner> {
        AngstromSigner::random()
    }

    /// A signed `execute` call to `angstrom_address` carrying one user order
    /// and one ToB order, with the hashes those orders have in `block`.
    fn bundle_transaction(angstrom_address: Address, block: u64) -> (TransactionSigned, Vec<B256>) {
        let signing_info = setup_signing_info();
        let top_of_block_order = ToBOrderBuilder::new()
            .signing_key(Some(signing_info.clone()))
            .build();
        let t = OrderWithStorageData { order: top_of_block_order, ..Default::default() };
        let user_order = UserOrderBuilder::new()
            .signing_key(Some(signing_info.clone()))
            .with_storage()
            .build();

        let outcome = OrderOutcome {
            id:      user_order.order_id,
            outcome: angstrom_types::orders::OrderFillState::CompleteFill
        };
        let pair = Pair {
            index0:       0,
            index1:       1,
            store_index:  0,
            price_1over0: U256::default()
        };

        let asset0 = Asset { addr: t.asset_out, ..Default::default() };
        let asset1 = Asset { addr: t.asset_in, ..Default::default() };

        let pair = vec![pair];
        let assets = vec![asset0, asset1];

        let finalized_user_order = UserOrder::from_internal_order_max_gas(&user_order, &outcome, 0);
        let finalized_tob = TopOfBlockOrder::of_max_gas(&t, 0);

        let order_hashes = vec![
            finalized_user_order.order_hash(&pair, &assets, block),
            finalized_tob.order_hash(&pair, &assets, block),
        ];

        let angstrom_bundle_with_orders = AngstromBundle::new(
            assets,
            pair,
            vec![],
            vec![finalized_tob],
            vec![finalized_user_order]
        );

        let leg = TxLegacy {
            to: TxKind::Call(angstrom_address),
            input: executeCall::new((angstrom_bundle_with_orders.pade_encode().into(),))
                .abi_encode()
                .into(),
            ..Default::default()
        };

        (TransactionSigned::new_unhashed(leg.into(), Signature::test_signature()), order_hashes)
    }

    #[test]
    fn test_fetch_filled_orders() {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let angstrom_address = Address::random();
        let eth = setup_non_subscription_eth_manager(Some(angstrom_address));

        let (mock_tx, order_hashes) = bundle_transaction(angstrom_address, 0);
        let mock_chain = MockChain { transactions: vec![mock_tx], ..Default::default() };
        let filled_set = eth.fetch_filled_order(&mock_chain).collect::<HashSet<_>>();

        for order_hash in order_hashes {
            assert!(filled_set.contains(&order_hash));
        }
    }

    /// A bundle that lands is recorded with the parent of the block it landed
    /// in, keyed by its order hashes, so it can be set against the parent the
    /// submitting node recorded building it for.
    #[test]
    fn a_landed_bundle_is_recorded_with_its_inclusion_parent() {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let (telemetry_tx, mut telemetry_rx) = tokio::sync::mpsc::unbounded_channel();
        telemetry_recorder::TELEMETRY_SENDER
            .set(telemetry_tx)
            .expect("only this test installs a telemetry sink");
        let angstrom_address = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(angstrom_address));

        let (mock_tx, order_hashes) = bundle_transaction(angstrom_address, 101);
        let (block_hash, parent_hash) = (BlockHash::random(), BlockHash::random());
        eth.handle_commit(Arc::new(MockChain {
            number: 101,
            hash: block_hash,
            parent_hash,
            transactions: vec![mock_tx.clone()],
            ..Default::default()
        }))
        .unwrap();

        let mut records = std::iter::from_fn(|| telemetry_rx.try_recv().ok()).filter_map(
            |message| match message {
                TelemetryMessage::BundleIncluded {
                    blocknum,
                    inclusion_block,
                    inclusion_parent,
                    order_hashes,
                    ..
                } => Some((blocknum, inclusion_block, inclusion_parent, order_hashes)),
                _ => None
            }
        );
        let committed = records.next().expect("the landed bundle was recorded");
        assert_eq!(committed.0, 101);
        assert_eq!(committed.1, block_hash, "recorded with the block it landed in");
        assert_eq!(committed.2, parent_hash, "and with the parent it executed on");
        assert_eq!(
            committed.3.iter().copied().collect::<HashSet<_>>(),
            order_hashes.into_iter().collect::<HashSet<_>>(),
            "keyed by the landed bundle's order hashes"
        );

        // The same bundle arriving on a replacement branch is recorded the same
        // way — including the order of the key — with that branch's parent.
        let (reorg_hash, reorg_parent) = (BlockHash::random(), BlockHash::random());
        eth.handle_reorg(
            Arc::new(MockChain { number: 101, hash: block_hash, ..Default::default() }),
            Arc::new(MockChain {
                number: 101,
                hash: reorg_hash,
                parent_hash: reorg_parent,
                transactions: vec![mock_tx],
                ..Default::default()
            })
        )
        .unwrap();
        let reorged = records.next().expect("the reorged bundle was recorded");
        assert_eq!((reorged.0, reorged.1, reorged.2), (101, reorg_hash, reorg_parent));
        assert_eq!(reorged.3, committed.3, "the key reads the same on both paths");
    }

    #[test]
    fn test_periphery_node_events() {
        let ang_addr = Address::random();
        let periphery_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.periphery_address = periphery_addr;

        // Test node added event
        let node_addr = Address::random();
        let node_added = NodeAdded { node: node_addr };
        let added_log = Log { address: periphery_addr, data: node_added.encode_log_data() };

        // Test node removed event
        let node_removed = NodeRemoved { node: node_addr };
        let removed_log = Log { address: periphery_addr, data: node_removed.encode_log_data() };

        let mock_recip =
            Receipt { logs: vec![added_log.clone(), removed_log.clone()], ..Default::default() };

        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        // Verify initial state
        assert!(!eth.node_set.contains(&node_addr));

        // Process the logs
        eth.apply_periphery_logs(&*mock_chain, None).unwrap();

        // Verify node was added then removed
        assert!(!eth.node_set.contains(&node_addr));
    }

    #[test]
    fn test_periphery_pool_events() {
        let ang_addr = Address::random();
        let periphery_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.periphery_address = periphery_addr;
        eth.angstrom_address = ang_addr;

        // Test pool configured event
        let asset0 = Address::random();
        let asset1 = Address::random();
        let fee = U24::try_from(3000).unwrap();
        let tick_spacing = 60u16;

        let pool_configured = PoolConfigured {
            asset0,
            asset1,
            bundleFee: fee,
            unlockedFee: fee,
            tickSpacing: tick_spacing,
            protocolUnlockedFee: fee
        };
        let configured_log =
            Log { address: periphery_addr, data: pool_configured.encode_log_data() };

        // Test pool removed event
        let pool_removed = PoolRemoved {
            asset0,
            asset1,
            feeInE6: fee,
            tickSpacing: I24::try_from(tick_spacing).unwrap()
        };
        let removed_log = Log { address: periphery_addr, data: pool_removed.encode_log_data() };

        let mock_recip = Receipt {
            logs: vec![configured_log.clone(), removed_log.clone()],
            ..Default::default()
        };

        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        // Verify initial state
        assert!(!eth.angstrom_tokens.contains_key(&asset0));
        assert!(!eth.angstrom_tokens.contains_key(&asset1));
        assert_eq!(eth.pool_store.length(), 0);

        // Process the logs
        eth.apply_periphery_logs(&*mock_chain, None).unwrap();

        // Verify final state after add and remove
        assert!(!eth.angstrom_tokens.contains_key(&asset0));
        assert!(!eth.angstrom_tokens.contains_key(&asset1));
        assert_eq!(eth.pool_store.length(), 0); // Should be 0 after removal
    }

    #[test]
    fn test_handle_reorg() {
        let ang_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));

        // Create mock chains for old and new state
        let old_chain =
            Arc::new(MockChain { number: 100, hash: BlockHash::random(), ..Default::default() });

        let new_chain =
            Arc::new(MockChain { number: 95, hash: BlockHash::random(), ..Default::default() });

        // Add a test event listener
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        eth.event_listeners.push(tx);

        // Trigger reorg
        eth.handle_reorg(old_chain, new_chain).unwrap();

        // Should receive both NewBlockTransitions and ReorgedOrders events
        let mut received_reorg = false;

        for _ in 0..1 {
            match rx.try_recv().expect("Should receive 1 event") {
                EthEvent::ReorgedOrders(_, range, _) => {
                    assert_eq!(*range.start(), 95);
                    assert_eq!(*range.end(), 95);
                    received_reorg = true;
                }
                _ => panic!("Unexpected event type")
            }
        }

        assert!(received_reorg, "Should have received ReorgedOrders event");
    }

    #[test]
    fn test_handle_commit() {
        let ang_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));

        let new_chain =
            Arc::new(MockChain { number: 100, hash: BlockHash::random(), ..Default::default() });

        // Add a test event listener
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        eth.event_listeners.push(tx);

        // Handle commit
        eth.handle_commit(new_chain).unwrap();

        // Verify new block transitions event was sent
        // `handle_commit` sends `NewBlock` ahead of the transitions.
        match published_transitions(&mut rx).expect("Should receive an event") {
            EthEvent::NewBlockTransitions { block_number, filled_orders, address_changeset } => {
                assert_eq!(block_number, 100);
                assert!(filled_orders.is_empty());
                assert!(address_changeset.is_empty());
            }
            _ => unreachable!()
        }
    }

    #[test]
    fn test_fetch_eoa_balance_approval_changes() {
        let ang_addr = Address::random();
        let transfer_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.angstrom_tokens = HashMap::from_iter(vec![(transfer_addr, 1)]);

        let changeset =
            vec![alloy::primitives::address!("ecc5a3c54f85ab375de921a40247d726bc8ed376")];

        let transfer_log = alloy::primitives::Log::new(
            transfer_addr,
            vec![
                b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"),
                b256!("000000000000000000000000ecc5a3c54f85ab375de921a40247d726bc8ed376"),
                b256!("00000000000000000000000094293bf0193f9acf3762b7440126f379eb70cbfd"),
            ],
            hex!("00000000000000000000000000000000000000000000000001166b47e1c20000").into()
        )
        .unwrap();

        let mock_recip = Receipt { logs: vec![transfer_log], success: true, ..Default::default() };

        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });
        let filled_set = eth.get_eoa(mock_chain);

        for change in changeset {
            assert!(filled_set.contains(&change));
        }
    }

    #[test]
    fn test_multiple_transfers_same_block() {
        let ang_addr = Address::random();
        let token_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.angstrom_tokens = HashMap::from_iter(vec![(token_addr, 1)]);

        let addr1 = Address::random();
        let addr2 = Address::random();
        let addr3 = Address::random();

        // Create multiple transfer logs
        let transfer1 = Transfer { _from: addr1, _to: addr2, _value: U256::from(100) };
        let transfer2 = Transfer { _from: addr2, _to: addr3, _value: U256::from(50) };
        let approval = Approval { _owner: addr1, _spender: addr3, _value: U256::from(200) };

        let logs = vec![
            Log { address: token_addr, data: transfer1.encode_log_data() },
            Log { address: token_addr, data: transfer2.encode_log_data() },
            Log { address: token_addr, data: approval.encode_log_data() },
        ];

        let mock_recip = Receipt { logs, success: true, ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        let eoas = eth.get_eoa(mock_chain);

        assert!(eoas.contains(&addr1));
        assert!(eoas.contains(&addr2));
        assert_eq!(eoas.len(), 3);
    }

    #[test]
    fn test_invalid_log_handling() {
        let ang_addr = Address::random();
        let token_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.angstrom_tokens = HashMap::from_iter(vec![(token_addr, 1)]);

        // Create an invalid log
        let invalid_log = Log {
            address: token_addr,
            data:    LogData::new_unchecked(vec![B256::random()], vec![1, 2, 3].into())
        };

        let valid_transfer = Transfer {
            _from:  Address::random(),
            _to:    Address::random(),
            _value: U256::from(100)
        };
        let valid_log = Log { address: token_addr, data: valid_transfer.encode_log_data() };

        let mock_recip =
            Receipt { logs: vec![invalid_log, valid_log], success: true, ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        let eoas = eth.get_eoa(mock_chain);
        // fix this
        assert_eq!(eoas.len(), 2);
    }

    #[test]
    fn test_empty_block_handling() {
        let ang_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));

        // Test with empty receipts
        let mock_chain =
            Arc::new(MockChain { receipts: vec![], number: 100, ..Default::default() });

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        eth.event_listeners.push(tx);

        eth.handle_commit(mock_chain).unwrap();

        // `handle_commit` sends `NewBlock` ahead of the transitions.
        match published_transitions(&mut rx).expect("Should receive an event") {
            EthEvent::NewBlockTransitions { block_number, filled_orders, address_changeset } => {
                assert_eq!(block_number, 100);
                assert!(filled_orders.is_empty());
                assert!(address_changeset.is_empty());
            }
            _ => unreachable!()
        }
    }

    #[test]
    fn test_multiple_node_changes() {
        let ang_addr = Address::random();
        let periphery_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.periphery_address = periphery_addr;

        let node1 = Address::random();
        let node2 = Address::random();
        let node3 = Address::random();

        // Create multiple node events
        enum NodeEvent {
            Added(NodeAdded),
            Removed(NodeRemoved)
        }

        let events = vec![
            NodeEvent::Added(NodeAdded { node: node1 }),
            NodeEvent::Added(NodeAdded { node: node2 }),
            NodeEvent::Removed(NodeRemoved { node: node1 }),
            NodeEvent::Removed(NodeRemoved { node: node2 }),
            NodeEvent::Added(NodeAdded { node: node3 }),
        ];

        let logs: Vec<Log> = events
            .into_iter()
            .map(|event| match event {
                NodeEvent::Added(added) => {
                    Log { address: periphery_addr, data: added.encode_log_data() }
                }
                NodeEvent::Removed(removed) => {
                    Log { address: periphery_addr, data: removed.encode_log_data() }
                }
            })
            .collect();

        let mock_recip = Receipt { logs, success: true, ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        eth.apply_periphery_logs(&*mock_chain, None).unwrap();

        assert!(!eth.node_set.contains(&node1));
        assert!(!eth.node_set.contains(&node2));
        assert!(eth.node_set.contains(&node3));
    }

    #[test]
    fn test_pool_config_edge_cases() {
        let ang_addr = Address::random();
        let periphery_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.periphery_address = periphery_addr;
        eth.angstrom_address = ang_addr;

        let asset0 = Address::random();
        let asset1 = Address::random();
        let fee = U24::try_from(3000).unwrap();
        let tick_spacing = 60u16;

        // Test reconfiguring same pool
        let configure1 = PoolConfigured {
            asset0,
            asset1,
            bundleFee: fee,
            unlockedFee: fee,
            tickSpacing: tick_spacing,
            protocolUnlockedFee: U24::ZERO
        };
        let configure2 = PoolConfigured {
            asset0,
            asset1,
            bundleFee: fee,
            unlockedFee: fee,
            tickSpacing: tick_spacing * 2,
            protocolUnlockedFee: U24::ZERO
        };
        let remove = PoolRemoved {
            asset0,
            asset1,
            feeInE6: fee,
            tickSpacing: I24::try_from(tick_spacing).unwrap()
        };

        let configured = Receipt {
            logs: vec![
                Log { address: periphery_addr, data: configure1.encode_log_data() },
                Log { address: periphery_addr, data: configure2.encode_log_data() },
            ],
            success: true,
            ..Default::default()
        };
        eth.apply_periphery_logs(
            &MockChain { receipts: vec![&configured], ..Default::default() },
            None
        )
        .unwrap();

        // Reconfigured in place, as the contract does it: one entry, at the index it
        // was given first, carrying the new tick spacing; each token counted once.
        let entry = eth.pool_store.get_entry(asset0, asset1).unwrap();
        assert_eq!(eth.pool_store.length(), 1);
        assert_eq!(entry.store_index, 0);
        assert_eq!(entry.tick_spacing, tick_spacing * 2);
        assert_eq!(eth.angstrom_tokens[&asset0], 1);
        assert_eq!(eth.angstrom_tokens[&asset1], 1);

        let removed = Receipt {
            logs: vec![Log { address: periphery_addr, data: remove.encode_log_data() }],
            success: true,
            ..Default::default()
        };
        eth.apply_periphery_logs(
            &MockChain { receipts: vec![&removed], ..Default::default() },
            None
        )
        .unwrap();

        // ...so removing it lets both tokens go.
        assert!(!eth.angstrom_tokens.contains_key(&asset0));
        assert!(!eth.angstrom_tokens.contains_key(&asset1));
        assert_eq!(eth.pool_store.length(), 0);
    }

    #[test]
    fn re_applying_a_notification_leaves_pool_and_node_state_unchanged() {
        let periphery_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(Address::random()));
        eth.periphery_address = periphery_addr;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        eth.event_listeners.push(tx);

        let (asset0, asset1, node) = (Address::random(), Address::random(), Address::random());
        let configure = PoolConfigured {
            asset0,
            asset1,
            bundleFee: U24::try_from(3000).unwrap(),
            unlockedFee: U24::ZERO,
            tickSpacing: 60,
            protocolUnlockedFee: U24::ZERO
        };
        let mock_recip = receipt(vec![
            Log { address: periphery_addr, data: configure.encode_log_data() },
            Log { address: periphery_addr, data: NodeAdded { node }.encode_log_data() },
        ]);
        let mock_chain = MockChain { receipts: vec![&mock_recip], ..Default::default() };

        eth.apply_periphery_logs(&mock_chain, None).unwrap();
        let entry = eth.pool_store.get_entry(asset0, asset1);
        let tokens = eth.angstrom_tokens.clone();
        let nodes = eth.node_set.clone();
        assert_eq!(std::iter::from_fn(|| rx.try_recv().ok()).count(), 2);

        // The same blocks delivered again change nothing and announce nothing.
        eth.apply_periphery_logs(&mock_chain, None).unwrap();
        assert_eq!(eth.pool_store.get_entry(asset0, asset1), entry);
        assert_eq!(eth.pool_store.length(), 1);
        assert_eq!(eth.angstrom_tokens, tokens);
        assert_eq!(eth.node_set, nodes);
        assert!(rx.try_recv().is_err(), "a re-applied log was re-announced");
    }

    #[test]
    fn test_non_angstrom_token_transfers() {
        let ang_addr = Address::random();
        let token_addr = Address::random();
        let non_tracked_token = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.angstrom_tokens = HashMap::from_iter(vec![(token_addr, 1)]);

        // Create transfer for non-tracked token
        let transfer = Transfer {
            _from:  Address::random(),
            _to:    Address::random(),
            _value: U256::from(100)
        };

        let logs = vec![Log {
            address: non_tracked_token, // Using non-tracked token address
            data:    transfer.encode_log_data()
        }];

        let mock_recip = Receipt { logs, ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        let eoas = eth.get_eoa(mock_chain);
        assert!(eoas.is_empty()); // Should ignore non-tracked token transfers
    }

    #[test]
    fn test_duplicate_pool_removal() {
        let ang_addr = Address::random();
        let periphery_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.periphery_address = periphery_addr;
        eth.angstrom_address = ang_addr;

        let asset0 = Address::random();
        let asset1 = Address::random();
        let fee = U24::try_from(3000).unwrap();
        let tick_spacing = 60u16;

        // Create pool and remove it twice
        let configure = PoolConfigured {
            asset0,
            asset1,
            bundleFee: fee,
            unlockedFee: fee,
            tickSpacing: tick_spacing,
            protocolUnlockedFee: U24::ZERO
        };
        let remove = PoolRemoved {
            asset0,
            asset1,
            feeInE6: fee,
            tickSpacing: I24::try_from(tick_spacing).unwrap()
        };

        let logs = vec![
            Log { address: periphery_addr, data: configure.encode_log_data() },
            Log { address: periphery_addr, data: remove.encode_log_data() },
            Log { address: periphery_addr, data: remove.encode_log_data() }, // Duplicate removal
        ];

        let mock_recip = Receipt { logs, ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        // Should handle duplicate removal gracefully
        eth.apply_periphery_logs(&*mock_chain, None).unwrap();
        assert_eq!(eth.pool_store.length(), 0);
        // ...without the second removal leaving a zero-count token behind.
        assert!(eth.angstrom_tokens.is_empty(), "{:?}", eth.angstrom_tokens);
    }

    #[test]
    fn test_remove_non_existent_node() {
        let ang_addr = Address::random();
        let periphery_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.periphery_address = periphery_addr;

        let non_existent_node = Address::random();
        let node_removed = NodeRemoved { node: non_existent_node };
        let removed_log = Log { address: periphery_addr, data: node_removed.encode_log_data() };

        let mock_recip = Receipt { logs: vec![removed_log], ..Default::default() };

        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        // Should handle removal of non-existent node gracefully
        eth.apply_periphery_logs(&*mock_chain, None).unwrap();
        assert!(!eth.node_set.contains(&non_existent_node));
    }

    #[test]
    fn test_malformed_transaction_input() {
        let angstrom_address = Address::random();
        let eth = setup_non_subscription_eth_manager(Some(angstrom_address));

        let leg = TxLegacy {
            to: TxKind::Call(angstrom_address),
            // Invalid input data
            input: vec![0, 1, 2, 3].into(),
            ..Default::default()
        };

        let mock_tx = TransactionSigned::new_unhashed(leg.into(), Signature::test_signature());
        let mock_chain = MockChain { transactions: vec![mock_tx], ..Default::default() };

        // Should handle malformed input gracefully
        let filled_set = eth.fetch_filled_order(&mock_chain).collect::<HashSet<_>>();
        assert!(filled_set.is_empty());
    }

    /// Builds an `LpDonationSplitsSet` log: the full pair before and after.
    fn splits_log(address: Address, old: (u32, u32), new: (u32, u32)) -> Log {
        Log {
            address,
            data: LpDonationSplitsSet {
                oldUserLpShareE6: old.0,
                oldTobLpShareE6:  old.1,
                newUserLpShareE6: new.0,
                newTobLpShareE6:  new.1
            }
            .encode_log_data()
        }
    }

    fn receipt(logs: Vec<Log>) -> Receipt {
        Receipt { logs, success: true, ..Default::default() }
    }

    /// The `NewBlockTransitions` published to listeners, if any.
    fn published_transitions(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<EthEvent>
    ) -> Option<EthEvent> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .find(|event| matches!(event, EthEvent::NewBlockTransitions { .. }))
    }

    /// The config snapshot published to listeners, if any.
    fn published_config(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<EthEvent>
    ) -> Option<DonationSplitSnapshot> {
        std::iter::from_fn(|| rx.try_recv().ok()).find_map(|event| match event {
            EthEvent::ProtocolFeeConfigUpdated(snapshot) => Some(snapshot),
            _ => None
        })
    }

    /// A cleanser with a config address, a listener, and storage the tests
    /// can put words into. Storage holds the seeded pair anywhere a test does
    /// not say otherwise, so a test that changes the config also says what
    /// storage holds at the tip.
    fn setup_config_eth_manager() -> (
        EthDataCleanser<GlobalBlockSync>,
        Address,
        tokio::sync::mpsc::UnboundedReceiver<EthEvent>,
        FakeStorage
    ) {
        let config_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(Address::random()));
        eth.protocol_fee_config_address = config_addr;
        let storage = FakeStorage::default();
        eth.storage = Box::new(storage.clone());

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        eth.event_listeners.push(tx);

        (eth, config_addr, rx, storage)
    }

    #[test]
    fn config_change_in_a_non_tip_block_is_applied() {
        let (mut eth, config_addr, mut rx, storage) = setup_config_eth_manager();

        // The change lands two blocks back; the tip itself carries nothing.
        let ancestor =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let tip = receipt(vec![]);
        let tip_hash = BlockHash::random();
        storage.holds(tip_hash, (800_000, 900_000));

        eth.handle_commit(Arc::new(MockChain {
            hash: tip_hash,
            number: 100,
            receipts: vec![&tip],
            ancestors: vec![(BlockHash::random(), vec![&ancestor])],
            ..Default::default()
        }))
        .unwrap();

        let expected = DonationSplits::new(800_000, 900_000).unwrap();
        assert_eq!(eth.protocol_fee_config.splits, expected);

        // Stamped with the notification tip, not the block the log was in.
        let published = published_config(&mut rx).expect("a config update is published");
        assert_eq!(published.splits, expected);
        assert_eq!(published.block_number, 100);
        assert_eq!(published.block_hash, tip_hash);
    }

    #[test]
    fn the_last_config_change_in_a_notification_wins() {
        let (mut eth, config_addr, mut rx, storage) = setup_config_eth_manager();

        let first =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let second = receipt(vec![splits_log(config_addr, (800_000, 900_000), (600_000, 500_000))]);
        let tip_hash = BlockHash::random();
        storage.holds(tip_hash, (600_000, 500_000));

        eth.handle_commit(Arc::new(MockChain {
            hash: tip_hash,
            number: 100,
            receipts: vec![&second],
            ancestors: vec![(BlockHash::random(), vec![&first])],
            ..Default::default()
        }))
        .unwrap();

        let expected = DonationSplits::new(600_000, 500_000).unwrap();
        assert_eq!(eth.protocol_fee_config.splits, expected);
        // One publication for the whole notification, carrying the final value.
        assert_eq!(published_config(&mut rx).unwrap().splits, expected);
        assert!(published_config(&mut rx).is_none(), "only one update per notification");
    }

    #[test]
    fn a_reorg_that_removes_a_setter_restores_the_previous_rates() {
        let (mut eth, config_addr, mut rx, _storage) = setup_config_eth_manager();

        // The setter that is about to be reorged out moved 75/100 -> 80/90.
        let removed =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let old = Arc::new(MockChain {
            hash: BlockHash::random(),
            number: 100,
            receipts: vec![&removed],
            ..Default::default()
        });
        let new_tip_hash = BlockHash::random();
        let new = Arc::new(MockChain { hash: new_tip_hash, number: 95, ..Default::default() });

        eth.handle_reorg(old, new).unwrap();

        // Back to the pre-image the removed event recorded.
        let expected = DonationSplits::new(750_000, 1_000_000).unwrap();
        assert_eq!(eth.protocol_fee_config.splits, expected);

        let published = published_config(&mut rx).expect("the revert is published");
        assert_eq!(published.splits, expected);
        assert_eq!(published.block_number, 95);
        assert_eq!(published.block_hash, new_tip_hash);
    }

    #[test]
    fn a_reorg_that_replaces_a_setter_ends_on_the_replacement() {
        let (mut eth, config_addr, mut rx, storage) = setup_config_eth_manager();

        let removed =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let replacement =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (250_000, 100_000))]);
        let new_tip_hash = BlockHash::random();
        storage.holds(new_tip_hash, (250_000, 100_000));

        eth.handle_reorg(
            Arc::new(MockChain {
                hash: BlockHash::random(),
                number: 100,
                receipts: vec![&removed],
                ..Default::default()
            }),
            Arc::new(MockChain {
                hash: new_tip_hash,
                number: 100,
                receipts: vec![&replacement],
                ..Default::default()
            })
        )
        .unwrap();

        let expected = DonationSplits::new(250_000, 100_000).unwrap();
        assert_eq!(eth.protocol_fee_config.splits, expected);
        assert_eq!(published_config(&mut rx).unwrap().splits, expected);
    }

    #[test]
    fn a_reorg_reverts_to_the_state_before_the_whole_removed_range() {
        let (mut eth, config_addr, _rx, _storage) = setup_config_eth_manager();

        // Two setters removed together: only the earliest one's pre-image is the
        // state as it was before the reorged-out range.
        let earliest =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let latest = receipt(vec![splits_log(config_addr, (800_000, 900_000), (10_000, 20_000))]);

        eth.handle_reorg(
            Arc::new(MockChain {
                hash: BlockHash::random(),
                number: 100,
                receipts: vec![&latest],
                ancestors: vec![(BlockHash::random(), vec![&earliest])],
                ..Default::default()
            }),
            Arc::new(MockChain { hash: BlockHash::random(), number: 98, ..Default::default() })
        )
        .unwrap();

        assert_eq!(
            eth.protocol_fee_config.splits,
            DonationSplits::new(750_000, 1_000_000).unwrap()
        );
    }

    /// The snapshot the telemetry stream carries for `eth` as it stands.
    ///
    /// `on_canon_update` emits this *after* the handlers, so building it from
    /// the cleanser's current state is exactly what that call produces. The
    /// notification is only stored on the snapshot as `chain_update` and is
    /// never read by the `From` impl, so an empty chain is enough.
    fn snapshot_of(eth: &EthDataCleanser<GlobalBlockSync>) -> EthUpdaterSnapshot {
        EthUpdaterSnapshot::from((
            eth,
            CanonStateNotification::Commit {
                new: Arc::new(reth_execution_types::Chain::default())
            }
        ))
    }

    #[test]
    fn the_snapshot_carries_the_splits_in_force_at_the_tip() {
        let (mut eth, config_addr, _rx, storage) = setup_config_eth_manager();
        let before = snapshot_of(&eth);

        let setter =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let (tip, next_tip) = (BlockHash::random(), BlockHash::random());
        storage.holds(tip, (800_000, 900_000));
        storage.holds(next_tip, (800_000, 900_000));
        eth.handle_commit(Arc::new(MockChain {
            hash: tip,
            number: 100,
            receipts: vec![&setter],
            ..Default::default()
        }))
        .unwrap();
        let after = snapshot_of(&eth);

        // The notification's own setter is included rather than lagging by one,
        // which is what emitting after the handlers buys.
        assert_eq!(
            after.protocol_fee_config.splits,
            DonationSplits::new(800_000, 900_000).unwrap()
        );
        // A notification that changed the config shows up as a diff between
        // consecutive snapshots — this is the change history, derived.
        assert_ne!(before.protocol_fee_config, after.protocol_fee_config);

        // ...and one that did not change it leaves them equal, so a diff is not
        // reported where no governance action happened.
        eth.handle_commit(Arc::new(MockChain {
            hash: next_tip,
            number: 101,
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(snapshot_of(&eth).protocol_fee_config, after.protocol_fee_config);
    }

    #[test]
    fn the_snapshot_round_trips_with_the_config_on_it() {
        // The consumer decodes this from json, so the added field has to survive
        // the trip the same way the rest of the snapshot does.
        let (eth, _config_addr, _rx, _storage) = setup_config_eth_manager();
        let snapshot = snapshot_of(&eth);

        let json = serde_json::to_value(&snapshot).unwrap();
        let decoded: EthUpdaterSnapshot = serde_json::from_value(json).unwrap();

        assert_eq!(decoded.protocol_fee_config, snapshot.protocol_fee_config);
    }

    #[test]
    fn a_notification_without_a_setter_leaves_the_seeded_config_alone() {
        let (mut eth, _config_addr, mut rx, _storage) = setup_config_eth_manager();
        let seeded = eth.protocol_fee_config;

        eth.handle_commit(Arc::new(MockChain {
            hash: BlockHash::random(),
            number: 100,
            ..Default::default()
        }))
        .unwrap();

        // The node starts with rates and keeps them until a log says otherwise.
        assert_eq!(eth.protocol_fee_config, seeded);
        assert!(published_config(&mut rx).is_none(), "nothing changed, nothing published");
    }

    #[test]
    fn a_pair_storage_does_not_hold_is_an_error_and_nothing_is_published() {
        let (mut eth, _config_addr, mut rx, storage) = setup_config_eth_manager();
        let seeded = eth.protocol_fee_config;
        let tip_hash = BlockHash::random();
        // Storage moved without a log this node saw: a dropped notification, a
        // receipt that did not resolve, a reorg whose `old` chain lacked the setter.
        storage.holds(tip_hash, (800_000, 900_000));

        // A notification without a setter still reconciles, so the drift is caught
        // at this head rather than never.
        let err = eth
            .handle_commit(Arc::new(MockChain {
                hash: tip_hash,
                number: 100,
                ..Default::default()
            }))
            .unwrap_err()
            .to_string();

        // Named: both pairs and the tip.
        assert!(err.contains(&format!("block 100 ({tip_hash})")), "{err}");
        assert!(err.contains("800000") && err.contains("750000"), "{err}");
        // Neither pair is taken, and nothing downstream is released on it.
        assert_eq!(eth.protocol_fee_config, seeded);
        assert!(rx.try_recv().is_err(), "an unreconciled notification published events");
    }

    #[test]
    fn a_setter_storage_does_not_confirm_is_not_applied() {
        let (mut eth, config_addr, mut rx, _storage) = setup_config_eth_manager();
        let seeded = eth.protocol_fee_config;
        let setter =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);

        // Storage still holds the seeded pair at this tip.
        let err = eth
            .handle_commit(Arc::new(MockChain {
                hash: BlockHash::random(),
                number: 100,
                receipts: vec![&setter],
                ..Default::default()
            }))
            .unwrap_err();

        assert!(err.to_string().contains("800000"), "{err}");
        assert_eq!(eth.protocol_fee_config, seeded, "the unconfirmed setter was taken");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_reorg_is_reconciled_too() {
        let (mut eth, config_addr, mut rx, storage) = setup_config_eth_manager();
        let seeded = eth.protocol_fee_config;
        let removed =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let new_tip_hash = BlockHash::random();
        // The logs say the setter was reverted; storage on the new branch disagrees.
        storage.holds(new_tip_hash, (800_000, 900_000));

        let err = eth
            .handle_reorg(
                Arc::new(MockChain {
                    hash: BlockHash::random(),
                    number: 100,
                    receipts: vec![&removed],
                    ..Default::default()
                }),
                Arc::new(MockChain { hash: new_tip_hash, number: 95, ..Default::default() })
            )
            .unwrap_err();

        assert!(err.to_string().contains("800000"), "{err}");
        assert_eq!(eth.protocol_fee_config, seeded);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_failed_storage_read_is_an_error() {
        let (mut eth, _config_addr, mut rx, storage) = setup_config_eth_manager();
        storage.fails();

        let err = eth
            .handle_commit(Arc::new(MockChain {
                hash: BlockHash::random(),
                number: 100,
                ..Default::default()
            }))
            .unwrap_err();

        assert!(err.to_string().contains("storage unavailable"), "{err}");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn blocks_at_or_before_the_deployed_block_are_not_reconciled() {
        let (mut eth, _config_addr, _rx, storage) = setup_config_eth_manager();
        // The deployed block is unset here, so block 0 is at it: the config there
        // is the baked-in const and storage has nothing to say.
        storage.fails();

        eth.apply_periphery_logs(&MockChain { number: 0, ..Default::default() }, None)
            .unwrap();
    }

    /// One block whose receipt carries `logs`, as a real notification does.
    fn chain(number: u64, hash: BlockHash, logs: Vec<Log>) -> Arc<Chain> {
        let header = Header { number, ..Default::default() };
        let block = RecoveredBlock::new(Block { header, body: Default::default() }, vec![], hash);
        let outcome =
            ExecutionOutcome::new(Default::default(), vec![vec![receipt(logs)]], number, vec![]);
        Arc::new(Chain::new([block], outcome, Default::default()))
    }

    #[tokio::test]
    async fn a_queued_backlog_is_applied_in_order_before_the_first_round() {
        let (mut eth, config_addr, mut rx, storage) = setup_config_eth_manager();
        let (canon_tx, canon_rx) = tokio::sync::broadcast::channel(8);
        eth.canonical_updates = BroadcastStream::new(canon_rx);
        let (first, second) = (BlockHash::random(), BlockHash::random());
        storage.holds(second, (800_000, 900_000));

        // Both land before the cleanser is polled once — the shape of startup, where
        // blocks queue on the subscription while pools are discovered.
        canon_tx
            .send(CanonStateNotification::Commit { new: chain(101, first, vec![]) })
            .unwrap();
        canon_tx
            .send(CanonStateNotification::Commit {
                new: chain(
                    102,
                    second,
                    vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]
                )
            })
            .unwrap();
        futures::future::poll_fn(|cx| {
            let _ = eth.poll_unpin(cx);
            Poll::Ready(())
        })
        .await;

        // Both blocks released, in order, and the later block's setter published
        // before that block's round can open.
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let heads: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                EthEvent::NewBlock(block) => Some(block.number),
                _ => None
            })
            .collect();
        assert_eq!(heads, vec![101, 102]);
        let config_at = events
            .iter()
            .position(|event| matches!(event, EthEvent::ProtocolFeeConfigUpdated(_)))
            .expect("the setter is published");
        let second_head_at = events
            .iter()
            .position(|event| matches!(event, EthEvent::NewBlock(block) if block.number == 102))
            .unwrap();
        assert!(config_at < second_head_at);
        assert_eq!(
            eth.protocol_fee_config,
            DonationSplitSnapshot {
                block_number: 102,
                block_hash:   second,
                splits:       DonationSplits::new(800_000, 900_000).unwrap()
            }
        );
    }
}
