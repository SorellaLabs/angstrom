// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Angstrom} from "../../src/Angstrom.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IUniV4, IPoolManager} from "../../src/interfaces/IUniV4.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";
import {Position} from "src/libraries/Positions.sol";
import {PoolConfigStore} from "src/libraries/pool-config/PoolConfigStore.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract ExtAngstrom is Angstrom {
    using IUniV4 for IPoolManager;
    using FixedPointMathLib for *;

    constructor(IPoolManager uniV4, address governance) Angstrom(uniV4, governance, address(0)) {}

    function lastBlockUpdated() public view returns (uint64) {
        return _lastBlockUpdated;
    }

    function configStore() public view returns (PoolConfigStore) {
        return _configStore;
    }

    function isNode(address addr) public view returns (bool) {
        return _isNode[addr];
    }

    function DOMAIN_SEPARATOR() external view returns (bytes32) {
        return _domainSeparator();
    }

    function hashTyped(bytes32 structHash) external view returns (bytes32) {
        return _hashTypedData(structHash);
    }

    function rewardGrowthOutside(PoolId id, int24 tick) external view returns (uint256) {
        return poolRewards[id].rewardGrowthOutside[uint24(tick)];
    }

    function globalGrowthOutside(PoolId id) external view returns (uint256) {
        return poolRewards[id].globalGrowth;
    }

    function positionRewards(
        PoolId id,
        address owner,
        int24 lowerTick,
        int24 upperTick,
        bytes32 salt,
        uint128 liquidity
    ) external view returns (uint256) {
        return poolRewards[id].getGrowthInside(UNI_V4.getSlot0(id).tick(), lowerTick, upperTick)
            .mulWad(liquidity) - pastRewards(id, owner, lowerTick, upperTick, salt);
    }

    function pastRewards(PoolId id, address owner, int24 lowerTick, int24 upperTick, bytes32 salt)
        public
        view
        returns (uint256)
    {
        (Position storage position,) = positions.get(id, owner, lowerTick, upperTick, salt);
        return position.pastRewards;
    }
}
