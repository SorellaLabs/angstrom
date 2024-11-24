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
use crate::{
    contracts::{
        anvil::SafeDeployPending,
        environment::{
            angstrom::AngstromEnv,
            uniswap::{TestUniswapEnv, UniswapEnv},
            TestAnvilEnvironment
        }
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
        let nonce = self
            .provider
            .provider
            .get_account(self.provider.controller())
            .await?
            .nonce;

        let (first_token_tx, first_token) =
            MintableMockERC20::deploy_builder(self.provider.provider_ref())
                .deploy_pending_creation(nonce, self.provider.controller())
                .await?;
        self.pending_state.add_pending_tx(first_token_tx);

        let (second_token_tx, second_token) =
            MintableMockERC20::deploy_builder(self.provider.provider_ref())
                .deploy_pending_creation(nonce + 1, self.provider.controller())
                .await?;
        self.pending_state.add_pending_tx(second_token_tx);

        let (currency0, currency1) = if first_token < second_token {
            (first_token, second_token)
        } else {
            (second_token, first_token)
        };

        let fee = U24::ZERO;
        let tick_spacing = 10;
        let pool = PoolKey {
            currency0,
            currency1,
            fee,
            tickSpacing: I24::unchecked_from(tick_spacing),
            hooks: Address::default()
        };
        self.pending_state.add_pool_key(pool.clone());

        let liquidity = 1_000_000_000_000_000_u128;
        let price = SqrtPriceX96::at_tick(100000)?;

        let init_pool = self
            .angstrom
            .initializePool(pool.currency0, pool.currency1, U256::ZERO, *price)
            .nonce(nonce + 2)
            .deploy_pending()
            .await?;
        self.pending_state.add_pending_tx(init_pool);

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
            .from(self.provider.controller())
            .nonce(nonce + 3)
            .deploy_pending()
            .await?;

        self.pending_state.add_pending_tx(add_liq);

        Ok(())
    }

    pub async fn initialize_state(&mut self) -> eyre::Result<InitialTestnetState> {
        let (pool_keys, txs) = self.pending_state.finalize_pending_txs().await?;
        for tx in txs {
            println!("tx {:?}", tx);
        }
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
        let (mut initializer, _) = AnvilInitializer::new(config).await.unwrap();

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