use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloy::{
    primitives::{I256, U256},
    signers::local::PrivateKeySigner
};
use alloy_primitives::Address;
use angstrom_types::{
    matching::{Ray, SqrtPriceX96},
    primitive::AngstromSigner,
    sol_bindings::{grouped_orders::AllOrders, rpc_orders::TopOfBlockOrder}
};
use rand::Rng;
use uniswap_v3_math::tick_math::{MAX_SQRT_RATIO, MIN_SQRT_RATIO};
use uniswap_v4::uniswap::{
    pool::{SwapResult, U256_1},
    pool_manager::SyncedUniswapPool
};

use crate::{
    order_generator::InternalBalanceMode,
    type_generator::orders::{ToBOrderBuilder, UserOrderBuilder}
};

pub struct OrderBuilder {
    keys:                  Vec<AngstromSigner<PrivateKeySigner>>,
    t0:                    Address,
    t1:                    Address,
    /// pools to based orders off of
    pool_data:             SyncedUniswapPool,
    internal_balance_mode: InternalBalanceMode
}

impl OrderBuilder {
    pub fn new(pool_data: SyncedUniswapPool, internal_balance_mode: InternalBalanceMode) -> Self {
        let lock = pool_data.read().unwrap();
        let t0 = lock.token0;
        let t1 = lock.token1;
        drop(lock);

        Self { keys: vec![AngstromSigner::random(); 10], pool_data, t0, t1, internal_balance_mode }
    }

    pub fn get_token0_token1(&self) -> (Address, Address) {
        (self.t0, self.t1)
    }

    /// Determines if an order should use internal balances based on the
    /// configured mode
    fn should_use_internal(&self) -> bool {
        match &self.internal_balance_mode {
            InternalBalanceMode::Always => true,
            InternalBalanceMode::Never => false,
            InternalBalanceMode::Random(probability) => {
                let mut rng = rand::rng();
                rng.random_bool(*probability)
            }
        }
    }

    pub fn build_tob_order(&self, cur_price: f64, block_number: u64, gas: u128) -> TopOfBlockOrder {
        let pool = self.pool_data.read().unwrap();

        // convert price to sqrtx96
        let price: U256 = SqrtPriceX96::from_float_price(cur_price).into();
        let price = price.clamp(MIN_SQRT_RATIO + U256_1, MAX_SQRT_RATIO - U256_1);
        let sqrt_price_x96 = pool.sqrt_price_x96;
        let float_price = SqrtPriceX96::from(sqrt_price_x96).as_f64();
        tracing::info!(?cur_price, ?float_price);

        let zfo = sqrt_price_x96 > price;
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
        let mut rng = rand::rng();

        if !zfo {
            std::mem::swap(&mut amount_in, &mut amount_out);
        }

        let range = (amount_in / 100).max(101);
        amount_in += rng.random_range(100..range);

        if zfo {
            amount_in += gas;
        } else {
            amount_out += gas;
        }

        ToBOrderBuilder::new()
            .signing_key(self.keys.get(rng.random_range(0..10)).cloned())
            .asset_in(if zfo { token0 } else { token1 })
            .asset_out(if !zfo { token0 } else { token1 })
            .quantity_in(amount_in)
            .quantity_out(amount_out)
            .valid_block(block_number)
            .max_gas(gas * 2)
            .use_internal(self.should_use_internal())
            .build()
    }

    pub fn build_user_order(
        &self,
        cur_price: f64,
        block_number: u64,
        partial_pct: f64,
        gas: u128
    ) -> AllOrders {
        let mut rng = rand::rng();
        let is_partial = rng.random_bool(partial_pct);

        let pool = self.pool_data.read().unwrap();

        let price: U256 = SqrtPriceX96::from_float_price(cur_price).into();
        let price = price.clamp(MIN_SQRT_RATIO + U256_1, MAX_SQRT_RATIO - U256_1);

        let sqrt_price_x96 = pool.sqrt_price_x96;

        // if current price is higher than target price, we have a ask
        let zfo = sqrt_price_x96 > price;

        let token0 = pool.token0;
        let token1 = pool.token1;

        let t_in = if zfo { token0 } else { token1 };

        let exact_in = rng.random_bool(0.5);
        let amount_specified = if exact_in { I256::MAX - I256::ONE } else { I256::MIN + I256::ONE };

        let SwapResult { amount0, amount1, sqrt_price_x_96, .. } = pool
            ._simulate_swap(t_in, amount_specified, Some(price), false)
            .unwrap();

        let mut amount_in = u128::try_from(amount0.abs()).unwrap();
        let mut amount_out = u128::try_from(amount1.abs()).unwrap();
        let mut price = Ray::from(SqrtPriceX96::from(sqrt_price_x_96));

        if !zfo {
            std::mem::swap(&mut amount_in, &mut amount_out);
            price.inv_ray_assign_round(true);
        }
        let mut price = price.scale_to_fee(pool.book_fee as u128);

        let pct = Ray::generate_ray_decimal(95, 2);
        price.mul_ray_assign(pct);

        let modifier = rng.random_range(1.1..1.5);

        let amount = if exact_in { amount_in } else { amount_out };
        let amount = (amount as f64 * modifier) as u128;

        let deadline = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
            + Duration::from_secs(38))
        .as_secs();

        UserOrderBuilder::new()
            .signing_key(self.keys.get(rng.random_range(0..10)).cloned())
            .is_exact(!is_partial)
            .asset_in(if zfo { token0 } else { token1 })
            .asset_out(if !zfo { token0 } else { token1 })
            .is_standing(rng.random_bool(0.5))
            .deadline(U256::from(deadline))
            .nonce(rng.random())
            .max_gas(gas)
            .exact_in(exact_in)
            .min_price(price)
            .block(block_number)
            .amount(amount)
            .use_internal(self.should_use_internal())
            .build()
    }
}
