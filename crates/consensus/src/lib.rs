#![feature(result_option_inspect)]
mod core;
mod evidence;
mod manager;
mod round;
mod round_robin_algo;
mod signer;

use std::pin::Pin;

use common::ConsensusState;
use futures::Stream;
pub use manager::*;

pub trait ConsensusListener: Send + Sync + 'static {
    fn subscribe_transitions(&self) -> Pin<Box<dyn Stream<Item = ConsensusState>>>;
}

pub trait ConsensusUpdater: Send + Sync + 'static {
    fn new_block(&self, block: ());
}
