/// mocks utils for different modules
pub mod mocks;
/// Tools for testing network setup
pub mod network;
/// Tools for testing order_pool functionality
pub mod order_pool;
/// Tools for generating different types of orders
pub mod type_generator;
/// Tools for validation module. Helps with db overrides and other
/// nuanced needs
pub mod validation;

use std::{path::Path, sync::Arc};

use reth_beacon_consensus::BeaconConsensus;
use reth_blockchain_tree::{
    BlockchainTree, BlockchainTreeConfig, ShareableBlockchainTree, TreeExternals
};
use reth_db::{mdbx::DatabaseArguments, models::client_version::ClientVersion, DatabaseEnv};
use reth_node_ethereum::EthEvmConfig;
use reth_primitives::{PruneModes, MAINNET};
use reth_provider::{providers::BlockchainProvider, ProviderFactory};
use reth_revm::EvmProcessorFactory;

pub type Provider = BlockchainProvider<
    Arc<DatabaseEnv>,
    ShareableBlockchainTree<Arc<DatabaseEnv>, EvmProcessorFactory<EthEvmConfig>>
>;

pub fn load_reth_db(db_path: &Path) -> Provider {
    let db = Arc::new(
        reth_db::open_db(db_path, DatabaseArguments::new(ClientVersion::default())).unwrap()
    );

    let mut static_files = db_path.to_path_buf();
    static_files.pop();
    static_files.push("static_files");

    let chain = MAINNET.clone();
    let provider_factory = ProviderFactory::new(db.clone(), Arc::clone(&chain), static_files)
        .expect("failed to start provider factory");

    let tree_externals = TreeExternals::new(
        provider_factory.clone(),
        Arc::new(BeaconConsensus::new(Arc::clone(&chain))),
        EvmProcessorFactory::new(chain.clone(), EthEvmConfig::default())
    );

    let tree_config = BlockchainTreeConfig::default();

    let blockchain_tree = ShareableBlockchainTree::new(
        BlockchainTree::new(tree_externals, tree_config, Some(PruneModes::none())).unwrap()
    );

    BlockchainProvider::new(provider_factory.clone(), blockchain_tree).unwrap()
}
