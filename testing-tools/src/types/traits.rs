use std::fmt::Debug;

use alloy::network::{Ethereum, EthereumWallet, Network, NetworkWallet};

use super::{config::TestingConfigKind, initial_state::InitialStateConfig};
use crate::{contracts::anvil::WalletProviderRpc, providers::WalletProvider};

pub trait WithWalletProvider<N: Network = Ethereum, W: NetworkWallet<N> + Clone = EthereumWallet>:
    Send + Sync + 'static
{
    fn wallet_provider(&self) -> WalletProvider<N, W>;

    fn rpc_provider(&self) -> WalletProviderRpc<N, W>;
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

    fn leader_eth_rpc_port(&self) -> u16;

    fn base_angstrom_rpc_port(&self) -> u16;
}
