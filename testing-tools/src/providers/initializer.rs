use alloy::{
    node_bindings::AnvilInstance,
    primitives::{
        Address,
        aliases::{I24, U24},
        keccak256
    },
    providers::{Provider, ext::AnvilApi}
};
use alloy_primitives::{FixedBytes, U256};
use alloy_sol_types::SolValue;
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::{AngstromInstance, PoolKey},
        controller_v_1::ControllerV1::ControllerV1Instance,
        pool_gate::PoolGate::PoolGateInstance
    },
    matching::SqrtPriceX96,
    testnet::InitialTestnetState
};
use rand::{Rng, thread_rng};

use super::WalletProvider;
use crate::{
    contracts::{
        anvil::{SafeDeployPending, WalletProviderRpc},
        environment::{
            TestAnvilEnvironment,
            angstrom::AngstromEnv,
            uniswap::{TestUniswapEnv, UniswapEnv}
        }
    },
    types::{
        GlobalTestingConfig, WithWalletProvider,
        config::TestingNodeConfig,
        initial_state::{InitialStateConfig, PartialConfigPoolKey, PendingDeployedPools}
    }
};

pub struct AnvilInitializer {
    provider:             WalletProvider,
    angstrom_env:         AngstromEnv<UniswapEnv<WalletProvider>>,
    controller_v1:        ControllerV1Instance<(), WalletProviderRpc>,
    angstrom:             AngstromInstance<(), WalletProviderRpc>,
    pool_gate:            PoolGateInstance<(), WalletProviderRpc>,
    pending_state:        PendingDeployedPools,
    initial_state_config: InitialStateConfig
}

impl AnvilInitializer {
    pub async fn new<G: GlobalTestingConfig>(
        config: TestingNodeConfig<G>,
        nodes: Vec<Address>
    ) -> eyre::Result<(Self, Option<AnvilInstance>)> {
        let (provider, anvil) = config.spawn_anvil_rpc().await?;

        tracing::debug!("deploying UniV4 enviroment");
        let uniswap_env = UniswapEnv::new(provider.clone()).await?;
        tracing::info!("deployed UniV4 enviroment");

        tracing::debug!("deploying Angstrom enviroment");
        let angstrom_env = AngstromEnv::new(uniswap_env, nodes).await?;
        let addr = angstrom_env.angstrom();
        tracing::info!(?addr, "deployed Angstrom enviroment");

        let angstrom =
            AngstromInstance::new(angstrom_env.angstrom(), angstrom_env.provider().clone());

        let pool_gate =
            PoolGateInstance::new(angstrom_env.pool_gate(), angstrom_env.provider().clone());

        let controller_v1 = ControllerV1Instance::new(
            angstrom_env.controller_v1(),
            angstrom_env.provider().clone()
        );

        let pending_state = PendingDeployedPools::new();

        let this = Self {
            provider,
            controller_v1,
            angstrom_env,
            angstrom,
            pending_state,
            pool_gate,
            initial_state_config: config.global_config.initial_state_config()
        };

        Ok((this, anvil))
    }

    /// deploys multiple pools (pool key, liquidity, sqrt price)
    pub async fn deploy_pool_fulls(
        &mut self,
        pool_keys: Vec<PartialConfigPoolKey>
    ) -> eyre::Result<()> {
        for (i, key) in pool_keys.into_iter().enumerate() {
            let (cur0, cur1) = self.deploy_currencies(&key).await?;
            self.deploy_pool_full(
                key.make_pool_key(*self.angstrom.address(), cur0, cur1),
                key.initial_liquidity(),
                key.sqrt_price(),
                U256::from(i)
            )
            .await?
        }

        Ok(())
    }

    async fn deploy_tokens(&mut self, nonce: &mut u64) -> eyre::Result<(Address, Address)> {
        let mut tokens = Vec::new();
        for token_to_deploy in &self.initial_state_config.tokens_to_deploy {
            let token = token_to_deploy
                .deploy_token(
                    &self.provider,
                    nonce,
                    Some(&self.initial_state_config.addresses_with_tokens)
                )
                .await?;
            tokens.push(token);
        }

        let (token0, token1) = (tokens[0], tokens[1]);
        let tokens = if token0 < token1 { (token0, token1) } else { (token1, token0) };

        self.rpc_provider().anvil_mine(Some(1), None).await?;

        Ok(tokens)
    }

    pub async fn deploy_currencies(
        &mut self,
        c: &PartialConfigPoolKey
    ) -> eyre::Result<(Address, Address)> {
        let mut nonce = self
            .provider
            .provider
            .get_transaction_count(self.provider.controller())
            .await?;

        let (currency0, currency1) = self.deploy_tokens(&mut nonce).await?;

        let pool_key = PoolKey {
            currency0,
            currency1,
            fee: U24::from(c.fee),
            tickSpacing: I24::unchecked_from(c.tick_spacing),
            hooks: *self.angstrom.address()
        };
        self.pending_state.add_pool_key(pool_key.clone());

        Ok((currency0, currency1))
    }

    /// deploys tokens, a uniV4 pool, angstrom pool
    pub async fn deploy_default_pool_full(&mut self) -> eyre::Result<()> {
        let mut nonce = self
            .provider
            .provider
            .get_transaction_count(self.provider.controller())
            .await?;

        let (currency0, currency1) = self.deploy_tokens(&mut nonce).await?;

        let pool_key = PoolKey {
            currency0,
            currency1,
            fee: U24::ZERO,
            tickSpacing: I24::unchecked_from(60),
            hooks: *self.angstrom.address()
        };
        self.pending_state.add_pool_key(pool_key.clone());

        let liquidity = u128::MAX - 1;
        let price = SqrtPriceX96::at_tick(100_020)?;

        self.deploy_pool_full(pool_key, liquidity, price, U256::ZERO)
            .await?;

        Ok(())
    }

    /// deploys tokens, a uniV4 pool, angstrom pool
    async fn deploy_pool_full(
        &mut self,
        pool_key: PoolKey,
        liquidity: u128,
        price: SqrtPriceX96,
        store_index: U256
    ) -> eyre::Result<()> {
        tracing::info!(?pool_key, ?liquidity, ?price, ?store_index);
        let nonce = self
            .provider
            .provider
            .get_transaction_count(self.provider.controller())
            .await?;

        self.pending_state.add_pool_key(pool_key.clone());
        let encoded = keccak256(pool_key.abi_encode());
        tracing::info!(?pool_key, ?encoded, ?price);

        tracing::debug!("configuring pool");
        let controller_configure_pool = self
            .controller_v1
            .configurePool(
                pool_key.currency0,
                pool_key.currency1,
                pool_key.tickSpacing.as_i32() as u16,
                pool_key.fee,
                pool_key.fee
            )
            .from(self.provider.controller())
            .nonce(nonce)
            .deploy_pending()
            .await?;
        tracing::debug!("success: controller_configure_pool");
        self.pending_state.add_pending_tx(controller_configure_pool);

        tracing::debug!("initializing pool");
        let initialize_angstrom_pool = self
            .angstrom
            .initializePool(pool_key.currency0, pool_key.currency1, store_index, *price)
            .from(self.provider.controller())
            .nonce(nonce + 1)
            .deploy_pending()
            .await?;
        tracing::debug!("success: angstrom.initializePool");
        self.pending_state.add_pending_tx(initialize_angstrom_pool);

        tracing::debug!("tick spacing");
        let pool_gate = self
            .pool_gate
            .tickSpacing(pool_key.tickSpacing)
            .from(self.provider.controller())
            .nonce(nonce + 2)
            .deploy_pending()
            .await?;
        tracing::debug!("success: pool_gate");
        self.pending_state.add_pending_tx(pool_gate);

        let mut rng = thread_rng();

        let tick = price.to_tick()?;
        for i in 0..400 {
            let lower = I24::unchecked_from(tick - (pool_key.tickSpacing.as_i32() * (200 - i)));
            let upper = lower + pool_key.tickSpacing;
            let liquidity = U256::from(rng.gen_range(liquidity / 2..liquidity));

            let add_liq = self
                .pool_gate
                .addLiquidity(
                    pool_key.currency0,
                    pool_key.currency1,
                    lower,
                    upper,
                    liquidity,
                    FixedBytes::<32>::default()
                )
                .from(self.provider.controller())
                .nonce(nonce + 3 + (i as u64))
                .deploy_pending()
                .await?;
            self.pending_state.add_pending_tx(add_liq);
            tracing::trace!(lower_tick = ?lower, upper_tick = ?upper, ?liquidity, "Adding liquidity to pool");
        }

        Ok(())
    }

    pub async fn initialize_state(&mut self) -> eyre::Result<InitialTestnetState> {
        let (pool_keys, _) = self.pending_state.finalize_pending_txs().await?;

        let state_bytes = self.provider.provider_ref().anvil_dump_state().await?;
        let state = InitialTestnetState::new(
            self.angstrom_env.angstrom(),
            self.angstrom_env.pool_manager(),
            Some(state_bytes),
            pool_keys.clone()
        );

        tracing::info!("initalized angstrom pool state");

        Ok(state)
    }

    pub async fn initialize_state_no_bytes(&mut self) -> eyre::Result<InitialTestnetState> {
        let (pool_keys, tx_hash) = self.pending_state.finalize_pending_txs().await?;
        for hash in tx_hash {
            let transaction = self
                .provider
                .provider
                .get_transaction_receipt(hash)
                .await
                .unwrap()
                .unwrap();
            let status = transaction.status();
            if !status {
                tracing::warn!(?hash, "transaction hash failed");
            }
        }

        let state = InitialTestnetState::new(
            self.angstrom_env.angstrom(),
            self.angstrom_env.pool_manager(),
            None,
            pool_keys.clone()
        );
        for key in pool_keys {
            let out = self
                .pool_gate
                .isInitialized(key.currency0, key.currency1)
                .call()
                .await?;
            if !out._0 {
                tracing::warn!(?key, "pool is still not initalized, even after deploying state");
            }
        }
        tracing::info!("initalized angstrom pool state");

        Ok(state)
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
    use alloy_primitives::address;
    use angstrom_types::sol_bindings::testnet::MockERC20::MockERC20Instance;
    use reth_provider::noop::NoopProvider;

    use super::*;
    use crate::{
        controllers::enviroments::AngstromTestnet,
        types::{
            config::TestnetConfig,
            initial_state::{Erc20ToDeploy, InitialStateConfig}
        },
        utils::{init_tracing, noop_agent}
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn test_hack_address_tokens() {
        init_tracing(5);

        let my_address = address!("796fB50EAe1456A523F869f6135dd557eeaEE226");
        let initial_state_config = InitialStateConfig {
            pool_keys:             vec![PartialConfigPoolKey {
                fee:               0,
                tick_spacing:      60,
                initial_liquidity: 34028236692,
                sqrt_price:        SqrtPriceX96::at_tick(100020).unwrap()
            }],
            addresses_with_tokens: vec![my_address],
            tokens_to_deploy:      vec![
                Erc20ToDeploy::new("Wrapped Bitcoin", "WBTC", None),
                Erc20ToDeploy::new("Wrapped Ethereum", "WETH", None),
            ]
        };
        let config = TestnetConfig::new(
            3,
            "ws://localhost:8546",
            false,
            Some(9445),
            Some(2624),
            initial_state_config
        );
        let testnet =
            AngstromTestnet::spawn_testnet(NoopProvider::default(), config, vec![noop_agent])
                .await
                .unwrap();

        let provider = testnet.node_provider(Some(1));

        let erc20_instance0 = MockERC20Instance::new(
            address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
            provider.rpc_provider()
        );
        let _my_balance = erc20_instance0.balanceOf(my_address).call().await.unwrap();
        // assert_eq!(my_balance.result, U256::from(HACKED_TOKEN_BALANCE));

        let erc20_instance1 = MockERC20Instance::new(
            address!("2260fac5e5542a773aa44fbcfedf7c193bc2c599"),
            provider.rpc_provider()
        );
        let _my_balance = erc20_instance1.balanceOf(my_address).call().await.unwrap();
        // assert_eq!(my_balance.result, U256::from(HACKED_TOKEN_BALANCE));
    }
}
