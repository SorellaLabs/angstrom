use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH}
};

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, I256, U256},
    providers::Provider,
    sol_types::SolCall
};
use alloy_primitives::TxKind;
use alloy_rpc_types::TransactionRequest;
use angstrom_rpc::api::OrderApiClient;
use angstrom_types::{
    matching::{Ray, SqrtPriceX96},
    primitive::{ANGSTROM_DOMAIN, AngstromSigner},
    sol_bindings::{grouped_orders::AllOrders, rpc_orders::OmitOrderMeta}
};
use testing_tools::type_generator::orders::{ToBOrderBuilder, UserOrderBuilder};
use uniswap_v4::uniswap::pool::{EnhancedUniswapPool, SwapResult};

use crate::env::ProviderType;

/// given a pool and a user, looks at balances of the user and generates trades
/// based off of this.
pub struct PoolIntentBundler<T>
where
    T: OrderApiClient + Send + Sync + 'static
{
    pool:            EnhancedUniswapPool,
    block_number:    u64,
    keys:            Vec<AngstromSigner>,
    provider:        Arc<ProviderType>,
    angstrom_client: Arc<T>
}

impl<T> PoolIntentBundler<T>
where
    T: OrderApiClient + Send + Sync + 'static
{
    pub fn new(
        pool: EnhancedUniswapPool,
        block_number: u64,
        keys: Vec<AngstromSigner>,
        provider: Arc<ProviderType>,
        angstrom_client: Arc<T>
    ) -> Self {
        Self { pool, block_number, keys, provider, angstrom_client }
    }

    pub async fn new_block(&mut self, block_number: u64) -> eyre::Result<()> {
        self.block_number = block_number;
        self.pool
            .update_to_block(Some(block_number), self.provider.clone())
            .await
            .map_err(Into::into)
    }

    pub async fn submit_new_orders_to_angstrom(&self) -> eyre::Result<()> {
        tracing::info!("building orders for block");
        let orders = self.generate_orders_for_block().await?;
        tracing::info!("orders for block {:#?}", orders);
        let res = self.angstrom_client.send_orders(orders).await?;
        for order in res {
            tracing::info!(?order);
        }

        Ok(())
    }

    async fn generate_orders_for_block(&self) -> eyre::Result<Vec<AllOrders>> {
        tokio::time::sleep(Duration::from_millis(15)).await;
        let mut all_orders = self.generate_book_intents().await?;
        // let mut all_orders = vec![];
        all_orders.push(self.generate_tob_intent().await?);

        Ok(all_orders)
    }

    async fn generate_tob_intent(&self) -> eyre::Result<AllOrders> {
        let pool_price = Ray::from(SqrtPriceX96::from(self.pool.sqrt_price));
        let mut gas = self
            .angstrom_client
            .estimate_gas(false, self.pool.token0, self.pool.token1)
            .await?
            .unwrap();
        // cannot have zero gas.
        if gas.is_zero() {
            gas += U256::from(1);
        }

        let key = &self.keys[0];
        let addr = key.address();
        tracing::info!(?addr, "signing tob with");

        let (amount, zfo) = self
            .fetch_direction_and_amounts(key, &pool_price, true)
            .await;

        // limit to crossing 30 ticks a swap
        let target_price = if zfo {
            uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(
                self.pool.tick - (5 * self.pool.tick_spacing)
            )
            .unwrap()
        } else {
            uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(
                self.pool.tick + (5 * self.pool.tick_spacing)
            )
            .unwrap()
        };

        let t_in = if zfo { self.pool.token0 } else { self.pool.token1 };
        let (amount_in, amount_out) = self
            .pool
            .simulate_swap(t_in, amount, Some(target_price))
            .unwrap();

        let mut amount_in = u128::try_from(amount_in.abs()).unwrap();
        let mut amount_out = u128::try_from(amount_out.abs()).unwrap();

        if !zfo {
            std::mem::swap(&mut amount_in, &mut amount_out);
        }
        let range = (amount_in / 100).max(101);
        amount_in += self.gen_range(100, range);

        let order = ToBOrderBuilder::new()
            .signing_key(Some(key.clone()))
            .asset_in(if zfo { self.pool.token0 } else { self.pool.token1 })
            .asset_out(if !zfo { self.pool.token0 } else { self.pool.token1 })
            .quantity_in(amount_in)
            .max_gas(gas.to())
            .quantity_out(amount_out)
            .valid_block(self.block_number + 1)
            .build();
        let recovery_order_hash = order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        tracing::info!(?order, ?recovery_order_hash);

        Ok(order.into())
    }

    /// based on the users distribution of tokens in the pool, will generate
    /// a order that goes in the direction to even out the token amount. This
    /// naturally will lead to orders being placed in both directions based
    /// off inventory.
    async fn generate_book_intents(&self) -> eyre::Result<Vec<AllOrders>> {
        let mut res = Vec::new();

        for key in &self.keys {
            res.push(self.angstrom_signer_inner(key).await?);
        }

        Ok(res)
    }

    async fn angstrom_signer_inner(&self, key: &AngstromSigner) -> eyre::Result<AllOrders> {
        let mut gas = self
            .angstrom_client
            .estimate_gas(true, self.pool.token0, self.pool.token1)
            .await?
            .unwrap();
        // cannot have zero gas.
        if gas.is_zero() {
            gas += U256::from(1);
        }
        let pool_price = Ray::from(SqrtPriceX96::from(self.pool.sqrt_price));

        let mut exact_in: bool = self.random_time_bool();
        let is_partial: bool = self.random_time_bool();
        if is_partial {
            exact_in = true;
        }

        let (amount, zfo) = self
            .fetch_direction_and_amounts(key, &pool_price, exact_in)
            .await;

        let t_in = if zfo { self.pool.token0 } else { self.pool.token1 };

        let target_price = if zfo {
            uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(
                self.pool.tick - (10 * self.pool.tick_spacing)
            )
            .unwrap()
        } else {
            uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(
                self.pool.tick + (10 * self.pool.tick_spacing)
            )
            .unwrap()
        };

        let SwapResult { amount0, amount1, sqrt_price_x_96, .. } =
            self.pool._simulate_swap(t_in, amount, Some(target_price))?;

        let mut clearing_price = Ray::from(SqrtPriceX96::from(sqrt_price_x_96));
        // how much we want to reduce our price from as we don't need the exact.
        // we shave 5% off
        let pct = Ray::generate_ray_decimal(95, 2);
        clearing_price.mul_ray_assign(pct);

        let amount = if zfo == exact_in {
            u128::try_from(amount0.abs()).unwrap()
        } else {
            u128::try_from(amount1.abs()).unwrap()
        };

        // 2% range, should be fine given we only move 2/3 of balance at a time
        let modifier = self.random_amount_modifier_time();
        let amount = (amount as f64 * modifier) as u128;

        if !zfo {
            // if we are a bid. we flip the price
            clearing_price.inv_ray_assign_round(true);
        }
        let deadline = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
            // 3 blocks
            + Duration::from_secs(36))
        .as_secs();

        Ok(UserOrderBuilder::new()
            .signing_key(Some(key.clone()))
            .is_exact(!is_partial)
            .asset_in(if zfo { self.pool.token0 } else { self.pool.token1 })
            .asset_out(if !zfo { self.pool.token0 } else { self.pool.token1 })
            .is_standing(true)
            .gas_price_asset_zero(gas.to())
            .deadline(U256::from(deadline))
            .nonce(deadline)
            .exact_in(exact_in)
            .min_price(clearing_price)
            .block(self.block_number + 1)
            .amount(amount)
            .build()
            .into())
    }

    async fn make_call<TY: SolCall>(&self, from: Address, target: Address, call: TY) -> TY::Return {
        let bytes = self
            .provider
            .call(
                TransactionRequest::default()
                    .with_from(from)
                    .with_kind(TxKind::Call(target))
                    .with_input(call.abi_encode())
            )
            .await
            .unwrap();
        TY::abi_decode_returns(&bytes, true).unwrap()
    }

    // (amount, zfo)
    async fn fetch_direction_and_amounts(
        &self,
        key: &AngstromSigner,
        pool_price: &Ray,
        exact_in: bool
    ) -> (I256, bool) {
        let token0_bal = self
            .make_call(key.address(), self.pool.token0, crate::balanceOfCall::new((key.address(),)))
            .await;
        let token1_bal = self
            .make_call(key.address(), self.pool.token1, crate::balanceOfCall::new((key.address(),)))
            .await;

        if token0_bal.balance.is_zero() || token1_bal.balance.is_zero() {
            panic!(
                "no funds are in the given wallet t0: {:?} t1: {:?} wallet: {:?}",
                self.pool.token0,
                self.pool.token1,
                key.address()
            );
        }

        let t1_with_current_price = pool_price.mul_quantity(token0_bal.balance);
        // if the current amount of t0 mulled through the price is more than our other
        // balance this means that we have more t0 then t1 and thus want to sell
        // some t0 for t1
        let zfo = t1_with_current_price > token1_bal.balance;

        let amount = if exact_in {
            // exact in will swap 1/6 of the balance
            I256::unchecked_from(if zfo {
                token0_bal.balance / U256::from(50)
            } else {
                token1_bal.balance / U256::from(50)
            })
        } else {
            // exact out
            I256::unchecked_from(if zfo {
                t1_with_current_price / U256::from(50)
            } else {
                token1_bal.balance / U256::from(50)
            })
            .wrapping_neg()
        };

        (amount, zfo)
    }

    // NOTE: these are all very unsafe for production. only should be used for
    // testing.

    fn gen_range(&self, lower: u128, upper: u128) -> u128 {
        assert!(lower < upper);
        let top = upper + 1;

        let modu = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            % top;

        modu.max(lower)
    }

    fn random_amount_modifier_time(&self) -> f64 {
        let modu = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            % 5) as f64;

        0.98 + (modu * 1e-2)
    }

    fn random_time_bool(&self) -> bool {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            % 2
            == 0
    }
}
