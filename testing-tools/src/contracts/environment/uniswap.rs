use alloy::{
    primitives::{aliases::I24, Address, FixedBytes, U256},
    transports::BoxTransport
};
use alloy_primitives::TxHash;
use angstrom_types::contract_bindings::{
    pool_gate::PoolGate::{self, PoolGateInstance},
    pool_manager::PoolManager
};
use tracing::debug;

use super::TestAnvilEnvironment;
use crate::{contracts::DebugTransaction, providers::WalletProvider};

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
    ) -> eyre::Result<TxHash>;
}

#[derive(Clone)]
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
        let pool_manager = *inner
            .execute_then_mine(PoolManager::deploy(inner.provider(), inner.controller()))
            .await?
            .address();
        debug!("Pool manager deployed at: {}", pool_manager);
        debug!("Deploying pool gate...");
        let pool_gate_instance = inner
            .execute_then_mine(PoolGate::deploy(inner.provider(), pool_manager))
            .await?;
        let pool_gate = *pool_gate_instance.address();
        debug!("Pool gate deployed at: {}", pool_gate);
        Ok(Self { inner, pool_manager, pool_gate })
    }

    pub fn pool_gate(&self) -> PoolGateInstance<BoxTransport, &E::P> {
        PoolGateInstance::new(self.pool_gate, self.provider())
    }
}

impl UniswapEnv<WalletProvider> {
    pub async fn with_anvil(anvil: WalletProvider) -> eyre::Result<Self> {
        Self::new(anvil).await
    }
}

impl<E> TestAnvilEnvironment for UniswapEnv<E>
where
    E: TestAnvilEnvironment
{
    type P = E::P;

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
    ) -> eyre::Result<TxHash> {
        self.pool_gate()
            .addLiquidity(asset0, asset1, lower_tick, upper_tick, liquidity, FixedBytes::default())
            .run_safe()
            .await
    }
}
