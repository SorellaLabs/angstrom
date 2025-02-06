use std::{future::Future, path::Path, pin::Pin, sync::Arc};

use angstrom_types::testnet::InitialTestnetState;
use reth_chainspec::MAINNET;
use reth_db::{mdbx::DatabaseArguments, ClientVersion, DatabaseEnv};
use reth_node_ethereum::EthereumNode;
use reth_node_types::NodeTypesWithDBAdapter;
use reth_provider::{
    providers::{BlockchainProvider, StaticFileProvider},
    ChainSpecProvider, ProviderFactory
};
use tracing::Level;
use tracing_subscriber::{
    layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer, Registry
};

use crate::agents::AgentConfig;

pub type Provider = BlockchainProvider<NodeTypesWithDBAdapter<EthereumNode, Arc<DatabaseEnv>>>;

pub fn load_reth_db(db_path: &Path) -> Provider {
    let db = Arc::new(
        reth_db::open_db_read_only(db_path, DatabaseArguments::new(ClientVersion::default()))
            .unwrap()
    );

    let mut static_files = db_path.to_path_buf();
    static_files.pop();
    static_files.push("static_files");

    let chain = MAINNET.clone();
    let static_file_provider = StaticFileProvider::read_only(static_files, true).expect(
        "static file
provider failed"
    );

    let provider_factory =
        ProviderFactory::new(db.clone(), Arc::clone(&chain), static_file_provider);
    let executor = reth_node_ethereum::EthExecutorProvider::ethereum(provider_factory.chain_spec());

    BlockchainProvider::new(provider_factory.clone()).unwrap()
}

pub fn workspace_dir() -> std::path::PathBuf {
    let output = std::process::Command::new(env!("CARGO"))
        .arg("locate-project")
        .arg("--workspace")
        .arg("--message-format=plain")
        .output()
        .unwrap()
        .stdout;
    let cargo_path = std::path::Path::new(std::str::from_utf8(&output).unwrap().trim());
    cargo_path.parent().unwrap().to_path_buf()
}

pub fn noop_agent<'a>(
    _: &'a InitialTestnetState,
    _: AgentConfig
) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>> {
    Box::pin(async { eyre::Ok(()) })
}

pub fn init_tracing(verbosity: u8) {
    let level = match verbosity - 1 {
        0 => Level::ERROR,
        1 => Level::WARN,
        2 => Level::INFO,
        3 => Level::DEBUG,
        _ => Level::TRACE
    };

    let layers = vec![
        layer_builder(format!("testnet={level}")),
        layer_builder(format!("devnet={level}")),
        layer_builder(format!("angstrom_rpc={level}")),
        layer_builder(format!("angstrom={level}")),
        layer_builder(format!("testing_tools={level}")),
        layer_builder(format!("matching_engine={level}")),
        layer_builder(format!("uniswap_v4={level}")),
        layer_builder(format!("consensus={level}")),
        layer_builder(format!("validation={level}")),
        layer_builder(format!("order_pool={level}")),
    ];

    tracing_subscriber::registry().with(layers).init();
}

fn layer_builder(filter_str: String) -> Box<dyn Layer<Registry> + Send + Sync> {
    let filter = EnvFilter::builder()
        .with_default_directive(filter_str.parse().unwrap())
        .from_env_lossy();

    tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_target(true)
        .with_filter(filter)
        .boxed()
}
