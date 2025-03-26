use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, atomic::AtomicUsize},
    task::{Context, Poll}
};

use alloy::{
    hex,
    primitives::{Address, BlockNumber, FixedBytes}
};
use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    consensus::{PreProposal, PreProposalAggregation, Proposal},
    primitive::PeerId
};
use futures::StreamExt;
use once_cell::unsync::Lazy;
use reth_eth_wire::DisconnectReason;
use reth_metrics::common::mpsc::UnboundedMeteredSender;
use reth_network::Peers;
use secp256k1::PublicKey;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::error;

use crate::{
    CachedPeer, NetworkOrderEvent, StromMessage, StromNetworkHandleMsg, Swarm, SwarmEvent
};
#[allow(unused_imports)]
use crate::{StromNetworkConfig, StromNetworkHandle, StromSessionManager};

// use a thread local lazy to avoid synchronization overhead since path is
// always the same
thread_local! {
    static CACHED_PEERS_TOML_PATH: Lazy<PathBuf> = Lazy::new(|| {
        let mut path = PathBuf::new();
        path.push(
            homedir::my_home()
                .unwrap()
                .expect("Failed to get home directory. Please set the HOME environment variable.")
        );
        path.push(".angstrom_cached_peers.toml");
        path
    });
}

#[allow(dead_code)]
pub struct StromNetworkManager<DB: Unpin, P: Peers + Unpin> {
    handle: StromNetworkHandle,

    from_handle_rx:       UnboundedReceiverStream<StromNetworkHandleMsg>,
    to_pool_manager:      Option<UnboundedMeteredSender<NetworkOrderEvent>>,
    to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>,
    eth_handle:           UnboundedReceiver<EthEvent>,

    event_listeners:  Vec<UnboundedSender<StromNetworkEvent>>,
    swarm:            Swarm<DB>,
    /// This is updated via internal events and shared via `Arc` with the
    /// [`NetworkHandle`] Updated by the `NetworkWorker` and loaded by the
    /// `NetworkService`.
    num_active_peers: Arc<AtomicUsize>,
    reth_network:     P
}

impl<DB: Unpin, P: Peers + Unpin> Drop for StromNetworkManager<DB, P> {
    fn drop(&mut self) {
        let node_pubkey = self
            .reth_network
            .local_node_record()
            .id
            .to_pubkey()
            .unwrap();

        tracing::info!("StromNetworkManager for node_id={} shutting down...", node_pubkey);
        self.save_known_peers();
        tracing::info!("Saved {} known peers", self.swarm().sessions().active_sessions.len());
    }
}

impl<DB: Unpin, P: Peers + Unpin> StromNetworkManager<DB, P> {
    pub fn new(
        swarm: Swarm<DB>,
        eth_handle: UnboundedReceiver<EthEvent>,
        to_pool_manager: Option<UnboundedMeteredSender<NetworkOrderEvent>>,
        to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>,
        reth_network: P
    ) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let node_pubkey = reth_network.local_node_record().id.to_pubkey().unwrap();
        tracing::info!("StromNetworkManager for node_id={} starting...", node_pubkey);

        // Load cached peers
        tracing::info!("Currently connected to {} peers", reth_network.num_connected_peers());
        let cached_peers = Self::load_known_peers();
        tracing::info!("Connecting to {} cached peers...", cached_peers.len());
        for peer in cached_peers {
            tracing::info!("Reconnecting to cached peer {}", peer.enr());
            reth_network.connect_peer(peer.peer_id, peer.addr);
        }
        tracing::info!("Now connected to {} peers", reth_network.num_connected_peers());

        let peers = Arc::new(AtomicUsize::default());
        let handle =
            StromNetworkHandle::new(peers.clone(), UnboundedMeteredSender::new(tx, "strom handle"));

        Self {
            handle: handle.clone(),
            eth_handle,
            num_active_peers: peers,
            swarm,
            from_handle_rx: rx.into(),
            to_pool_manager,
            to_consensus_manager,
            event_listeners: Vec::new(),
            reth_network
        }
    }

    pub fn load_known_peers() -> Vec<CachedPeer> {
        CACHED_PEERS_TOML_PATH.with(|toml_path| {
            match std::fs::read_to_string(toml_path.as_path()) {
                Ok(data) => toml::from_str(&data).unwrap_or_default(),
                Err(_) => {
                    tracing::warn!(
                        "Known peers file not found at {}, creating peers list from scratch",
                        toml_path.display()
                    );
                    Vec::new()
                }
            }
        })
    }

    /// saves known peers at this point to the [`CACHED_PEERS_TOML_PATH`] file.
    ///
    /// This is only mutable because there is immutable version of
    /// `swarm.sessions_mut`.
    pub fn save_known_peers(&self) {
        let peers = self
            .swarm
            .sessions()
            .active_sessions
            .values()
            .map(|session| CachedPeer { peer_id: session.remote_id, addr: session.socket_addr })
            .collect::<Vec<_>>();
        CACHED_PEERS_TOML_PATH.with(|toml_path| match toml::to_string(&peers) {
            Ok(serialized) => {
                if let Err(err) = std::fs::write(toml_path.as_path(), serialized) {
                    tracing::error!(
                        "Failed to save known peers to {}: {}",
                        toml_path.display(),
                        err
                    );
                }
            }
            Err(err) => {
                tracing::error!("Failed to serialize known peers to TOML: {}", err);
            }
        });
    }

    pub fn install_consensus_manager(&mut self, tx: UnboundedMeteredSender<StromConsensusEvent>) {
        self.to_consensus_manager = Some(tx);
    }

    pub fn remove_consensus_manager(&mut self) {
        self.to_consensus_manager.take();
    }

    pub fn swap_consensus_manager(
        &mut self,
        tx: UnboundedMeteredSender<StromConsensusEvent>
    ) -> Option<UnboundedMeteredSender<StromConsensusEvent>> {
        let mut other = Some(tx);
        std::mem::swap(&mut self.to_consensus_manager, &mut other);
        other
    }

    pub fn install_pool_manager(&mut self, tx: UnboundedMeteredSender<NetworkOrderEvent>) {
        self.to_pool_manager = Some(tx);
    }

    pub fn remove_pool_manager(&mut self) {
        self.to_pool_manager.take();
    }

    pub fn swap_pool_manager(
        &mut self,
        tx: UnboundedMeteredSender<NetworkOrderEvent>
    ) -> Option<UnboundedMeteredSender<NetworkOrderEvent>> {
        let mut other = Some(tx);
        std::mem::swap(&mut self.to_pool_manager, &mut other);
        other
    }

    pub fn swarm_mut(&mut self) -> &mut Swarm<DB> {
        &mut self.swarm
    }

    pub fn swarm(&self) -> &Swarm<DB> {
        &self.swarm
    }

    pub fn get_handle(&self) -> StromNetworkHandle {
        self.handle.clone()
    }

    // Handler for received messages from a handle
    fn on_handle_message(&mut self, msg: StromNetworkHandleMsg) {
        tracing::trace!(?msg, "received network message");
        match msg {
            StromNetworkHandleMsg::SubscribeEvents(tx) => self.event_listeners.push(tx),
            StromNetworkHandleMsg::SendStromMessage { peer_id, msg } => {
                self.swarm.sessions_mut().send_message(&peer_id, msg)
            }
            StromNetworkHandleMsg::Shutdown(tx) => {
                self.save_known_peers();

                self.swarm
                    .sessions_mut()
                    .disconnect_all(Some(DisconnectReason::ClientQuitting));
                let _ = tx.send(());
            }
            StromNetworkHandleMsg::RemovePeer(peer_id) => {
                self.swarm.state_mut().peers_mut().remove_peer(peer_id);
            }
            StromNetworkHandleMsg::ReputationChange(peer_id, kind) => self
                .swarm
                .state_mut()
                .peers_mut()
                .change_weight(peer_id, kind),
            StromNetworkHandleMsg::BroadcastStromMessage { msg } => {
                self.swarm_mut().sessions_mut().broadcast_message(msg);
            }
            StromNetworkHandleMsg::DisconnectPeer(id, reason) => {
                self.swarm_mut().sessions_mut().disconnect(id, reason);
            }
        }
    }

    fn notify_listeners(&mut self, event: StromNetworkEvent) {
        self.event_listeners
            .retain(|tx| tx.send(event.clone()).is_ok());
    }
}

impl<DB: Unpin, P: Peers + Unpin> Future for StromNetworkManager<DB, P> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // process incoming messages from a handle
        let mut work = 30;
        loop {
            work -= 1;
            if work == 0 {
                cx.waker().wake_by_ref();
                break;
            }

            match self.from_handle_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(msg)) => self.on_handle_message(msg),
                Poll::Ready(None) => {
                    // This is only possible if the channel was deliberately closed since we always
                    // have an instance of `NetworkHandle`
                    error!("Strom network message channel closed.");
                    return Poll::Ready(());
                }
                _ => {}
            };

            // make sure we add and remove validators properly
            if let Poll::Ready(Some(eth_event)) = self.eth_handle.poll_recv(cx) {
                match eth_event {
                    EthEvent::AddedNode(addr) => {
                        self.swarm().state().add_validator(addr);
                    }
                    EthEvent::RemovedNode(addr) => {
                        self.swarm_mut().state_mut().remove_validator(addr);
                    }
                    _ => {}
                }
            }

            if let Poll::Ready(Some(event)) = self.swarm.poll_next_unpin(cx) {
                match event {
                    SwarmEvent::ValidMessage { peer_id, msg } => {
                        let address =
                            Address::try_from(&alloy::primitives::keccak256(peer_id)[12..])
                                .unwrap();

                        match msg {
                            StromMessage::PrePropose(p) => {
                                self.to_consensus_manager.as_ref().inspect(|tx| {
                                    let _ = tx.send(StromConsensusEvent::PreProposal(address, p));
                                });
                            }
                            StromMessage::PreProposeAgg(p) => {
                                self.to_consensus_manager.as_ref().inspect(|tx| {
                                    let _ =
                                        tx.send(StromConsensusEvent::PreProposalAgg(address, p));
                                });
                            }
                            StromMessage::Propose(a) => {
                                self.to_consensus_manager.as_ref().inspect(|tx| {
                                    let _ = tx.send(StromConsensusEvent::Proposal(address, a));
                                });
                            }
                            StromMessage::PropagatePooledOrders(a) => {
                                self.to_pool_manager.as_ref().inspect(|tx| {
                                    let _ = tx.send(NetworkOrderEvent::IncomingOrders {
                                        peer_id,
                                        orders: a
                                    });
                                });
                            }
                            StromMessage::OrderCancellation(a) => {
                                self.to_pool_manager.as_ref().inspect(|tx| {
                                    let _ = tx.send(NetworkOrderEvent::CancelOrder {
                                        peer_id,
                                        request: a
                                    });
                                });
                            }
                            StromMessage::Status(_) => {}
                        }
                    }
                    SwarmEvent::Disconnected { peer_id } => {
                        self.notify_listeners(StromNetworkEvent::SessionClosed {
                            peer_id,
                            reason: None
                        })
                    }
                    SwarmEvent::SessionEstablished { peer_id } => {
                        self.num_active_peers
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        self.notify_listeners(StromNetworkEvent::SessionEstablished { peer_id })
                    }
                }
            }
        }

        Poll::Pending
    }
}

/// (Non-exhaustive) Events emitted by the network that are of interest for
/// subscribers.
///
/// This includes any event types that may be relevant to tasks, for metrics,
/// keep track of peers etc.
#[derive(Debug, Clone)]
pub enum StromNetworkEvent {
    /// Closed the peer session.
    SessionClosed {
        /// The identifier of the peer to which a session was closed.
        peer_id: PeerId,
        /// Why the disconnect was triggered
        reason:  Option<DisconnectReason>
    },
    /// Established a new session with the given peer.
    SessionEstablished {
        /// The identifier of the peer to which a session was established.
        peer_id: PeerId /* #[cfg(feature = "testnet")]
                         * initial_state: Option<angstrom_types::testnet::InitialTestnetState> */
    },
    /// Event emitted when a new peer is added
    PeerAdded(PeerId),
    /// Event emitted when a new peer is removed
    PeerRemoved(PeerId)
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum StromConsensusEvent {
    PreProposal(Address, PreProposal),
    PreProposalAgg(Address, PreProposalAggregation),
    Proposal(Address, Proposal)
}

impl StromConsensusEvent {
    pub fn message_type(&self) -> &'static str {
        match self {
            StromConsensusEvent::PreProposal(..) => "PreProposal",
            StromConsensusEvent::PreProposalAgg(..) => "PreProposalAggregation",
            StromConsensusEvent::Proposal(..) => "Proposal"
        }
    }

    pub fn sender(&self) -> Address {
        match self {
            StromConsensusEvent::PreProposal(peer_id, _)
            | StromConsensusEvent::Proposal(peer_id, _)
            | StromConsensusEvent::PreProposalAgg(peer_id, _) => *peer_id
        }
    }

    pub fn payload_source(&self) -> PeerId {
        match self {
            StromConsensusEvent::PreProposal(_, pre_proposal) => pre_proposal.source,
            StromConsensusEvent::PreProposalAgg(_, pre_proposal) => pre_proposal.source,
            StromConsensusEvent::Proposal(_, proposal) => proposal.source
        }
    }

    pub fn block_height(&self) -> BlockNumber {
        match self {
            StromConsensusEvent::PreProposal(_, PreProposal { block_height, .. }) => *block_height,
            StromConsensusEvent::PreProposalAgg(_, p) => p.block_height,
            StromConsensusEvent::Proposal(_, Proposal { block_height, .. }) => *block_height
        }
    }
}

impl From<StromConsensusEvent> for StromMessage {
    fn from(event: StromConsensusEvent) -> Self {
        match event {
            StromConsensusEvent::PreProposal(_, pre_proposal) => {
                StromMessage::PrePropose(pre_proposal)
            }
            StromConsensusEvent::PreProposalAgg(_, agg) => StromMessage::PreProposeAgg(agg),

            StromConsensusEvent::Proposal(_, proposal) => StromMessage::Propose(proposal)
        }
    }
}

pub trait ToPubkey {
    fn to_pubkey(&self) -> Result<PublicKey, secp256k1::Error>;
}

impl ToPubkey for FixedBytes<64> {
    fn to_pubkey(&self) -> Result<PublicKey, secp256k1::Error> {
        PublicKey::from_slice(&[0x04].into_iter().chain(self.0).collect::<Vec<_>>())
    }
}
