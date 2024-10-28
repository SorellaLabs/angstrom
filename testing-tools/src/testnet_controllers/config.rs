#[derive(Debug, Clone, Copy, Default)]
pub struct AngstromTestnetConfig {
    pub intial_node_count:       u64,
    pub initial_rpc_port:        u16,
    pub testnet_block_time_secs: u64,
    pub testnet_kind:            TestnetKind
}

impl AngstromTestnetConfig {
    pub fn new(
        intial_node_count: u64,
        initial_rpc_port: u16,
        testnet_block_time_secs: u64,
        testnet_kind: TestnetKind
    ) -> Self {
        Self { intial_node_count, initial_rpc_port, testnet_block_time_secs, testnet_kind }
    }

    pub fn rpc_port_with_node_id(&self, node_id: u64) -> u64 {
        self.initial_rpc_port as u64 + node_id
    }

    pub fn state_machine_config(&self) -> Option<StateMachineConfig> {
        match self.testnet_kind {
            TestnetKind::StateMachine(c) => Some(c),
            _ => None
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum TestnetKind {
    StateMachine(StateMachineConfig),
    #[default]
    Raw
}

impl TestnetKind {
    pub fn new_raw() -> Self {
        Self::Raw
    }

    pub fn new_state_machine(start_block: u64, end_block: u64) -> Self {
        Self::StateMachine(StateMachineConfig { start_block, end_block })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StateMachineConfig {
    pub start_block: u64,
    pub end_block:   u64
}
