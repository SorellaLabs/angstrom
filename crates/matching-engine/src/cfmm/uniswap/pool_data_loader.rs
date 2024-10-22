use std::{future::Future, sync::Arc};

use alloy::{
    primitives::{aliases::I24, Address, BlockNumber, U256},
    providers::{Network, Provider},
    sol,
    sol_types::SolType,
    transports::Transport
};
use amms::errors::AMMError;
use angstrom_types::primitive::PoolId;

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    IGetUniswapV3TickDataBatchRequest,
    "src/cfmm/uniswap/GetUniswapV3TickDataBatchRequest.json"
}

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    IGetUniswapV3PoolDataBatchRequest,
    "src/cfmm/uniswap/GetUniswapV3PoolDataBatchRequest.json"
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

#[derive(Debug, Clone)]
pub struct UniswapV3TickData {
    pub initialized:     bool,
    pub tick:            i32,
    pub liquidity_gross: u128,
    pub liquidity_net:   i128
}

#[derive(Default, Clone, Debug)]
pub struct DataLoader<A> {
    address: A
}

impl<A> DataLoader<A> {
    pub fn new(address: A) -> Self {
        DataLoader { address }
    }
}

pub trait PoolDataLoader<A> {
    fn load_tick_data<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &self,
        current_tick: I24,
        zero_for_one: bool,
        num_ticks: u16,
        tick_spacing: I24,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> impl Future<Output = Result<(Vec<UniswapV3TickData>, U256), AMMError>> + Send;

    fn address(&self) -> A;

    fn load_pool_data<T, N, P>(
        &self,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> impl Future<Output = Result<PoolData, AMMError>> + Send
    where
        T: Transport + Clone,
        N: Network,
        P: Provider<T, N>;
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
    ) -> Result<(Vec<UniswapV3TickData>, U256), AMMError> {
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

        let tick_data: Vec<UniswapV3TickData> = result
            .ticks
            .iter()
            .map(|tick| UniswapV3TickData {
                initialized:     tick.initialized,
                tick:            tick.tick.as_i32(),
                liquidity_gross: tick.liquidityGross,
                liquidity_net:   tick.liquidityNet
            })
            .collect();
        Ok((tick_data, result.blockNumber))
    }

    fn address(&self) -> Address {
        self.address
    }

    async fn load_pool_data<T, N, P>(
        &self,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<PoolData, AMMError>
    where
        T: Transport + Clone,
        N: Network,
        P: Provider<T, N>
    {
        let deployer = IGetUniswapV3PoolDataBatchRequest::deploy_builder(provider, self.address);
        let res = if let Some(block_number) = block_number {
            deployer.block(block_number.into()).call_raw().await?
        } else {
            deployer.call_raw().await?
        };

        let pool_data = PoolData::abi_decode(&res, true)?;
        Ok(pool_data)
    }
}

impl PoolDataLoader<PoolId> for DataLoader<PoolId> {
    async fn load_tick_data<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &self,
        current_tick: I24,
        zero_for_one: bool,
        num_ticks: u16,
        tick_spacing: I24,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<(Vec<UniswapV3TickData>, U256), AMMError> {
        todo!()
    }

    fn address(&self) -> PoolId {
        self.address
    }

    async fn load_pool_data<T, N, P>(
        &self,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<PoolData, AMMError>
    where
        T: Transport + Clone,
        N: Network,
        P: Provider<T, N>
    {
        todo!()
    }
}
