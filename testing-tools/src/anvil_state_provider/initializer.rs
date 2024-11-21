use alloy::{
    node_bindings::AnvilInstance,
    providers::{ext::AnvilApi, Provider},
    pubsub::PubSubFrontend
};
use alloy_primitives::{
    aliases::{I24, U24},
    Address, FixedBytes, U256
};
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::{AngstromInstance, PoolKey},
        mintable_mock_erc_20::MintableMockERC20,
        pool_gate::PoolGate::PoolGateInstance
    },
    matching::SqrtPriceX96,
    testnet::InitialTestnetState
};

use super::{utils::WalletProviderRpc, WalletProvider};
use crate::contracts::DebugTransaction;
use crate::{
    contracts::environment::{
        angstrom::AngstromEnv,
        uniswap::{TestUniswapEnv, UniswapEnv},
        TestAnvilEnvironment
    },
    types::{initial_state::PendingDeployedPools, TestingConfig}
};

const ANVIL_TESTNET_DEPLOYMENT_ENDPOINT: &str = "temp_deploy";

pub struct AnvilInitializer {
    provider:      WalletProvider,
    angstrom_env:  AngstromEnv<UniswapEnv<WalletProvider>>,
    angstrom:      AngstromInstance<PubSubFrontend, WalletProviderRpc>,
    pool_gate:     PoolGateInstance<PubSubFrontend, WalletProviderRpc>,
    pending_state: PendingDeployedPools
    // TODO: revert after testing finished
    // _instance:     AnvilInstance
}

impl AnvilInitializer {
    pub async fn new(config: impl TestingConfig) -> eyre::Result<Self> {
        let (wallet_provider, _anvil) = config.spawn_rpc(ANVIL_TESTNET_DEPLOYMENT_ENDPOINT).await?;

        tracing::debug!("deploying UniV4 enviroment");
        let uniswap_env = UniswapEnv::new(wallet_provider.clone()).await?;
        tracing::info!("deployed UniV4 enviroment");

        tracing::debug!("deploying Angstrom enviroment");
        let angstrom_env = AngstromEnv::new(uniswap_env).await?;
        tracing::info!("deployed Angstrom enviroment");

        let angstrom =
            AngstromInstance::new(angstrom_env.angstrom(), angstrom_env.provider().clone());
        let pool_gate =
            PoolGateInstance::new(angstrom_env.pool_gate(), angstrom_env.provider().clone());

        let pending_state = PendingDeployedPools::new();

        Ok(Self {
            provider: wallet_provider,
            angstrom_env,
            angstrom,
            pool_gate,
            pending_state /* TODO: revert after testing finished
                           * _instance: anvil.unwrap() */
        })
    }

    /// deploys tokens, a uniV4 pool, angstrom pool
    pub async fn deploy_pool_full(&mut self) -> eyre::Result<()> {
        let first_token = MintableMockERC20::deploy(self.provider.provider_ref()).await?;
        let second_token = MintableMockERC20::deploy(self.provider.provider_ref()).await?;

        let (currency0, currency1) = if *first_token.address() < *second_token.address() {
            (*first_token.address(), *second_token.address())
        } else {
            (*second_token.address(), *first_token.address())
        };

        let fee = U24::ZERO;
        let pool = PoolKey {
            currency0,
            currency1,
            fee,
            tickSpacing: I24::unchecked_from(10),
            hooks: *self.angstrom.address()
        };
        self.pending_state.add_pool_key(pool.clone());

        let liquidity = 1_000_000_000_000_000_u128;
        let price = SqrtPriceX96::at_tick(100000)?;

        self.angstrom
            .configurePool(pool.currency0, pool.currency1, 10, U24::ZERO)
            .from(self.provider.controller_address)
            .run_safe()
            .await?;
        
        self.angstrom
            .initializePool(pool.currency0, pool.currency1, U256::ZERO, *price)
            .from(self.provider.controller_address)
            .run_safe()
            .await?;

        let set_tick_spacing = self
            .pool_gate
            .tickSpacing(pool.tickSpacing)
            .from(self.provider.controller_address)
            .run_safe()
            .await?;

        let add_liq = self
            .pool_gate
            .addLiquidity(
                pool.currency0,
                pool.currency1,
                I24::unchecked_from(99000),
                I24::unchecked_from(101000),
                U256::from(liquidity),
                FixedBytes::<32>::default()
            )
            .from(self.provider.controller_address)
            .run_safe()
            .await?;

        Ok(())
    }

    pub async fn initialize_state(&mut self) -> eyre::Result<InitialTestnetState> {
        let (pool_keys, _) = self.pending_state.finalize_pending_txs().await?;
        let state_bytes = self.provider.provider_ref().anvil_dump_state().await?;
        Ok(InitialTestnetState::new(
            self.angstrom_env.angstrom(),
            self.angstrom_env.pool_manager(),
            Some(state_bytes),
            pool_keys
        ))
    }
}

#[cfg(test)]
mod tests {
    use alloy::providers::Provider;

    use super::*;
    use crate::controllers::devnet::DevnetConfig;

    async fn get_block(provider: &WalletProvider) -> eyre::Result<u64> {
        Ok(provider.provider.get_block_number().await?)
    }

    #[tokio::test]
    async fn test_can_deploy() {
        let config = DevnetConfig::default();
        let mut initializer = AnvilInitializer::new(config).await.unwrap();
        initializer.deploy_pool_full().await.unwrap();

        let current_block = get_block(&initializer.provider).await.unwrap();

        let _ = initializer.provider.provider.evm_mine(None).await.unwrap();

        assert_eq!(current_block + 1, get_block(&initializer.provider).await.unwrap());

        initializer
            .pending_state
            .finalize_pending_txs()
            .await
            .unwrap();

        assert_eq!(current_block + 1, get_block(&initializer.provider).await.unwrap());
    }
}
