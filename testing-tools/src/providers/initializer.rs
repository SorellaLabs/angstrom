use alloy::{node_bindings::AnvilInstance, providers::ext::AnvilApi, pubsub::PubSubFrontend};
use alloy_primitives::{
    aliases::{I24, U24},
    FixedBytes, U256
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
use crate::{
    contracts::{
        environment::{
            angstrom::AngstromEnv,
            uniswap::{TestUniswapEnv, UniswapEnv},
            TestAnvilEnvironment
        },
        DebugTransaction
    },
    types::{
        config::TestingNodeConfig, initial_state::PendingDeployedPools, GlobalTestingConfig,
        WithWalletProvider
    }
};

pub const ANVIL_TESTNET_DEPLOYMENT_ENDPOINT: &str = "temp_deploy";

pub struct AnvilInitializer {
    provider:      WalletProvider,
    //uniswap_env:   UniswapEnv<WalletProvider>,
    angstrom_env:  AngstromEnv<UniswapEnv<WalletProvider>>,
    angstrom:      AngstromInstance<PubSubFrontend, WalletProviderRpc>,
    pool_gate:     PoolGateInstance<PubSubFrontend, WalletProviderRpc>,
    pending_state: PendingDeployedPools
}

impl AnvilInitializer {
    pub async fn new<G: GlobalTestingConfig>(
        config: TestingNodeConfig<G>
    ) -> eyre::Result<(Self, Option<AnvilInstance>)> {
        let (provider, anvil) = config.spawn_anvil_rpc().await?;

        tracing::debug!("deploying UniV4 enviroment");
        let uniswap_env = UniswapEnv::new(provider.clone()).await?;
        tracing::info!("deployed UniV4 enviroment");

        tracing::debug!("deploying Angstrom enviroment");
        let angstrom_env = AngstromEnv::new(uniswap_env).await?;
        tracing::info!("deployed Angstrom enviroment");

        let angstrom =
            AngstromInstance::new(angstrom_env.angstrom(), angstrom_env.provider().clone());
        let pool_gate =
            PoolGateInstance::new(angstrom_env.pool_gate(), angstrom_env.provider().clone());

        let pending_state = PendingDeployedPools::new();

        let this = Self { provider, angstrom_env, angstrom, pool_gate, pending_state };

        Ok((this, anvil))
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
            .from(self.provider.controller_address())
            .run_safe()
            .await?;

        self.angstrom
            .initializePool(pool.currency0, pool.currency1, U256::ZERO, *price)
            .from(self.provider.controller_address())
            .run_safe()
            .await?;

        self.pool_gate
            .tickSpacing(pool.tickSpacing)
            .from(self.provider.controller_address())
            .run_safe()
            .await?;

        self.pool_gate
            .addLiquidity(
                pool.currency0,
                pool.currency1,
                I24::unchecked_from(99000),
                I24::unchecked_from(101000),
                U256::from(liquidity),
                FixedBytes::<32>::default()
            )
            .from(self.provider.controller_address())
            .run_safe()
            .await?;

        Ok(())
    }

    pub async fn initialize_state(&mut self) -> eyre::Result<InitialTestnetState> {
        let (pool_keys, txs) = self.pending_state.finalize_pending_txs().await?;
        let state_bytes = self.provider.provider_ref().anvil_dump_state().await?;
        Ok(InitialTestnetState::new(
            self.angstrom_env.angstrom(),
            self.angstrom_env.pool_manager(),
            Some(state_bytes),
            pool_keys
        ))
    }
}

impl WithWalletProvider for AnvilInitializer {
    fn wallet_provider(&self) -> WalletProvider {
        self.provider.clone()
    }

    fn rpc_provider(&self) -> WalletProviderRpc {
        self.provider.provider.clone()
    }
}

#[cfg(test)]
mod tests {
    use alloy::providers::Provider;
    use rand::thread_rng;
    use secp256k1::{Secp256k1, SecretKey};

    use super::*;
    use crate::types::config::DevnetConfig;

    async fn get_block(provider: &WalletProvider) -> eyre::Result<u64> {
        Ok(provider.provider.get_block_number().await?)
    }

    #[tokio::test]
    async fn test_can_deploy() {
        let sk = SecretKey::new(&mut thread_rng());
        let config = TestingNodeConfig {
            node_id:       0,
            global_config: DevnetConfig::default(),
            pub_key:       sk.public_key(&Secp256k1::default()),
            secret_key:    sk,
            voting_power:  100
        };
        let (mut initializer, _anvil) = AnvilInitializer::new(config).await.unwrap();

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
