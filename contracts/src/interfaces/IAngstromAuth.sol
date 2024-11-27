// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

interface IAngstromAuth {
    function setController(address newController) external;
    function removePool(address expectedStore, uint256 storeIndex) external;
    function configurePool(address assetA, address assetB, uint16 tickSpacing, uint24 feeInE6)
        external;
    function pullFee(address asset, uint256 amount) external;
    function toggleNodes(address[] calldata nodes) external;
    function extsload(bytes32 key) external view returns (bytes32);
}
