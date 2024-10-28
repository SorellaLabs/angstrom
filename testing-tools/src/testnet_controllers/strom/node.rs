use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc
};

use alloy::sol_types::SolValue;
use alloy_primitives::Address;
use angstrom::cli::StromHandles;
use angstrom_network::{
    NetworkOrderEvent, StromNetworkEvent, StromNetworkHandle, StromNetworkManager
};
use angstrom_types::{
    primitive::PeerId,
    sol_bindings::{grouped_orders::AllOrders, sol::ContractBundle, testnet::random::RandomValues}
};
use consensus::AngstromValidator;
use parking_lot::RwLock;
use reth_chainspec::Hardforks;
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use reth_network::{
    test_utils::{Peer, PeerHandle},
    NetworkHandle, NetworkInfo, Peers
};
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider};
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::strom_internals::AngstromTestnetNodeInternals;
use crate::{
    anvil_state_provider::RpcStateProviderFactoryWrapper,
    network::{EthPeerPool, TestnetNodeNetwork},
    testnet_controllers::AngstromTestnetConfig
};

pub struct TestnetNode<C> {
    _testnet_node_id: u64,
    network:          TestnetNodeNetwork<C>,
    strom:            AngstromTestnetNodeInternals
}

impl<C> TestnetNode<C>
where
    C: BlockReader
        + HeaderProvider
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static
{
    pub async fn new(
        _testnet_node_id: u64,
        network: TestnetNodeNetwork<C>,
        strom_handles: StromHandles,
        config: AngstromTestnetConfig,
        initial_validators: Vec<AngstromValidator>
    ) -> eyre::Result<Self> {
        let strom = AngstromTestnetNodeInternals::new(
            _testnet_node_id,
            strom_handles,
            network.strom_handle.network_handle().clone(),
            network.secret_key,
            config,
            initial_validators
        )
        .await?;

        Ok(Self { _testnet_node_id, network, strom })
    }

    /// General
    /// -------------------------------------
    pub fn peer_id(&self) -> PeerId {
        *self.eth_network_handle().peer_id()
    }

    pub fn state_provider(&self) -> &RpcStateProviderFactoryWrapper {
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
        self.network.networks.strom_network(f)
    }

    pub fn strom_network_manager_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut StromNetworkManager<C>) -> R
    {
        self.network.networks.strom_network_mut(f)
    }

    pub fn eth_peer<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Peer<C>) -> R
    {
        self.network.networks.eth_peer(f)
    }

    pub fn eth_peer_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Peer<C>) -> R
    {
        self.network.networks.eth_peer_mut(f)
    }

    pub fn start_network(&self, blocking: bool) {
        if blocking {
            self.network.blocking_start_network()
        } else {
            self.network.start_network()
        }
    }

    pub fn stop_network(&self, blocking: bool) {
        if blocking {
            self.network.blocking_stop_network()
        } else {
            self.network.stop_network()
        }
    }

    pub fn is_network_on(&self) -> bool {
        self.network.is_network_on()
    }

    pub fn is_network_off(&self) -> bool {
        self.network.is_network_off()
    }

    /// Testing Utils
    /// -------------------------------------

    fn add_validator_bidirectional(&mut self, other: &Self) {
        self.add_strom_validator(other.network.pubkey());
        other.add_strom_validator(self.network.pubkey());
    }

    pub async fn connect_to_all_peers(&mut self, other_peers: &mut HashMap<u64, Self>) {
        self.network.start_network();
        other_peers.iter().for_each(|(_, peer)| {
            self.connect_to_eth_peer(peer.network.pubkey(), peer.eth_socket_addr());

            self.add_validator_bidirectional(peer);
        });

        let connections_expected = other_peers.len();
        self.network
            .initialize_connections(connections_expected)
            .await;
    }

    pub fn pre_post_network_event_channel_swap<E>(
        &mut self,
        is_pre_event: bool,
        f: impl FnOnce(&mut StromNetworkManager<C>) -> Option<UnboundedMeteredSender<E>>
    ) -> UnboundedMeteredSender<E> {
        if is_pre_event {
            self.network.blocking_stop_network();
        } else {
            self.network.blocking_start_network();
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

    pub async fn execute_bundles_locally(&self) -> eyre::Result<()> {
        let orders = ContractBundle::gen();
        let hashes = orders.get_filled_hashes();
        tracing::debug!("executing a angstrom bundle with hashes: {:#?}", hashes);

        let tx_hash = self
            .strom
            .testnet_hub
            .execute(orders.abi_encode().into())
            .send()
            .await?
            .watch()
            .await?;

        tracing::debug!(?tx_hash, "tx hash with angstrom contract sent");

        Ok(())
    }
}
