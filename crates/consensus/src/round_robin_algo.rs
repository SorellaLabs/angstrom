use std::{collections::HashMap, sync::Arc};

use reth_primitives::{Block, B512};
use serde::{Deserialize, Serialize};
#[allow(dead_code)]
const ROUND_ROBIN_CACHE: &str = "./";

/// This gets written to a cache location on your local machine
/// in order to avoid having to resync durning downtime.
#[derive(Debug, Serialize, Deserialize)]
pub struct RoundRobinAlgo {
    /// this is just a placeholder
    angstroms_with_score: HashMap<B512, u64>,
    current_block_height: u64
}

impl RoundRobinAlgo {
    /// also returns what the cache height is up to in order to properly deal
    /// with syncing
    #[allow(dead_code)]
    pub fn new() -> (Self, u64) {
        if let Some(cache) = Self::load_cache() {
            let bh = cache.current_block_height;
            (cache, bh)
        } else {
            (Self { angstroms_with_score: HashMap::new(), current_block_height: 0 }, 0)
        }
    }

    #[allow(dead_code)]
    fn load_cache() -> Option<Self> {
        todo!()
    }

    #[allow(dead_code)]
    /// who the leader is for this round
    pub fn on_new_block(&mut self, _block: Arc<Block>) -> B512 {
        todo!()
    }
}
