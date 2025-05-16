use std::{
    collections::{HashMap, VecDeque},
    sync::Arc
};

use alloy::{
    primitives::{Address, U256, address},
    providers::Provider
};
use angstrom_types::{
    matching::SqrtPriceX96, orders::UpdatedGas, pair_with_price::PairsWithPrice, primitive::PoolId,
    sol_bindings::Ray
};
use futures::StreamExt;
use tracing::warn;
use uniswap_v4::uniswap::{pool_data_loader::PoolDataLoader, pool_manager::SyncedUniswapPools};

use crate::order::sim::{BOOK_GAS, BOOK_GAS_INTERNAL, TOB_GAS, TOB_GAS_INTERNAL};

const BLOCKS_TO_AVG_PRICE: u64 = 15;
pub const WETH_ADDRESS: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

// crazy that this is a thing
#[allow(clippy::too_long_first_doc_paragraph)]
/// The token price generator gives us the avg instantaneous price of the last 5
/// blocks of the underlying V4 pool. This is then used in order to convert the
/// gas used from eth to token0 of the pool the user is swapping over.
/// In the case of NON direct eth pairs. we assume that any token liquid enough
/// to trade on angstrom not with eth will always have a eth pair 1 hop away.
/// this allows for a simple lookup.
#[derive(Clone)]
pub struct TokenPriceGenerator {
    uniswap_pools:       SyncedUniswapPools,
    prev_prices:         HashMap<PoolId, VecDeque<PairsWithPrice>>,
    pair_to_pool:        HashMap<(Address, Address), PoolId>,
    /// the token that is the wrapped version of the gas token on the given
    /// chain
    base_gas_token:      Address,
    cur_block:           u64,
    blocks_to_avg_price: u64,
    base_wei:            u128
}

impl TokenPriceGenerator {
    /// is a bit of a pain as we need todo a look-back in-order to grab last 5
    /// blocks.
    pub async fn new<P: Provider>(
        provider: Arc<P>,
        current_block: u64,
        uni: SyncedUniswapPools,
        base_gas_token: Address,
        blocks_to_avg_price_override: Option<u64>
    ) -> eyre::Result<Self> {
        let mut pair_to_pool = HashMap::default();
        for id in uni.iter() {
            let key = id.key();
            let pool = id.value();
            let pool = pool.read().unwrap();
            pair_to_pool.insert((pool.token0, pool.token1), *key);
        }
        let new_gas_wei = provider.get_gas_price().await.unwrap_or_default();

        let blocks_to_avg_price = blocks_to_avg_price_override.unwrap_or(BLOCKS_TO_AVG_PRICE);
        // for each pool, we want to load the last 5 blocks and get the sqrt_price_96
        // and then convert it into the price of the underlying pool
        let pools = futures::stream::iter(uni.iter())
            .map(|id| {
                let pool_key = *id.key();
                let pool = id.value().clone();
                let provider = provider.clone();

                async move {
                    let mut queue = VecDeque::new();
                    // scoping
                    let data_loader = {
                        let pool_read = pool.read().unwrap();
                        let data_loader = pool_read.data_loader();
                        drop(pool_read);
                        data_loader
                    };

                    for block_number in
                        current_block.saturating_sub(blocks_to_avg_price)..current_block
                    {
                        tracing::debug!(block_number, current_block, ?pool_key, "loading pool");
                        let pool_data = data_loader
                            .load_pool_data(Some(block_number), provider.clone())
                            .await
                            .expect("failed to load historical price for token price conversion");

                        // price as ray
                        let price = pool_data.get_raw_price();

                        queue.push_back(PairsWithPrice {
                            token0:         pool_data.tokenA,
                            token1:         pool_data.tokenB,
                            block_num:      block_number,
                            price_1_over_0: price
                        });
                    }

                    (pool_key, queue)
                }
            })
            .fold(HashMap::default(), |mut acc, x| async {
                let (key, prices) = x.await;
                acc.insert(key, prices);
                acc
            })
            .await;

        Ok(Self {
            prev_prices: pools,
            base_gas_token,
            cur_block: current_block,
            pair_to_pool,
            blocks_to_avg_price,
            uniswap_pools: uni,
            base_wei: new_gas_wei
        })
    }

    pub fn generate_gas_updates(&self) -> Vec<UpdatedGas> {
        self.pair_to_pool
            .iter()
            .filter_map(|(&(mut token0, mut token1), pool_id)| {
                if token1 < token0 {
                    std::mem::swap(&mut token0, &mut token1)
                };

                let price = self.get_eth_conversion_price(token0, token1)?;

                let book_internal = price.inverse_quantity(BOOK_GAS_INTERNAL as u128, false);
                let book_external = price.inverse_quantity(BOOK_GAS as u128, false);
                let tob_internal = price.inverse_quantity(TOB_GAS_INTERNAL as u128, false);
                let tob_external = price.inverse_quantity(TOB_GAS as u128, false);

                Some(UpdatedGas {
                    pool_id:           *pool_id,
                    gas_internal_book: book_internal,
                    gas_internal_tob:  tob_internal,
                    gas_external_tob:  tob_external,
                    gas_external_book: book_external,
                    block_number:      self.cur_block
                })
            })
            .collect()
    }

    pub fn generate_lookup_map(&self) -> HashMap<(Address, Address), Ray> {
        self.pair_to_pool
            .keys()
            .filter_map(|&(mut token0, mut token1)| {
                if token1 < token0 {
                    std::mem::swap(&mut token0, &mut token1)
                };

                let price = self.get_eth_conversion_price(token0, token1)?;

                Some(((token0, token1), price))
            })
            .collect()
    }

    pub fn apply_update(&mut self, new_gas_wei: u128, updates: Vec<PairsWithPrice>) {
        // we will duplicate same price if no update for pool.
        let mut updated_pool_keys = Vec::new();
        self.base_wei = new_gas_wei;

        for mut pool_update in updates {
            // make sure we aren't replaying
            assert!(pool_update.block_num == self.cur_block + 1);

            let pool_key = if let Some(p) = self
                .pair_to_pool
                .get(&(pool_update.token0, pool_update.token1))
            {
                *p
            } else {
                let pk = self
                    .uniswap_pools
                    .iter()
                    .find_map(|val| {
                        let (pool_key, pool) = val.pair();
                        let (t0, t1) = {
                            let pool_read = pool.read().unwrap();
                            let ts = (pool_read.token0, pool_read.token1);
                            drop(pool_read);
                            ts
                        };
                        (t0 == pool_update.token0 && t1 == pool_update.token1).then_some(*pool_key)
                    })
                    .expect("got pool update that we don't have stored");

                self.pair_to_pool
                    .insert((pool_update.token0, pool_update.token1), pk);
                self.prev_prices.insert(pk, VecDeque::new());
                pk
            };

            updated_pool_keys.push(pool_key);
            let prev_prices = self
                .prev_prices
                .get_mut(&pool_key)
                .expect("don't have prev_prices for update");

            pool_update.replace_price_if_empty(|| {
                self.uniswap_pools
                    .get(&pool_key)
                    .map(|pool| Ray::from(SqrtPriceX96::from(pool.read().unwrap().sqrt_price)))
                    .unwrap_or_default()
            });

            if !pool_update.price_1_over_0.is_zero() {
                prev_prices.push_back(pool_update);
            }

            // only pop front if we extend
            if prev_prices.len() as u64 == self.blocks_to_avg_price + 1 {
                prev_prices.pop_front();
            }
        }
        self.cur_block += 1;

        self.prev_prices
            .iter_mut()
            .filter(|(k, _)| !updated_pool_keys.contains(k))
            .for_each(|(_, queue)| {
                let Some(last) = queue.back() else { return };
                let mut new_back = *last;
                new_back.block_num = self.cur_block;

                queue.push_back(new_back);

                // only pop front if we extend
                if queue.len() as u64 == self.blocks_to_avg_price + 1 {
                    queue.pop_front();
                }
            });

        // given that we have added new pools that we got updates for,
        // we also want to make sure to add new pools that haven't been updated
        // as trading is not eligible since there's no price. Kinda a catch 22
        let remove_keys = self
            .prev_prices
            .keys()
            .copied()
            .filter(|f| !self.uniswap_pools.contains_key(f))
            .collect::<Vec<_>>();

        for key in remove_keys {
            self.prev_prices.remove(&key);
            self.pair_to_pool.retain(|_, v| *v != key);
        }

        // look at all pools uniswap contains. we want to remove
        self.uniswap_pools.iter().for_each(|entry| {
            let (key, pool) = entry.pair();
            // already have
            if self.prev_prices.contains_key(key) {
                return;
            }
            // new
            let pool_r = pool.read().unwrap();
            let price: Ray = SqrtPriceX96::from(pool_r.sqrt_price).into();

            let mut queue = VecDeque::new();
            queue.push_back(PairsWithPrice {
                token0:         pool_r.token0,
                token1:         pool_r.token1,
                block_num:      self.cur_block,
                price_1_over_0: price
            });
            self.prev_prices.insert(*key, queue);
            self.pair_to_pool
                .insert((pool_r.token0, pool_r.token1), *key);
        });
    }

    pub fn new_pool_update(
        &mut self,
        pool_id: PoolId,
        token_0: Address,
        token_1: Address,
        prev_prices: VecDeque<PairsWithPrice>
    ) {
        self.pair_to_pool.insert((token_0, token_1), pool_id);
        self.prev_prices.insert(pool_id, prev_prices);
    }

    /// NOTE: assumes tokens are properly sorted.
    /// the previous prices are stored in RAY (1e27).
    /// returns price in GAS / t0
    pub fn get_eth_conversion_price(&self, token_0: Address, token_1: Address) -> Option<Ray> {
        let wei = if self.base_wei == 0 { 1e18 as u128 } else { self.base_wei };
        // if token zero is weth, then we mul by 1
        if token_0 == self.base_gas_token {
            return Some(Ray::scale_to_ray(U256::from(1)).mul_wad(wei, 18));
        }
        // should only be called if token_1 is weth or needs multi-hop as otherwise
        // conversion factor will be 1-1
        if token_1 == self.base_gas_token {
            // if so, just pull the price
            let pool_key = self.pair_to_pool.get(&(token_0, token_1))?;

            let prices = self.prev_prices.get(pool_key)?;
            let size = prices.len() as u64;

            if self.blocks_to_avg_price > 0 && size != self.blocks_to_avg_price {
                warn!(?size,?self.blocks_to_avg_price,"size of loaded blocks doesn't match the value we set");
            }

            // if t1 == gas, then t0am  * t1 / t0 = am t1
            return Some(
                (prices.iter().map(|p| p.price_1_over_0).sum::<Ray>() / U256::from(size))
                    .mul_wad(wei, 18)
            );
        }

        // need to pass through a pair.
        let (first_flip, token_0_hop1, token_1_hop1) = if token_0 < self.base_gas_token {
            (true, token_0, self.base_gas_token)
        } else {
            (false, self.base_gas_token, token_0)
        };

        let (second_flip, token_0_hop2, token_1_hop2) = if token_1 < self.base_gas_token {
            (true, token_1, self.base_gas_token)
        } else {
            (false, self.base_gas_token, token_1)
        };

        // check token_0 first for a weth pair. otherwise, check token_1.
        if let Some(key) = self.pair_to_pool.get(&(token_0_hop1, token_1_hop1)) {
            // there is a hop from token_0 to weth
            let prices = self.prev_prices.get(key)?;
            let size = prices.len() as u64;

            if self.blocks_to_avg_price > 0 && size != self.blocks_to_avg_price {
                warn!("size of loaded blocks doesn't match the value we set");
            }

            // if we have this, this means that (p0, p1) has (p0, gas) pair.
            // because of this, we can just convert directly on this.
            // if first_flip = true, means token0 < gas, were price is gas / token0.
            // thus gas_am t0 * price = gas.

            Some(
                (prices
                    .iter()
                    .map(|price| {
                        // if true, means gas is token zero
                        if first_flip {
                            price.price_1_over_0
                        } else {
                            price.price_1_over_0.inv_ray()
                        }
                    })
                    .sum::<Ray>()
                    / U256::from(size))
                .mul_wad(wei, 18)
            )
        } else if let Some(key) = self.pair_to_pool.get(&(token_0_hop2, token_1_hop2)) {
            // because we are going through token1 here and we want token zero, we need to
            // do some extra math
            let default_pool_key = self
                .pair_to_pool
                .get(&(token_0, token_1))
                .expect("got pool update that we don't have stored");

            let prices = self.prev_prices.get(default_pool_key)?;
            println!("{:?}", prices);
            let size = prices.len() as u64;

            if self.blocks_to_avg_price > 0 && size != self.blocks_to_avg_price {
                warn!("size of loaded blocks doesn't match the value we set");
            }
            // price as t1 / t0
            let first_hop_price =
                prices.iter().map(|price| price.price_1_over_0).sum::<Ray>() / U256::from(size);

            // grab second hop
            let prices = self.prev_prices.get(key)?;
            let size = prices.len() as u64;

            if self.blocks_to_avg_price > 0 && size != self.blocks_to_avg_price {
                warn!("size of loaded blocks doesn't match the value we set");
            }

            // if flip = true, then  gas / token1, otherwise token1 / gas

            // token1 / WETH
            let second_hop_price = prices
                .iter()
                .map(|price| {
                    // means gas is token1
                    if second_flip { price.price_1_over_0 } else { price.price_1_over_0.inv_ray() }
                })
                .sum::<Ray>()
                / U256::from(size);

            // t1 / t0 *  gas / t1 = gas / t0
            Some(first_hop_price.mul_ray(second_hop_price).mul_wad(wei, 18))
        } else {
            tracing::error!("found a token that doesn't have a 1 hop to WETH");
            None
        }
    }

    #[cfg(any(feature = "testnet", feature = "testnet-sepolia"))]
    pub fn pairs_to_pools(&self) -> HashMap<(Address, Address), PoolId> {
        self.pair_to_pool.clone()
    }

    #[cfg(any(feature = "testnet", feature = "testnet-sepolia"))]
    pub fn prev_prices(&self) -> HashMap<PoolId, VecDeque<PairsWithPrice>> {
        self.prev_prices.clone()
    }
}

impl std::fmt::Debug for TokenPriceGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenPriceGenerator")
            .field("prev_prices", &self.prev_prices)
            .field("pair_to_pool", &self.pair_to_pool)
            .field("base_gas_token", &self.base_gas_token)
            .field("cur_block", &self.cur_block)
            .field("blocks_to_avg_price", &self.blocks_to_avg_price)
            .field("base_gas_token", &self.base_gas_token)
            .finish()
    }
}

#[cfg(test)]
pub mod test {
    use std::collections::{HashMap, VecDeque};

    use alloy::{
        node_bindings::WEI_IN_ETHER,
        primitives::{Address, FixedBytes, U256}
    };
    use angstrom_types::{pair_with_price::PairsWithPrice, sol_bindings::Ray};
    use revm::primitives::address;
    use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

    use super::{BLOCKS_TO_AVG_PRICE, TokenPriceGenerator, WETH_ADDRESS};

    const TOKEN0: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
    const TOKEN1: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc3");
    const TOKEN2: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc1");
    const TOKEN3: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc5");
    const TOKEN4: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc0");
    const TOKEN5: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc6");

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
            price_1_over_0: Ray::scale_to_ray(pair1_rate)
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
            price_1_over_0: Ray::scale_to_ray(pair2_rate)
        };
        let queue = VecDeque::from([pair; 5]);
        prices.insert(FixedBytes::<32>::with_last_byte(2), queue);

        // simple conversion rate of 2/1 on 18 decimals
        let pair3_rate = U256::from(2) * WEI_IN_ETHER;

        let pair = PairsWithPrice {
            token0:         TOKEN2,
            token1:         TOKEN3,
            block_num:      0,
            price_1_over_0: Ray::scale_to_ray(pair3_rate)
        };
        let queue = VecDeque::from([pair; 5]);
        prices.insert(FixedBytes::<32>::with_last_byte(3), queue);

        // token 1 is 18 decimals, token 0 is 6 with a conversion rate of 1/8
        let pair4_rate = U256::from(1e18) / U256::from(8e6);

        let pair = PairsWithPrice {
            token0:         TOKEN4,
            token1:         TOKEN1,
            block_num:      0,
            price_1_over_0: Ray::scale_to_ray(pair4_rate)
        };

        let queue = VecDeque::from([pair; 5]);
        prices.insert(FixedBytes::<32>::with_last_byte(4), queue);

        TokenPriceGenerator {
            base_wei:            0,
            cur_block:           0,
            prev_prices:         prices,
            base_gas_token:      WETH_ADDRESS,
            pair_to_pool:        pairs_to_key,
            blocks_to_avg_price: BLOCKS_TO_AVG_PRICE,
            uniswap_pools:       SyncedUniswapPools::new(
                Default::default(),
                tokio::sync::mpsc::channel(1).0
            )
        }
    }

    #[test]
    fn test_direct_conversion() {
        let token_conversion = setup();
        let rate = token_conversion
            .get_eth_conversion_price(TOKEN2, TOKEN0)
            .unwrap();

        let expected_rate = Ray::scale_to_ray(U256::from(5) * WEI_IN_ETHER);
        println!("rate: {:?} got: {:?}", rate, expected_rate);
        assert_eq!(rate, expected_rate)
    }

    #[test]
    fn test_multi_hop_where_token0_matches() {
        let token_conversion = setup();
        let rate = token_conversion
            .get_eth_conversion_price(TOKEN2, TOKEN3)
            .unwrap();

        // t3 / t2 = pair 1 = 2e45
        // weth / t2 = pair 2 =  5e45
        //
        //  given t2 < weth
        //
        //  conversion = price =  weth / t2  == 5e45

        let expected_rate = Ray::scale_to_ray(U256::from(5e18));
        println!("rate: {:?} got: {:?}", rate, expected_rate);
        assert_eq!(rate, expected_rate)
    }

    #[test]
    fn test_multi_hop_where_token1_matches() {
        // 625000000000000000000000000000000

        let token_conversion = setup();
        let rate = token_conversion
            .get_eth_conversion_price(TOKEN4, TOKEN1)
            .unwrap();

        // we have a  t1 / t4 pair and a t1 / weth pair
        //
        // t1 / t4 = 125000000000
        //
        // t1 / weth = 200000000000000000000000000000000
        //
        // what we want is weth / t4
        //
        //  if weth t0 = true, then we flip
        //  so then t1 /t4 * weth / t1 =  weth / t4
        //
        // 125000000000 * (1e54 / 200000000000000000000000000000000) =
        // 625000000000000000000000000000000

        // hop 1 rate
        // assumes token1 is 6 decimals and token 0 is 18 with a conversion rate of 0.2
        // gives us 200000 TOKEN1 / WETH
        //
        // hop 2 rate
        // token 1 is 18 decimals, token 0 is 6 with a conversion rate of 1/8
        // let pair4_rate = U256::from(1e18) / U256::from(8e6);
        //
        // gives us 0.2 * 0.8 = 0.16;
        let expected_rate = Ray::scale_to_ray(U256::from(625000));
        assert_eq!(rate, expected_rate)
    }

    #[test]
    fn test_weth_direct_cases() {
        let token_conversion = setup();

        // WETH as token0 should return 1
        let rate = token_conversion
            .get_eth_conversion_price(WETH_ADDRESS, TOKEN1)
            .unwrap();
        assert_eq!(rate, Ray::scale_to_ray(U256::from(1)));

        // 5 weth .inv
        let rate = token_conversion
            .get_eth_conversion_price(TOKEN2, WETH_ADDRESS)
            .unwrap();

        assert_eq!(rate, Ray::scale_to_ray(U256::from(5) * WEI_IN_ETHER));
    }

    #[test]
    fn test_price_averaging() {
        // 3000000000000000000000000000000000000000000000

        let mut token_conversion = setup();

        // Create varying prices over 5 blocks
        let mut updates = Vec::new();
        for i in 1..=5 {
            updates.push(PairsWithPrice {
                token0:         TOKEN2,
                token1:         TOKEN0,
                block_num:      i,
                price_1_over_0: Ray::scale_to_ray(U256::from(i) * WEI_IN_ETHER)
            });
        }

        // Apply the updates
        for update in updates {
            token_conversion.apply_update(0, vec![update]);
        }

        // Average should be (1 + 2 + 3 + 4 + 5) / 5 = 3
        let rate = token_conversion
            .get_eth_conversion_price(TOKEN2, TOKEN0)
            .unwrap();

        let mut sum = Ray::default();
        for i in 1..=5 {
            sum += Ray::scale_to_ray(U256::from(i) * WEI_IN_ETHER);
        }
        let expected = sum / U256::from(5);

        assert_eq!(rate, expected);
    }

    #[test]
    fn test_generate_lookup_map() {
        let token_conversion = setup();
        let lookup_map = token_conversion.generate_lookup_map();

        // Check that all pairs are properly ordered (token0 < token1)
        for ((token0, token1), _) in lookup_map.iter() {
            assert!(token0 < token1, "Tokens should be ordered in lookup map");
        }

        // Verify expected number of pairs
        assert_eq!(lookup_map.len(), 4, "Should have all valid pairs in lookup map");
    }

    #[test]
    #[should_panic]
    fn test_apply_update_validation() {
        let mut token_conversion = setup();

        // Should panic on non-sequential block updates
        token_conversion.apply_update(
            0,
            vec![PairsWithPrice {
                token0:         TOKEN2,
                token1:         TOKEN0,
                block_num:      5, // Non-sequential block
                price_1_over_0: Ray::scale_to_ray(U256::from(1) * WEI_IN_ETHER)
            }]
        );
    }

    #[test]
    fn test_missing_pool() {
        let token_conversion = setup();

        // Try to get price for non-existent pool
        let rate = token_conversion.get_eth_conversion_price(
            address!("1111111111111111111111111111111111111111"),
            address!("2222222222222222222222222222222222222222")
        );
        assert!(rate.is_none(), "Should return None for missing pool");
    }

    #[test]
    fn test_insufficient_price_data() {
        // 1000000000000000000000000000000000000000000000
        let mut token_conversion = setup();

        // Create a pool with insufficient price data
        let pool_id = FixedBytes::<32>::with_last_byte(6);
        token_conversion
            .pair_to_pool
            .insert((TOKEN5, WETH_ADDRESS), pool_id);

        let mut queue = VecDeque::new();
        queue.push_back(PairsWithPrice {
            token0:         TOKEN5,
            token1:         WETH_ADDRESS,
            block_num:      0,
            price_1_over_0: Ray::scale_to_ray(U256::from(1) * WEI_IN_ETHER)
        });
        token_conversion.prev_prices.insert(pool_id, queue);

        let rate = token_conversion
            .get_eth_conversion_price(TOKEN5, WETH_ADDRESS)
            .unwrap();

        assert_eq!(rate, Ray::scale_to_ray(U256::from(1) * WEI_IN_ETHER));
    }

    #[test]
    fn test_empty_pool_gas_price() {
        // Create a TokenPriceGenerator with no pools
        let token_conversion = TokenPriceGenerator {
            base_wei:            0,
            cur_block:           0,
            prev_prices:         HashMap::default(),
            base_gas_token:      WETH_ADDRESS,
            pair_to_pool:        HashMap::default(),
            blocks_to_avg_price: BLOCKS_TO_AVG_PRICE,
            uniswap_pools:       SyncedUniswapPools::new(
                Default::default(),
                tokio::sync::mpsc::channel(1).0
            )
        };

        // Test direct WETH case (should still work)
        let rate = token_conversion.get_eth_conversion_price(WETH_ADDRESS, TOKEN1);
        assert_eq!(
            rate,
            Some(Ray::scale_to_ray(U256::from(1))),
            "WETH as token0 should return 1 even with no pools"
        );

        // Test non-WETH tokens (should return None)
        let random_token1 = address!("1111111111111111111111111111111111111111");
        let random_token2 = address!("2222222222222222222222222222222222222222");

        let rate = token_conversion.get_eth_conversion_price(random_token1, random_token2);
        assert!(rate.is_none(), "Should return None when no pools exist");

        let rate = token_conversion.get_eth_conversion_price(random_token1, WETH_ADDRESS);
        assert!(rate.is_none(), "Should return None for direct WETH pair when no pools exist");

        // Test generate_lookup_map with no pools
        let lookup_map = token_conversion.generate_lookup_map();
        assert!(lookup_map.is_empty(), "Lookup map should be empty when no pools exist");
    }
}
