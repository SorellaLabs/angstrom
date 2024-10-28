use alloy::primitives::{aliases::I24, Address, FixedBytes, U256};
use angstrom_types::contract_bindings::{
    pool_gate::PoolGate::{self, PoolGateInstance},
    pool_manager::PoolManager
};
use tracing::debug;

use super::{SpawnedAnvil, TestAnvilEnvironment};
use crate::contracts::DebugTransaction;

pub trait TestUniswapEnv: TestAnvilEnvironment {
    fn pool_manager(&self) -> Address;
    fn pool_gate(&self) -> Address;
    #[allow(async_fn_in_trait)]
    async fn add_liquidity_position(
        &self,
        asset0: Address,
        asset1: Address,
        lower_tick: I24,
        upper_tick: I24,
        liquidity: U256
    ) -> eyre::Result<()>;
}

pub struct UniswapEnv<E: TestAnvilEnvironment> {
    inner:        E,
    pool_manager: Address,
    pool_gate:    Address
}

impl<E> UniswapEnv<E>
where
    E: TestAnvilEnvironment
{
    pub async fn new(inner: E) -> eyre::Result<Self> {
        debug!("Deploying pool manager...");
        let pool_manager = *PoolManager::deploy(inner.provider())
            .await
            .unwrap()
            .address();
        debug!("Pool manager deployed at: {}", pool_manager);
        debug!("Deploying pool gate...");
        let pool_gate_instance = PoolGate::deploy(inner.provider(), pool_manager)
            .await
            .unwrap();
        let pool_gate = *pool_gate_instance.address();
        debug!("Pool gate deployed at: {}", pool_gate);
        Ok(Self { inner, pool_manager, pool_gate })
    }

    pub fn pool_gate(&self) -> PoolGateInstance<E::T, &E::P> {
        PoolGateInstance::new(self.pool_gate, self.provider())
    }
}

impl UniswapEnv<SpawnedAnvil> {
    pub async fn spawn_anvil() -> eyre::Result<Self> {
        let inner = SpawnedAnvil::new().await?;
        Self::new(inner).await
    }
}

impl<E> TestAnvilEnvironment for UniswapEnv<E>
where
    E: TestAnvilEnvironment
{
    type P = E::P;
    type T = E::T;

    fn provider(&self) -> &Self::P {
        self.inner.provider()
    }

    fn controller(&self) -> Address {
        self.inner.controller()
    }
}

impl<E> TestUniswapEnv for UniswapEnv<E>
where
    E: TestAnvilEnvironment
{
    fn pool_gate(&self) -> Address {
        self.pool_gate
    }

    fn pool_manager(&self) -> Address {
        self.pool_manager
    }

    async fn add_liquidity_position(
        &self,
        asset0: Address,
        asset1: Address,
        lower_tick: I24,
        upper_tick: I24,
        liquidity: U256
    ) -> eyre::Result<()> {
        self.pool_gate()
            .addLiquidity(asset0, asset1, lower_tick, upper_tick, liquidity, FixedBytes::default())
            .run_safe()
            .await
    }
}
