pub mod angstrom;
mod components;
mod config;
mod handles;
mod metrics;
pub mod op_angstrom;

use std::{collections::HashSet, sync::Arc};

use alloy::primitives::Address;
use angstrom_amm_quoter::QuoterHandle;
use angstrom_network::{AngstromNetworkBuilder, pool_manager::PoolHandle};
use angstrom_rpc::{
    ConsensusApi, OrderApi,
    api::{ConsensusApiServer, OrderApiServer}
};
use angstrom_types::primitive::{AngstromMetaSigner, AngstromSigner};
use consensus::ConsensusHandler;
use parking_lot::RwLock;
use reth::{chainspec::ChainSpec, tasks::TaskExecutor};
use reth_db::DatabaseEnv;
use reth_node_builder::{Node, NodeBuilder, NodeHandle, WithLaunchContext};
use reth_node_ethereum::{EthereumAddOns, EthereumNode};
use validation::validator::ValidationClient;

use crate::{
    components::{init_network_builder, initialize_strom_components},
    config::AngstromConfig,
    handles::{AngstromMode, StromHandles}
};
