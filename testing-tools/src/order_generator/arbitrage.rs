use std::time::Duration;

use alloy::primitives::I256;
use alloy_primitives::U256;
use angstrom_types::sol_bindings::rpc_orders::TopOfBlockOrder};
use cex_exchanges::{
    binance::ws::{channels::BinanceBookTicker, BinanceWsMessage},
    clients::ws::MutliWsStream,
    normalized::{
        types::RawTradingPair,
        ws::{CombinedWsMessage, NormalizedExchangeBuilder, NormalizedWsChannelKinds}
    },
    CexExchange
};
use futures::{stream::BoxStream, StreamExt};
use uniswap_v4::uniswap::{
    pool::EnhancedUniswapV3Pool, pool_manager::UniswapPoolManager,
    pool_providers::PoolManagerProvider
};
use rand_distr::num_traits::Float;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone, Debug)]
pub struct PriceLevel {
    pub price:    f64,
    pub quantity: f64
}

pub struct ArbitrageGenerator<P> {
    pool_manager: UniswapPoolManager<P>,
    symbol:       String
}

impl<P> ArbitrageGenerator<P>
where
    P: PoolManagerProvider + Send + Sync + 'static
{
    pub fn new(pool_manager: UniswapPoolManager<P>, symbol: String) -> Self {
        Self { pool_manager, symbol }
    }

    fn create_price_feed_stream(&self) -> MutliWsStream {
        let mut builder = NormalizedExchangeBuilder::new();
        builder.add_pairs_single_channel_all_exchanges(
            &[CexExchange::Binance],
            NormalizedWsChannelKinds::Quotes,
            &[RawTradingPair::RawNoDelim { pair: self.symbol.clone() }]
        );

        builder
            .build_all_multistream(Some(3), Some(1))
            .expect("Failed to build multistream")
            .expect("Failed to unwrap multistream")
    }

    pub async fn monitor(&self) {
        let (mut pool_update_rx, _join_handles) =
            match self.pool_manager.subscribe_state_changes().await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!("Failed to subscribe to state changes: {}", e);
                    return;
                }
            };

        let mut price_feed = self.process_price_feed();
        let mut price_update: Option<BinanceBookTicker> = None;
        let mut last_check = tokio::time::Instant::now();
        loop {
            tokio::select! {
                Some((_address, _block_number)) = pool_update_rx.recv() => {
                    let pool = self.pool_manager.pool().await;
                    let _ = self.check_arbitrage(&pool, price_update.clone());
                }
                Some(feed_update) = price_feed.next() => {
                    let pool = self.pool_manager.pool().await;
                    price_update = feed_update;
                    if last_check.elapsed() >= Duration::from_secs(1) {
                        let _ = self.check_arbitrage(&pool, price_update.clone());
                        last_check = tokio::time::Instant::now();
                    }
                }
            }
        }
    }

    pub async fn order_stream(&self) -> BoxStream<Option<TopOfBlockOrder>> {
        let (pool_update_rx, _) = self.pool_manager.subscribe_state_changes().await.unwrap();
        let pool_update_stream = ReceiverStream::new(pool_update_rx).map(|_| None);
        let price_feed = self.process_price_feed();
        let pool_and_price_stream = futures::stream::select(pool_update_stream, price_feed);

        pool_and_price_stream
            .scan(None::<BinanceBookTicker>, |price_cache, message| {
                if let Some(price_update) = message {
                    *price_cache = Some(price_update.clone());
                    return futures::future::ready(Some(price_update))
                }
                futures::future::ready(price_cache.clone())
            })
            .then(|price_update| async {
                let pool = self.pool_manager.pool().await;
                self.check_arbitrage(&pool, Some(price_update))
            })
            .boxed()
    }

    fn process_price_feed(&self) -> BoxStream<Option<BinanceBookTicker>> {
        self.create_price_feed_stream()
            .map(|message| match message {
                CombinedWsMessage::Binance(BinanceWsMessage::BookTicker(book_ticker)) => {
                    Some(book_ticker)
                }
                e => {
                    tracing::error!("unhandled message {:?}", e);
                    None
                }
            })
            .boxed()
    }

    fn check_arbitrage(
        &self,
        pool: &EnhancedUniswapV3Pool,
        price_update: Option<BinanceBookTicker>
    ) -> Option<TopOfBlockOrder> {
        let price_update = price_update?;
        let BinanceBookTicker {
            best_ask_amt, best_ask_price, best_bid_amt, best_bid_price, ..
        } = price_update;
        let best_bid = PriceLevel { price: best_bid_price, quantity: best_bid_amt };
        let best_ask = PriceLevel { price: best_ask_price, quantity: best_ask_amt };

        tracing::debug!(
            "Best Bid on Binance: Price: {:.6} USDC, Quantity: {:.6} ETH | Best Ask on Binance: \
             Price: {:.6} USDC, Quantity: {:.6} ETH",
            best_bid_price,
            best_bid_amt,
            best_ask_price,
            best_ask_amt
        );

        let (ask_profit, ask_binance_amount, ask_uniswap_fill_price, ask_uniswap_amount) =
            self.try_sell_on_uniswap(pool, &best_ask);
        let (bid_profit, bid_binance_amount, bid_uniswap_fill_price, bid_uniswap_amount) =
            self.try_buy_on_uniswap(pool, &best_bid);

        tracing::debug!(
            "Ask Profit: {:.2} USDC vs Bid Profit: {:.2} USDC | Uniswap Ask Fill Price: {:.3} \
             USDC vs Bid Fill Price: {:.3} USDC | Ask Amount: {:.6} ETH vs Bid Amount: {:.6} USDC",
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

        if profit < 0.01 {
            return None;
        }

        tracing::info!(
            "{} on Binance vs {} on Uniswap | Binance: Price: {:.2} USDC vs Uniswap: Fill Price: \
             {:.2} USDC | Binance: Amount: {:.6} ETH vs Uniswap: Amount: {:.6} ETH | Profit: \
             {:.2} USDC",
            binance_trade_type,
            if binance_trade_type == "SELL" { "BUY" } else { "SELL" },
            binance_price,
            uniswap_fill_price,
            binance_amount,
            uniswap_amount,
            profit,
        );

        let zero_for_one = binance_trade_type == "SELL";
        Some(Self::create_order(zero_for_one, uniswap_amount))
    }

    fn create_order(zero_for_one: bool, amount: I256) -> TopOfBlockOrder {
        TopOfBlockOrder {
            amountIn: if zero_for_one { U256::try_from(amount.abs()).unwrap() } else { U256::ZERO },
            amountOut: if zero_for_one {
                U256::ZERO
            } else {
                U256::try_from(amount.abs()).unwrap()
            },
            ..TopOfBlockOrder::default()
        }
    }

    fn try_sell_on_uniswap(
        &self,
        pool: &EnhancedUniswapV3Pool,
        best_ask: &PriceLevel
    ) -> (f64, f64, f64, I256) {
        let eth = pool.token_b;
        let ask_amount = best_ask.quantity * 10_f64.powi(pool.token_b_decimals as i32);
        let ask_amount_in =
            I256::from_dec_str(ask_amount.to_string().split('.').next().unwrap()).unwrap();


        let (ask_swap_amount_in, ask_swap_amount_out) =
            pool.simulate_swap(eth, ask_amount_in, None).unwrap();

        let ask_uniswap_fill_price =
            self.calculate_uniswap_fill_price(pool, ask_swap_amount_in, ask_swap_amount_out);

        let token_b_scale = BigFloat::from(10).powi(pool.token_b_decimals as i32);
        let ask_binance_amount = best_ask.quantity;
        let ask_uniswap_amount =
            BigFloat::from(u128::try_from(ask_swap_amount_out.abs()).unwrap()).div(&token_b_scale);

        let ask_profit = (ask_uniswap_fill_price.to_f64() * ask_binance_amount)
            - (best_ask.price * ask_uniswap_amount.to_f64());

        (ask_profit, ask_binance_amount, ask_uniswap_fill_price.to_f64(), ask_amount_in)
    }

    fn try_buy_on_uniswap(
        &self,
        pool: &EnhancedUniswapV3Pool,
        best_bid: &PriceLevel
    ) -> (f64, f64, f64, I256) {
        let bid_amount =
            best_bid.quantity * best_bid.price * 10_f64.powi(pool.token_a_decimals as i32);
        let bid_amount_in =
            I256::from_dec_str(bid_amount.to_string().split('.').next().unwrap()).unwrap();
        let usdc = pool.token_a;

        let (bid_swap_amount_in, bid_swap_amount_out) =
            pool.simulate_swap(usdc, bid_amount_in, None).unwrap();

        let bid_uniswap_fill_price =
            self.calculate_uniswap_fill_price(pool, bid_swap_amount_in, bid_swap_amount_out);

        let token_b_scale = BigFloat::from(10).powi(pool.token_b_decimals as i32);
        let bid_binance_amount =  BigFloat::from(bid_amount).div(&token_b_scale).to_f64();
        let bid_uniswap_amount =
            BigFloat::from_u128(u128::try_from(bid_swap_amount_out.abs()).unwrap())
                .div(&token_b_scale);

        let bid_profit = (best_bid.price * bid_binance_amount)
            - (bid_uniswap_fill_price.to_f64() * bid_uniswap_amount.to_f64());

        (bid_profit, bid_binance_amount, bid_uniswap_fill_price.to_f64(), bid_amount_in)
    }

    fn calculate_uniswap_fill_price(
        &self,
        pool: &EnhancedUniswapV3Pool,
        swap_amount_in: I256,
        swap_amount_out: I256
    ) -> BigFloat {
        let amount_in = BigFloat::from_u128(u128::try_from(swap_amount_in.abs()).unwrap());
        let amount_out = BigFloat::from_u128(u128::try_from(swap_amount_out.abs()).unwrap());
        let decimal_adjustment = BigFloat::from(10)
            .pow(&BigFloat::from_u8(pool.token_b_decimals - pool.token_a_decimals));

        (amount_in / amount_out) * decimal_adjustment
    }
}
