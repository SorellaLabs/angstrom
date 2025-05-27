use crate::types::GlobalTestingConfig;

#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// A testnet replay will be replayed on a local chain prepared with the
    /// testnet configuration files associated with our runner.  If this is
    /// false, this is a "live" replay that weill be replayed on a specified
    /// chain forked to a local anvil
    testnet_replay: bool,
    eth_ws_url:     String
}

impl ReplayConfig {
    pub fn new(testnet_replay: bool, eth_ws_url: String) -> Self {
        Self { testnet_replay, eth_ws_url }
    }

    pub fn testnet_replay(&self) -> bool {
        self.testnet_replay
    }
}

impl GlobalTestingConfig for ReplayConfig {
    fn is_leader(&self, node_id: u64) -> bool {
        true
    }

    fn anvil_rpc_endpoint(&self, node_id: u64) -> String {
        unreachable!()
    }

    fn base_angstrom_rpc_port(&self) -> u16 {
        unreachable!()
    }

    fn node_count(&self) -> u64 {
        1
    }

    fn leader_eth_rpc_port(&self) -> u16 {
        unreachable!()
    }

    fn config_type(&self) -> super::TestingConfigKind {
        super::TestingConfigKind::Replay
    }

    fn initial_state_config(&self) -> crate::types::initial_state::InitialStateConfig {
        unreachable!()
    }

    fn eth_ws_url(&self) -> String {
        self.eth_ws_url.clone()
    }

    fn fork_config(&self) -> Option<(u64, String)> {
        None
    }
}
