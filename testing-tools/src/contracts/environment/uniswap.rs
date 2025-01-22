use alloy::{
    primitives::{aliases::I24, Address, FixedBytes, U256},
    transports::BoxTransport
};
use alloy_primitives::{address, TxHash};
use angstrom_types::contract_bindings::{
    i_position_descriptor::IPositionDescriptor,
    pool_gate::PoolGate::{self, PoolGateInstance},
    pool_manager::PoolManager,
    position_manager::PositionManager
};
use tracing::debug;
use validation::common::WETH_ADDRESS;

use super::TestAnvilEnvironment;
use crate::{contracts::DebugTransaction, providers::WalletProvider};

const PERMIT2_ADDRESS: Address = address!("000000000022d473030f116ddee9f6b43ac78ba3");

pub trait TestUniswapEnv: TestAnvilEnvironment {
    fn pool_manager(&self) -> Address;
    fn position_manager(&self) -> Address;
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
    inner:            E,
    pool_manager:     Address,
    position_manager: Address,
    pool_gate:        Address
}

impl<E> UniswapEnv<E>
where
    E: TestAnvilEnvironment
{
    pub async fn new(inner: E) -> eyre::Result<Self> {
        let pool_manager = Self::deploy_pool_manager(&inner).await?;
        let position_manager = Self::deploy_position_manager(&inner, pool_manager).await?;
        let pool_gate = Self::deploy_pool_gate(&inner, pool_manager).await?;

        Ok(Self { inner, pool_manager, pool_gate, position_manager })
    }

    async fn deploy_pool_manager(inner: &E) -> eyre::Result<Address> {
        debug!("Deploying pool manager...");
        let pool_manager_addr = *inner
            .execute_then_mine(PoolManager::deploy(inner.provider(), inner.controller()))
            .await?
            .address();

        debug!("Pool manager deployed at: {}", pool_manager_addr);
        Ok(pool_manager_addr)
    }

    async fn deploy_pool_gate(inner: &E, pool_manager: Address) -> eyre::Result<Address> {
        debug!("Deploying pool gate...");
        let pool_gate_instance = inner
            .execute_then_mine(PoolGate::deploy(inner.provider(), pool_manager))
            .await?;
        let pool_gate_addr = *pool_gate_instance.address();

        debug!("Pool gate deployed at: {}", pool_gate_addr);
        Ok(pool_gate_addr)
    }

    async fn deploy_position_manager(inner: &E, pool_manager: Address) -> eyre::Result<Address> {
        let position_descriptor_addr = inner
            .execute_then_mine(IPositionDescriptor::deploy(inner.provider()))
            .await?;

        debug!("Deploying position manager...");
        let position_manager = inner
            .execute_then_mine(PositionManager::deploy(
                inner.provider(),
                pool_manager,
                PERMIT2_ADDRESS,
                U256::from(100000),
                *position_descriptor_addr.address(),
                WETH_ADDRESS
            ))
            .await?;
        let position_manager_addr = *position_manager.address();

        debug!("Position manager deployed");
        Ok(position_manager_addr)
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

    fn position_manager(&self) -> Address {
        self.position_manager
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
