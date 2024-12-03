use alloy::primitives::{I256, U256};
use angstrom_types::{
    matching::{Ray, SqrtPriceX96},
    primitive::{AngstromSigner, PoolId},
    sol_bindings::{grouped_orders::GroupedVanillaOrder, rpc_orders::TopOfBlockOrder}
};
use rand::Rng;
use tracing::info;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::type_generator::orders::{ToBOrderBuilder, UserOrderBuilder};

pub struct OrderBuilder {
    keys:      Vec<AngstromSigner>,
    pool_id:   PoolId,
    /// pools to based orders off of
    pool_data: SyncedUniswapPools
}

impl OrderBuilder {
    pub fn new(pool_id: PoolId, pool_data: SyncedUniswapPools) -> Self {
        Self { keys: vec![AngstromSigner::random(); 10], pool_id, pool_data }
    }

    pub fn build_tob_order(&self, cur_price: f64, block_number: u64) -> TopOfBlockOrder {
        let pool = self.pool_data.get(&self.pool_id).unwrap().read().unwrap();
        let p_price = pool.calculate_price();
        // if the pool price > than price we want. given t1 / t0 -> more t0 less t1 ->
        // cur_price
        let zfo = p_price > cur_price;

        // convert price to sqrtx96
        let price: U256 = SqrtPriceX96::from_float_price(cur_price).into();

        let token0 = pool.token_a;
        let token1 = pool.token_b;
        let t_in = if zfo { token0 } else { token1 };

        // want to swap to SqrtPriceX96. we set amount to negative so it will
        // just fil till we hit limit.
        let (amount_in, amount_out) = pool.simulate_swap(t_in, I256::MIN, Some(price)).unwrap();
        let amount_in = u128::try_from(amount_in.abs()).unwrap();
        let amount_out = u128::try_from(amount_out.abs()).unwrap();
        info!(%amount_in, %amount_out, %cur_price, pool_price=%p_price, "tob order builder");
        let mut rng = rand::thread_rng();

        ToBOrderBuilder::new()
            .signing_key(self.keys.get(rng.gen_range(0..10)).cloned())
            .asset_in(if zfo { token0 } else { token1 })
            .asset_out(if !zfo { token0 } else { token1 })
            .quantity_in(amount_in)
            .quantity_out(amount_out)
            .valid_block(block_number)
            .build()
    }

    pub fn build_user_order(
        &self,
        cur_price: f64,
        block_number: u64,
        partial_pct: f64
    ) -> GroupedVanillaOrder {
        let mut rng = rand::thread_rng();
        let is_partial = rng.gen_bool(partial_pct);

        let pool = self.pool_data.get(&self.pool_id).unwrap().read().unwrap();
        let p_price = pool.calculate_price();
        let unshifted_price = Ray::from(pool.calculate_price_unshifted());
        // if the pool price > than price we want. given t1 / t0 -> more t0 less t1 ->
        // cur_price
        let zfo = p_price > cur_price;
        let token0 = pool.token_a;
        let token1 = pool.token_b;

        let t_in = if zfo { token0 } else { token1 };
        let price: U256 = SqrtPriceX96::from_float_price(cur_price).into();
        let (amount_in, amount_out) = pool.simulate_swap(t_in, I256::MIN, Some(price)).unwrap();

        let amount_in = u128::try_from(amount_in.abs()).unwrap();
        let amount_out = u128::try_from(amount_out.abs()).unwrap();

        info!(%amount_in, %amount_out, %cur_price, pool_price=%p_price, "tob order builder");

        // 50% amount range
        let modifier = rng.gen_range(0.5..=1.5);
        let amount = (amount_in as f64 * modifier) as u128;

        UserOrderBuilder::new()
            .signing_key(self.keys.get(rng.gen_range(0..10)).cloned())
            .is_exact(!is_partial)
            .asset_in(if zfo { token0 } else { token1 })
            .asset_out(if !zfo { token0 } else { token1 })
            .is_standing(false)
            .exact_in(true)
            .min_price(unshifted_price)
            .block(block_number)
            .amount(amount)
            .build()
    }
}
