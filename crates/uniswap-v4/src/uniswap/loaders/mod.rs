#[rustfmt::skip]
pub mod get_uniswap_v_4_pool_data {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        GetUniswapV4PoolData,
        "../../abi-v4/GetUniswapV4PoolData.sol/GetUniswapV4PoolData.json"
    );
}
#[rustfmt::skip]
pub mod get_uniswap_v_4_tick_data {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        GetUniswapV4TickData,
        "../../abi-v4/GetUniswapV4TickData.sol/GetUniswapV4TickData.json"
    );
}
