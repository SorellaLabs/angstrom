use super::TestingConfigKind;
use crate::types::{GlobalTestingConfig, initial_state::InitialStateConfig};

#[derive(Debug, Clone)]
pub struct TestnetConfig {
    pub node_count:           u64,
    /// only the leader can have this
    pub eth_ws_url:           String,
    pub mev_guard:            bool,
    pub leader_eth_rpc_port:  u16,
    pub initial_state_config: InitialStateConfig,
    seed:                     u16,
    angstrom_base_rpc_port:   u16
}

impl TestnetConfig {
    pub fn new(
        node_count: u64,
        eth_ws_url: impl ToString,
        mev_guard: bool,
        leader_eth_rpc_port: Option<u16>,
        angstrom_base_rpc_port: Option<u16>,
        initial_state_config: InitialStateConfig
    ) -> Self {
        Self {
            node_count,
            eth_ws_url: eth_ws_url.to_string(),
            mev_guard,
            seed: rand::random(),
            leader_eth_rpc_port: leader_eth_rpc_port.unwrap_or_else(rand::random),
            angstrom_base_rpc_port: angstrom_base_rpc_port.unwrap_or_else(rand::random),
            initial_state_config
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

    fn leader_eth_rpc_port(&self) -> u16 {
        self.leader_eth_rpc_port
    }

    fn base_angstrom_rpc_port(&self) -> u16 {
        self.angstrom_base_rpc_port
    }

    fn initial_state_config(&self) -> InitialStateConfig {
        self.initial_state_config.clone()
    }
}
