use std::sync::Arc;

use alloy::{providers::Provider, pubsub::PubSubFrontend};
use alloy_primitives::Address;
use alloy_rpc_types::{BlockId, Transaction};
use angstrom::components::StromHandles;
use angstrom_eth::handle::Eth;
use angstrom_network::{pool_manager::PoolHandle, PoolManagerBuilder, StromNetworkHandle};
use angstrom_rpc::{api::OrderApiServer, OrderApi};
use angstrom_types::{
    block_sync::GlobalBlockSync,
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    pair_with_price::PairsWithPrice,
    primitive::UniswapPoolRegistry,
    sol_bindings::testnet::TestnetHub,
    testnet::InitialTestnetState
};
use consensus::{AngstromValidator, ConsensusManager, ManagerNetworkDeps, Signer};
use futures::{StreamExt, TryStreamExt};
use jsonrpsee::server::ServerBuilder;
use matching_engine::{configure_uniswap_manager, manager::MatcherHandle, MatchingManager};
use order_pool::{order_storage::OrderStorage, PoolConfig};
use reth_provider::CanonStateSubscriptions;
use reth_tasks::TokioTaskExecutor;
use secp256k1::SecretKey;
use tokio_stream::wrappers::BroadcastStream;
use validation::{
    common::TokenPriceGenerator, order::state::pools::AngstromPoolsTracker,
    validator::ValidationClient
};

use crate::{
    anvil_state_provider::{
        utils::StromContractInstance, AnvilEthDataCleanser, AnvilStateProvider,
        AnvilStateProviderWrapper
    },
    types::{SendingStromHandles, TestingConfig},
    validation::TestOrderValidator
};

pub struct AngstromTestNodeInternals {
    node_id:          u64,
    rpc_port:         u64,
    is_leader:        bool,
    state_provider:   AnvilStateProviderWrapper,
    order_storage:    Arc<OrderStorage>,
    pool_handle:      PoolHandle,
    tx_strom_handles: SendingStromHandles,
    testnet_hub:      StromContractInstance,
    validator:        TestOrderValidator<AnvilStateProvider>
}
