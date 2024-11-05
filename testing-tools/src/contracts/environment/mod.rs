use std::str::FromStr;

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::AnvilInstance,
    primitives::Address,
    providers::ProviderBuilder,
    pubsub::PubSubFrontend,
    signers::local::PrivateKeySigner,
    transports::http::{Client, Http}
};
use tracing::debug;

use super::anvil::{AnvilWalletRpc, LocalAnvilRpc};
use crate::contracts::anvil::spawn_anvil;

pub mod angstrom;
pub mod mockreward;
pub mod uniswap;

pub trait TestAnvilEnvironment {
    type T: Clone + Send + Sync + alloy::transports::Transport;
    type P: alloy::providers::Provider<Self::T, Ethereum>;

    fn provider(&self) -> &Self::P;
    fn controller(&self) -> Address;
}

pub struct SpawnedAnvil {
    #[allow(dead_code)]
    anvil:      AnvilInstance,
    provider:   AnvilWalletRpc,
    controller: Address
}

impl SpawnedAnvil {
    pub async fn new() -> eyre::Result<Self> {
        debug!("Spawning Anvil...");
        let (anvil, provider) = spawn_anvil(7).await?;
        let controller = anvil.addresses()[7];
        debug!("Anvil spawned");
        Ok(Self { anvil, provider, controller })
    }
}

impl TestAnvilEnvironment for SpawnedAnvil {
    type P = AnvilWalletRpc;
    type T = PubSubFrontend;

    fn provider(&self) -> &Self::P {
        &self.provider
    }

    fn controller(&self) -> Address {
        self.controller
    }
}

pub struct LocalAnvil {
    _url:     String,
    provider: LocalAnvilRpc
}

impl LocalAnvil {
    pub async fn new(url: String) -> eyre::Result<Self> {
        let sk: PrivateKeySigner = PrivateKeySigner::from_str(
            "4bbbf85ce3377467afe5d46f804f221813b2bb87f24d81f60f1fcdbf7cbf4356"
        )
        .unwrap();
        let wallet = EthereumWallet::new(sk);
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(url.clone().parse()?);
        Ok(Self { _url: url, provider })
    }
}

impl TestAnvilEnvironment for LocalAnvil {
    type P = LocalAnvilRpc;
    type T = Http<Client>;

    fn provider(&self) -> &Self::P {
        &self.provider
    }

    fn controller(&self) -> Address {
        Address::from_str("14dC79964da2C08b23698B3D3cc7Ca32193d9955").unwrap()
    }
}
