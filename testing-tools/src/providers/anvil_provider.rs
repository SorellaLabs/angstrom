use std::future::Future;

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    providers::{builder, ext::AnvilApi},
    rpc::types::{anvil::MineOptions, Block},
    signers::local::PrivateKeySigner
};
use alloy_primitives::Bytes;

use super::{utils::WalletProviderRpc, AnvilStateProvider, WalletProvider};
use crate::types::WithWalletProvider;

#[derive(Debug)]
pub struct AnvilProvider<P> {
    provider:  AnvilStateProvider<P>,
    _instance: Option<AnvilInstance>
}
impl<P> AnvilProvider<P>
where
    P: WithWalletProvider
{
    pub async fn new<F>(fut: F) -> eyre::Result<Self>
    where
        F: Future<Output = eyre::Result<(P, Option<AnvilInstance>)>>
    {
        let (provider, anvil) = fut.await?;
        Ok(Self { provider: AnvilStateProvider::new(provider), _instance: anvil })
    }

    pub fn into_state_provider(&mut self) -> AnvilProvider<WalletProvider> {
        AnvilProvider {
            provider:  self.provider.as_wallet_state_provider(),
            _instance: self._instance.take()
        }
    }

    pub fn state_provider(&self) -> AnvilStateProvider<WalletProvider> {
        self.provider.as_wallet_state_provider()
    }

    pub fn wallet_provider(&self) -> WalletProvider {
        self.provider.provider().wallet_provider()
    }

    pub fn rpc_provider(&self) -> WalletProviderRpc {
        self.provider.provider().rpc_provider()
    }

    pub fn provider(&self) -> &AnvilStateProvider<P> {
        &self.provider
    }

    pub fn provider_mut(&mut self) -> &mut AnvilStateProvider<P> {
        &mut self.provider
    }

    pub async fn execute_and_return_state(&self) -> eyre::Result<(Bytes, Block)> {
        let block = self.mine_block().await?;

        Ok((
            self.provider
                .provider()
                .rpc_provider()
                .anvil_dump_state()
                .await?,
            block
        ))
    }

    pub async fn return_state(&self) -> eyre::Result<Bytes> {
        Ok(self
            .provider
            .provider()
            .rpc_provider()
            .anvil_dump_state()
            .await?)
    }

    pub async fn set_state(&self, state: Bytes) -> eyre::Result<()> {
        self.provider
            .provider()
            .rpc_provider()
            .anvil_load_state(state)
            .await?;

        Ok(())
    }

    pub async fn mine_block(&self) -> eyre::Result<Block> {
        let mined = self
            .provider
            .provider()
            .rpc_provider()
            .anvil_mine_detailed(Some(MineOptions::Options { timestamp: None, blocks: Some(1) }))
            .await?
            .first()
            .cloned()
            .unwrap();

        self.provider.update_canon_chain(&mined)?;

        Ok(mined)
    }
}

impl AnvilProvider<WalletProvider> {
    pub async fn spawn_new_isolated() -> eyre::Result<Self> {
        let anvil = Anvil::new()
            .block_time(12)
            .chain_id(1)
            .arg("--ipc")
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit")
            .try_spawn()?;

        let ipc = alloy::providers::IpcConnect::new("/tmp/anvil.ipc".to_string());
        let sk: PrivateKeySigner = anvil.keys()[7].clone().into();

        let wallet = EthereumWallet::new(sk.clone());
        let rpc = builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_ipc(ipc)
            .await?;

        tracing::info!("connected to anvil");

        Ok(Self {
            provider:  AnvilStateProvider::new(WalletProvider::new_with_provider(rpc, sk)),
            _instance: Some(anvil)
        })
    }
}
