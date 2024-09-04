pub mod price_feed;

use std::{marker::PhantomData, sync::Arc};

use alloy::{
    hex,
    network::{Ethereum, Network, TransactionBuilder}
};
use alloy_primitives::{address, Address, Bytes, I256, U128};
use alloy_provider::Provider;
use alloy_transport::Transport;
use amms::amm::consts::U256_1;
use angstrom_types::matching::SqrtPriceX96;
use matching_engine::cfmm::uniswap::{
    pool::EnhancedUniswapV3Pool, pool_manager::UniswapPoolManager,
    pool_providers::PoolManagerProvider
};
use num_bigfloat::BigFloat;
use uniswap_v3_math::tick_math::MAX_SQRT_RATIO;

use crate::order_generator::price_feed::{PriceFeed, PriceLevel};

pub struct OrderGenerator<P, B, T, N> {
    pool_manager: UniswapPoolManager<P>,
    binance_feed: PriceFeed,
    provider:     Arc<B>,
    _phantom:     PhantomData<(T, N)>
}

impl<P, B, T, N> OrderGenerator<P, B, T, N>
where
    P: PoolManagerProvider + Send + Sync + 'static,
    B: Provider<T, N> + Send + Sync,
    T: Transport + Clone + Send + Sync,
    N: Network + Send + Sync
{
    pub async fn new(
        pool_manager: UniswapPoolManager<P>,
        binance_feed: PriceFeed,
        provider: Arc<B>
    ) -> Self {
        Self { pool_manager, binance_feed, provider, _phantom: PhantomData }
    }

    pub async fn start(self) {
        let (mut pool_update_rx, _join_handles) =
            match self.pool_manager.subscribe_state_changes().await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!("Failed to subscribe to state changes: {}", e);
                    return;
                }
            };

        let mut price_feed_rx = self.binance_feed.subscribe();

        loop {
            tokio::select! {
                Some((_address, _block_number)) = pool_update_rx.recv() => {
                    self.check_arbitrage().await;
                }
                Ok(_) = price_feed_rx.recv() => {
                    self.check_arbitrage().await;
                }
            }
        }
    }

    async fn check_arbitrage(&self) {
        // do it once at the top
        let pool = self.pool_manager.pool().await;

        let (mut bids, mut asks) = self.binance_feed.price_cache().await;
        let best_bid = bids.pop();
        let best_ask = asks.pop();
        tracing::debug!(
            "Best Bid on Binance: Price: {:.6} USDC, Quantity: {:.6} ETH | Best Ask on Binance: \
             Price: {:.6} USDC, Quantity: {:.6} ETH",
            best_bid.as_ref().map_or(0.0, |bid| bid.price),
            best_bid.as_ref().map_or(0.0, |bid| bid.quantity),
            best_ask.as_ref().map_or(0.0, |ask| ask.price),
            best_ask.as_ref().map_or(0.0, |ask| ask.quantity)
        );

        if let (Some(best_bid), Some(best_ask)) = (best_bid, best_ask) {
            let (ask_profit, ask_binance_amount, ask_uniswap_fill_price, ask_uniswap_amount) =
                self.try_sell_on_uniswap(&pool, &best_ask).await;
            let (bid_profit, bid_binance_amount, bid_uniswap_fill_price, bid_uniswap_amount) =
                self.try_buy_on_uniswap(&pool, &best_bid).await;

            tracing::debug!(
                "Ask Profit: {:.2} USDC vs Bid Profit: {:.2} USDC | Uniswap Ask Fill Price: {:.3} \
                 USDC vs Bid Fill Price: {:.3} USDC | Ask Amount: {:.6} ETH vs Bid Amount: {:.6} \
                 USDC",
                ask_profit,
                bid_profit,
                ask_uniswap_fill_price,
                bid_uniswap_fill_price,
                ask_uniswap_amount,
                bid_uniswap_amount
            );

            let (
                profit,
                binance_trade_type,
                binance_price,
                binance_amount,
                uniswap_fill_price,
                uniswap_amount
            ) = if ask_profit > bid_profit {
                (
                    ask_profit,
                    "BUY",
                    best_ask.price,
                    ask_binance_amount,
                    ask_uniswap_fill_price,
                    ask_uniswap_amount
                )
            } else {
                (
                    bid_profit,
                    "SELL",
                    best_bid.price,
                    bid_binance_amount,
                    bid_uniswap_fill_price,
                    bid_uniswap_amount
                )
            };

            if profit > 0.01_f64 {
                // let zero_for_one  = binance_trade_type == "SELL";
                // self.execute_trade(&pool, zero_for_one, uniswap_amount).await;
                tracing::info!(
                    "{} on Binance vs {} on Uniswap | Binance: Price: {:.2} USDC vs Uniswap: Fill \
                     Price: {:.2} USDC | Binance: Amount: {:.6} ETH vs Uniswap: Amount: {:.6} ETH \
                     | Profit: {:.2} USDC",
                    binance_trade_type,
                    if binance_trade_type == "SELL" { "BUY" } else { "SELL" },
                    binance_price,
                    uniswap_fill_price,
                    binance_amount,
                    uniswap_amount,
                    profit,
                );
            }
        }
    }

    async fn execute_trade(&self, pool: &EnhancedUniswapV3Pool, zero_for_one: bool, amount: I256) {
        let to_address = address!("4a18a50a8328b42773268B4b436254056b7d70CE");
        let call_data = pool
            .swap_calldata(to_address, zero_for_one, amount, MAX_SQRT_RATIO - U256_1, vec![])
            .unwrap();

        match self.submit_eth_call(to_address, call_data).await {
            Ok(response) => tracing::info!("Transaction submitted successfully: {:?}", response),
            Err(e) => tracing::error!("Failed to submit transaction: {}", e)
        }
    }

    // sell ETH
    async fn try_sell_on_uniswap(
        &self,
        pool: &EnhancedUniswapV3Pool,
        best_ask: &PriceLevel
    ) -> (f64, f64, f64, I256) {
        let eth = pool.token_b;
        let ask_amount = best_ask.quantity * 10_f64.powi(pool.token_b_decimals as i32);
        let ask_amount_in =
            I256::from_dec_str(ask_amount.to_string().split(".").next().unwrap()).unwrap();

        let (ask_swap_amount_in, ask_swap_amount_out) =
            pool.simulate_swap(eth, ask_amount_in, None).unwrap();

        // ETH/USDC
        let ask_uniswap_fill_price =
            self.calculate_uniswap_fill_price(pool, ask_swap_amount_in, ask_swap_amount_out);

        let ask_binance_amount = best_ask.quantity;
        let ask_uniswap_amount =
            ask_swap_amount_out.abs().as_u64() as f64 / 10_f64.powi(pool.token_b_decimals as i32);

        let ask_profit = (ask_uniswap_fill_price.to_f64() * ask_binance_amount)
            - (best_ask.price * ask_uniswap_amount);

        (ask_profit, ask_binance_amount, ask_uniswap_fill_price.to_f64(), ask_amount_in)
    }

    // buy ETH
    async fn try_buy_on_uniswap(
        &self,
        pool: &EnhancedUniswapV3Pool,
        best_bid: &PriceLevel
    ) -> (f64, f64, f64, I256) {
        let bid_amount =
            best_bid.quantity * best_bid.price * 10_f64.powi(pool.token_a_decimals as i32);
        let bid_amount_in =
            I256::from_dec_str(bid_amount.to_string().split(".").next().unwrap()).unwrap();
        let usdc = pool.token_a;

        let (bid_swap_amount_in, bid_swap_amount_out) =
            pool.simulate_swap(usdc, bid_amount_in, None).unwrap();

        // ETH/USDC
        let bid_uniswap_fill_price =
            self.calculate_uniswap_fill_price(pool, bid_swap_amount_in, bid_swap_amount_out);

        let bid_binance_amount = bid_amount / 10_f64.powi(pool.token_b_decimals as i32);
        let bid_uniswap_amount =
            bid_swap_amount_out.abs().as_u64() as f64 / 10_f64.powi(pool.token_b_decimals as i32);

        let bid_profit = (best_bid.price * bid_binance_amount)
            - (bid_uniswap_fill_price.to_f64() * bid_uniswap_amount);

        (bid_profit, bid_binance_amount, bid_uniswap_fill_price.to_f64(), bid_amount_in)
    }

    async fn submit_eth_call(
        &self,
        to_address: Address,
        call_data: Bytes
    ) -> Result<Bytes, Box<dyn std::error::Error>> {
        let mut tx = <N as Network>::TransactionRequest::default();
        tx.set_input(call_data);
        tx.set_to(to_address);
        let res = self.provider.call(&tx).await?;
        tracing::info!("eth_call response: {}", hex::encode(&res));
        Ok(res)
    }

    fn calculate_uniswap_fill_price(
        &self,
        pool: &EnhancedUniswapV3Pool,
        swap_amount_in: I256,
        swap_amount_out: I256
    ) -> BigFloat {
        // assumes b_dec > a_dec
        let amount_in = BigFloat::from_u128(u128::try_from(swap_amount_in.abs()).unwrap());
        let amount_out = BigFloat::from_u128(u128::try_from(swap_amount_out.abs()).unwrap());
        let decimal_adjustment = BigFloat::from(10)
            .pow(&BigFloat::from_u8(pool.token_b_decimals - pool.token_a_decimals));

        (amount_in / amount_out) * decimal_adjustment
    }
}
