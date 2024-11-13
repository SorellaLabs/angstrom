use std::{
    collections::HashSet,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::{
    primitives::{Address, BlockHash, BlockNumber, B256},
    sol_types::SolEvent
};
use angstrom_types::{
    contract_bindings,
    contract_payloads::angstrom::{AngstromBundle, AngstromPoolConfigStore},
    primitive::NewInitializedPool
};
use futures::Future;
use futures_util::{FutureExt, StreamExt};
use pade::PadeDecode;
use reth_primitives::{Receipt, TransactionSigned};
use reth_provider::{CanonStateNotification, CanonStateNotifications, Chain};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::handle::{EthCommand, EthHandle};

alloy::sol!(
    event Transfer(address indexed _from, address indexed _to, uint256 _value);
    event Approval(address indexed _owner, address indexed _spender, uint256 _value);
);

/// Listens for CanonStateNotifications and sends the appropriate updates to be
/// executed by the order pool
pub struct EthDataCleanser {
    angstrom_address: Address,
    /// our command receiver
    commander:        ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners:  Vec<UnboundedSender<EthEvent>>,

    /// Notifications for Canonical Block updates
    canonical_updates: BroadcastStream<CanonStateNotification>,
    angstrom_tokens:   HashSet<Address>,

    /// TODO: Once the periphery contracts are finished. we will add a watcher
    /// on the contract that every time a new pair is added, we update the
    /// pool store globally.
    _pool_store: Arc<AngstromPoolConfigStore>
}

impl EthDataCleanser {
    pub fn spawn<TP: TaskSpawner>(
        angstrom_address: Address,
        canonical_updates: CanonStateNotifications,
        tp: TP,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>,
        angstrom_tokens: HashSet<Address>,
        pool_store: Arc<AngstromPoolConfigStore>
    ) -> anyhow::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);

        let this = Self {
            angstrom_address,
            canonical_updates: BroadcastStream::new(canonical_updates),
            commander: stream,
            event_listeners: Vec::new(),
            angstrom_tokens,
            _pool_store: pool_store
        };
        tp.spawn_critical("eth handle", this.boxed());

        let handle = EthHandle::new(tx);

        Ok(handle)
    }

    fn send_events(&mut self, event: EthEvent) {
        self.event_listeners
            .retain(|e| e.send(event.clone()).is_ok());
    }

    fn on_command(&mut self, command: EthCommand) {
        match command {
            EthCommand::SubscribeEthNetworkEvents(tx) => self.event_listeners.push(tx)
        }
    }

    fn on_canon_update(&mut self, canonical_updates: CanonStateNotification) {
        match canonical_updates {
            CanonStateNotification::Reorg { old, new } => self.handle_reorg(old, new),
            CanonStateNotification::Commit { new } => self.handle_commit(new)
        }
    }

    fn handle_reorg(&mut self, old: Arc<impl ChainExt>, new: Arc<impl ChainExt>) {
        let mut eoas = self.get_eoa(old.clone());
        eoas.extend(self.get_eoa(new.clone()));

        // get all reorged orders
        let old_filled: HashSet<_> = self.fetch_filled_order(&old).collect();
        let new_filled: HashSet<_> = self.fetch_filled_order(&new).collect();

        let difference: Vec<_> = old_filled.difference(&new_filled).copied().collect();
        let reorged_orders = EthEvent::ReorgedOrders(difference);

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
        self.handle_new_pools(new.clone());

        let filled_orders = self.fetch_filled_order(&new).collect::<Vec<_>>();

        let eoas = self.get_eoa(new.clone());

        let transitions = EthEvent::NewBlockTransitions {
            block_number: new.tip_number(),
            filled_orders,
            address_changeset: eoas
        };
        self.send_events(transitions);
    }

    fn handle_new_pools(&mut self, chain: Arc<impl ChainExt>) {
        Self::get_new_pools(&chain)
            .inspect(|pool| {
                let token_0 = pool.currency_in;
                let token_1 = pool.currency_out;
                self.angstrom_tokens.insert(token_0);
                self.angstrom_tokens.insert(token_1);
            })
            .map(EthEvent::NewPool)
            .for_each(|pool_event| {
                // didn't use send event fn because of lifetimes.
                self.event_listeners
                    .retain(|e| e.send(pool_event.clone()).is_ok());
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

    /// gets any newly initialized pools in this block
    /// do we want to use logs here?
    fn get_new_pools(chain: &impl ChainExt) -> impl Iterator<Item = NewInitializedPool> + '_ {
        chain
            .receipts_by_block_hash(chain.tip_hash())
            .unwrap()
            .into_iter()
            .flat_map(|receipt| {
                receipt.logs.iter().filter_map(|log| {
                    contract_bindings::pool_manager::PoolManager::Initialize::decode_log(log, true)
                        .map(Into::into)
                        .ok()
                })
            })
    }
}

impl Future for EthDataCleanser {
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
    ReorgedOrders(Vec<B256>),
    FinalizedBlock(u64),
    NewPool(NewInitializedPool)
}

#[auto_impl::auto_impl(&,Arc)]
pub trait ChainExt {
    fn tip_number(&self) -> BlockNumber;
    fn tip_hash(&self) -> BlockHash;
    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&Receipt>>;
    fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_;
}

impl ChainExt for Chain {
    fn tip_hash(&self) -> BlockHash {
        self.tip().hash()
    }

    fn tip_number(&self) -> BlockNumber {
        self.tip().number
    }

    fn receipts_by_block_hash(&self, block_hash: BlockHash) -> Option<Vec<&Receipt>> {
        self.receipts_by_block_hash(block_hash)
    }

    fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_ {
        self.tip().transactions()
    }
}

#[cfg(test)]
pub mod test {
    use alloy::{
        hex,
        primitives::{b256, TxKind, U256},
        signers::local::PrivateKeySigner
    };
    use angstrom_types::{
        contract_payloads::{
            angstrom::{TopOfBlockOrder, UserOrder},
            Asset, Pair
        },
        orders::OrderOutcome,
        primitive::ANGSTROM_DOMAIN,
        sol_bindings::grouped_orders::OrderWithStorageData
    };
    use pade::PadeEncode;
    use reth_primitives::Transaction;
    use testing_tools::type_generator::orders::{SigningInfo, ToBOrderBuilder, UserOrderBuilder};

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

        fn tip_number(&self) -> BlockNumber {
            self.number
        }

        fn receipts_by_block_hash(&self, _: BlockHash) -> Option<Vec<&Receipt>> {
            self.receipts.clone()
        }

        fn tip_transactions(&self) -> impl Iterator<Item = &TransactionSigned> + '_ {
            self.transactions.iter()
        }
    }

    fn setup_non_subscription_eth_manager(angstrom_address: Option<Address>) -> EthDataCleanser {
        let (_command_tx, command_rx) = tokio::sync::mpsc::channel(3);
        let (_cannon_tx, cannon_rx) = tokio::sync::broadcast::channel(3);
        EthDataCleanser {
            commander:         ReceiverStream::new(command_rx),
            event_listeners:   vec![],
            angstrom_tokens:   HashSet::default(),
            angstrom_address:  angstrom_address.unwrap_or_default(),
            canonical_updates: BroadcastStream::new(cannon_rx),
            _pool_store:       Default::default()
        }
    }

    fn setup_signing_info() -> SigningInfo {
        // let mut rand = rand::rng();
        let wallet = PrivateKeySigner::random();
        let addr = wallet.address();
        let key = wallet.credential().clone();
        SigningInfo { domain: ANGSTROM_DOMAIN, address: addr, key }
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

        let finalized_user_order = UserOrder::from_internal_order(&user_order, &outcome, 0);
        let finalized_tob = TopOfBlockOrder::of(&t, 0);

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
