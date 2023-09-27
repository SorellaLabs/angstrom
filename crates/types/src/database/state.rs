use bytes::Bytes;
use reth_codecs::{main_codec, Compact};
use serde::{Deserialize, Serialize};

use super::BlockId;
use crate::consensus::{Block, GuardSet, Time};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct State {
    // basic info
    pub chain_id:          u64,
    pub inital_height:     u64,
    pub last_block_height: u64,
    pub last_block_id:     BlockId,
    pub last_block_time:   Time,

    // guard info
    pub next_guards:                GuardSet,
    pub guards:                     GuardSet,
    pub last_guards:                GuardSet,
    pub last_height_guards_changed: u64,

    pub consensus_params:                     u8,
    pub last_height_consensus_params_changed: u64,

    /// Merkle root of the results from executing prev block
    pub last_root: Bytes,
    pub app_hash:  Bytes
}

impl State {
    /// moves to the new block state. returns old state.
    pub fn transition(&mut self, block: Block) -> State {
        todo!()
    }
}
