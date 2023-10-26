#![feature(result_option_inspect)]
mod core;
mod evidence;
mod manager;
mod round;
mod round_robin_algo;
mod signer;

pub use core::{ConsensusCore, ConsensusMessage};
use std::pin::Pin;

use ethers_core::types::{Block, H256};
use futures::Stream;
pub use manager::*;

/// Listener for consensus data
pub trait ConsensusListener: Send + Sync + 'static {
    /// subscribes to new messages from our consensus
    fn subscribe_messages(&self) -> Pin<Box<dyn Stream<Item = ConsensusMessage>>>;
}
