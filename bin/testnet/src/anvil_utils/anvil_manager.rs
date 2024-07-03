use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::Anvil,
    signers::local::PrivateKeySigner
};
use alloy_node_bindings::AnvilInstance;
use alloy_provider::{
    builder,
    fillers::{ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller},
    Identity, IpcConnect, Provider, ProviderBuilder, RootProvider
};
use alloy_pubsub::PubSubFrontend;

pub type AnvilWalletRpc = FillProvider<
    JoinFill<
        JoinFill<JoinFill<JoinFill<Identity, GasFiller>, NonceFiller>, ChainIdFiller>,
        WalletFiller<EthereumWallet>
    >,
    RootProvider<PubSubFrontend>,
    PubSubFrontend,
    Ethereum
>;

pub async fn spawn_anvil(
    block_time: u64,
    fork_url: String
) -> eyre::Result<(AnvilInstance, AnvilWalletRpc)> {
    let anvil = Anvil::new()
        .block_time(block_time)
        .fork_block_number(20214717)
        .fork(fork_url)
        .chain_id(1)
        .arg("--ipc")
        .arg("--code-size-limit")
        .arg("0x60000")
        .arg("--disable-block-gas-limit")
        .try_spawn()?;

    let endpoint = "/tmp/anvil.ipc";
    tracing::info!(?endpoint);
    let ipc = IpcConnect::new(endpoint.to_string());
    let sk: PrivateKeySigner = anvil.keys()[0].clone().into();

    let wallet = EthereumWallet::new(sk);
    let rpc = builder::<Ethereum>()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_ipc(ipc)
        .await?;

    tracing::info!("connected to anvil");

    Ok((anvil, rpc))
}
