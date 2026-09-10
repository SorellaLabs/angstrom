use std::{
    collections::{HashMap, HashSet},
    ops::RangeInclusive,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::{
    consensus::Transaction,
    primitives::{Address, B256, Log, aliases::I24},
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
        protocol_fees::{DonationSplitSnapshot, DonationSplits}
    },
    traits::ChainExt
};
use futures::Future;
use futures_util::{FutureExt, StreamExt};
use itertools::Itertools;
use pade::PadeDecode;
use reth_provider::{CanonStateNotification, CanonStateNotifications};
use reth_tasks::TaskExecutor;
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
    pub(crate) protocol_fee_config: DonationSplitSnapshot
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
            protocol_fee_config
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

    fn on_canon_update(&mut self, canonical_updates: CanonStateNotification) {
        tracing::info!(?canonical_updates, "got new block update!!!!!");
        telemetry_recorder::telemetry_event!(EthUpdaterSnapshot::from((
            &*self,
            canonical_updates.clone()
        )));

        match canonical_updates.clone() {
            CanonStateNotification::Reorg { old, new } => self.handle_reorg(old, new),
            CanonStateNotification::Commit { new } => self.handle_commit(new)
        }
        let _ = self.cannon_sender.send(canonical_updates);
    }

    fn handle_reorg(&mut self, old: Arc<impl ChainExt>, new: Arc<impl ChainExt>) {
        // Removing a setter carries no replacement event, so the value it overwrote
        // has to be reinstated from the removed logs before the new chain's own logs
        // land on top of it.
        let reverted_splits = self.reverted_protocol_fee_config(&old);
        self.apply_periphery_logs(&new, reverted_splits);

        // notify producer of reorg if one happened. NOTE: reth also calls this
        // on reverts
        let tip = new.tip_number();
        let reorg = old.reorged_range(&new).unwrap_or(tip..=tip);
        self.block_sync.reorg(reorg.clone());

        let mut eoas = self.get_eoa(old.clone());
        eoas.extend(self.get_eoa(new.clone()));

        // get all reorged orders
        let old_filled: HashSet<_> = self.fetch_filled_order(&old).collect();
        let new_filled: HashSet<_> = self.fetch_filled_order(&new).collect();

        let difference: Vec<_> = old_filled.difference(&new_filled).copied().collect();
        let reorged_orders = EthEvent::ReorgedOrders(difference, reorg);

        self.send_events(reorged_orders);
    }

    fn handle_commit(&mut self, new: Arc<impl ChainExt>) {
        // handle this first so the newest state is the first available
        self.apply_periphery_logs(&new, None);

        let tip = new.tip_number();
        tracing::info!(?self.block_sync);
        self.block_sync.new_block(tip);

        let filled_orders = self.fetch_filled_order(&new).collect::<Vec<_>>();
        tracing::info!(?filled_orders, "filled orders found");

        let eoas = self.get_eoa(new.clone());

        let transitions = EthEvent::NewBlockTransitions {
            block_number: new.tip_number(),
            filled_orders,
            address_changeset: eoas
        };

        self.send_events(EthEvent::NewBlock(tip));
        self.send_events(transitions);
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
    fn apply_periphery_logs(
        &mut self,
        chain: &impl ChainExt,
        reverted_splits: Option<DonationSplits>
    ) {
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

            if let Ok(remove_node) = NodeRemoved::decode_log(log) {
                tracing::info!(?remove_node.node, "node removed from set");
                self.node_set.remove(&remove_node.node);
                self.send_events(EthEvent::RemovedNode(remove_node.node));
                continue;
            }
            if let Ok(added_node) = NodeAdded::decode_log(log) {
                tracing::info!(?added_node.node, "new node added to set");
                self.node_set.insert(added_node.node);
                self.send_events(EthEvent::AddedNode(added_node.node));
                continue;
            }
            if let Ok(removed_pool) = PoolRemoved::decode_log(log) {
                tracing::info!("new pool removed log");

                self.pool_store
                    .remove_pair(removed_pool.asset0, removed_pool.asset1);

                let t0 = *self.angstrom_tokens.entry(removed_pool.asset0).or_default();
                let t1 = *self.angstrom_tokens.entry(removed_pool.asset1).or_default();

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
                let entry = AngPoolConfigEntry {
                    pool_partial_key: AngstromPoolConfigStore::derive_store_key(asset0, asset1),
                    tick_spacing:     added_pool.tickSpacing,
                    fee_in_e6:        added_pool.bundleFee.to(),
                    store_index:      self.pool_store.length()
                };

                let pool_key = PoolKey {
                    currency0:   asset0,
                    currency1:   asset1,
                    fee:         added_pool.bundleFee,
                    tickSpacing: I24::unchecked_from(added_pool.tickSpacing),
                    hooks:       self.angstrom_address
                };

                self.pool_store.new_pool(asset0, asset1, entry);
                *self.angstrom_tokens.entry(asset0).or_default() += 1;
                *self.angstrom_tokens.entry(asset1).or_default() += 1;

                self.send_events(EthEvent::NewPool { pool: pool_key });
            }
        }

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
        while let Poll::Ready(is_some) = self.canonical_updates.poll_next_unpin(cx).map(|res| {
            res.transpose()
                .ok()
                .flatten()
                .map(|update| self.on_canon_update(update))
                .is_some()
        }) {
            if !is_some {
                return Poll::Ready(());
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
    NewBlock(u64),
    NewBlockTransitions {
        block_number:      u64,
        filled_orders:     Vec<B256>,
        address_changeset: Vec<Address>
    },
    ReorgedOrders(Vec<B256>, RangeInclusive<u64>),
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
    use alloy::{
        consensus::TxLegacy,
        hex,
        primitives::{BlockHash, BlockNumber, Log, TxKind, U256, aliases::U24, b256},
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
    use reth_primitives_traits::{LogData, RecoveredBlock};
    use testing_tools::type_generator::orders::{ToBOrderBuilder, UserOrderBuilder};

    use super::*;

    #[derive(Default)]
    pub struct MockChain<'a> {
        pub hash:         BlockHash,
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
            }
        }
    }

    fn setup_signing_info() -> AngstromSigner<PrivateKeySigner> {
        AngstromSigner::random()
    }

    #[test]
    fn test_fetch_filled_orders() {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let signing_info = setup_signing_info();
        let angstrom_address = Address::random();
        let eth = setup_non_subscription_eth_manager(Some(angstrom_address));

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
            finalized_user_order.order_hash(&pair, &assets, 0),
            finalized_tob.order_hash(&pair, &assets, 0),
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

        let mock_tx = TransactionSigned::new_unhashed(leg.into(), Signature::test_signature());
        let mock_chain = MockChain { transactions: vec![mock_tx], ..Default::default() };
        let filled_set = eth.fetch_filled_order(&mock_chain).collect::<HashSet<_>>();

        for order_hash in order_hashes {
            assert!(filled_set.contains(&order_hash));
        }
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
        eth.apply_periphery_logs(&*mock_chain, None);

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
        eth.apply_periphery_logs(&*mock_chain, None);

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
        eth.handle_reorg(old_chain, new_chain);

        // Should receive both NewBlockTransitions and ReorgedOrders events
        let mut received_reorg = false;

        for _ in 0..1 {
            match rx.try_recv().expect("Should receive 1 event") {
                EthEvent::ReorgedOrders(_, range) => {
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
        eth.handle_commit(new_chain);

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

        eth.handle_commit(mock_chain);

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

        eth.apply_periphery_logs(&*mock_chain, None);

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

        let logs = vec![
            Log { address: periphery_addr, data: configure1.encode_log_data() },
            Log { address: periphery_addr, data: configure2.encode_log_data() },
            Log { address: periphery_addr, data: remove.encode_log_data() },
        ];

        let mock_recip = Receipt { logs, success: true, ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        eth.apply_periphery_logs(&*mock_chain, None);

        // Verify final state
        assert!(eth.angstrom_tokens.contains_key(&asset0));
        assert!(eth.angstrom_tokens.contains_key(&asset1));
        assert_eq!(eth.pool_store.length(), 0); // Should be removed
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
        eth.apply_periphery_logs(&*mock_chain, None);
        assert_eq!(eth.pool_store.length(), 0);
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
        eth.apply_periphery_logs(&*mock_chain, None);
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

    fn setup_config_eth_manager()
    -> (EthDataCleanser<GlobalBlockSync>, Address, tokio::sync::mpsc::UnboundedReceiver<EthEvent>)
    {
        let config_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(Address::random()));
        eth.protocol_fee_config_address = config_addr;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        eth.event_listeners.push(tx);

        (eth, config_addr, rx)
    }

    #[test]
    fn config_change_in_a_non_tip_block_is_applied() {
        let (mut eth, config_addr, mut rx) = setup_config_eth_manager();

        // The change lands two blocks back; the tip itself carries nothing.
        let ancestor =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let tip = receipt(vec![]);
        let tip_hash = BlockHash::random();

        eth.handle_commit(Arc::new(MockChain {
            hash: tip_hash,
            number: 100,
            receipts: vec![&tip],
            ancestors: vec![(BlockHash::random(), vec![&ancestor])],
            ..Default::default()
        }));

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
        let (mut eth, config_addr, mut rx) = setup_config_eth_manager();

        let first =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let second = receipt(vec![splits_log(config_addr, (800_000, 900_000), (600_000, 500_000))]);

        eth.handle_commit(Arc::new(MockChain {
            hash: BlockHash::random(),
            number: 100,
            receipts: vec![&second],
            ancestors: vec![(BlockHash::random(), vec![&first])],
            ..Default::default()
        }));

        let expected = DonationSplits::new(600_000, 500_000).unwrap();
        assert_eq!(eth.protocol_fee_config.splits, expected);
        // One publication for the whole notification, carrying the final value.
        assert_eq!(published_config(&mut rx).unwrap().splits, expected);
        assert!(published_config(&mut rx).is_none(), "only one update per notification");
    }

    #[test]
    fn a_reorg_that_removes_a_setter_restores_the_previous_rates() {
        let (mut eth, config_addr, mut rx) = setup_config_eth_manager();

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

        eth.handle_reorg(old, new);

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
        let (mut eth, config_addr, mut rx) = setup_config_eth_manager();

        let removed =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (800_000, 900_000))]);
        let replacement =
            receipt(vec![splits_log(config_addr, (750_000, 1_000_000), (250_000, 100_000))]);

        eth.handle_reorg(
            Arc::new(MockChain {
                hash: BlockHash::random(),
                number: 100,
                receipts: vec![&removed],
                ..Default::default()
            }),
            Arc::new(MockChain {
                hash: BlockHash::random(),
                number: 100,
                receipts: vec![&replacement],
                ..Default::default()
            })
        );

        let expected = DonationSplits::new(250_000, 100_000).unwrap();
        assert_eq!(eth.protocol_fee_config.splits, expected);
        assert_eq!(published_config(&mut rx).unwrap().splits, expected);
    }

    #[test]
    fn a_reorg_reverts_to_the_state_before_the_whole_removed_range() {
        let (mut eth, config_addr, _rx) = setup_config_eth_manager();

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
        );

        assert_eq!(
            eth.protocol_fee_config.splits,
            DonationSplits::new(750_000, 1_000_000).unwrap()
        );
    }

    #[test]
    fn a_notification_without_a_setter_leaves_the_seeded_config_alone() {
        let (mut eth, _config_addr, mut rx) = setup_config_eth_manager();
        let seeded = eth.protocol_fee_config;

        eth.handle_commit(Arc::new(MockChain {
            hash: BlockHash::random(),
            number: 100,
            ..Default::default()
        }));

        // The node starts with rates and keeps them until a log says otherwise.
        assert_eq!(eth.protocol_fee_config, seeded);
        assert!(published_config(&mut rx).is_none(), "nothing changed, nothing published");
    }
}
