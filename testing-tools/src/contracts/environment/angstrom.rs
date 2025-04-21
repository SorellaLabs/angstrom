use alloy_primitives::{Address, TxHash};
use angstrom_types::contract_bindings::{
    angstrom::Angstrom::AngstromInstance, controller_v_1::ControllerV1,
    pool_gate::PoolGate::PoolGateInstance, position_fetcher::PositionFetcher
};
use tracing::{debug, info};

use super::{TestAnvilEnvironment, uniswap::TestUniswapEnv};
use crate::contracts::{DebugTransaction, deploy::angstrom::deploy_angstrom_create3};

pub trait TestAngstromEnv: TestAnvilEnvironment + TestUniswapEnv {
    fn angstrom(&self) -> Address;
}

#[derive(Clone)]
pub struct AngstromEnv<E: TestUniswapEnv> {
    #[allow(dead_code)]
    inner:            E,
    angstrom:         Address,
    controller_v1:    Address,
    position_fetcher: Address
}

impl<E> AngstromEnv<E>
where
    E: TestUniswapEnv
{
    pub async fn new(inner: E, nodes: Vec<Address>) -> eyre::Result<Self> {
        let angstrom = Self::deploy_angstrom(&inner, nodes).await?;
        let controller_v1 = Self::deploy_controller_v1(&inner, angstrom).await?;
        let position_fetcher = Self::deploy_position_fetcher(&inner, angstrom).await?;

        info!("Environment deploy complete!");

        Ok(Self { inner, angstrom, controller_v1, position_fetcher })
    }

    async fn deploy_angstrom(inner: &E, nodes: Vec<Address>) -> eyre::Result<Address> {
        let provider = inner.provider();
        debug!("Deploying Angstrom...");

        let angstrom_addr = inner
            .execute_then_mine(deploy_angstrom_create3(
                provider,
                inner.pool_manager(),
                inner.controller()
            ))
            .await?;
        debug!("Angstrom deployed at: {}", angstrom_addr);

        // gotta toggle nodes
        let ang_i = AngstromInstance::new(angstrom_addr, &provider);
        let _ = inner
            .execute_then_mine(ang_i.toggleNodes(nodes).from(inner.controller()).run_safe())
            .await?;

        Ok(angstrom_addr)
    }

    async fn deploy_controller_v1(inner: &E, angstrom_addr: Address) -> eyre::Result<Address> {
        debug!("Deploying ControllerV1...");
        let controller_v1_addr = *inner
            .execute_then_mine(ControllerV1::deploy(
                inner.provider(),
                angstrom_addr,
                inner.controller()
            ))
            .await?
            .address();
        debug!("ControllerV1 deployed at: {}", controller_v1_addr);

        let angstrom = AngstromInstance::new(angstrom_addr, inner.provider());
        let _ = inner
            .execute_then_mine(
                angstrom
                    .setController(controller_v1_addr)
                    .from(inner.controller())
                    .run_safe()
            )
            .await?;

        // Set the PoolGate's hook to be our Mock
        debug!("Setting PoolGate hook...");
        let pool_gate_instance = PoolGateInstance::new(inner.pool_gate(), inner.provider());
        inner
            .execute_then_mine(
                pool_gate_instance
                    .setHook(angstrom_addr)
                    .from(inner.controller())
                    .run_safe()
            )
            .await?;

        Ok(controller_v1_addr)
    }

    async fn deploy_position_fetcher(inner: &E, angstrom: Address) -> eyre::Result<Address> {
        debug!("Deploying PositionFetcher...");
        let position_fetcher_addr = *inner
            .execute_then_mine(PositionFetcher::deploy(
                inner.provider(),
                inner.position_manager(),
                angstrom
            ))
            .await?
            .address();

        debug!("PositionFetcher deployed at: {}", position_fetcher_addr);

        Ok(position_fetcher_addr)
    }

    pub fn angstrom(&self) -> Address {
        self.angstrom
    }

    pub fn controller_v1(&self) -> Address {
        self.controller_v1
    }

    pub fn position_fetcher(&self) -> Address {
        self.position_fetcher
    }
}

impl<E> TestUniswapEnv for AngstromEnv<E>
where
    E: TestUniswapEnv
{
    fn pool_manager(&self) -> Address {
        self.inner.pool_manager()
    }

    fn pool_gate(&self) -> Address {
        self.inner.pool_gate()
    }

    fn position_manager(&self) -> Address {
        self.inner.position_manager()
    }

    async fn add_liquidity_position(
        &self,
        asset0: Address,
        asset1: Address,
        lower_tick: alloy_primitives::aliases::I24,
        upper_tick: alloy_primitives::aliases::I24,
        liquidity: alloy_primitives::U256
    ) -> eyre::Result<TxHash> {
        self.inner
            .add_liquidity_position(asset0, asset1, lower_tick, upper_tick, liquidity)
            .await
    }
}

impl<E> TestAnvilEnvironment for AngstromEnv<E>
where
    E: TestUniswapEnv
{
    type P = E::P;

    fn provider(&self) -> &Self::P {
        self.inner.provider()
    }

    fn controller(&self) -> Address {
        self.inner.controller()
    }
}
