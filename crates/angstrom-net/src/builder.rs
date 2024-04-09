//! Builder structs for messages.

use std::{
    collections::HashSet,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH}
};

use futures::FutureExt;
use parking_lot::RwLock;
use reth_metrics::common::mpsc::{MeteredPollSender, UnboundedMeteredSender};
use reth_primitives::{
    alloy_primitives::FixedBytes, keccak256, Address, BufMut, BytesMut, Chain, PeerId
};
use reth_tasks::TaskSpawner;
use secp256k1::{Message, SecretKey, SECP256K1};
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::sync::PollSender;

use crate::{
    manager::StromConsensusEvent, state::StromState, types::status::StatusState, NetworkOrderEvent,
    Status, StromNetworkHandle, StromNetworkHandleMsg, StromNetworkManager, StromProtocolHandler,
    StromSessionManager, StromSessionMessage, Swarm, VerificationSidecar
};

pub struct NetworkBuilder {
    to_pool_manager:      Option<UnboundedMeteredSender<NetworkOrderEvent>>,
    to_consensus_manager: Option<UnboundedMeteredSender<StromConsensusEvent>>,
    session_manager_rx:   Option<Receiver<StromSessionMessage>>,

    validator_set: Arc<RwLock<HashSet<Address>>>,
    verification:  VerificationSidecar
}

impl NetworkBuilder {
    pub fn new(verification: VerificationSidecar) -> Self {
        Self {
            verification,
            to_pool_manager: None,
            to_consensus_manager: None,
            session_manager_rx: None,

            validator_set: Default::default()
        }
    }

    pub fn with_consensus_manager(
        mut self,
        tx: UnboundedMeteredSender<StromConsensusEvent>
    ) -> Self {
        self.to_consensus_manager = Some(tx);
        self
    }

    pub fn with_pool_manager(mut self, tx: UnboundedMeteredSender<NetworkOrderEvent>) -> Self {
        self.to_pool_manager = Some(tx);
        self
    }

    pub fn with_validator_set(mut self, validator_set: Arc<RwLock<HashSet<Address>>>) -> Self {
        self.validator_set = validator_set;
        self
    }

    pub fn build_protocol_handler(&mut self) -> StromProtocolHandler {
        let (session_manager_tx, session_manager_rx) = tokio::sync::mpsc::channel(100);
        let protocol = StromProtocolHandler::new(
            MeteredPollSender::new(PollSender::new(session_manager_tx), "session manager"),
            self.verification.clone(),
            self.validator_set.clone()
        );
        self.session_manager_rx = Some(session_manager_rx);

        protocol
    }

    /// builds the network spawning it on its own thread, returning the
    /// communication channel along with returning the protocol it
    /// represents.
    pub fn build_handle<TP: TaskSpawner, DB: Send + Unpin + 'static>(
        mut self,
        tp: TP,
        db: DB
    ) -> StromNetworkHandle {
        let state = StromState::new(db, self.validator_set.clone());
        let sessions = StromSessionManager::new(self.session_manager_rx.take().unwrap());
        let swarm = Swarm::new(sessions, state);

        let network =
            StromNetworkManager::new(swarm, self.to_pool_manager, self.to_consensus_manager);

        let handle = network.get_handle();
        tp.spawn_critical("strom network", network.boxed());

        handle
    }
}

/// Builder for [`Status`] messages.
#[derive(Debug)]
pub struct StatusBuilder {
    state: StatusState
}

impl StatusBuilder {
    pub fn new(peer: PeerId) -> StatusBuilder {
        Self { state: StatusState::new(peer) }
    }

    /// Consumes the type and creates the actual [`Status`] message, Signing the
    /// payload
    pub fn build(mut self, key: SecretKey) -> Status {
        // set state timestamp to now;
        self.state.timestamp_now();

        let message = self.state.to_message();
        let sig = reth_primitives::sign_message(FixedBytes(key.secret_bytes()), message).unwrap();

        Status { state: self.state, signature: angstrom_types::primitive::Signature(sig) }
    }

    /// Sets the protocol version.
    pub fn version(mut self, version: u8) -> Self {
        self.state.version = version;
        self
    }

    /// Sets the chain id.
    pub fn chain(mut self, chain: Chain) -> Self {
        self.state.chain = chain;
        self
    }
}

impl From<StatusState> for StatusBuilder {
    fn from(value: StatusState) -> Self {
        Self { state: value }
    }
}
