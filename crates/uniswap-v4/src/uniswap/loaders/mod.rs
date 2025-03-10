#[rustfmt::skip]
pub mod get_uniswap_v_3_pool_data {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        GetUniswapV3PoolData,
        "../../contracts/out/GetUniswapV3PoolData.sol/GetUniswapV3PoolData.json"
    );
}
#[rustfmt::skip]
pub mod get_uniswap_v_3_tick_data {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        GetUniswapV3TickData,
        "../../contracts/out/GetUniswapV3TickData.sol/GetUniswapV3TickData.json"
    );
}
#[rustfmt::skip]
pub mod get_uniswap_v_4_pool_data {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        GetUniswapV4PoolData,
        "../../contracts/out/GetUniswapV4PoolData.sol/GetUniswapV4PoolData.json"
    );
}
#[rustfmt::skip]
pub mod get_uniswap_v_4_tick_data {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        GetUniswapV4TickData,
        "../../contracts/out/GetUniswapV4TickData.sol/GetUniswapV4TickData.json"
    );
}
