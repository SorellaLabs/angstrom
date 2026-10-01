use std::fmt::Debug;

use super::{config::TestingConfigKind, initial_state::InitialStateConfig};
use crate::{contracts::anvil::WalletProviderRpc, providers::WalletProvider};

pub trait WithWalletProvider: Send + Sync + 'static {
    fn wallet_provider(&self) -> WalletProvider;

    fn rpc_provider(&self) -> WalletProviderRpc;
}

pub trait GlobalTestingConfig: Debug + Clone + Send + Sync {
    fn eth_ws_url(&self) -> String;

    fn fork_config(&self) -> Option<(u64, String)>;

    /// Determines whether we should be performing testnet setup when we fork
    /// our chain
    fn use_testnet(&self) -> bool;

    fn config_type(&self) -> TestingConfigKind;

    fn anvil_rpc_endpoint(&self, node_id: u64) -> String;

    fn is_leader(&self, node_id: u64) -> bool;

    fn node_count(&self) -> u64;

    fn initial_state_config(&self) -> InitialStateConfig;

    /// `None` lets the OS assign the port. A fixed draw over the whole `u16`
    /// range can land on a privileged port (`EACCES`) or one already in use
    /// (`EADDRINUSE`); only pin a port when a caller needs to know it up front.
    fn leader_eth_rpc_port(&self) -> Option<u16>;

    fn base_angstrom_rpc_port(&self) -> Option<u16>;
}
