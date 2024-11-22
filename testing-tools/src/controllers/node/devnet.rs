use std::{
    collections::{HashMap, HashSet},
    future::Future,
    marker::PhantomData,
    net::SocketAddr,
    sync::Arc,
    task::Poll
};

use alloy::{providers::Provider, pubsub::PubSubFrontend};
use alloy_primitives::Address;
use alloy_rpc_types::Transaction;
use angstrom::components::StromHandles;
use angstrom_network::{
    manager::StromConsensusEvent, NetworkOrderEvent, StromMessage, StromNetworkEvent,
    StromNetworkHandle, StromNetworkManager
};
use angstrom_types::{
    block_sync::GlobalBlockSync,
    primitive::PeerId,
    sol_bindings::{grouped_orders::AllOrders, testnet::random::RandomValues},
    testnet::InitialTestnetState
};
use consensus::{AngstromValidator, ConsensusManager};
use futures::{StreamExt, TryFutureExt};
use matching_engine::manager::MatcherHandle;
use parking_lot::RwLock;
use rand::Rng;
use reth_chainspec::Hardforks;
use reth_metrics::common::mpsc::{
    metered_unbounded_channel, UnboundedMeteredReceiver, UnboundedMeteredSender
};
use reth_network::{
    test_utils::{Peer, PeerHandle},
    NetworkHandle, NetworkInfo, Peers
};
use reth_network_peers::pk2id;
use reth_provider::{test_utils::NoopProvider, BlockReader, ChainSpecProvider, HeaderProvider};
use tokio_stream::wrappers::{BroadcastStream, UnboundedReceiverStream};
use tracing::{span, Instrument, Level};

use super::AngstromTestnetNode;
use crate::{
    anvil_state_provider::{
        utils::async_to_sync, AnvilInitializer, AnvilStateProviderWrapper, TestnetBlockProvider
    },
    controllers::{
        strom::{initialize_new_node, AngstromDevnetNodeInternals, TestnetNode},
        TestnetStateFutureLock
    },
    network::{EthPeerPool, TestnetNodeNetwork},
    types::TestingConfig
};

impl
    AngstromTestnetNode<
        NoopProvider,
        AngstromDevnetNodeInternals,
        TestnetStateFutureLock<NoopProvider, PubSubFrontend>
    >
{
    pub async fn new_devnet(
        testnet_node_id: u64,
        network: TestnetNodeNetwork,
        strom_network_manager: StromNetworkManager<NoopProvider>,
        eth_peer: Peer<NoopProvider>,
        strom_handles: StromHandles,
        config: impl TestingConfig,
        initial_validators: Vec<AngstromValidator>,
        block_rx: BroadcastStream<(u64, Vec<Transaction>)>,
        inital_angstrom_state: InitialTestnetState
    ) -> eyre::Result<Self> {
        let (strom, consensus) = AngstromDevnetNodeInternals::new(
            testnet_node_id,
            strom_handles,
            network.strom_handle.network_handle().clone(),
            network.secret_key,
            config,
            initial_validators,
            block_rx,
            inital_angstrom_state
        )
        .await?;

        tracing::debug!("created strom internals");

        let state_lock = TestnetStateFutureLock::new(
            testnet_node_id,
            eth_peer,
            strom_network_manager,
            consensus
        );

        Ok(Self::new(testnet_node_id, network, strom, state_lock))
    }

    /// General
    /// -------------------------------------

    pub fn testnet_node_id(&self) -> u64 {
        self.testnet_node_id
    }

    pub fn peer_id(&self) -> PeerId {
        *self.eth_network_handle().peer_id()
    }

    pub fn state_provider(&self) -> &AnvilStateProviderWrapper {
        &self.strom.state_provider
    }

    /// Eth
    /// -------------------------------------
    pub fn eth_peer_handle(&self) -> &PeerHandle<EthPeerPool> {
        self.network.eth_handle.peer_handle()
    }

    pub fn eth_network_handle(&self) -> &NetworkHandle {
        self.network.eth_handle.network_handle()
    }

    pub fn connect_to_eth_peer(&self, id: PeerId, addr: SocketAddr) {
        self.eth_network_handle().add_peer(id, addr);
    }

    pub fn eth_socket_addr(&self) -> SocketAddr {
        self.eth_network_handle().local_addr()
    }

    /// Angstrom
    /// -------------------------------------
    pub fn strom_network_handle(&self) -> &StromNetworkHandle {
        self.network.strom_handle.network_handle()
    }

    pub fn strom_validator_set(&self) -> Arc<RwLock<HashSet<Address>>> {
        self.network.strom_handle.validator_set()
    }

    pub fn disconnect_strom_peer(&self, id: PeerId) {
        self.network.strom_handle.disconnect_peer(id);
    }

    pub fn strom_peer_count(&self) -> usize {
        self.network.strom_handle.peer_count()
    }

    pub fn remove_strom_validator(&self, id: PeerId) {
        self.network.strom_handle.remove_validator(id);
    }

    pub fn add_strom_validator(&self, id: PeerId) {
        self.network.strom_handle.add_validator(id);
    }

    pub fn subscribe_strom_network_events(&self) -> UnboundedReceiverStream<StromNetworkEvent> {
        self.network.strom_handle.subscribe_network_events()
    }

    /// Network
    /// -------------------------------------
    pub fn strom_network_manager<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&StromNetworkManager<NoopProvider>) -> R
    {
        self.state_lock.strom_network_manager(f)
    }

    pub fn strom_network_manager_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut StromNetworkManager<NoopProvider>) -> R
    {
        self.state_lock.strom_network_manager_mut(f)
    }

    pub fn eth_peer<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Peer<NoopProvider>) -> R
    {
        self.state_lock.eth_peer(f)
    }

    pub fn eth_peer_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Peer<NoopProvider>) -> R
    {
        self.state_lock.eth_peer_mut(f)
    }

    pub fn start_network(&self) {
        self.state_lock.set_network(true);
    }

    pub fn stop_network(&self) {
        self.state_lock.set_network(false);
    }

    pub fn is_network_on(&self) -> bool {
        self.state_lock.network_state()
    }

    pub fn is_network_off(&self) -> bool {
        !self.state_lock.network_state()
    }

    pub fn start_network_and_consensus(&self) {
        self.start_network();
        self.start_conensus();
    }

    pub fn stop_network_and_consensus(&self) {
        self.stop_network();
        self.stop_consensus();
    }

    /// Consensus
    /// -------------------------------------
    pub fn strom_consensus<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ConsensusManager<PubSubFrontend, MatcherHandle, GlobalBlockSync>) -> R
    {
        self.state_lock.strom_consensus(f)
    }

    pub fn strom_consensus_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ConsensusManager<PubSubFrontend, MatcherHandle, GlobalBlockSync>) -> R
    {
        self.state_lock.strom_consensus_mut(f)
    }

    pub fn start_conensus(&self) {
        self.state_lock.set_consensus(true, false);
    }

    pub fn stop_consensus(&self) {
        self.state_lock.set_consensus(false, false);
    }

    pub fn is_consensus_on(&self) -> bool {
        self.state_lock.consensus_state()
    }

    pub fn is_consensus_off(&self) -> bool {
        !self.state_lock.consensus_state()
    }

    /// Testing Utils
    /// -------------------------------------

    fn add_validator_bidirectional(&mut self, other: &Self) {
        self.add_strom_validator(other.network.pubkey());
        other.add_strom_validator(self.network.pubkey());
    }

    pub async fn connect_to_all_peers(&mut self, other_peers: &mut HashMap<u64, Self>) {
        self.start_network();
        other_peers.iter().for_each(|(_, peer)| {
            self.connect_to_eth_peer(peer.network.pubkey(), peer.eth_socket_addr());

            self.add_validator_bidirectional(peer);
        });

        let connections_expected = other_peers.len();
        self.initialize_internal_connections(connections_expected)
            .await;
    }

    pub fn pre_post_network_event_channel_swap<E>(
        &mut self,
        is_pre_event: bool,
        f: impl FnOnce(&mut StromNetworkManager<NoopProvider>) -> Option<UnboundedMeteredSender<E>>
    ) -> UnboundedMeteredSender<E> {
        if is_pre_event {
            self.stop_network();
        } else {
            self.start_network();
        }

        self.strom_network_manager_mut(f)
            .expect("old network event channel is empty")
    }

    pub fn send_bundles_to_network(&self, peer_id: PeerId, bundles: usize) -> eyre::Result<()> {
        let orders = AllOrders::gen_many(bundles);
        let num_orders = orders.len();
        tracing::debug!("submitting a angstrom bundle with {num_orders} orders to the network");

        self.strom
            .tx_strom_handles
            .network_tx
            .send(NetworkOrderEvent::IncomingOrders { peer_id, orders })?;

        tracing::info!("sent {num_orders} bundles to the network");

        Ok(())
    }

    pub(crate) async fn initialize_internal_connections(&mut self, connections_needed: usize) {
        tracing::debug!(pubkey = ?self.network.pubkey, "attempting connections to {connections_needed} peers");
        let mut last_peer_count = 0;
        std::future::poll_fn(|cx| loop {
            if self
                .state_lock
                .poll_fut_to_initialize_network_connections(cx)
                .is_ready()
            {
                panic!("peer connection failed");
            }

            let peer_cnt = self.network.strom_handle.peer_count();
            if last_peer_count != peer_cnt {
                tracing::trace!("connected to {peer_cnt}/{connections_needed} peers");
                last_peer_count = peer_cnt;
            }

            if connections_needed == peer_cnt {
                return Poll::Ready(())
            }
        })
        .await
    }

    pub(crate) async fn testnet_future(self) {
        self.start_network_and_consensus();
        self.state_lock.await;
    }
}
