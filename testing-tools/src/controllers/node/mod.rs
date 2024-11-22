pub mod devnet;
pub mod internals;
pub mod testnet;

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    marker::PhantomData
};

use alloy::{providers::Provider, pubsub::PubSubFrontend};
use alloy_rpc_types::Transaction;
use angstrom::components::StromHandles;
use angstrom_network::{
    manager::StromConsensusEvent, NetworkOrderEvent, StromMessage, StromNetworkManager
};
use angstrom_types::{sol_bindings::grouped_orders::AllOrders, testnet::InitialTestnetState};
use consensus::AngstromValidator;
use futures::{StreamExt, TryFutureExt};
use rand::Rng;
use reth_chainspec::Hardforks;
use reth_metrics::common::mpsc::{
    metered_unbounded_channel, UnboundedMeteredReceiver, UnboundedMeteredSender
};
use reth_network::test_utils::Peer;
use reth_network_peers::pk2id;
use reth_provider::{test_utils::NoopProvider, BlockReader, ChainSpecProvider, HeaderProvider};
use tokio_stream::wrappers::BroadcastStream;
use tracing::{span, Instrument, Level};

use crate::{
    anvil_state_provider::{utils::async_to_sync, AnvilInitializer, TestnetBlockProvider},
    controllers::{
        strom::{initialize_new_node, AngstromDevnetNodeInternals, TestnetNode},
        TestnetStateFutureLock
    },
    network::TestnetNodeNetwork,
    types::TestingConfig
};

pub struct AngstromTestnetNode<Provider, Internals, FutureLock> {
    pub(crate) testnet_node_id: u64,
    pub(crate) network:         TestnetNodeNetwork,
    pub(crate) strom:           Internals,
    pub(crate) state_lock:      FutureLock,
    _phantom:                   PhantomData<Provider>
}

impl<Provider, Internals, FutureLock> AngstromTestnetNode<Provider, Internals, FutureLock> {
    pub(crate) fn new(
        testnet_node_id: u64,
        network: TestnetNodeNetwork,
        strom: Internals,
        state_lock: FutureLock
    ) -> Self {
        Self { testnet_node_id, network, strom, state_lock, _phantom: PhantomData }
    }
}
