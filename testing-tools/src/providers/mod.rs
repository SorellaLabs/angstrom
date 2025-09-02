mod anvil_submission;
mod rpc_provider;
use alloy_primitives::Address;
pub use anvil_submission::*;
pub use rpc_provider::*;
mod anvil_provider;
mod state_provider;
pub use anvil_provider::*;
pub use state_provider::*;
mod block_provider;
pub mod utils;
pub use block_provider::*;
mod initializer;

use alloy::{
    network::{Ethereum, EthereumWallet, NetworkWallet},
    node_bindings::AnvilInstance,
    providers::{Network, Provider, RootProvider},
    signers::local::PrivateKeySigner
};
pub use initializer::*;
pub mod compat;

use crate::{
    contracts::{anvil::WalletProviderRpc, environment::TestAnvilEnvironment},
    types::{GlobalTestingConfig, WithWalletProvider, config::TestingNodeConfig}
};

#[derive(Clone)]
pub struct WalletProvider<N: Network = Ethereum, W = EthereumWallet>
where
    W: NetworkWallet<N> + Clone
{
    provider:                  WalletProviderRpc<N, W>,
    pub controller_secret_key: PrivateKeySigner
}

impl WalletProvider<Ethereum> {
    pub async fn new<G: GlobalTestingConfig>(
        config: TestingNodeConfig<G>
    ) -> eyre::Result<(Self, Option<AnvilInstance>)> {
        config.spawn_anvil_rpc().await
    }
}

impl<N: Network, W: NetworkWallet<N> + Clone> WalletProvider<N, W> {
    pub(crate) fn new_with_provider(
        provider: WalletProviderRpc<N, W>,
        controller_secret_key: PrivateKeySigner
    ) -> Self {
        Self { provider, controller_secret_key }
    }

    pub fn provider_ref(&self) -> &WalletProviderRpc<N, W> {
        &self.provider
    }

    pub fn provider(&self) -> WalletProviderRpc<N, W> {
        self.provider.clone()
    }
}

impl<N: Network, W: NetworkWallet<N> + Clone> TestAnvilEnvironment for WalletProvider<N, W> {
    type P = WalletProviderRpc<N, W>;

    fn provider(&self) -> &WalletProviderRpc<N, W> {
        &self.provider
    }

    fn controller(&self) -> Address {
        self.controller_secret_key.address()
    }
}

impl<N: Network, W: NetworkWallet<N> + Clone + 'static> WithWalletProvider<N, W>
    for WalletProvider<N, W>
{
    fn wallet_provider(&self) -> WalletProvider<N, W> {
        self.clone()
    }

    fn rpc_provider(&self) -> WalletProviderRpc<N, W> {
        self.provider.clone()
    }
}

impl<N: Network, W: NetworkWallet<N> + Clone> Provider<N> for WalletProvider<N, W>
where
    N: Network,
    W: NetworkWallet<N>,
    WalletProviderRpc<N, W>: Provider<N>
{
    fn root(&self) -> &RootProvider<N> {
        self.provider.root()
    }
}
