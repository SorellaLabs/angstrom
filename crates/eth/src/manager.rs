use std::{
    collections::HashSet,
    ops::RangeInclusive,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::{
    consensus::Transaction,
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
use pade::PadeDecode;
use reth_primitives::{Receipt, SealedBlockWithSenders, TransactionSigned};
use reth_provider::{CanonStateNotification, CanonStateNotifications, Chain};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::handle::{EthCommand, EthHandle};

alloy::sol!(
    event Transfer(address indexed _from, address indexed _to, uint256 _value);
    event Approval(address indexed _owner, address indexed _spender, uint256 _value);
);

const MAX_REORG_DEPTH: u64 = 30;

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
            .unwrap()
            .into_iter()
            .flat_map(|receipt| &receipt.logs)
            .filter(|log| log.address == periphery_address)
            .for_each(|log| {
                if let Ok(remove_node) = NodeRemoved::decode_log(log, true) {
                    self.node_set.remove(&remove_node.address);
                    self.send_events(EthEvent::RemovedNode(remove_node.node));
                }
                if let Ok(added_node) = NodeAdded::decode_log(log, true) {
                    self.node_set.insert(added_node.address);
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
                        fee_in_e6:        added_pool.feeInE6.to(),
                        store_index:      self.pool_store.length()
                    };

                    let pool_key = PoolKey {
                        currency1:   asset1,
                        currency0:   asset0,
                        fee:         added_pool.feeInE6,
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

    /// TODO: check contract for state change. if there is change. fetch the
    /// transaction on Angstrom and process call-data to pull order-hashes.
    fn fetch_filled_order<'a>(
        &'a self,
        chain: &'a impl ChainExt
    ) -> impl Iterator<Item = B256> + 'a {
        chain
            .tip_transactions()
            .filter(|tx| tx.transaction.to() == Some(self.angstrom_address))
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
            .unwrap()
            .into_iter()
            .flat_map(|receipt| &receipt.logs)
            .filter(|log| self.angstrom_tokens.contains(&log.address))
            .flat_map(|logs| {
                Transfer::decode_log(logs, true)
                    .map(|log| log._from)
                    .or_else(|_| Approval::decode_log(logs, true).map(|log| log._owner))
            })
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
    fn blocks_iter(&self) -> impl Iterator<Item = &SealedBlockWithSenders> + '_;
}

impl ChainExt for Chain {
    fn tip_hash(&self) -> BlockHash {
        self.tip().hash()
    }

    fn reorged_range(&self, new: impl ChainExt) -> Option<RangeInclusive<u64>> {
        let tip = new.tip_number();
        // search 30 blocks back;
        let start = tip - MAX_REORG_DEPTH;

        let mut range = self
            .blocks_iter()
            .filter(|b| b.block.number >= start)
            .zip(new.blocks_iter().filter(|b| b.block.number >= start))
            .filter(|&(old, new)| (old.block.hash() != new.block.hash()))
            .map(|(_, new)| new.block.number)
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

    fn blocks_iter(&self) -> impl Iterator<Item = &SealedBlockWithSenders> + '_ {
        self.blocks_iter()
    }

    fn tip_number(&self) -> BlockNumber {
        self.tip().number
    }

    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&Receipt>> {
        self.receipts_by_block_hash(block_hash)
    }

    fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_ {
        self.tip().transactions().iter()
    }
}

#[cfg(test)]
pub mod test {
    use alloy::{
        hex,
        primitives::{b256, TxKind, U256}
    };
    use angstrom_types::{
        block_sync::*,
        contract_payloads::{
            angstrom::{TopOfBlockOrder, UserOrder},
            Asset, Pair
        },
        orders::OrderOutcome,
        primitive::AngstromSigner,
        sol_bindings::grouped_orders::OrderWithStorageData
    };
    use pade::PadeEncode;
    use reth_primitives::Transaction;
    use testing_tools::type_generator::orders::{ToBOrderBuilder, UserOrderBuilder};

    use super::*;

    #[derive(Default)]
    pub struct MockChain<'a> {
        pub hash:         BlockHash,
        pub number:       BlockNumber,
        pub transactions: Vec<TransactionSigned>,
        pub receipts:     Option<Vec<&'a Receipt>>
    }

    impl ChainExt for MockChain<'_> {
        fn tip_hash(&self) -> BlockHash {
            self.hash
        }

        fn blocks_iter(&self) -> impl Iterator<Item = &SealedBlockWithSenders> + '_ {
            vec![].into_iter()
        }

        fn tip_number(&self) -> BlockNumber {
            self.number
        }

        fn receipts_by_block_hash(&self, _: BlockHash) -> Option<Vec<&Receipt>> {
            self.receipts.clone()
        }

        fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_ {
            self.transactions.iter()
        }

        fn reorged_range(&self, _: impl ChainExt) -> Option<RangeInclusive<u64>> {
            None
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

        let mut mock_tx = TransactionSigned::default();

        if let Transaction::Legacy(leg) = &mut mock_tx.transaction {
            leg.to = TxKind::Call(angstrom_address);
            leg.input = angstrom_bundle_with_orders.pade_encode().into();
        }

        let mock_chain = MockChain { transactions: vec![mock_tx], ..Default::default() };
        let filled_set = eth.fetch_filled_order(&mock_chain).collect::<HashSet<_>>();

        for order_hash in order_hashes {
            assert!(filled_set.contains(&order_hash));
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

        let mock_chain =
            Arc::new(MockChain { receipts: Some(vec![&mock_recip]), ..Default::default() });
        let filled_set = eth.get_eoa(mock_chain);

        for change in changeset {
            assert!(filled_set.contains(&change));
        }
    }
}
