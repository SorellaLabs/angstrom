use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::Anvil,
    primitives::Address,
    providers::{builder, IpcConnect},
    signers::local::PrivateKeySigner
};
use angstrom_types::contract_bindings::pool_manager::PoolManager;

type SpawnedProvider = alloy::providers::fillers::FillProvider<
    alloy::providers::fillers::JoinFill<
        alloy::providers::fillers::JoinFill<
            alloy::providers::Identity,
            alloy::providers::fillers::JoinFill<
                alloy::providers::fillers::GasFiller,
                alloy::providers::fillers::JoinFill<
                    alloy::providers::fillers::BlobGasFiller,
                    alloy::providers::fillers::JoinFill<
                        alloy::providers::fillers::NonceFiller,
                        alloy::providers::fillers::ChainIdFiller
                    >
                >
            >
        >,
        alloy::providers::fillers::WalletFiller<EthereumWallet>
    >,
    alloy::providers::RootProvider<alloy::pubsub::PubSubFrontend>,
    alloy::pubsub::PubSubFrontend,
    Ethereum
>;

pub async fn spawn_local_anvil() -> eyre::Result<SpawnedProvider> {
    // Spawn an Anvil instance
    let anvil = Anvil::new()
        .chain_id(1)
        .arg("--ipc")
        .arg("--code-size-limit")
        .arg("393216")
        .arg("--disable-block-gas-limit")
        .try_spawn()?;

    let endpoint = "/tmp/anvil.ipc";
    let ipc = IpcConnect::new(endpoint.to_string());
    let controller = anvil.addresses()[7];
    let sk: PrivateKeySigner = anvil.keys()[7].clone().into();

    let wallet = EthereumWallet::new(sk.clone());
    let provider = builder::<Ethereum>()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_ipc(ipc)
        .await?;
    // let boxed = Box::new(provider);
    // Ok(boxed)
    Ok(provider)
}

async fn do_contract_test<T, N, P>(provider: P) -> eyre::Result<()>
where
    T: Clone + Send + Sync + alloy::transports::Transport,
    N: alloy::providers::Network + Send + Sync,
    P: alloy::providers::Provider<T, N>
{
    Ok(())
}

async fn deploy_pool_manager<T, N, P>(provider: &P) -> eyre::Result<Address>
where
    T: Clone + Send + Sync + alloy::transports::Transport,
    N: alloy::providers::Network + Send + Sync,
    P: alloy::providers::Provider<T, N>
{
    let pool_manager = PoolManager::deploy(provider).await?;
    let pool_manager_address = pool_manager.address();
    Ok(*pool_manager_address)
}
