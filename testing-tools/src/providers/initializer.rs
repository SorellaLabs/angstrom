use alloy::{
    node_bindings::AnvilInstance,
    primitives::{
        Address,
        aliases::{I24, U24}
    },
    providers::{Provider, ext::AnvilApi}
};
use alloy_primitives::{FixedBytes, U256};
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::{AngstromInstance, PoolKey},
        controller_v_1::ControllerV1::ControllerV1Instance,
        pool_gate::PoolGate::PoolGateInstance
    },
    matching::SqrtPriceX96,
    testnet::InitialTestnetState
};
use itertools::Itertools;
use reth_tasks::TaskExecutor;

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
            for (j, (cur0, cur1)) in self.deploy_currencies(&key).await?.into_iter().enumerate() {
                self.deploy_pool_full(
                    key.make_pool_key(*self.angstrom.address(), cur0, cur1),
                    key.initial_liquidity(),
                    key.sqrt_price(),
                    U256::from(i + j)
                )
                .await?
            }
        }

        Ok(())
    }

    async fn deploy_tokens(&mut self, nonce: &mut u64) -> eyre::Result<Vec<(Address, Address)>> {
        // deploys the tokens
        let mut tokens_with_meta = Vec::new();
        for token_to_deploy in &self.initial_state_config.tokens_to_deploy {
            let token_addr = token_to_deploy
                .deploy_token(&self.provider, nonce, &mut self.pending_state)
                .await?;
            tokens_with_meta.push((token_addr, token_to_deploy.clone()));
        }
        self.pending_state.finalize_pending_txs().await?;

        // sets the token metas
        let mut tokens = Vec::new();
        for (token_addr, token_meta) in &tokens_with_meta {
            token_meta
                .set_token_meta(&self.provider, *token_addr, nonce, &mut self.pending_state)
                .await?;
        }
        self.pending_state.finalize_pending_txs().await?;

        // overrides address and sets user amounts
        for (token_addr, token_meta) in &mut tokens_with_meta {
            token_meta
                .set_address_overrides(
                    &self.provider,
                    token_addr,
                    nonce,
                    &mut self.pending_state,
                    Some(&self.initial_state_config.addresses_with_tokens)
                )
                .await?;

            tokens.push(*token_addr);
        }
        self.pending_state.finalize_pending_txs().await?;
        let tokens = tokens
            .into_iter()
            .tuple_windows()
            .map(|(t0, t1)| {
                let (token0, token1) = (t0, t1);
                if token0 < token1 { (token0, token1) } else { (token1, token0) }
            })
            .collect_vec();

        self.rpc_provider().anvil_mine(Some(1), None).await?;

        Ok(tokens)
    }

    pub async fn deploy_currencies(
        &mut self,
        c: &PartialConfigPoolKey
    ) -> eyre::Result<Vec<(Address, Address)>> {
        let mut nonce = self
            .provider
            .provider
            .get_transaction_count(self.provider.controller())
            .await?;

        let cur = self.deploy_tokens(&mut nonce).await?;
        for (currency0, currency1) in &cur {
            let pool_key = PoolKey {
                currency0:   *currency0,
                currency1:   *currency1,
                fee:         U24::from(c.fee),
                tickSpacing: I24::unchecked_from(c.tick_spacing),
                hooks:       *self.angstrom.address()
            };
            self.pending_state.add_pool_key(pool_key.clone());
        }

        Ok(cur)
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

        let tick = price.to_tick()?;
        let lowest_tick = I24::unchecked_from(tick - (pool_key.tickSpacing.as_i32() * 1000));
        let highest_tick = I24::unchecked_from(tick + (pool_key.tickSpacing.as_i32() * 1000));
        tracing::info!(?lowest_tick, ?highest_tick, "initalizing single range at");

        let add_liq = self
            .pool_gate
            .addLiquidity(
                pool_key.currency0,
                pool_key.currency1,
                lowest_tick,
                highest_tick,
                U256::from(liquidity),
                FixedBytes::<32>::default()
            )
            .from(self.provider.controller())
            .nonce(nonce + 3)
            .deploy_pending()
            .await?;

        self.pending_state.add_pending_tx(add_liq);
        self.rpc_provider().anvil_mine(Some(1), None).await?;

        Ok(())
    }

    pub async fn initialize_state(
        &mut self,
        ex: TaskExecutor
    ) -> eyre::Result<InitialTestnetState> {
        let (pool_keys, _) = self.pending_state.finalize_pending_txs().await?;

        let state_bytes = self.provider.provider_ref().anvil_dump_state().await?;
        let state = InitialTestnetState::new(
            self.angstrom_env.angstrom(),
            self.angstrom_env.pool_manager(),
            Some(state_bytes),
            pool_keys.clone(),
            ex
        );

        tracing::info!("initalized angstrom pool state");

        Ok(state)
    }

    pub async fn initialize_state_no_bytes(
        &mut self,
        ex: TaskExecutor
    ) -> eyre::Result<InitialTestnetState> {
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
            pool_keys.clone(),
            ex
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
