use std::{collections::HashMap, future::Future, sync::Arc};

use alloy::{
    primitives::{aliases::I24, Address, BlockNumber, U256},
    providers::{Network, Provider},
    sol,
    sol_types::{SolEvent, SolType},
    transports::Transport
};
use alloy_primitives::{Log, B256, I256};
use angstrom_types::primitive::{PoolId as AngstromPoolId, UniswapPoolRegistry};
use itertools::Itertools;
use malachite::{num::conversion::traits::RoundingInto, Natural, Rational};

use crate::uniswap::{i128_to_i256, i256_to_i128, pool::PoolError};

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    IGetUniswapV3TickDataBatchRequest,
    "src/uniswap/loaders/GetUniswapV3TickData.json"
}

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    IGetUniswapV3PoolDataBatchRequest,
    "src/uniswap/loaders/GetUniswapV3PoolData.json"
}

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    IGetUniswapV4TickDataBatchRequest,
    "src/uniswap/loaders/GetUniswapV4TickData.json"
}

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    IGetUniswapV4PoolDataBatchRequest,
    "src/uniswap/loaders/GetUniswapV4PoolData.json"
}

sol! {
    struct PoolData {
        address tokenA;
        uint8 tokenADecimals;
        address tokenB;
        uint8 tokenBDecimals;
        uint128 liquidity;
        uint160 sqrtPrice;
        int24 tick;
        int24 tickSpacing;
        uint24 fee;
        int128 liquidityNet;
    }

    struct PoolDataV4 {
        uint8 token0Decimals;
        uint8 token1Decimals;
        uint128 liquidity;
        uint160 sqrtPrice;
        int24 tick;
        int128 liquidityNet;
    }

    struct TickData {
        bool initialized;
        int24 tick;
        uint128 liquidityGross;
        int128 liquidityNet;
    }

    struct TicksWithBlock {
        TickData[] ticks;
        uint256 blockNumber;
    }
}
use angstrom_types::matching::SqrtPriceX96;

impl PoolData {
    /// converts the loaded sqrt_price_x96 into the limit price that is used
    /// for internal math. this is different than just the raw conversion given
    /// that we don't do any decimal adjustments
    pub fn get_raw_price(&self) -> U256 {
        let this = SqrtPriceX96::from(self.sqrtPrice);
        let tick = this.to_tick().expect("should never fail");
        // TODO: not a fan of this given precision will be lost. could cause problems
        // down the road.
        let normalized_price = 1.0001_f64.powi(tick);

        // because this is user set values and we want to upkeep readability on the
        // frontend, this value is not stored as a RAY and instead is a
        // unadjusted t1/t0
        let price = Rational::try_from(normalized_price).unwrap();
        let (output, _): (Natural, _) =
            price.rounding_into(malachite::rounding_modes::RoundingMode::Floor);
        let limbs = output.to_limbs_asc();

        U256::from_limbs_slice(&limbs)
    }
}

sol! {
    type PoolId is bytes32;

    #[derive(Debug, PartialEq, Eq)]
    contract IUniswapV4Pool {
        event Swap(PoolId indexed id, address indexed sender, int128 amount0, int128 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick, uint24 fee);
        event ModifyLiquidity(PoolId indexed id, address indexed sender, int24 tickLower, int24 tickUpper, int256 liquidityDelta, bytes32 salt);
    }

    #[derive(Debug, PartialEq, Eq)]
    contract IUniswapV3Pool {
        event Swap(address indexed sender, address indexed recipient, int256 amount0, int256 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick);
        event Burn(address indexed owner, int24 indexed tickLower, int24 indexed tickUpper, uint128 amount, uint256 amount0, uint256 amount1);
        event Mint(address sender, address indexed owner, int24 indexed tickLower, int24 indexed tickUpper, uint128 amount, uint256 amount0, uint256 amount1);
    }
}

#[derive(Debug, Clone)]
pub struct SwapEvent {
    pub sender:         Address,
    pub amount0:        I256,
    pub amount1:        I256,
    pub sqrt_price_x96: U256,
    pub liquidity:      u128,
    pub tick:           i32
}

#[derive(Debug, Clone)]
pub struct ModifyPositionEvent {
    pub sender:          Address,
    pub tick_lower:      i32,
    pub tick_upper:      i32,
    pub liquidity_delta: i128
}

#[derive(Default, Clone)]
pub struct DataLoader<A> {
    address:       A,
    pool_registry: Option<UniswapPoolRegistry>,
    pool_manager:  Option<Address>
}

pub trait PoolDataLoader<A>: Clone {
    fn load_tick_data<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &self,
        current_tick: I24,
        zero_for_one: bool,
        num_ticks: u16,
        tick_spacing: I24,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> impl Future<Output = Result<(Vec<TickData>, U256), PoolError>> + Send;

    fn load_pool_data<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &self,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> impl Future<Output = Result<PoolData, PoolError>> + Send;

    fn address(&self) -> A;

    fn group_logs(logs: Vec<Log>) -> HashMap<A, Vec<Log>>;
    fn event_signatures() -> Vec<B256>;
    fn is_swap_event(log: &Log) -> bool;
    fn is_modify_position_event(log: &Log) -> bool;
    fn decode_swap_event(log: &Log) -> Result<SwapEvent, PoolError>;
    fn decode_modify_position_event(log: &Log) -> Result<ModifyPositionEvent, PoolError>;
}

impl PoolDataLoader<Address> for DataLoader<Address> {
    async fn load_tick_data<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &self,
        current_tick: I24,
        zero_for_one: bool,
        num_ticks: u16,
        tick_spacing: I24,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<(Vec<TickData>, U256), PoolError> {
        let deployer = IGetUniswapV3TickDataBatchRequest::deploy_builder(
            provider.clone(),
            self.address,
            zero_for_one,
            current_tick,
            num_ticks,
            tick_spacing
        );

        let data = match block_number {
            Some(number) => deployer.block(number.into()).call_raw().await?,
            None => deployer.call_raw().await?
        };

        let result = TicksWithBlock::abi_decode(&data, true)?;

        Ok((result.ticks, result.blockNumber))
    }

    async fn load_pool_data<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &self,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<PoolData, PoolError> {
        let deployer = IGetUniswapV3PoolDataBatchRequest::deploy_builder(provider, self.address);
        let res = if let Some(block_number) = block_number {
            deployer.block(block_number.into()).call_raw().await?
        } else {
            deployer.call_raw().await?
        };

        let pool_data = PoolData::abi_decode(&res, true)?;
        Ok(pool_data)
    }

    fn address(&self) -> Address {
        self.address
    }

    fn group_logs(logs: Vec<Log>) -> HashMap<Address, Vec<Log>> {
        logs.into_iter()
            .map(|log| (log.address, log))
            .into_group_map()
    }

    fn event_signatures() -> Vec<B256> {
        vec![
            IUniswapV3Pool::Swap::SIGNATURE_HASH,
            IUniswapV3Pool::Mint::SIGNATURE_HASH,
            IUniswapV3Pool::Burn::SIGNATURE_HASH,
        ]
    }

    fn is_swap_event(log: &Log) -> bool {
        log.topics()
            .iter()
            .any(|t| *t == IUniswapV3Pool::Swap::SIGNATURE_HASH)
    }

    fn is_modify_position_event(log: &Log) -> bool {
        log.topics().iter().any(|t| {
            *t == IUniswapV3Pool::Mint::SIGNATURE_HASH || *t == IUniswapV3Pool::Burn::SIGNATURE_HASH
        })
    }

    fn decode_swap_event(log: &Log) -> Result<SwapEvent, PoolError> {
        let swap_event = IUniswapV3Pool::Swap::decode_log(log, true)?;
        Ok(SwapEvent {
            sender:         swap_event.sender,
            amount0:        swap_event.amount0,
            amount1:        swap_event.amount1,
            sqrt_price_x96: U256::from(swap_event.sqrtPriceX96),
            liquidity:      swap_event.liquidity,
            tick:           swap_event.tick.as_i32()
        })
    }

    fn decode_modify_position_event(log: &Log) -> Result<ModifyPositionEvent, PoolError> {
        if log.topics()[0] == IUniswapV3Pool::Mint::SIGNATURE_HASH {
            let mint_event = IUniswapV3Pool::Mint::decode_log(log, true)?;
            Ok(ModifyPositionEvent {
                sender:          mint_event.sender,
                tick_lower:      mint_event.tickLower.as_i32(),
                tick_upper:      mint_event.tickUpper.as_i32(),
                liquidity_delta: mint_event.amount as i128
            })
        } else {
            let burn_event = IUniswapV3Pool::Burn::decode_log(log, true)?;
            Ok(ModifyPositionEvent {
                sender:          burn_event.owner,
                tick_lower:      burn_event.tickLower.as_i32(),
                tick_upper:      burn_event.tickUpper.as_i32(),
                liquidity_delta: -(burn_event.amount as i128)
            })
        }
    }
}

impl DataLoader<Address> {
    pub fn new(address: Address) -> Self {
        DataLoader { address, pool_registry: None, pool_manager: None }
    }
}

impl DataLoader<AngstromPoolId> {
    fn pool_manager(&self) -> Address {
        self.pool_manager
            .expect("pool_manager must be set for V4 pools")
    }

    pub fn new_with_registry(
        address: AngstromPoolId,
        registry: UniswapPoolRegistry,
        pool_manager: Address
    ) -> Self {
        Self { address, pool_registry: Some(registry), pool_manager: Some(pool_manager) }
    }
}

impl PoolDataLoader<AngstromPoolId> for DataLoader<AngstromPoolId> {
    async fn load_pool_data<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &self,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<PoolData, PoolError> {
        let pool_key = self
            .pool_registry
            .as_ref()
            .unwrap()
            .get(&self.address())
            .unwrap()
            .clone();

        let deployer = IGetUniswapV4PoolDataBatchRequest::deploy_builder(
            provider,
            self.address(),
            self.pool_manager(),
            pool_key.currency0,
            pool_key.currency1
        );

        let data = match block_number {
            Some(number) => deployer.block(number.into()).call_raw().await?,
            None => deployer.call_raw().await?
        };
        let pool_data_v4 = PoolDataV4::abi_decode(&data, true)?;

        Ok(PoolData {
            tokenA:         pool_key.currency0,
            tokenADecimals: pool_data_v4.token0Decimals,
            tokenB:         pool_key.currency1,
            tokenBDecimals: pool_data_v4.token1Decimals,
            liquidity:      pool_data_v4.liquidity,
            sqrtPrice:      pool_data_v4.sqrtPrice,
            tick:           pool_data_v4.tick,
            tickSpacing:    pool_key.tickSpacing,
            fee:            pool_key.fee,
            liquidityNet:   pool_data_v4.liquidityNet
        })
    }

    async fn load_tick_data<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &self,
        current_tick: I24,
        zero_for_one: bool,
        num_ticks: u16,
        tick_spacing: I24,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<(Vec<TickData>, U256), PoolError> {
        let deployer = IGetUniswapV4TickDataBatchRequest::deploy_builder(
            provider.clone(),
            self.address(),
            self.pool_manager(),
            zero_for_one,
            current_tick,
            num_ticks,
            tick_spacing
        );

        let data = match block_number {
            Some(number) => deployer.block(number.into()).call_raw().await?,
            None => deployer.call_raw().await?
        };

        let result = TicksWithBlock::abi_decode(&data, true)?;

        Ok((result.ticks, result.blockNumber))
    }

    fn address(&self) -> AngstromPoolId {
        self.address
    }

    fn group_logs(logs: Vec<Log>) -> HashMap<AngstromPoolId, Vec<Log>> {
        logs.into_iter()
            .filter_map(|log| {
                if Self::is_modify_position_event(&log) {
                    let modify_event =
                        IUniswapV4Pool::ModifyLiquidity::decode_log(&log, true).ok()?;
                    return Some((modify_event.id, log))
                } else if Self::is_swap_event(&log) {
                    let swap = IUniswapV4Pool::Swap::decode_log(&log, true).ok()?;
                    return Some((swap.id, log))
                };
                None
            })
            .into_group_map()
    }

    fn event_signatures() -> Vec<B256> {
        vec![IUniswapV4Pool::Swap::SIGNATURE_HASH, IUniswapV4Pool::ModifyLiquidity::SIGNATURE_HASH]
    }

    fn is_swap_event(log: &Log) -> bool {
        log.topics()
            .iter()
            .any(|t| *t == IUniswapV4Pool::Swap::SIGNATURE_HASH)
    }

    fn is_modify_position_event(log: &Log) -> bool {
        log.topics()
            .iter()
            .any(|t| *t == IUniswapV4Pool::ModifyLiquidity::SIGNATURE_HASH)
    }

    fn decode_swap_event(log: &Log) -> Result<SwapEvent, PoolError> {
        let swap_event = IUniswapV4Pool::Swap::decode_log(log, true)?;
        Ok(SwapEvent {
            sender:         swap_event.sender,
            amount0:        i128_to_i256(swap_event.amount0),
            amount1:        i128_to_i256(swap_event.amount1),
            sqrt_price_x96: U256::from(swap_event.sqrtPriceX96),
            liquidity:      swap_event.liquidity,
            tick:           swap_event.tick.as_i32()
        })
    }

    fn decode_modify_position_event(log: &Log) -> Result<ModifyPositionEvent, PoolError> {
        let modify_event = IUniswapV4Pool::ModifyLiquidity::decode_log(log, true)?;
        Ok(ModifyPositionEvent {
            sender:          modify_event.sender,
            tick_lower:      modify_event.tickLower.as_i32(),
            tick_upper:      modify_event.tickUpper.as_i32(),
            // v4-core seems to be doing the same so it's ok
            // contracts/lib/v4-core/src/PoolManager.sol:160
            liquidity_delta: i256_to_i128(modify_event.liquidityDelta)?
        })
    }
}
