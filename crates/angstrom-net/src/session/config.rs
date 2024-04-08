//! Configuration types for
//! [StromSessionManager](crate::session::StromSessionManager).

use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc
    },
    time::Duration
};

use reth_network::Direction;

/// Default request timeout for a single request.
pub const INITIAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Default timeout after which we'll consider the peer to be in violation of
/// the protocol.
///
/// This is the time a peer has to answer a response.
pub const MAX_STROM_INBOUND_PEERS: u32 = 25;
pub const MAX_STROM_OUTBOUND_PEERS: u32 = 25;

pub const PROTOCOL_BREACH_REQUEST_TIMEOUT: Duration = Duration::from_secs(2 * 60);

/// Configuration options when creating a
/// [SessionManager](crate::session::SessionManager).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct SessionsConfig {
    /// Size of the session command buffer (per session task).
    pub session_command_buffer: usize,
    /// Size of the session event channel buffer.
    pub session_event_buffer: usize,
    /// Limits to enforce.
    pub limits: SessionLimits,
    pub protocol_breach_request_timeout: Duration
}

#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionLimits {
    /// Limits to enforce.
    max_inbound:  u32,
    max_outbound: u32
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self { max_inbound: MAX_STROM_INBOUND_PEERS, max_outbound: MAX_STROM_OUTBOUND_PEERS }
    }
}

impl Default for SessionsConfig {
    fn default() -> Self {
        SessionsConfig {
            // This should be sufficient to slots for handling commands sent to the session task,
            // since the manager is the sender.
            session_command_buffer: 32,
            // This should be greater since the manager is the receiver. The total size will be
            // `buffer + num sessions`. Each session can therefore fit at least 1 message in the
            // channel. The buffer size is additional capacity. The channel is always drained on
            // `poll`.
            // The default is twice the maximum number of available slots, if all slots are
            // occupied the buffer will have capacity for 3 messages per session
            // (average).
            session_event_buffer: ((MAX_STROM_INBOUND_PEERS + MAX_STROM_OUTBOUND_PEERS) * 2)
                as usize,
            limits: SessionLimits::default(),
            protocol_breach_request_timeout: PROTOCOL_BREACH_REQUEST_TIMEOUT
        }
    }
}

impl SessionsConfig {
    /// Sets the buffer size for the bounded communication channel between the
    /// manager and its sessions for events emitted by the sessions.
    ///
    /// It is expected, that the background session task will stall if they
    /// outpace the manager. The buffer size provides backpressure on the
    /// network I/O.
    pub fn with_session_event_buffer(mut self, n: usize) -> Self {
        self.session_event_buffer = n;
        self
    }
}

/// Keeps track of all sessions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionCounter {
    /// Number of active inbound sessions.
    active_inbound:  Arc<AtomicU32>,
    /// Number of active outbound sessions.
    active_outbound: Arc<AtomicU32>
}

/// The error thrown when the max configured limit has been reached and no more
/// connections are accepted.
#[derive(Debug, Clone, thiserror::Error)]
#[error("session limit reached {0}")]
pub struct ExceedsSessionLimit(pub(crate) u32);

// === impl SessionCounter ===

impl SessionCounter {
    #[allow(dead_code)]
    pub(crate) fn new(_limits: SessionLimits) -> Self {
        Self {
            active_inbound:  Arc::new(AtomicU32::new(0)),
            active_outbound: Arc::new(AtomicU32::new(0))
        }
    }

    #[allow(dead_code)]
    pub(crate) fn inc_active(&self, direction: &Direction) {
        match direction {
            Direction::Outgoing(_) => {
                self.active_outbound.fetch_add(1, Ordering::SeqCst);
            }
            Direction::Incoming => {
                self.active_inbound.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn dec_active(&self, direction: &Direction) {
        match direction {
            Direction::Outgoing(_) => {
                self.active_outbound.fetch_sub(1, Ordering::SeqCst);
            }
            Direction::Incoming => {
                self.active_inbound.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }

    #[allow(dead_code)]
    fn ensure(current: u32, limit: Option<u32>) -> Result<(), ExceedsSessionLimit> {
        if let Some(limit) = limit {
            if current >= limit {
                return Err(ExceedsSessionLimit(limit))
            }
        }
        Ok(())
    }
}
