//! CLI definition and entrypoint to executable
use std::path::Path;

use angstrom_eth::{
    handle::{Eth, EthHandle},
    manager::EthDataCleanser
};
use angstrom_network::{
    pool_manager::PoolHandle, NetworkBuilder, PoolManagerBuilder, StatusState, StromNetworkHandle,
    VerificationSidecar
};
use angstrom_rpc::{
    api::{ConsensusApiServer, OrderApiServer, QuotingApiServer},
    ConsensusApi, OrderApi, QuotesApi
};
use angstrom_types::rpc::{
    EcRecoveredComposableLimitOrder, EcRecoveredComposableSearcherOrder, EcRecoveredLimitOrder,
    EcRecoveredSearcherOrder
};
use clap::Parser;
use consensus::{ConsensusHandle, ConsensusManager};
use reth::{
    args::get_secret_key,
    cli::{
        components::{RethNodeComponents, RethRpcComponents},
        config::{RethNetworkConfig, RethRpcConfig},
        ext::{RethCliExt, RethNodeCommandConfig},
        Cli
    },
    dirs::{DataDirPath, MaybePlatformPath},
    primitives::{Chain, PeerId},
    providers::CanonStateSubscriptions
};
use validation::{init_validation, validator::ValidationClient};

/// Convenience function for parsing CLI options, set up logging and run the
/// chosen command.
#[inline]
pub fn run() -> eyre::Result<()> {
    Cli::<StromRethExt>::parse()
        .with_node_extension(AngstromConfig::default())
        .run()
}

struct StromRethExt;

impl RethCliExt for StromRethExt {
    type Node = AngstromConfig;
}

#[derive(Debug, Clone, Default, clap::Args)]
struct AngstromConfig {
    #[clap(long)]
    pub mev_guard: bool,

    #[arg(long, value_name = "DATA_DIR", verbatim_doc_comment, default_value_t)]
    pub datadir: MaybePlatformPath<DataDirPath>,

    /// init state. Set when network is started. We store the data here
    /// so that we can give handles to rpc modules
    #[clap(skip)]
    state: AngstromInitState
}

type DefaultPoolHandle = PoolHandle<
    EcRecoveredLimitOrder,
    EcRecoveredComposableLimitOrder,
    EcRecoveredSearcherOrder,
    EcRecoveredComposableSearcherOrder
>;

/// This holds all the handles that are started with the network that our rpc
/// modules will need.
#[derive(Debug, Clone, Default)]
#[allow(unused)]
struct AngstromInitState {
    pub network_handle: Option<StromNetworkHandle>,
    pub validation:     Option<ValidationClient>,
    pub consensus:      Option<ConsensusHandle>,
    pub eth_handle:     Option<EthHandle>,
    pub pool_handle:    Option<DefaultPoolHandle>
}

impl RethNodeCommandConfig for AngstromConfig {
    fn configure_network<Conf, Reth>(
        &mut self,
        config: &mut Conf,
        components: &Reth
    ) -> eyre::Result<()>
    where
        Conf: RethNetworkConfig,
        Reth: RethNodeComponents
    {
        let path = Path::new("");
        let secret_key = get_secret_key(&path).unwrap();

        let state = StatusState {
            version:   0,
            chain:     Chain::mainnet(),
            peer:      PeerId::default(),
            timestamp: 0
        };

        let verification =
            VerificationSidecar { status: state, has_sent: false, has_received: false, secret_key };

        let eth_handle = EthDataCleanser::new(
            components.events().subscribe_to_canonical_state(),
            components.provider(),
            components.task_executor()
        )
        .unwrap();

        let validator =
            init_validation(components.provider(), eth_handle.subscribe_network_stream());

        let (pool_tx, pool_rx) =
            reth_metrics::common::mpsc::metered_unbounded_channel("order pool");

        let (consensus_tx, consensus_rx) =
            reth_metrics::common::mpsc::metered_unbounded_channel("consensus");

        let (protocol, network_handle) = NetworkBuilder::new(components.provider(), verification)
            .with_pool_manager(pool_tx)
            .with_consensus_manager(consensus_tx)
            .build(components.task_executor());

        // init our custom sub-protocol
        config.add_rlpx_sub_protocol(protocol);

        let pool_handle = PoolManagerBuilder::new(
            validator.clone(),
            network_handle.clone(),
            eth_handle.subscribe_network(),
            pool_rx
        )
        .build(components.task_executor());

        let consensus_handle = ConsensusManager::new(
            components.task_executor(),
            network_handle.clone(),
            pool_handle.clone(),
            validator.clone(),
            components.events().subscribe_to_canonical_state(),
            consensus_rx
        );

        self.state = GuardInitState {
            pool_handle:    Some(pool_handle),
            eth_handle:     Some(eth_handle),
            network_handle: Some(network_handle),
            validation:     Some(validator),
            consensus:      Some(consensus_handle)
        };

        Ok(())
    }

    fn extend_rpc_modules<Conf, Reth>(
        &mut self,
        _config: &Conf,
        _components: &Reth,
        rpc_components: RethRpcComponents<'_, Reth>
    ) -> eyre::Result<()>
    where
        Conf: RethRpcConfig,
        Reth: RethNodeComponents
    {
        //TODO: Add the handle to the order pool & consensus module
        let consensus = _components.network();
        let pool = self.state.pool_handle.clone().unwrap();

        let order_api = OrderApi { pool: pool.clone() };
        let quotes_api = QuotesApi { pool: pool.clone() };
        let consensus_api = ConsensusApi { consensus };
        rpc_components
            .modules
            .merge_configured(order_api.into_rpc())?;
        rpc_components
            .modules
            .merge_configured(quotes_api.into_rpc())?;
        rpc_components
            .modules
            .merge_configured(consensus_api.into_rpc())?;

        //_components.task_executor().spawn_critical();

        Ok(())
    }
}
