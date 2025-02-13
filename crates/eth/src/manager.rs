use std::{
    collections::HashSet,
    ops::RangeInclusive,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::{
    consensus::{BlockHeader, Transaction},
    primitives::{aliases::I24, Address, BlockHash, BlockNumber, B256},
    sol_types::SolEvent
};
use angstrom_types::{
    block_sync::BlockSyncProducer,
    contract_bindings::{
        angstrom::Angstrom::PoolKey,
        controller_v_1::ControllerV1::{NodeAdded, NodeRemoved, PoolConfigured, PoolRemoved}
    },
    contract_payloads::angstrom::{AngPoolConfigEntry, AngstromBundle, AngstromPoolConfigStore}
};
use futures::Future;
use futures_util::{FutureExt, StreamExt};
use itertools::Itertools;
use pade::PadeDecode;
use reth_ethereum_primitives::{Block, Receipt, TransactionSigned};
use reth_primitives_traits::RecoveredBlock;
use reth_provider::{CanonStateNotification, CanonStateNotifications, Chain};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::handle::{EthCommand, EthHandle};

alloy::sol!(
    event Transfer(address indexed _from, address indexed _to, uint256 _value);
    event Approval(address indexed _owner, address indexed _spender, uint256 _value);
);

const MAX_REORG_DEPTH: u64 = 150;

/// Listens for CanonStateNotifications and sends the appropriate updates to be
/// executed by the order pool
pub struct EthDataCleanser<Sync> {
    angstrom_address:  Address,
    periphery_address: Address,
    /// our command receiver
    commander:         ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners:   Vec<UnboundedSender<EthEvent>>,
    /// for rebroadcasting
    cannon_sender:     tokio::sync::broadcast::Sender<CanonStateNotification>,
    /// Notifications for Canonical Block updates
    canonical_updates: BroadcastStream<CanonStateNotification>,
    angstrom_tokens:   HashSet<Address>,
    /// handles syncing of blocks.
    block_sync:        Sync,
    /// updated by periphery contract.
    pool_store:        Arc<AngstromPoolConfigStore>,
    /// the set of currently active nodes.
    node_set:          HashSet<Address>
}

impl<Sync> EthDataCleanser<Sync>
where
    Sync: BlockSyncProducer
{
    pub fn spawn<TP: TaskSpawner>(
        angstrom_address: Address,
        periphery_address: Address,
        canonical_updates: CanonStateNotifications,
        tp: TP,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>,
        angstrom_tokens: HashSet<Address>,
        pool_store: Arc<AngstromPoolConfigStore>,
        sync: Sync,
        node_set: HashSet<Address>,
        event_listeners: Vec<UnboundedSender<EthEvent>>
    ) -> anyhow::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);
        let (cannon_tx, _) = tokio::sync::broadcast::channel(1000);

        let mut this = Self {
            angstrom_address,
            periphery_address,
            canonical_updates: BroadcastStream::new(canonical_updates),
            commander: stream,
            angstrom_tokens,
            cannon_sender: cannon_tx,
            block_sync: sync,
            pool_store,
            node_set,
            event_listeners
        };
        // ensure we broadcast node set. will allow for proper connections
        // on the network side
        for n in &this.node_set {
            this.event_listeners
                .retain(|e| e.send(EthEvent::AddedNode(*n)).is_ok());
        }

        tp.spawn_critical("eth handle", this.boxed());

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
        match canonical_updates.clone() {
            CanonStateNotification::Reorg { old, new } => self.handle_reorg(old, new),
            CanonStateNotification::Commit { new } => self.handle_commit(new)
        }
        let _ = self.cannon_sender.send(canonical_updates);
    }

    fn handle_reorg(&mut self, old: Arc<impl ChainExt>, new: Arc<impl ChainExt>) {
        self.apply_periphery_logs(&new);
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

        let transitions = EthEvent::NewBlockTransitions {
            block_number:      new.tip_number(),
            filled_orders:     new_filled.into_iter().collect(),
            address_changeset: eoas
        };

        self.send_events(transitions);
        self.send_events(reorged_orders);
    }

    fn handle_commit(&mut self, new: Arc<impl ChainExt>) {
        // handle this first so the newest state is the first available
        self.apply_periphery_logs(&new);

        let tip = new.tip_number();
        self.block_sync.new_block(tip);

        let filled_orders = self.fetch_filled_order(&new).collect::<Vec<_>>();

        let eoas = self.get_eoa(new.clone());

        let transitions = EthEvent::NewBlockTransitions {
            block_number: new.tip_number(),
            filled_orders,
            address_changeset: eoas
        };
        self.send_events(transitions);
    }

    /// looks at all periphery contrct events updating the internal state +
    /// sending out info.
    fn apply_periphery_logs(&mut self, chain: &impl ChainExt) {
        let periphery_address = self.periphery_address;
        chain
            .receipts_by_block_hash(chain.tip_hash())
            .unwrap_or_default()
            .into_iter()
            .flat_map(|receipt| &receipt.logs)
            .filter(|log| log.address == periphery_address)
            .for_each(|log| {
                if let Ok(remove_node) = NodeRemoved::decode_log(log, true) {
                    self.node_set.remove(&remove_node.node);
                    self.send_events(EthEvent::RemovedNode(remove_node.node));
                }
                if let Ok(added_node) = NodeAdded::decode_log(log, true) {
                    self.node_set.insert(added_node.node);
                    self.send_events(EthEvent::AddedNode(added_node.node));
                }
                if let Ok(removed_pool) = PoolRemoved::decode_log(log, true) {
                    self.pool_store
                        .remove_pair(removed_pool.asset0, removed_pool.asset1);

                    let pool_key = PoolKey {
                        currency1:   removed_pool.asset1,
                        currency0:   removed_pool.asset0,
                        fee:         removed_pool.feeInE6,
                        tickSpacing: removed_pool.tickSpacing,
                        hooks:       self.angstrom_address
                    };
                    self.send_events(EthEvent::RemovedPool { pool: pool_key });
                }
                if let Ok(added_pool) = PoolConfigured::decode_log(log, true) {
                    let asset0 = added_pool.asset0;
                    let asset1 = added_pool.asset1;
                    let entry = AngPoolConfigEntry {
                        pool_partial_key: AngstromPoolConfigStore::derive_store_key(asset0, asset1),
                        tick_spacing:     added_pool.tickSpacing,
                        fee_in_e6:        added_pool.bundleFee.to(),
                        store_index:      self.pool_store.length()
                    };

                    let pool_key = PoolKey {
                        currency1:   asset1,
                        currency0:   asset0,
                        fee:         added_pool.bundleFee,
                        tickSpacing: I24::try_from_be_slice(&{
                            let bytes = added_pool.tickSpacing.to_be_bytes();
                            let mut a = [0u8; 3];
                            a[1..3].copy_from_slice(&bytes);
                            a
                        })
                        .unwrap(),
                        hooks:       self.angstrom_address
                    };

                    self.pool_store.new_pool(asset0, asset1, entry);
                    self.angstrom_tokens.insert(asset0);
                    self.angstrom_tokens.insert(asset1);

                    self.send_events(EthEvent::NewPool { pool: pool_key });
                }
            });
    }

    fn fetch_filled_order<'a>(
        &'a self,
        chain: &'a impl ChainExt
    ) -> impl Iterator<Item = B256> + 'a {
        let tip_txs = chain.tip_transactions().cloned();

        tip_txs
            .filter(|tx| tx.to() == Some(self.angstrom_address))
            .filter_map(|transaction| {
                let mut input: &[u8] = transaction.input();
                AngstromBundle::pade_decode(&mut input, None).ok()
            })
            .flat_map(move |bundle| {
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
            .flat_map(|receipt| &receipt.logs)
            .filter(|log| self.angstrom_tokens.contains(&log.address))
            .flat_map(|logs| {
                Transfer::decode_log(logs, true)
                    .map(|log| log._from)
                    .or_else(|_| Approval::decode_log(logs, true).map(|log| log._owner))
            })
            .unique()
            .collect()
    }
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
                return Poll::Ready(())
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
    RemovedNode(Address)
}

#[auto_impl::auto_impl(&,Arc)]
pub trait ChainExt {
    fn tip_number(&self) -> BlockNumber;
    fn tip_hash(&self) -> BlockHash;
    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&Receipt>>;
    fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_;
    fn reorged_range(&self, new: impl ChainExt) -> Option<RangeInclusive<u64>>;
    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<Block>> + '_;
}

impl ChainExt for Chain {
    fn tip_number(&self) -> BlockNumber {
        self.tip().number
    }

    fn tip_hash(&self) -> BlockHash {
        self.tip().hash()
    }

    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&Receipt>> {
        self.receipts_by_block_hash(block_hash)
    }

    fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_ {
        self.tip().body().transactions.iter()
    }

    fn reorged_range(&self, new: impl ChainExt) -> Option<RangeInclusive<u64>> {
        let tip = new.tip_number();
        // search 150 blocks back;
        let start = tip - MAX_REORG_DEPTH;

        let mut range = self
            .blocks_iter()
            .filter(|b| b.number() >= start)
            .zip(new.blocks_iter().filter(|b| b.number() >= start))
            .filter(|&(old, new)| (old.hash() != new.hash()))
            .map(|(_, new)| new.number())
            .collect::<Vec<_>>();

        match range.len() {
            0 => None,
            1 => {
                let r = range.remove(0);
                Some(r..=r)
            }
            _ => {
                let start = *range.first().unwrap();
                let end = *range.last().unwrap();
                Some(start..=end)
            }
        }
    }

    fn blocks_iter(&self) -> impl Iterator<Item = &RecoveredBlock<Block>> + '_ {
        self.blocks_iter()
    }
}

#[cfg(test)]
pub mod test {
    use alloy::{
        consensus::TxLegacy,
        hex,
        primitives::{aliases::U24, b256, Log, TxKind, U256},
        signers::Signature,
        sol_types::SolEvent
    };
    use angstrom_types::{
        block_sync::*,
        contract_bindings::controller_v_1::ControllerV1::{
            NodeAdded, NodeRemoved, PoolConfigured, PoolRemoved
        },
        contract_payloads::{
            angstrom::{TopOfBlockOrder, UserOrder},
            Asset, Pair
        },
        orders::OrderOutcome,
        primitive::AngstromSigner,
        sol_bindings::grouped_orders::OrderWithStorageData
    };
    use pade::PadeEncode;
    use reth_primitives::LogData;
    use testing_tools::type_generator::orders::{ToBOrderBuilder, UserOrderBuilder};

    use super::*;

    #[derive(Default)]
    pub struct MockChain<'a> {
        pub hash:         BlockHash,
        pub number:       BlockNumber,
        pub transactions: Vec<TransactionSigned>,
        pub receipts:     Vec<&'a Receipt>
    }

    impl ChainExt for MockChain<'_> {
        fn tip_number(&self) -> BlockNumber {
            self.number
        }

        fn tip_hash(&self) -> BlockHash {
            self.hash
        }

        fn receipts_by_block_hash(&self, _: BlockHash) -> Option<Vec<&Receipt>> {
            Some(self.receipts.clone())
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
    }

    fn setup_non_subscription_eth_manager(
        angstrom_address: Option<Address>
    ) -> EthDataCleanser<GlobalBlockSync> {
        let (_command_tx, command_rx) = tokio::sync::mpsc::channel(3);
        let (_cannon_tx, cannon_rx) = tokio::sync::broadcast::channel(3);
        let (tx, _) = tokio::sync::broadcast::channel(3);
        EthDataCleanser {
            commander:         ReceiverStream::new(command_rx),
            event_listeners:   vec![],
            angstrom_tokens:   HashSet::default(),
            node_set:          HashSet::default(),
            angstrom_address:  angstrom_address.unwrap_or_default(),
            periphery_address: Address::default(),
            canonical_updates: BroadcastStream::new(cannon_rx),
            block_sync:        GlobalBlockSync::new(1),
            cannon_sender:     tx,
            pool_store:        Default::default()
        }
    }

    fn setup_signing_info() -> AngstromSigner {
        AngstromSigner::random()
    }

    #[test]
    fn test_fetch_filled_orders() {
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
            input: angstrom_bundle_with_orders.pade_encode().into(),
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
        eth.apply_periphery_logs(&*mock_chain);

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
            tickSpacing: tick_spacing
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
        assert!(!eth.angstrom_tokens.contains(&asset0));
        assert!(!eth.angstrom_tokens.contains(&asset1));
        assert_eq!(eth.pool_store.length(), 0);

        // Process the logs
        eth.apply_periphery_logs(&*mock_chain);

        // Verify final state after add and remove
        assert!(eth.angstrom_tokens.contains(&asset0));
        assert!(eth.angstrom_tokens.contains(&asset1));
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
        let mut received_transitions = false;
        let mut received_reorg = false;

        for _ in 0..2 {
            match rx.try_recv().expect("Should receive two events") {
                EthEvent::NewBlockTransitions { block_number, .. } => {
                    assert_eq!(block_number, 95);
                    received_transitions = true;
                }
                EthEvent::ReorgedOrders(_, range) => {
                    assert_eq!(*range.start(), 95);
                    assert_eq!(*range.end(), 95);
                    received_reorg = true;
                }
                _ => panic!("Unexpected event type")
            }
        }

        assert!(received_transitions, "Should have received NewBlockTransitions event");
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
        match rx.try_recv().expect("Should receive an event") {
            EthEvent::NewBlockTransitions { block_number, filled_orders, address_changeset } => {
                assert_eq!(block_number, 100);
                assert!(filled_orders.is_empty());
                assert!(address_changeset.is_empty());
            }
            _ => panic!("Expected NewBlockTransitions event")
        }
    }

    #[test]
    fn test_fetch_eoa_balance_approval_changes() {
        let ang_addr = Address::random();
        let transfer_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.angstrom_tokens = HashSet::from_iter(vec![transfer_addr]);

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

        let mock_recip = Receipt { logs: vec![transfer_log], ..Default::default() };

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
        eth.angstrom_tokens = HashSet::from_iter(vec![token_addr]);

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

        let mock_recip = Receipt { logs, ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        let eoas = eth.get_eoa(mock_chain);

        assert!(eoas.contains(&addr1));
        assert!(eoas.contains(&addr2));
        assert_eq!(eoas.len(), 2); // addr3 only appears as recipient/spender
    }

    #[test]
    fn test_invalid_log_handling() {
        let ang_addr = Address::random();
        let token_addr = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.angstrom_tokens = HashSet::from_iter(vec![token_addr]);

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

        let mock_recip = Receipt { logs: vec![invalid_log, valid_log], ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        let eoas = eth.get_eoa(mock_chain);
        assert_eq!(eoas.len(), 1); // Should only process the valid log
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

        match rx.try_recv().expect("Should receive an event") {
            EthEvent::NewBlockTransitions { block_number, filled_orders, address_changeset } => {
                assert_eq!(block_number, 100);
                assert!(filled_orders.is_empty());
                assert!(address_changeset.is_empty());
            }
            _ => panic!("Expected NewBlockTransitions event")
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

        let mock_recip = Receipt { logs, ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        eth.apply_periphery_logs(&*mock_chain);

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
            tickSpacing: tick_spacing
        };
        let configure2 = PoolConfigured {
            asset0,
            asset1,
            bundleFee: fee,
            unlockedFee: fee,
            tickSpacing: tick_spacing * 2 // Different spacing
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

        let mock_recip = Receipt { logs, ..Default::default() };
        let mock_chain = Arc::new(MockChain { receipts: vec![&mock_recip], ..Default::default() });

        eth.apply_periphery_logs(&*mock_chain);

        // Verify final state
        assert!(eth.angstrom_tokens.contains(&asset0));
        assert!(eth.angstrom_tokens.contains(&asset1));
        assert_eq!(eth.pool_store.length(), 0); // Should be removed
    }

    #[test]
    fn test_non_angstrom_token_transfers() {
        let ang_addr = Address::random();
        let token_addr = Address::random();
        let non_tracked_token = Address::random();
        let mut eth = setup_non_subscription_eth_manager(Some(ang_addr));
        eth.angstrom_tokens = HashSet::from_iter(vec![token_addr]);

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
            tickSpacing: tick_spacing
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
        eth.apply_periphery_logs(&*mock_chain);
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
        eth.apply_periphery_logs(&*mock_chain);
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
}
