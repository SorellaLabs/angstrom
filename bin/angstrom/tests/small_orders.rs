use std::{
    collections::{HashMap, HashSet},
    sync::Arc
};

use alloy::{
    network::Ethereum,
    primitives::{Address, address},
    providers::{Provider, ProviderBuilder},
    rpc::{
        types as alloy_rpc_types,
        types::{BlockId, BlockNumberOrTag, Filter}
    },
    sol_types::SolEvent
};
use alloy_primitives::{B256, BlockNumber, I256, U256, aliases::I24};
use angstrom::components::initialize_strom_handles;
use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    block_sync::GlobalBlockSync,
    contract_bindings::{angstrom::Angstrom::PoolKey, controller_v_1::ControllerV1::*},
    contract_payloads::angstrom::{AngstromPoolConfigStore, UniswapAngstromRegistry},
    matching::SqrtPriceX96,
    pair_with_price::PairsWithPrice,
    primitive::*,
    reth_db_wrapper::{DBError, SetBlock},
    sol_bindings::Ray
};
use eyre::eyre;
use futures::StreamExt;
use matching_engine::MatchingManager;
use reth::{
    primitives::EthPrimitives,
    providers::{BlockHashReader, BlockNumReader, ProviderResult},
    revm as reth_revm,
    tasks::TokioTaskExecutor
};
use reth_provider::{CanonStateNotification, CanonStateSubscriptions, NodePrimitivesProvider};
use revm::{bytecode::Bytecode, state::AccountInfo};
use serde::{Deserialize, Serialize};
use testing_tools::type_generator::orders::UserOrderBuilder;
use tracing::*;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, *};
use uniswap_v4::{
    configure_uniswap_manager,
    uniswap::{
        pool::SwapResult,
        pool_manager::{SyncedUniswapPool, SyncedUniswapPools}
    }
};
use validation::{common::TokenPriceGenerator, init_validation, validator::ValidationClient};

const USDT: Address = address!("0xdac17f958d2ee523a2206206994597c13d831ec7");
const WETH: Address = address!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

// The goal of this test is to spinup matching engine and assert that our
// uniswap v4 pool swaps match against all critical points,
// 1) matching engine output <> bundle building endpoint
// 2) bundle_bundling <> revm sim
#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
pub async fn test_small_orders_matching_engine() {
    init_with_chain_id(1);
    init_tracing();

    let channels = initialize_strom_handles();
    let validation_client = ValidationClient(channels.validator_tx.clone());
    let url = std::env::var("RPC_URL").unwrap();

    let querying_provider: Arc<_> = ProviderBuilder::<_, _, Ethereum>::default()
        .with_recommended_fillers()
        // backup
        .connect(&url)
        .await
        .unwrap()
        .into();

    let block_id = querying_provider.get_block_number().await.unwrap();

    let angstrom_address = *ANGSTROM_ADDRESS.get().unwrap();
    let controller = *CONTROLLER_V1_ADDRESS.get().unwrap();
    let deploy_block = *ANGSTROM_DEPLOYED_BLOCK.get().unwrap();
    let gas_token = *GAS_TOKEN_ADDRESS.get().unwrap();
    let pool_manager = *POOL_MANAGER_ADDRESS.get().unwrap();

    let pool_config_store = Arc::new(
        AngstromPoolConfigStore::load_from_chain(
            angstrom_address,
            BlockId::Number(BlockNumberOrTag::Latest),
            &querying_provider
        )
        .await
        .unwrap()
    );

    // load the angstrom pools;
    tracing::info!("starting search for pools");
    let pools = fetch_angstrom_pools(
        deploy_block as usize,
        block_id as usize,
        angstrom_address,
        &querying_provider
    )
    .await;
    tracing::info!("found pools");
    let angstrom_tokens = pools
        .iter()
        .flat_map(|pool| [pool.currency0, pool.currency1])
        .fold(HashMap::<Address, usize>::new(), |mut acc, x| {
            *acc.entry(x).or_default() += 1;
            acc
        });

    let global_block_sync = GlobalBlockSync::new(block_id);

    let uniswap_registry: UniswapPoolRegistry = pools.into();
    let uni_ang_registry =
        UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());

    let (tx, rx) = tokio::sync::broadcast::channel(50);

    let (wrap_tx, wrap_rx) = tokio::sync::broadcast::channel(50);

    let network_stream = Box::pin(
        tokio_stream::wrappers::BroadcastStream::new(wrap_rx)
            .map(|data: Result<EthEvent, _>| data.unwrap())
    );

    let uniswap_pool_manager = configure_uniswap_manager::<_, 100>(
        querying_provider.clone(),
        rx,
        uniswap_registry.clone(),
        block_id,
        global_block_sync.clone(),
        pool_manager,
        network_stream
    )
    .await;

    let uniswap_pools = uniswap_pool_manager.pools();

    let price_generator = TokenPriceGenerator::new(
        querying_provider.clone(),
        block_id,
        uniswap_pools.clone(),
        gas_token,
        None
    )
    .await
    .expect("failed to start token price generator");
    let stream = MockStream { tx: tx.clone() };

    let update_stream = Box::pin(PairsWithPrice::into_price_update_stream(
        angstrom_address,
        stream.canonical_state_stream(),
        querying_provider.clone()
    ));

    init_validation(
        StateProvider { provider: querying_provider.clone() },
        block_id,
        angstrom_address,
        address!("0xc41ae140ca9b281d8a1dc254c50e446019517d04"),
        update_stream,
        uniswap_pools.clone(),
        price_generator,
        pool_config_store.clone(),
        channels.validator_rx
    );

    // get pool_key
    let pool_key = uniswap_registry.get_pools_by_token_pair(WETH, USDT)[0].clone();
    let pool = uniswap_pools
        .get(&PoolId::from(pool_key))
        .unwrap()
        .value()
        .clone();

    let estimation = OrderToEstimate {
        order_params: OrderParams {
            amount:      4e6 as u128,
            exact_in:    true,
            limit_price: None,
            slippage:    None,
            token_in:    USDT,
            token_out:   WETH
        }
    };

    println!("{:#?}", estimation);

    let output = quote_single_hop_order_inner(estimation, pool)
        .await
        .unwrap();
    println!("{:#?}", output);

    // next thing we need to do is run the matching engine to see results
    let signer = AngstromSigner::random();

    let order = UserOrderBuilder::default()
        .asset_in(USDT)
        .asset_out(WETH)
        .min_price(output.ucp)
        .exact_in(true)
        .exact()
        .kill_or_fill()
        .block(block_id + 1)
        .signing_key(Some(signer))
        .amount(output.amt_in)
        .with_storage()
        .bid()
        .valid_block(block_id + 1)
        .pool_id(PoolId::from(pool_key))
        .build();
    println!("{:#?}", order);

    let pool_snapshots = uniswap_pools
        .iter()
        .filter_map(|item| {
            let key = item.key();
            let pool = item.value();
            tracing::info!(?key, "getting snapshot");
            let (token_a, token_b, snapshot) = pool.read().unwrap().fetch_pool_snapshot().ok()?;
            let entry = uni_ang_registry.get_ang_entry(key)?;

            Some((*key, (token_a, token_b, snapshot, entry.store_index as u16)))
        })
        .collect::<HashMap<_, _>>();

    let executor = TokioTaskExecutor::default();
    let matching_manager = MatchingManager::new(executor, validation_client.clone());

    let output = matching_manager
        .build_proposal(vec![order], vec![], pool_snapshots)
        .await;

    assert!(true);
}

const INTERNAL_SLIPPAGE_BPS: u128 = 350;

async fn quote_single_hop_order_inner(
    order: OrderToEstimate,
    pool_state: SyncedUniswapPool
) -> eyre::Result<EstimatedOrder> {
    tracing::debug!("order to estimate: {order:?}");

    let mut amount = I256::unchecked_from(order.amount());
    if !order.exact_in() {
        amount = -amount;
    };

    let pool_state = pool_state.read().unwrap();

    // take the price t1 over t0, ensure we are within the sqrt price bounds.
    // then convert properly into sqrtPriceX96. Filtering is fine here as
    // the swap function will bound it to min / max
    let limit_price: Option<U256> = order
        .limit_price()
        .map(SqrtPriceX96::from)
        .map(|lp| if order.is_bid() { Ray::from(lp).inv_ray() } else { Ray::from(lp) })
        .filter(|price_1_over_0| price_1_over_0.within_sqrt_price_bounds())
        .map(|price| SqrtPriceX96::from(price).into());

    let SwapResult { amount0, amount1, sqrt_price_x_96, .. } = pool_state
        ._simulate_swap(order.token_in(), amount, limit_price, false)
        .map_err(|_| eyre!("no gas found"))?;

    tracing::info!(?sqrt_price_x_96, "simulation sqrt price");
    let mut amount_in = u128::try_convert_from(amount0.abs())?;
    let mut amount_out = u128::try_convert_from(amount1.abs())?;
    let mut price = Ray::from(SqrtPriceX96::from(sqrt_price_x_96));

    let mut gas_amount_t0 = 10;
    // modify to give tolerance.
    gas_amount_t0 = gas_amount_t0 * 4 / 3;

    // t1 -> t0, then when we flip price, will cross, so we want to make lower so
    // when flip its higher
    let price = if order.token_in() != pool_state.token0 {
        std::mem::swap(&mut amount_in, &mut amount_out);
        price.inv_ray_assign_round(false);

        // we are subtracting gas fee here to make price higher.
        let new_price = if order.exact_in() {
            let amount_out = price.quantity(amount_in, false) + gas_amount_t0;
            Ray::from_quantities(amount_out, amount_in, true)
        } else {
            let amount_in = price.inverse_quantity(amount_out - gas_amount_t0, true);
            Ray::from_quantities(amount_out, amount_in, true)
        };

        new_price.scale_to_fee(pool_state.book_fee as u128 + INTERNAL_SLIPPAGE_BPS)
    } else {
        // t0 -> t1, given this, we want to lower price.
        let new_price = if order.exact_in() {
            let amount_out = price.quantity(amount_in + gas_amount_t0, false);
            Ray::from_quantities(amount_out, amount_in, true)
        } else {
            let amount_in = price.inverse_quantity(amount_out, true) - gas_amount_t0;
            Ray::from_quantities(amount_out, amount_in, true)
        };

        new_price.scale_to_fee(pool_state.book_fee as u128 + INTERNAL_SLIPPAGE_BPS)
    };

    let estimated_order =
        EstimatedOrder::new(order.token_in(), order.token_out(), amount_in, amount_out, price);
    tracing::debug!("estimated order: {estimated_order:?}");

    Ok(estimated_order)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EstimatedOrder {
    pub token_in:  Address,
    pub token_out: Address,
    pub amt_in:    u128,
    pub amt_out:   u128,
    pub ucp:       Ray
}

impl EstimatedOrder {
    pub fn new(
        token_in: Address,
        token_out: Address,
        amt_in: u128,
        amt_out: u128,
        ucp: Ray
    ) -> Self {
        Self { token_in, token_out, amt_in, amt_out, ucp }
    }
}

async fn fetch_angstrom_pools<P>(
    // the block angstrom was deployed at
    mut deploy_block: usize,
    end_block: usize,
    angstrom_address: Address,
    db: &P
) -> Vec<PoolKey>
where
    P: Provider
{
    let mut filters = vec![];
    let controller_address = *CONTROLLER_V1_ADDRESS.get().unwrap();

    loop {
        let this_end_block = std::cmp::min(deploy_block + 99_999, end_block);

        if this_end_block == deploy_block {
            break;
        }

        let filter = Filter::new()
            .from_block(deploy_block as u64)
            .to_block(this_end_block as u64)
            .address(controller_address);

        filters.push(filter);

        deploy_block = std::cmp::min(end_block, this_end_block);
    }

    let logs = futures::stream::iter(filters)
        .map(|filter| async move {
            db.get_logs(&filter)
                .await
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .buffered(10)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    logs.into_iter()
        .fold(HashSet::new(), |mut set, log| {
            if let Ok(pool) = PoolConfigured::decode_log(&log.clone().into_inner()) {
                let pool_key = PoolKey {
                    currency0:   pool.asset0,
                    currency1:   pool.asset1,
                    fee:         pool.bundleFee,
                    tickSpacing: I24::try_from_be_slice(&{
                        let bytes = pool.tickSpacing.to_be_bytes();
                        let mut a = [0u8; 3];
                        a[1..3].copy_from_slice(&bytes);
                        a
                    })
                    .unwrap(),
                    hooks:       angstrom_address
                };

                set.insert(pool_key);
                return set;
            }

            if let Ok(pool) = PoolRemoved::decode_log(&log.clone().into_inner()) {
                let pool_key = PoolKey {
                    currency0:   pool.asset0,
                    currency1:   pool.asset1,
                    fee:         pool.feeInE6,
                    tickSpacing: pool.tickSpacing,
                    hooks:       angstrom_address
                };

                set.remove(&pool_key);
                return set;
            }
            set
        })
        .into_iter()
        .collect::<Vec<_>>()
}

pub struct MockStream {
    tx: tokio::sync::broadcast::Sender<CanonStateNotification>
}

impl NodePrimitivesProvider for MockStream {
    type Primitives = EthPrimitives;
}

impl CanonStateSubscriptions for MockStream {
    fn subscribe_to_canonical_state(
        &self
    ) -> reth_provider::CanonStateNotifications<Self::Primitives> {
        self.tx.subscribe()
    }
}

#[derive(Debug, Clone)]
pub struct StateProvider<P> {
    provider: Arc<P>
}

impl<P: Send + Sync + 'static> SetBlock for StateProvider<P> {
    fn set_block(&self, _: u64) {}
}

impl<P: Provider> reth_revm::DatabaseRef for StateProvider<P> {
    type Error = DBError;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let acc = async_to_sync(self.provider.get_account(address).latest().into_future())?;
        let code = async_to_sync(self.provider.get_code_at(address).latest().into_future())?;
        let code = Some(Bytecode::new_raw(code));

        Ok(Some(AccountInfo {
            code_hash: acc.code_hash,
            balance: acc.balance,
            nonce: acc.nonce,
            code
        }))
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let acc = async_to_sync(self.provider.get_storage_at(address, index).into_future())?;
        Ok(acc)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        let acc = async_to_sync(
            self.provider
                .get_block_by_number(alloy_rpc_types::BlockNumberOrTag::Number(number))
                .into_future()
        )?;

        let Some(block) = acc else { return Err(DBError::String("no block".to_string())) };
        Ok(block.header.hash)
    }

    fn code_by_hash_ref(&self, _: B256) -> Result<Bytecode, Self::Error> {
        panic!("This should not be called, as the code is already loaded");
    }
}
impl<P: Provider> BlockNumReader for StateProvider<P> {
    fn chain_info(&self) -> ProviderResult<reth::chainspec::ChainInfo> {
        panic!("never used");
    }

    fn block_number(&self, _: alloy_primitives::B256) -> ProviderResult<Option<BlockNumber>> {
        panic!("never used");
    }

    fn convert_number(
        &self,
        _: alloy_rpc_types::BlockHashOrNumber
    ) -> ProviderResult<Option<alloy_primitives::B256>> {
        panic!("never used");
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        Ok(async_to_sync(self.provider.get_block_number().into_future()).unwrap())
    }

    fn last_block_number(&self) -> ProviderResult<BlockNumber> {
        Ok(async_to_sync(self.provider.get_block_number().into_future()).unwrap())
    }

    fn convert_hash_or_number(
        &self,
        _: alloy_rpc_types::BlockHashOrNumber
    ) -> ProviderResult<Option<BlockNumber>> {
        panic!("never used");
    }
}
impl<P: Provider> BlockHashReader for StateProvider<P> {
    fn block_hash(&self, _: BlockNumber) -> ProviderResult<Option<alloy_primitives::B256>> {
        panic!("never used");
    }

    fn convert_block_hash(
        &self,
        _: alloy_rpc_types::BlockHashOrNumber
    ) -> ProviderResult<Option<alloy_primitives::B256>> {
        panic!("never used");
    }

    fn canonical_hashes_range(
        &self,
        _: BlockNumber,
        _: BlockNumber
    ) -> ProviderResult<Vec<alloy_primitives::B256>> {
        panic!("never used");
    }
}

pub fn async_to_sync<F: Future>(f: F) -> F::Output {
    let handle = tokio::runtime::Handle::try_current().expect("No tokio runtime found");
    tokio::task::block_in_place(|| handle.block_on(f))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderToEstimate {
    order_params: OrderParams
}

impl OrderToEstimate {
    pub fn new(
        token_in: Address,
        token_out: Address,
        amount: u128,
        exact_in: bool,
        limit_price: Option<U256>,
        slippage: Option<u128>
    ) -> Self {
        Self {
            order_params: OrderParams {
                amount,
                exact_in,
                slippage,
                limit_price,
                token_in,
                token_out
            }
        }
    }

    pub fn pool_token0(&self) -> Address {
        std::cmp::min(self.order_params.token_in, self.order_params.token_out)
    }

    pub fn pool_token1(&self) -> Address {
        std::cmp::max(self.order_params.token_in, self.order_params.token_out)
    }

    pub fn amount(&self) -> u128 {
        self.order_params.amount
    }

    pub fn limit_price(&self) -> Option<U256> {
        self.order_params.limit_price
    }

    pub fn token_in(&self) -> Address {
        self.order_params.token_in
    }

    pub fn token_out(&self) -> Address {
        self.order_params.token_out
    }

    pub fn exact_in(&self) -> bool {
        self.order_params.exact_in
    }

    pub fn is_bid(&self) -> bool {
        self.token_in() > self.token_out()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderParams {
    pub amount:      u128,
    pub exact_in:    bool,
    pub limit_price: Option<U256>,
    pub slippage:    Option<u128>,
    pub token_in:    Address,
    pub token_out:   Address
}

#[allow(clippy::result_large_err)]
pub trait TryFromConversion<T, E>
where
    Self: Sized,
    Self: TryFrom<T, Error = E> + Sized,
    E: ToString
{
    fn try_convert_from(value: T) -> eyre::Result<Self> {
        Self::try_from(value).map_err(|e: E| eyre!("failed to convert {}", e.to_string()))
    }
}

impl<D, T, E> TryFromConversion<T, E> for D
where
    D: TryFrom<T, Error = E> + Sized,
    E: ToString
{
}

pub fn init_tracing() {
    let envfilter = filter::EnvFilter::builder().try_from_env().ok();
    let format = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_target(true);

    let _ = tracing_subscriber::registry()
        .with(format)
        .with(envfilter)
        .try_init();
}

fn layer_builder(filter_str: String) -> Box<dyn Layer<Registry> + Send + Sync> {
    let filter = EnvFilter::builder()
        .with_default_directive(filter_str.parse().unwrap())
        .from_env_lossy();

    tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_target(true)
        .with_filter(filter)
        .boxed()
}
