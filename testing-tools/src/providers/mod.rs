mod anvil_cleanser;
mod rpc_provider;

use alloy_primitives::Address;
pub use rpc_provider::*;
mod anvil_provider;
mod state_provider;
pub use anvil_cleanser::*;
pub use anvil_provider::*;
pub use state_provider::*;
mod block_provider;
pub mod utils;
pub use block_provider::*;
mod initializer;
use alloy::{
    node_bindings::AnvilInstance, pubsub::PubSubFrontend, signers::local::PrivateKeySigner
};
pub use initializer::*;
use utils::WalletProviderRpc;

use crate::{
    contracts::environment::TestAnvilEnvironment,
    types::{config::TestingNodeConfig, GlobalTestingConfig, WithWalletProvider}
};

#[derive(Debug, Clone)]
pub struct WalletProvider {
    provider:                  WalletProviderRpc,
    pub controller_secret_key: PrivateKeySigner
}

impl WalletProvider {
    pub fn new(provider: WalletProviderRpc, controller_secret_key: PrivateKeySigner) -> Self {
        Self { provider, controller_secret_key }
    }

    pub fn provider_ref(&self) -> &WalletProviderRpc {
        &self.provider
    }

    pub fn provider(&self) -> WalletProviderRpc {
        self.provider.clone()
    }
}

impl TestAnvilEnvironment for WalletProvider {
    type P = WalletProviderRpc;
    type T = PubSubFrontend;

    fn provider(&self) -> &WalletProviderRpc {
        &self.provider
    }

    fn controller(&self) -> Address {
        self.controller_secret_key.address()
    }
}

impl WithWalletProvider for WalletProvider {
    async fn initialize<G: GlobalTestingConfig>(
        config: TestingNodeConfig<G>
    ) -> eyre::Result<(Self, Option<AnvilInstance>)>
    where
        Self: Sized
    {
        Ok(config.spawn_anvil_rpc().await?)
    }

    fn wallet_provider(&self) -> WalletProvider {
        self.clone()
    }

    fn rpc_provider(&self) -> WalletProviderRpc {
        self.provider.clone()
    }
}
