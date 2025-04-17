use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloy::primitives::{I256, U256};
use angstrom_types::{
    matching::{Ray, SqrtPriceX96},
    primitive::AngstromSigner,
    sol_bindings::{grouped_orders::GroupedVanillaOrder, rpc_orders::TopOfBlockOrder}
};
use rand::Rng;
use uniswap_v3_math::tick_math::{MAX_SQRT_RATIO, MIN_SQRT_RATIO};
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPool;

use crate::type_generator::orders::{ToBOrderBuilder, UserOrderBuilder};

pub struct OrderBuilder {
    keys:      Vec<AngstromSigner>,
    /// pools to based orders off of
    pool_data: SyncedUniswapPool
}

impl OrderBuilder {
    pub fn new(pool_data: SyncedUniswapPool) -> Self {
        Self { keys: vec![AngstromSigner::random(); 10], pool_data }
    }

    pub fn build_tob_order(&self, cur_price: f64, block_number: u64) -> TopOfBlockOrder {
        let pool = self.pool_data.read().unwrap();

        // convert price to sqrtx96
        let price: U256 = SqrtPriceX96::from_float_price(cur_price).into();
        let price = price.clamp(MIN_SQRT_RATIO, MAX_SQRT_RATIO);
        let sqrt_price = pool.sqrt_price;
        let float_price = SqrtPriceX96::from(sqrt_price).as_f64();
        tracing::info!(?cur_price, ?float_price);

        let zfo = sqrt_price > price;
        tracing::info!(?zfo, "generated tob order direction");

        let token0 = pool.token0;
        let token1 = pool.token1;
        // if zfo, sqrtprice < pool price
        // always zero for 1
        let t_in = if zfo { token0 } else { token1 };
        let amount_specified = if zfo { I256::MAX - I256::ONE } else { I256::MIN + I256::ONE };
        // if zero for 1, sqrt lowever

        let (amount_in, amount_out) = pool
            .simulate_swap(t_in, amount_specified, Some(price))
            .unwrap();

        let mut amount_in = u128::try_from(amount_in.abs()).unwrap();
        let mut amount_out = u128::try_from(amount_out.abs()).unwrap();
        let mut rng = rand::thread_rng();

        if !zfo {
            std::mem::swap(&mut amount_in, &mut amount_out);
        }

        let range = (amount_in / 100).max(101);
        amount_in += rng.gen_range(100..range);

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

        let pool = self.pool_data.read().unwrap();

        let mut unshifted_price = Ray::from(
            pool.calculate_price_unshifted(SqrtPriceX96::from_float_price(cur_price).into())
        );
        // if the pool price > than price we want. given t1 / t0 -> more t0 less t1 ->
        // cur_price

        let price: U256 = SqrtPriceX96::from_float_price(cur_price).into();
        let price = price.clamp(MIN_SQRT_RATIO, MAX_SQRT_RATIO);

        let sqrt_price = pool.sqrt_price;

        let zfo = sqrt_price > price;

        let token0 = pool.token0;
        let token1 = pool.token1;

        let t_in = if zfo { token0 } else { token1 };
        let amount_specified = if zfo { I256::MAX - I256::ONE } else { I256::MIN + I256::ONE };

        let (amount0, amount1) = pool
            .simulate_swap(t_in, amount_specified, Some(price))
            .unwrap();

        // amount of token zero

        let mut amount_in = u128::try_from(amount0.abs()).unwrap();
        let mut amount_out = u128::try_from(amount1.abs()).unwrap();
        if !zfo {
            std::mem::swap(&mut amount_in, &mut amount_out);
        }

        let exact_in = rng.gen_bool(0.5);
        let modifier = rng.gen_range(0.099..=1.001);

        let amount = if exact_in { amount_in } else { amount_out };

        let amount = (amount as f64 * modifier) as u128;

        // allow for more flexibility
        let pct = Ray::generate_ray_decimal(75, 2);
        unshifted_price.mul_ray_assign(pct);

        // if the random direction changes the swap. inv the price
        if !zfo {
            unshifted_price.inv_ray_assign_round(true);
        }

        let deadline = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
            + Duration::from_secs(38))
        .as_secs();

        UserOrderBuilder::new()
            .signing_key(self.keys.get(rng.gen_range(0..10)).cloned())
            .is_exact(!is_partial)
            .asset_in(if zfo { token0 } else { token1 })
            .asset_out(if !zfo { token0 } else { token1 })
            .is_standing(rng.gen_bool(0.5))
            .deadline(U256::from(deadline))
            .nonce(rng.r#gen())
            .exact_in(exact_in)
            .min_price(unshifted_price)
            .block(block_number)
            .amount(amount)
            .build()
    }
}
