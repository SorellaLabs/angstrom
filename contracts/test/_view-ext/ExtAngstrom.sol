// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Angstrom} from "../../src/Angstrom.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IUniV4, IPoolManager} from "../../src/interfaces/IUniV4.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Position} from "src/types/Positions.sol";
import {X128MathLib} from "src/libraries/X128MathLib.sol";
import {PoolConfigStore} from "src/libraries/PoolConfigStore.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract ExtAngstrom is Angstrom {
    using IUniV4 for IPoolManager;

    constructor(IPoolManager uniV4, address controller) Angstrom(uniV4, controller) {}

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
        unchecked {
            (Position storage position,) = positions.get(id, owner, lowerTick, upperTick, salt);
            uint256 growthInside =
                poolRewards[id].getGrowthInside(UNI_V4.getSlot0(id).tick(), lowerTick, upperTick);
            return X128MathLib.fullMulX128(growthInside - position.lastGrowthInside, liquidity);
        }
    }
}
