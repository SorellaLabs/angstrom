use super::TestingConfigKind;
use crate::types::{GlobalTestingConfig, initial_state::InitialStateConfig};

#[derive(Debug, Clone)]
pub struct OpTestnetConfig {
    pub eth_ws_url:           String,
    pub initial_state_config: InitialStateConfig,
    seed:                     u16,
    angstrom_base_rpc_port:   u16
}

impl OpTestnetConfig {
    pub fn new(
        eth_ws_url: impl ToString,
        angstrom_base_rpc_port: Option<u16>,
        initial_state_config: InitialStateConfig
    ) -> Self {
        let base_port = angstrom_base_rpc_port.unwrap_or_else(rand::random);
        tracing::info!("OpTestnetConfig: using RPC base port {}", base_port);
        Self {
            eth_ws_url: eth_ws_url.to_string(),
            seed: rand::random(),
            angstrom_base_rpc_port: base_port,
            initial_state_config
        }
    }
}

impl GlobalTestingConfig for OpTestnetConfig {
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

    fn use_testnet(&self) -> bool {
        true
    }

    fn is_leader(&self, node_id: u64) -> bool {
        node_id == 0
    }

    fn base_angstrom_rpc_port(&self) -> u16 {
        self.angstrom_base_rpc_port
    }

    fn initial_state_config(&self) -> InitialStateConfig {
        self.initial_state_config.clone()
    }

    fn node_count(&self) -> u64 {
        1
    }

    fn leader_eth_rpc_port(&self) -> u16 {
        self.base_angstrom_rpc_port() + 1
    }
}
