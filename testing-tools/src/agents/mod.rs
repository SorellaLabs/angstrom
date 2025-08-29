use std::net::SocketAddr;

use reth_primitives::EthPrimitives;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::providers::{AnvilStateProvider, WalletProvider};

// pub mod e2e_orders;

#[derive(Clone)]
pub struct AgentConfig<P: reth_node_types::NodePrimitives = EthPrimitives> {
    pub uniswap_pools:  SyncedUniswapPools,
    pub rpc_address:    SocketAddr,
    pub agent_id:       u64,
    pub current_block:  u64,
    pub state_provider: AnvilStateProvider<WalletProvider, P>
}
