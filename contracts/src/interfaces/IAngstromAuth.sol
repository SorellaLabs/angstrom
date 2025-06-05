// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

import {StoreKey} from "../types/StoreKey.sol";
import {PoolConfigStore} from "../libraries/PoolConfigStore.sol";

struct ConfigEntryUpdate {
    uint256 index;
    StoreKey key;
    uint24 bundleFee;
    uint24 unlockedFee;
    uint24 protocolUnlockedFee;
}

interface IAngstromAuth {
    function setController(address newController) external;

    function collect_unlock_swap_fees(address to, bytes calldata packed_assets) external;

    function configurePool(
        address assetA,
        address assetB,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee,
        uint24 protocolUnlockedFee
    ) external;

    function removePool(StoreKey key, PoolConfigStore expectedStore, uint256 storeIndex) external;

    function batchUpdatePools(PoolConfigStore expected_store, ConfigEntryUpdate[] calldata updates)
        external;

    function pullFee(address asset, uint256 amount) external;

    function toggleNodes(address[] calldata nodes) external;

    function extsload(uint256 slot) external view returns (uint256);
}
