use alloy::primitives::{
    aliases::{I24, U24},
    Address, U256
};
use alloy_primitives::TxHash;
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::PoolKey,
        mock_rewards_manager::MockRewardsManager::MockRewardsManagerInstance,
        pool_gate::PoolGate::PoolGateInstance
    },
    matching::{uniswap::PoolSnapshot, SqrtPriceX96}
};
use futures::future::join_all;
use tracing::debug;

use super::{
    uniswap::{TestUniswapEnv, UniswapEnv},
    TestAnvilEnvironment
};
use crate::{
    anvil_state_provider::WalletProvider,
    contracts::{
        deploy::{mockreward::deploy_mock_rewards_manager, tokens::mint_token_pair},
        DebugTransaction
    }
};

pub trait TestMockRewardEnv: TestAnvilEnvironment {
    fn mock_reward(&self) -> Address;
}

pub struct MockRewardEnv<E: TestUniswapEnv> {
    inner:       E,
    mock_reward: Address
}

impl<E> MockRewardEnv<E>
where
    E: TestUniswapEnv
{
    pub async fn new(inner: E) -> eyre::Result<Self> {
        let provider = inner.provider();
        debug!("Deploying mock rewards manager...");
        let mock_reward =
            deploy_mock_rewards_manager(&provider, inner.pool_manager(), inner.controller()).await;
        debug!("Mock rewards manager deployed at: {}", mock_reward);
        // Set the PoolGate's hook to be our Mock
        debug!("Setting PoolGate hook...");
        let pool_gate_instance = PoolGateInstance::new(inner.pool_gate(), &provider);
        pool_gate_instance
            .setHook(mock_reward)
            .from(inner.controller())
            .run_safe()
            .await?;
        debug!("Environment deploy complete!");
        Ok(Self { inner, mock_reward })
    }

    /// Creates a pool based on a given PoolSnapshot
    pub async fn create_pool_and_tokens_from_snapshot(
        &self,
        tick_spacing: I24,
        pool_fee: U24,
        snapshot: PoolSnapshot
    ) -> eyre::Result<PoolKey> {
        let pool_key = self
            .create_pool_and_tokens(
                snapshot.current_price().as_sqrtpricex96(),
                tick_spacing,
                pool_fee
            )
            .await?;
        let _results = join_all(snapshot.ranges().map(|r| {
            let lower_tick = I24::unchecked_from(r.lower_tick());
            let upper_tick = I24::unchecked_from(r.upper_tick());
            let liquidity = U256::from(r.liquidity());
            self.add_liquidity_position(
                pool_key.currency0,
                pool_key.currency1,
                lower_tick,
                upper_tick,
                liquidity
            )
        }))
        .await;
        Ok(pool_key)
    }

    pub fn mock_reward(&self) -> MockRewardsManagerInstance<E::T, &E::P> {
        MockRewardsManagerInstance::new(self.mock_reward, self.provider())
    }

    /// Creates a pool using two newly minted tokens at a given initial price
    pub async fn create_pool_and_tokens(
        &self,
        initial_price: SqrtPriceX96,
        tick_spacing: I24,
        pool_fee: U24
    ) -> eyre::Result<PoolKey> {
        let (asset0, asset1) = mint_token_pair(self.provider()).await;
        self.create_pool(asset0, asset1, initial_price, tick_spacing, pool_fee)
            .await
    }

    async fn create_pool(
        &self,
        asset0: Address,
        asset1: Address,
        initial_price: SqrtPriceX96,
        tick_spacing: I24,
        pool_fee: U24
    ) -> eyre::Result<PoolKey> {
        self.mock_reward()
            .initializePool(asset0, asset1, U256::ZERO, *initial_price)
            .from(self.controller())
            .run_safe()
            .await?;
        let pool_key = PoolKey {
            currency0:   asset0,
            currency1:   asset1,
            fee:         pool_fee,
            tickSpacing: tick_spacing,
            hooks:       self.mock_reward
        };
        // TODO:  Make this tick spacing work properly
        self.mock_reward()
            .configurePool(asset0, asset1, 60, pool_fee)
            .from(self.controller())
            .run_safe()
            .await?;

        Ok(pool_key)
    }
}

impl MockRewardEnv<UniswapEnv<WalletProvider>> {
    pub async fn with_anvil(anvil: WalletProvider) -> eyre::Result<Self> {
        let inner = UniswapEnv::with_anvil(anvil).await?;
        Self::new(inner).await
    }
}

impl<E> TestAnvilEnvironment for MockRewardEnv<E>
where
    E: TestUniswapEnv
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

impl<E> TestUniswapEnv for MockRewardEnv<E>
where
    E: TestUniswapEnv
{
    fn pool_gate(&self) -> Address {
        self.inner.pool_gate()
    }

    fn pool_manager(&self) -> Address {
        self.inner.pool_manager()
    }

    async fn add_liquidity_position(
        &self,
        asset0: Address,
        asset1: Address,
        lower_tick: I24,
        upper_tick: I24,
        liquidity: U256
    ) -> eyre::Result<TxHash> {
        self.inner
            .add_liquidity_position(asset0, asset1, lower_tick, upper_tick, liquidity)
            .await
    }
}

impl<E> TestMockRewardEnv for MockRewardEnv<E>
where
    E: TestUniswapEnv
{
    fn mock_reward(&self) -> Address {
        self.mock_reward
    }
}

#[cfg(test)]
mod tests {
    use super::MockRewardEnv;
    use crate::{
        anvil_state_provider::AnvilStateProviderWrapper,
        contracts::environment::uniswap::UniswapEnv
    };

    #[tokio::test]
    async fn can_be_constructed() {
        let anvil = AnvilStateProviderWrapper::spawn_new_isolated()
            .await
            .unwrap();
        let uniswap = UniswapEnv::new(anvil.wallet_provider()).await.unwrap();
        MockRewardEnv::new(uniswap).await.unwrap();
    }
}
