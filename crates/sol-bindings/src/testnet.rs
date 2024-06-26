use alloy_sol_macro::sol;

sol! {
    #[sol(rpc)]
    #[allow(clippy::too_many_arguments)]
    contract MockERC20 {
        function name() public view returns (string memory);
        function symbol() public view returns (string memory);
        function decimals() public view returns (uint8);
        function totalSupply() public view returns (uint256 result);

        function mint(address to, uint256 amount) external;

        function balanceOf(address owner) public view returns (uint256 result);
        function allowance(address owner, address spender) public view returns (uint256 allowed);
        function approve(address spender, uint256 amount) public returns (bool);
        function transfer(address to, uint256 amount) public returns (bool);
        function transferFrom(address from, address to, uint256 amount) public returns (bool);

        function nonces(address owner) public view returns (uint256 result);
        function permit(
            address owner,
            address spender,
            uint256 value,
            uint256 deadline,
            uint8 v,
            bytes32 r,
            bytes32 s
        ) external;
        function DOMAIN_SEPARATOR() public view returns (bytes32 result);
    }

    #[sol(rpc)]
    #[derive(Debug)]
    contract TestnetHub {
        function execute(bytes data) external;

        function initializePool(address asset0, address asset1, uint160 initialSqrtPriceX96) public;

        function isInitialized(address asset0, address asset1) public view returns (bool);

        function modifyLiquidity(address asset0, address asset1, int24 tickLower, int24 tickUpper, int256 liquidityDelta) public;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_testnet_hub() {
        dbg!(TestnetHub::TestnetHubCalls::SELECTORS);
    }
}
