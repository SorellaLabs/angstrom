use std::{
    collections::{HashMap, VecDeque},
    sync::Arc
};

use alloy::{
    primitives::{address, Address, U256},
    providers::{Network, Provider},
    transports::Transport
};
use angstrom_types::{pair_with_price::PairsWithPrice, primitive::PoolId};
use futures::StreamExt;
use matching_engine::cfmm::uniswap::{
    pool_data_loader::PoolDataLoader, pool_manager::SyncedUniswapPools
};
use tracing::warn;

const BLOCKS_TO_AVG_PRICE: u64 = 5;
pub const WETH_ADDRESS: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

// crazy that this is a thing
#[allow(clippy::too_long_first_doc_paragraph)]
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
            pair_to_pool.insert((pool.token_a, pool.token_b), *key);
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
                            .pool_data_for_block(block_number, provider.clone())
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
            .fold(HashMap::default(), |mut acc, x| async {
                let (key, prices) = x.await;
                acc.insert(key, prices);
                acc
            })
            .await;

        Ok(Self { prev_prices: pools, cur_block: current_block, pair_to_pool })
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
    pub fn get_eth_conversion_price(&self, token_0: Address, token_1: Address) -> Option<U256> {
        if token_0 == WETH_ADDRESS {
            return Some(U256::from(1))
        }
        // should only be called if token_1 is weth or needs multi-hop as otherwise
        // conversion factor will be 1-1
        if token_1 == WETH_ADDRESS {
            // if so, just pull the price
            let pool_key = self
                .pair_to_pool
                .get(&(token_0, token_1))
                .expect("got pool update that we don't have stored");

            let prices = self.prev_prices.get(pool_key)?;
            let size = prices.len() as u64;

            if size != BLOCKS_TO_AVG_PRICE {
                warn!("size of loaded blocks doesn't match the value we set");
            }

            return Some(
                prices
                    .iter()
                    .map(|price| {
                        // need to flip. add 18 decimal precision then reciprocal
                        U256::from(1e36) / price.price_1_over_0
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
            let size = prices.len() as u64;

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
                            U256::from(1e36) / price.price_1_over_0
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
            let size = prices.len() as u64;

            if size != BLOCKS_TO_AVG_PRICE {
                warn!("size of loaded blocks doesn't match the value we set");
            }
            // token 0 / token 1
            let first_hop_price = prices
                .iter()
                .map(|price| {
                    // need to flip. add 18 decimal precision then reciprocal
                    U256::from(1e36) / price.price_1_over_0
                })
                .sum::<U256>()
                / U256::from(size);

            // grab second hop
            let prices = self.prev_prices.get(key)?;
            let size = prices.len() as u64;

            if size != BLOCKS_TO_AVG_PRICE {
                warn!("size of loaded blocks doesn't match the value we set");
            }

            // token1 / WETH
            let second_hop_price = prices
                .iter()
                .map(|price| {
                    // means weth is token0
                    if second_flip {
                        price.price_1_over_0
                    } else {
                        // need to flip. add 18 decimal precision then reciprocal
                        U256::from(1e36) / price.price_1_over_0
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

#[cfg(test)]
pub mod test {
    use std::collections::{HashMap, VecDeque};

    use alloy::{
        node_bindings::WEI_IN_ETHER,
        primitives::{Address, FixedBytes, U256}
    };
    use angstrom_types::pair_with_price::PairsWithPrice;
    use revm::primitives::address;

    use super::TokenPriceGenerator;

    const TOKEN0: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
    const TOKEN1: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc3");
    const TOKEN2: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc1");
    const TOKEN3: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc5");
    const TOKEN4: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc0");

    /// sets up pools with prices for all scenarios
    fn setup() -> TokenPriceGenerator {
        let mut pairs_to_key = HashMap::default();
        // setup pair lookup

        // pair 1 direct NOTE: case is where weth is token1
        pairs_to_key.insert((TOKEN2, TOKEN0), FixedBytes::<32>::with_last_byte(1));

        // pair 2 direct NOTE: case is where weth is token0
        pairs_to_key.insert((TOKEN0, TOKEN1), FixedBytes::<32>::with_last_byte(2));

        // multi-hop where token0 matches
        pairs_to_key.insert((TOKEN2, TOKEN3), FixedBytes::<32>::with_last_byte(3));

        // multi-hop where token1 matches
        pairs_to_key.insert((TOKEN4, TOKEN1), FixedBytes::<32>::with_last_byte(4));

        // setup price conversions
        let mut prices = HashMap::default();

        // assumes both 18 decimal
        let pair1_rate = U256::from(5) * WEI_IN_ETHER;
        let pair = PairsWithPrice {
            token0:         TOKEN2,
            token1:         TOKEN0,
            block_num:      0,
            price_1_over_0: pair1_rate
        };
        let queue = VecDeque::from([pair; 5]);
        prices.insert(FixedBytes::<32>::with_last_byte(1), queue);

        // assumes token1 is 6 decimals and token 0 is 18 with a conversion rate of 0.2
        // gives us 200000
        let pair2_rate = U256::from(200000);

        let pair = PairsWithPrice {
            token0:         TOKEN0,
            token1:         TOKEN1,
            block_num:      0,
            price_1_over_0: pair2_rate
        };
        let queue = VecDeque::from([pair; 5]);
        prices.insert(FixedBytes::<32>::with_last_byte(2), queue);

        // simple conversion rate of 2/1 on 18 decimals
        let pair3_rate = U256::from(2e18);

        let pair = PairsWithPrice {
            token0:         TOKEN2,
            token1:         TOKEN3,
            block_num:      0,
            price_1_over_0: pair3_rate
        };
        let queue = VecDeque::from([pair; 5]);
        prices.insert(FixedBytes::<32>::with_last_byte(3), queue);

        // token 1 is 18 decimals, token 0 is 6 with a conversion rate of 1/8
        let pair4_rate = U256::from(1e36) / U256::from(8e6);

        let pair = PairsWithPrice {
            token0:         TOKEN4,
            token1:         TOKEN1,
            block_num:      0,
            price_1_over_0: pair4_rate
        };

        let queue = VecDeque::from([pair; 5]);
        prices.insert(FixedBytes::<32>::with_last_byte(4), queue);

        TokenPriceGenerator { cur_block: 0, prev_prices: prices, pair_to_pool: pairs_to_key }
    }

    #[test]
    fn test_direct_conversion() {
        let token_conversion = setup();
        let rate = token_conversion
            .get_eth_conversion_price(TOKEN2, TOKEN0)
            .unwrap();

        let expected_rate = U256::from(1e36) / U256::from(5e18);
        assert_eq!(rate, expected_rate)
    }

    #[test]
    fn test_multi_hop_where_token0_matches() {
        let token_conversion = setup();
        let rate = token_conversion
            .get_eth_conversion_price(TOKEN2, TOKEN3)
            .unwrap();

        let expected_rate = U256::from(1e36) / U256::from(5e18);
        assert_eq!(rate, expected_rate)
    }

    #[test]
    fn test_multi_hop_where_token1_matches() {
        let token_conversion = setup();
        let rate = token_conversion
            .get_eth_conversion_price(TOKEN4, TOKEN1)
            .unwrap();

        // hop 1 rate
        // assumes token1 is 6 decimals and token 0 is 18 with a conversion rate of 0.2
        // gives us 200000 TOKEN1 / WETH
        //
        // hop 2 rate
        // token 1 is 18 decimals, token 0 is 6 with a conversion rate of 1/8
        // let pair4_rate = U256::from(1e36) / U256::from(8e6);
        //
        // gives us 0.2 * 0.8 = 0.16;
        let expected_rate = U256::from(1600000000000u128);
        assert_eq!(rate, expected_rate)
    }
}
