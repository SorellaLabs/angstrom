use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::Arc
};

use alloy::{
    primitives::{Address, FixedBytes, U256},
    providers::{Network, Provider},
    rpc::types::error,
    transports::Transport
};
use angstrom_types::{
    contract_payloads::rewards::PoolUpdate, pair_with_price::PairsWithPrice, primitive::PoolId
};
use futures::{Stream, StreamExt};
use matching_engine::cfmm::uniswap::{
    pool_data_loader::PoolDataLoader,
    pool_manager::{SyncedUniswapPools, UniswapPoolManager},
    pool_providers::PoolManagerProvider
};
use revm::primitives::{address, ruint::aliases::U256};

const BLOCKS_TO_AVG_PRICE: usize = 5;

pub const WETH_ADDRESS: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
/// The token price generator gives us the avg instantaneous price of the last 5
/// blocks of the underlying V4 pool. This is then used in order to convert the
/// gas used from eth to token0 of the pool the user is swapping over.
/// In the case of NON direct eth pairs. we assume that any token liquid enough
/// to trade on angstrom not with eth will always have a eth pair 1 hop away.
/// this allows for a simple lookup.
#[derive(Debug, Clone)]
pub struct TokenPriceGenerator {
    prev_prices:  HashMap<PoolId, VecDeque<PairsWithPrice>>,
    pair_to_pool: HashMap<(Address, Address), PoolId>,
    cur_block:    u64
}

impl TokenPriceGenerator {
    /// is a bit of a pain as we need todo a look-back in-order to grab last 5
    /// blocks.
    pub async fn new<P: Provider<T, N>, T: Transport + Clone, N: Network, Loader>(
        provider: Arc<P>,
        current_block: u64,
        uni: SyncedUniswapPools<PoolId, Loader>
    ) -> eyre::Result<Self>
    where
        Loader: PoolDataLoader<PoolId> + Default + Clone + Send + Sync + 'static
    {
        let mut pair_to_pool = HashMap::default();
        for (key, pool) in uni.iter() {
            let pool = pool.read().await;
            pair_to_pool.insert((pool.tokenA, pool.tokenB), key);
        }

        // for each pool, we want to load the last 5 blocks and get the sqrt_price_96
        // and then convert it into the price of the underlying pool
        let pools = futures::stream::iter(uni.iter())
            .map(|(pool_key, pool)| {
                let provider = provider.clone();

                async move {
                    let mut queue = VecDeque::new();
                    let pool_read = pool.read().await;

                    for block_number in current_block - BLOCKS_TO_AVG_PRICE..=current_block {
                        let pool_data = pool_read
                            .pool_data_for_block(block_number, provider)
                            .await
                            .expect("failed to load historical price for token price conversion");
                        let price = pool_data.get_raw_price();

                        queue.push_back(PairsWithPrice {
                            token0:         pool_data.tokenA,
                            token1:         pool_data.tokenB,
                            block_num:      block_number,
                            price_1_over_0: price
                        });
                    }

                    (*pool_key, queue)
                }
            })
            .collect::<HashMap<_, _>>()
            .await;

        Self { prev_prices: pools, cur_block: current_block, pair_to_pool }
    }

    pub fn apply_update(&mut self, updates: Vec<PairsWithPrice>) {
        for pool_update in updates {
            // make sure we aren't replaying
            assert!(pool_update.block_num == self.cur_block + 1);

            let pool_key = self
                .pair_to_pool
                .get(&(pool_update.token0, pool_update.token1))
                .expect("got pool update that we don't have stored");
            let prev_prices = self
                .prev_prices
                .get_mut(pool_key)
                .expect("don't have prev_prices for update");
            prev_prices.pop_front();
            prev_prices.push_back(pool_update);
        }
        self.cur_block += 1;
    }

    /// NOTE: assumes tokens are properly sorted
    /// returns the conversion ratio of the pair to eth, this looks like
    /// non-weth / weth. This then allows for the simple calcuation of
    /// gas_in_wei * conversion price in order to get the used token_0
    pub fn get_eth_conversion_price(
        &self,
        mut token_0: Address,
        mut token_1: Address
    ) -> Option<U256> {
        // should only be called if token_1 is weth or needs multi-hop as otherwise
        // conversion factor will be 1-1
        if token_1 == WETH_ADDRESS {
            // if so, just pull the price
            let pool_key = self
                .pair_to_pool
                .get(&(token_0, token_1))
                .expect("got pool update that we don't have stored");

            let prices = self.prev_prices.get(pool_key)?;
            let size = prices.len();

            if size != BLOCKS_TO_AVG_PRICE {
                warn!("size of loaded blocks doesn't match the value we set");
            }

            return Some(
                prices
                    .iter()
                    .map(|price| {
                        // need to flip. add 18 decimal precision then reciprocal
                        U256::from(1e18 as u128) / price.price_1_over_0
                    })
                    .sum::<U256>()
                    / U256::from(size)
            )
        }

        // need to pass through a pair.
        let (first_flip, token_0_hop1, token_1_hop1) = if token_0 < WETH_ADDRESS {
            (false, token_0, WETH_ADDRESS)
        } else {
            (true, WETH_ADDRESS, token_0)
        };

        let (second_flip, token_0_hop2, token_1_hop2) = if token_1 < WETH_ADDRESS {
            (false, token_1, WETH_ADDRESS)
        } else {
            (true, WETH_ADDRESS, token_1)
        };

        // check token_0 first for a weth pair. otherwise, check token_1.
        if let Some(key) = self.pair_to_pool.get(&(token_0_hop1, token_1_hop1)) {
            // there is a hop from token_0 to weth
            let prices = self.prev_prices.get(key)?;
            let size = prices.len();

            if size != BLOCKS_TO_AVG_PRICE {
                warn!("size of loaded blocks doesn't match the value we set");
            }

            return Some(
                prices
                    .iter()
                    .map(|price| {
                        // means weth is token0
                        if first_flip {
                            price.price_1_over_0
                        } else {
                            // need to flip. add 18 decimal precision then reciprocal
                            U256::from(1e18 as u128) / price.price_1_over_0
                        }
                    })
                    .sum::<U256>()
                    / U256::from(size)
            )
        } else if let Some(key) = self.pair_to_pool.get(&(token_0_hop2, token_1_hop2)) {
            // because we are going through token1 here and we want token zero, we need to
            // do some extra math
            let default_pool_key = self
                .pair_to_pool
                .get(&(token_0, token_1))
                .expect("got pool update that we don't have stored");

            let prices = self.prev_prices.get(default_pool_key)?;
            let size = prices.len();

            if size != BLOCKS_TO_AVG_PRICE {
                warn!("size of loaded blocks doesn't match the value we set");
            }
            // token 0 / token 1
            let first_hop_price = prices
                .iter()
                .map(|price| {
                    // need to flip. add 18 decimal precision then reciprocal
                    U256::from(1e18 as u128) / price.price_1_over_0
                })
                .sum::<U256>()
                / U256::from(size);

            // grab second hop
            let prices = self.prev_prices.get(key)?;
            let size = prices.len();

            if size != BLOCKS_TO_AVG_PRICE {
                warn!("size of loaded blocks doesn't match the value we set");
            }

            // token1 / WETH
            let second_hop_price = prices
                .iter()
                .map(|price| {
                    // means weth is token0
                    if second_flip_flip {
                        price.price_1_over_0
                    } else {
                        // need to flip. add 18 decimal precision then reciprocal
                        U256::from(1e18 as u128) / price.price_1_over_0
                    }
                })
                .sum::<U256>()
                / U256::from(size);

            // token 0 / token1 * token1 / weth  = token0 / weth
            Some(first_hop_price * second_hop_price)
        } else {
            panic!("found a token that doesn't have a 1 hop to WETH")
        }
    }
}
