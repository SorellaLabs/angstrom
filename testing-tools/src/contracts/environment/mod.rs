use std::{str::FromStr, sync::Arc, time::Duration};

use alloy::{
    network::EthereumWallet,
    node_bindings::AnvilInstance,
    primitives::Address,
    providers::{Provider, ProviderBuilder, ext::AnvilApi},
    signers::local::PrivateKeySigner
};
use futures::Future;
use tracing::debug;

use super::anvil::WalletProviderRpc;
use crate::contracts::anvil::{LocalAnvilRpc, spawn_anvil};

pub mod angstrom;
pub mod uniswap;

#[allow(async_fn_in_trait)]
pub trait TestAnvilEnvironment: Clone {
    type P: alloy::providers::Provider + alloy::providers::WalletProvider;

    fn provider(&self) -> &Self::P;
    fn controller(&self) -> Address;

    async fn execute_then_mine<O>(&self, f: impl Future<Output = O> + Send) -> O {
        let mut fut = Box::pin(f);
        // poll for 500 ms. if not resolves then we mine
        tokio::select! {
            o = &mut fut => {
                let _ = self.provider().anvil_mine(Some(1), None).await;
                return o
            },
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
            }
        };

        let mine_one_fut = self.provider().anvil_mine(Some(1), None);
        let _ = mine_one_fut.await;
        fut.await
    }

    async fn override_address(
        &self,
        from_addr: &mut Address,
        to_addr: Address
    ) -> eyre::Result<()> {
        let provider = self.provider();

        let code = provider.get_code_at(*from_addr).await?;
        provider.anvil_set_code(to_addr, code).await?;

        *from_addr = to_addr;

        Ok(())
    }
}

#[derive(Clone)]
pub struct SpawnedAnvil {
    #[allow(dead_code)]
    pub anvil:  Arc<AnvilInstance>,
    provider:   WalletProviderRpc,
    controller: Address
}

impl SpawnedAnvil {
    pub async fn new() -> eyre::Result<Self> {
        debug!("Spawning Anvil...");
        let (anvil, provider) = spawn_anvil(7).await?;
        let controller = anvil.addresses()[7];
        debug!("Anvil spawned");
        Ok(Self { anvil: anvil.into(), provider, controller })
    }
}

impl TestAnvilEnvironment for SpawnedAnvil {
    type P = WalletProviderRpc;

    fn provider(&self) -> &Self::P {
        &self.provider
    }

    fn controller(&self) -> Address {
        self.controller
    }
}

#[derive(Clone)]
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
            .wallet(wallet)
            .connect(&url)
            .await
            .unwrap();

        Ok(Self { _url: url, provider })
    }
}

impl TestAnvilEnvironment for LocalAnvil {
    type P = LocalAnvilRpc;

    fn provider(&self) -> &Self::P {
        &self.provider
    }

    fn controller(&self) -> Address {
        Address::from_str("14dC79964da2C08b23698B3D3cc7Ca32193d9955").unwrap()
    }
}
