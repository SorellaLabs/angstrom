use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::Poll
};

use alloy::pubsub::PubSubFrontend;
use alloy_primitives::Address;
use angstrom::components::initialize_strom_handles;
use angstrom_network::{
    NetworkOrderEvent, StromNetworkEvent, StromNetworkHandle, StromNetworkManager
};
use angstrom_types::{
    block_sync::GlobalBlockSync,
    primitive::PeerId,
    sol_bindings::{grouped_orders::AllOrders, testnet::random::RandomValues},
    testnet::InitialTestnetState
};
use consensus::{AngstromValidator, ConsensusManager};
use futures::Future;
use matching_engine::manager::MatcherHandle;
use parking_lot::RwLock;
use reth_chainspec::Hardforks;
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use reth_network::{
    test_utils::{Peer, PeerHandle},
    NetworkHandle, NetworkInfo, Peers
};
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};
use tokio_stream::wrappers::{BroadcastStream, UnboundedReceiverStream};
use tracing::instrument;

use super::internals::AngstromDevnetNodeInternals;
use crate::{
    agents::AgentConfig,
    contracts::anvil::WalletProviderRpc,
    controllers::TestnetStateFutureLock,
    network::{EthPeerPool, TestnetNodeNetwork},
    providers::AnvilProvider,
    types::{config::TestingNodeConfig, GlobalTestingConfig, WithWalletProvider}
};

pub struct TestnetNode<C, P> {
    testnet_node_id: u64,
    network:         TestnetNodeNetwork,
    strom:           AngstromDevnetNodeInternals<P>,
    state_lock:      TestnetStateFutureLock<C, WalletProviderRpc>
}

impl<C, P> TestnetNode<C, P>
where
    C: BlockReader<Block = reth_primitives::Block>
        + HeaderProvider<Header = reth_primitives::Header>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static,
    P: WithWalletProvider
{
    #[instrument(name = "node", level = "trace", skip(node_config, c, state_provider, initial_validators, inital_angstrom_state, block_provider, agents), fields(id = node_config.node_id))]
    pub async fn new<G: GlobalTestingConfig, F>(
        c: C,
        node_config: TestingNodeConfig<G>,
        state_provider: AnvilProvider<P>,
        initial_validators: Vec<AngstromValidator>,
        inital_angstrom_state: InitialTestnetState,
        block_provider: BroadcastStream<(u64, Vec<alloy_rpc_types::Transaction>)>,
        agents: Vec<F>,
        block_sync: GlobalBlockSync
    ) -> eyre::Result<Self>
    where
        F: for<'a> Fn(
            &'a InitialTestnetState,
            AgentConfig
        ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>,
        F: Clone
    {
        tracing::info!("spawning node");

        let strom_handles = initialize_strom_handles();
        let (strom_network, eth_peer, strom_network_manager) = TestnetNodeNetwork::new(
            c,
            &node_config,
            Some(strom_handles.pool_tx.clone()),
            Some(strom_handles.consensus_tx_op.clone())
        )
        .await;

        let (strom, consensus, validation) = AngstromDevnetNodeInternals::new(
            node_config.clone(),
            state_provider,
            strom_handles,
            strom_network.strom_handle.network_handle().clone(),
            initial_validators,
            block_provider,
            inital_angstrom_state,
            agents,
            block_sync
        )
        .await?;

        tracing::info!("created strom internals");

        let state_lock = TestnetStateFutureLock::new(
            node_config.node_id,
            eth_peer,
            strom_network_manager,
            consensus,
            validation
        );

        Ok(Self { testnet_node_id: node_config.node_id, network: strom_network, strom, state_lock })
    }

    /// General
    /// -------------------------------------
    pub fn node_rpc_url(&self) -> String {
        let port = (4200 + self.testnet_node_id) as u16;
        format!("http://localhost:{port}")
    }

    pub fn testnet_node_id(&self) -> u64 {
        self.testnet_node_id
    }

    pub fn peer_id(&self) -> PeerId {
        *self.eth_network_handle().peer_id()
    }

    pub fn state_provider(&self) -> &AnvilProvider<P> {
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
        F: FnOnce(&StromNetworkManager<C>) -> R
    {
        self.state_lock.strom_network_manager(f)
    }

    pub fn strom_network_manager_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut StromNetworkManager<C>) -> R
    {
        self.state_lock.strom_network_manager_mut(f)
    }

    pub fn eth_peer<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Peer<C>) -> R
    {
        self.state_lock.eth_peer(f)
    }

    pub fn eth_peer_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Peer<C>) -> R
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

    pub fn start_network_and_consensus_and_validation(&self) {
        self.start_network();
        self.start_conensus();
        self.state_lock.set_validation(true);
    }

    pub fn stop_network_and_consensus(&self) {
        self.stop_network();
        self.stop_consensus();
    }

    /// Consensus
    /// -------------------------------------
    pub fn strom_consensus<F, R>(&self, f: F) -> R
    where
        F: FnOnce(
            &ConsensusManager<WalletProviderRpc, PubSubFrontend, MatcherHandle, GlobalBlockSync>
        ) -> R
    {
        self.state_lock.strom_consensus(f)
    }

    pub fn strom_consensus_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(
            &mut ConsensusManager<WalletProviderRpc, PubSubFrontend, MatcherHandle, GlobalBlockSync>
        ) -> R
    {
        self.state_lock.strom_consensus_mut(f)
    }

    pub fn start_conensus(&self) {
        self.state_lock.set_consensus(true);
    }

    pub fn stop_consensus(&self) {
        self.state_lock.set_consensus(false);
    }

    pub fn is_consensus_on(&self) -> bool {
        self.state_lock.consensus_state()
    }

    pub fn is_consensus_off(&self) -> bool {
        !self.state_lock.consensus_state()
    }

    /// Testing Utils
    /// -------------------------------------

    fn add_validator_bidirectional(&self, other: &Self) {
        self.add_strom_validator(other.network.pubkey());
        other.add_strom_validator(self.network.pubkey());
    }

    pub async fn connect_to_all_peers(
        &mut self,
        other_peers: &mut HashMap<u64, TestnetNode<C, P>>
    ) {
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
        f: impl FnOnce(&mut StromNetworkManager<C>) -> Option<UnboundedMeteredSender<E>>
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
        self.start_network_and_consensus_and_validation();
        self.state_lock.await;
    }
}
