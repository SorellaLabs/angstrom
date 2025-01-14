use super::TestingConfigKind;
use crate::types::{initial_state::PartialConfigPoolKey, GlobalTestingConfig};

#[derive(Debug, Clone)]
pub struct TestnetConfig {
    pub node_count: u64,
    pub pool_keys:  Vec<PartialConfigPoolKey>,
    /// only the leader can have this
    pub eth_ws_url: String,
    pub mev_guard:  bool,
    seed:           u16
}

impl TestnetConfig {
    pub fn new(
        node_count: u64,
        pool_keys: Vec<PartialConfigPoolKey>,
        eth_ws_url: impl ToString,
        mev_guard: bool
    ) -> Self {
        Self {
            node_count,
            pool_keys,
            eth_ws_url: eth_ws_url.to_string(),
            mev_guard,
            seed: rand::random()
        }
    }
}

impl GlobalTestingConfig for TestnetConfig {
    fn eth_ws_url(&self) -> String {
        self.eth_ws_url.clone()
    }

    fn fork_config(&self) -> Option<(u64, String)> {
        Some((0, self.eth_ws_url.clone()))
    }

    fn config_type(&self) -> TestingConfigKind {
        TestingConfigKind::Testnet
    }

    fn anvil_rpc_endpoint(&self, _: u64) -> String {
        format!("/tmp/testnet_anvil_{}.ipc", self.seed)
    }

    fn is_leader(&self, node_id: u64) -> bool {
        node_id == 0
    }

    fn node_count(&self) -> u64 {
        self.node_count
    }

    fn pool_keys(&self) -> Vec<PartialConfigPoolKey> {
        self.pool_keys.clone()
    }
}
