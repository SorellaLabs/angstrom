use std::fmt::Debug;

use super::config::TestingConfigKind;
use crate::providers::{utils::WalletProviderRpc, WalletProvider};

pub trait WithWalletProvider: Send + Sync {
    fn wallet_provider(&self) -> WalletProvider;

    fn rpc_provider(&self) -> WalletProviderRpc;
}

pub trait GlobalTestingConfig: Debug + Clone + Send + Sync {
    fn eth_ws_url(&self) -> String;

    fn fork_config(&self) -> Option<(u64, String)>;

    fn config_type(&self) -> TestingConfigKind;

    fn anvil_rpc_endpoint(&self, node_id: u64) -> String;

    fn is_leader(&self, node_id: u64) -> bool;

    fn node_count(&self) -> u64;
}
