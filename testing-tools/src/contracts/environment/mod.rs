use alloy::{
    network::Ethereum, node_bindings::AnvilInstance, primitives::Address, pubsub::PubSubFrontend
};
use tracing::debug;

use super::anvil::AnvilWalletRpc;
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

    fn provider(&self) -> &AnvilWalletRpc {
        &self.provider
    }

    fn controller(&self) -> Address {
        self.controller
    }
}
