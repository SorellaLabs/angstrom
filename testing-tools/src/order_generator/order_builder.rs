use alloy::primitives::{I256, U256};
use angstrom_types::{
    matching::SqrtPriceX96,
    primitive::{AngstromSigner, PoolId},
    sol_bindings::{grouped_orders::GroupedVanillaOrder, rpc_orders::TopOfBlockOrder}
};
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::type_generator::orders::ToBOrderBuilder;

pub struct OrderBuilder {
    keys:      Vec<AngstromSigner>,
    pool_id:   PoolId,
    /// pools to based orders off of
    pool_data: SyncedUniswapPools
}

impl OrderBuilder {
    pub fn new(pool_id: PoolId, pool_data: SyncedUniswapPools) -> Self {
        Self { keys: vec![], pool_id, pool_data }
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

        ToBOrderBuilder::new()
            .signing_key(self.keys.first().cloned())
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
        is_partial: bool
    ) -> GroupedVanillaOrder {
        let rng = rand::thread_rng();

        let pool = self.pool_data.get(&self.pool_id).unwrap().read().unwrap();
        let p_price = pool.calculate_price();
        // if the pool price > than price we want. given t1 / t0 -> more t0 less t1 ->
        // cur_price
        let zfo = p_price > cur_price;
        todo!()
    }
}
