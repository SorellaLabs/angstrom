use alloy::primitives::Address;
use angstrom_types::contract_bindings::pool_gate::PoolGate::PoolGateInstance;
use tracing::debug;

use super::{uniswap::TestUniswapEnv, TestAnvilEnvironment};
use crate::contracts::{deploy::angstrom::deploy_angstrom, DebugTransaction};

pub trait TestAngstromEnv: TestAnvilEnvironment {
    fn angstrom(&self) -> Address;
}

pub struct AngstromEnv<E: TestUniswapEnv> {
    #[allow(dead_code)]
    inner:    E,
    angstrom: Address
}

impl<E> AngstromEnv<E>
where
    E: TestUniswapEnv
{
    pub async fn new(inner: E) -> eyre::Result<Self> {
        let provider = inner.provider();
        debug!("Deploying mock rewards manager...");
        let angstrom = deploy_angstrom(
            &provider,
            inner.pool_manager(),
            inner.controller(),
            Address::default()
        )
        .await;
        debug!("Angstrom deployed at: {}", angstrom);
        // Set the PoolGate's hook to be our Mock
        debug!("Setting PoolGate hook...");
        let pool_gate_instance = PoolGateInstance::new(inner.pool_gate(), &provider);
        pool_gate_instance
            .setHook(angstrom)
            .from(inner.controller())
            .run_safe()
            .await?;
        debug!("Environment deploy complete!");
        Ok(Self { inner, angstrom })
    }

    pub fn angstrom(&self) -> Address {
        self.angstrom
    }
}

#[cfg(test)]
mod tests {
    use super::AngstromEnv;
    use crate::contracts::environment::{uniswap::UniswapEnv, SpawnedAnvil};

    #[tokio::test]
    async fn can_be_constructed() {
        let anvil = SpawnedAnvil::new().await.unwrap();
        let uniswap = UniswapEnv::new(anvil).await.unwrap();
        AngstromEnv::new(uniswap).await.unwrap();
    }
}
