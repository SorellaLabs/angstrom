use alloy::{
    node_bindings::AnvilInstance,
    primitives::{
        aliases::{I24, U24},
        keccak256, Address
    },
    providers::{ext::AnvilApi, Provider},
    pubsub::PubSubFrontend
};
use alloy_primitives::{FixedBytes, U256};
use alloy_sol_types::SolValue;
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::{AngstromInstance, PoolKey},
        controller_v_1::ControllerV1::ControllerV1Instance,
        mintable_mock_erc_20::MintableMockERC20,
        pool_gate::PoolGate::PoolGateInstance
    },
    matching::SqrtPriceX96,
    testnet::InitialTestnetState
};
use validation::common::WETH_ADDRESS;

use super::WalletProvider;
use crate::{
    contracts::{
        anvil::{SafeDeployPending, WalletProviderRpc},
        environment::{
            angstrom::AngstromEnv,
            uniswap::{TestUniswapEnv, UniswapEnv},
            TestAnvilEnvironment
        }
    },
    types::{
        config::TestingNodeConfig,
        initial_state::{PartialConfigPoolKey, PendingDeployedPools},
        GlobalTestingConfig, WithWalletProvider
    }
};

pub struct AnvilInitializer {
    provider:      WalletProvider,
    angstrom_env:  AngstromEnv<UniswapEnv<WalletProvider>>,
    controller_v1: ControllerV1Instance<PubSubFrontend, WalletProviderRpc>,
    angstrom:      AngstromInstance<PubSubFrontend, WalletProviderRpc>,
    pool_gate:     PoolGateInstance<PubSubFrontend, WalletProviderRpc>,
    pending_state: PendingDeployedPools
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
        tracing::info!("deployed Angstrom enviroment");

        let angstrom =
            AngstromInstance::new(angstrom_env.angstrom(), angstrom_env.provider().clone());
        let pool_gate =
            PoolGateInstance::new(angstrom_env.pool_gate(), angstrom_env.provider().clone());
        let controller_v1 = ControllerV1Instance::new(
            angstrom_env.controller_v1(),
            angstrom_env.provider().clone()
        );

        let pending_state = PendingDeployedPools::new();

        let this =
            Self { provider, controller_v1, angstrom_env, angstrom, pool_gate, pending_state };

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

    pub async fn deploy_currencies(
        &mut self,
        c: &PartialConfigPoolKey
    ) -> eyre::Result<(Address, Address)> {
        let nonce = self
            .provider
            .provider
            .get_transaction_count(self.provider.controller())
            .await?;

        let (first_token_tx, first_token) =
            MintableMockERC20::deploy_builder(self.provider.provider_ref())
                .deploy_pending_creation(nonce, self.provider.controller())
                .await?;
        self.pending_state.add_pending_tx(first_token_tx);

        let (second_token_tx, mut second_token) =
            MintableMockERC20::deploy_builder(self.provider.provider_ref())
                .deploy_pending_creation(nonce + 1, self.provider.controller())
                .await?;
        self.pending_state.add_pending_tx(second_token_tx);

        // wait for them to be mined.
        self.pending_state.finalize_pending_txs().await?;

        // lets load the bytecode of the second token, then use the bytecode to override
        // the weth address
        let code = self
            .provider
            .rpc_provider()
            .get_code_at(second_token)
            .await
            .unwrap();
        self.provider
            .rpc_provider()
            .anvil_set_code(WETH_ADDRESS, code)
            .await?;
        second_token = WETH_ADDRESS;

        let (currency0, currency1) = if first_token < second_token {
            (first_token, second_token)
        } else {
            (second_token, first_token)
        };

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
        let nonce = self
            .provider
            .provider
            .get_transaction_count(self.provider.controller())
            .await?;

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
        let nonce = self
            .provider
            .provider
            .get_transaction_count(self.provider.controller())
            .await?;

        self.pending_state.add_pool_key(pool_key.clone());
        let encoded = keccak256(pool_key.abi_encode());
        tracing::info!(?pool_key, ?encoded, ?price);

        tracing::debug!("configuring pool");
        let configure_pool = self
            .angstrom
            .configurePool(
                pool_key.currency0,
                pool_key.currency1,
                pool_key.tickSpacing.as_i32() as u16,
                pool_key.fee
            )
            .from(self.provider.controller())
            .nonce(nonce)
            .deploy_pending()
            .await?;
        self.pending_state.add_pending_tx(configure_pool);

        let controller_configure_pool = self
            .controller_v1
            .addPoolToMap(pool_key.currency0, pool_key.currency1)
            // .configurePool(
            //     pool_key.currency0,
            //     pool_key.currency1,
            //     pool_key.tickSpacing.as_i32() as u16,
            //     pool_key.fee
            // )
            .from(self.provider.controller())
            .nonce(nonce + 1)
            .deploy_pending()
            .await?;
        self.pending_state.add_pending_tx(controller_configure_pool);

        tracing::debug!("initializing pool");
        self.angstrom
            .initializePool(pool_key.currency0, pool_key.currency1, store_index, *price)
            .from(self.provider.controller())
            .nonce(nonce + 2)
            .deploy_pending()
            .await?;

        tracing::debug!("tick spacing");
        let pool_gate = self
            .pool_gate
            .tickSpacing(pool_key.tickSpacing)
            .from(self.provider.controller())
            .nonce(nonce + 3)
            .deploy_pending()
            .await?;

        self.pending_state.add_pending_tx(pool_gate);

        let tick = price.to_tick()?;
        for i in 0..200 {
            let lower = I24::unchecked_from(tick - (pool_key.tickSpacing.as_i32() * (101 - i)));
            let upper = lower + pool_key.tickSpacing;

            let add_liq = self
                .pool_gate
                .addLiquidity(
                    pool_key.currency0,
                    pool_key.currency1,
                    lower,
                    upper,
                    U256::from(liquidity),
                    FixedBytes::<32>::default()
                )
                .from(self.provider.controller())
                .nonce(nonce + 4 + (i as u64))
                .deploy_pending()
                .await?;

            self.pending_state.add_pending_tx(add_liq);
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

    pub async fn initialize_state_no_bytes(&mut self) -> eyre::Result<InitialTestnetState> {
        let (pool_keys, _) = self.pending_state.finalize_pending_txs().await?;

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
    use alloy::providers::Provider;
    use alloy_primitives::TxKind;
    use alloy_rpc_types::{BlockId, TransactionInput, TransactionRequest};
    use alloy_sol_types::SolCall;
    use angstrom_types::{
        contract_bindings::controller_v_1::ControllerV1,
        contract_payloads::angstrom::AngstromPoolConfigStore
    };

    use super::*;
    use crate::types::config::DevnetConfig;

    async fn get_block(provider: &WalletProvider) -> eyre::Result<u64> {
        Ok(provider.provider.get_block_number().await?)
    }

    async fn view_call<IC>(
        provider: &WalletProvider,
        contract: Address,
        call: IC
    ) -> eyre::Result<IC::Return>
    where
        IC: SolCall + Send
    {
        let tx = TransactionRequest {
            to: Some(TxKind::Call(contract)),
            input: TransactionInput::both(call.abi_encode().into()),
            ..Default::default()
        };

        Ok(IC::abi_decode_returns(&provider.provider().call(&tx).await?, true)?)
    }

    #[tokio::test]
    async fn test_can_deploy() {
        let config = TestingNodeConfig::new(0, DevnetConfig::default(), 100);

        let (mut initializer, _anvil) = AnvilInitializer::new(config, vec![]).await.unwrap();

        initializer.deploy_default_pool_full().await.unwrap();

        let current_block = get_block(&initializer.provider).await.unwrap();

        let _ = initializer.provider.provider.evm_mine(None).await.unwrap();

        assert_eq!(current_block + 1, get_block(&initializer.provider).await.unwrap());

        initializer
            .pending_state
            .finalize_pending_txs()
            .await
            .unwrap();

        assert_eq!(current_block + 1, get_block(&initializer.provider).await.unwrap());

        let config_store = AngstromPoolConfigStore::load_from_chain(
            *initializer.angstrom.address(),
            BlockId::latest(),
            &initializer.provider.provider()
        )
        .await
        .unwrap();

        let partial_keys = config_store
            .all_entries()
            .iter()
            .map(|val| FixedBytes::from(*val.pool_partial_key))
            .collect::<Vec<_>>();

        println!("{partial_keys:?}");

        let all_pools_call = view_call(
            &initializer.provider,
            *initializer.controller_v1.address(),
            ControllerV1::getAllPoolsCall { storeKeys: partial_keys }
        )
        .await
        .unwrap();

        println!("{all_pools_call:?}")
    }
}
