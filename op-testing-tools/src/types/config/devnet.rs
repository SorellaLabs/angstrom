use super::TestingConfigKind;
use crate::types::{GlobalTestingConfig, initial_state::InitialStateConfig};

#[derive(Debug, Clone)]
pub struct DevnetConfig {
    pub initial_rpc_port:     u16,
    pub fork_block_number:    Option<u64>,
    pub fork_url:             Option<String>,
    pub initial_state_config: InitialStateConfig
}

impl DevnetConfig {
    pub fn new(
        initial_rpc_port: u16,
        fork_block_number: Option<u64>,
        fork_url: Option<String>,
        initial_state_config: InitialStateConfig
    ) -> Self {
        Self { initial_rpc_port, fork_block_number, fork_url, initial_state_config }
    }

    pub fn rpc_port_with_node_id(&self, node_id: Option<u64>) -> u64 {
        if let Some(id) = node_id {
            self.initial_rpc_port as u64 + id
        } else {
            self.initial_rpc_port as u64
        }
    }
}

impl GlobalTestingConfig for DevnetConfig {
    fn eth_ws_url(&self) -> String {
        unreachable!()
    }

    fn fork_config(&self) -> Option<(u64, String)> {
        self.fork_block_number.zip(self.fork_url.clone())
    }

    fn config_type(&self) -> TestingConfigKind {
        TestingConfigKind::Devnet
    }

    fn use_testnet(&self) -> bool {
        false
    }

    fn anvil_rpc_endpoint(&self, node_id: u64) -> String {
        format!("/tmp/anvil_{node_id}.ipc")
    }

    fn is_leader(&self, node_id: u64) -> bool {
        node_id == 0
    }

    fn node_count(&self) -> u64 {
        1
    }

    fn leader_eth_rpc_port(&self) -> u16 {
        unreachable!("only available in Testnet mode");
    }

    fn base_angstrom_rpc_port(&self) -> u16 {
        self.initial_rpc_port
    }

    fn initial_state_config(&self) -> InitialStateConfig {
        self.initial_state_config.clone()
    }
}
