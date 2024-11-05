use angstrom_types::primitive::PeerId;
use reth_network::Direction;
use tokio::{sync::mpsc, time::Instant};

use crate::{session::DisconnectReason, types::message::StromMessage};
/// Commands that can be sent to the spawned session.
//TODO: Create a subvariant of messages only for bidirectional messages received during an active
// session
#[derive(Debug)]
pub enum SessionCommand {
    /// Disconnect the connection
    Disconnect {
        /// The direction of the session, either `Inbound` or `Outgoing`
        reason: Option<DisconnectReason>
    },
    /// Sends a message to the peer
    Message(StromMessage)
}

/// An established session with a remote peer.
#[derive(Debug)]
#[allow(dead_code)]
pub struct StromSessionHandle {
    /// The direction of the session
    pub(crate) direction:           Direction,
    /// The identifier of the remote peer
    pub(crate) remote_id:           PeerId,
    /// The timestamp when the session has been established.
    pub(crate) established:         Instant,
    /// Sender half of the command channel used send commands _to_ the spawned
    /// session
    pub(crate) commands_to_session: mpsc::Sender<SessionCommand>
}

impl StromSessionHandle {
    /// Sends a disconnect command to the session.
    pub fn disconnect(&self, reason: Option<DisconnectReason>) {
        // Note: we clone the sender which ensures the channel has capacity to send the
        // message
        let _ = self
            .commands_to_session
            .clone()
            .try_send(SessionCommand::Disconnect { reason });
    }
}
