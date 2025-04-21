// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {PoolConfigStore} from "../libraries/PoolConfigStore.sol";
import {StoreKey} from "../libraries/PoolConfigStore.sol";

/// @author philogy <https://github.com/philogy>
library AngstromView {
    uint256 constant CONTROLLER_SLOT = 0;

    uint256 constant IS_NODE_SLOT = 1;

    uint256 constant UNLOCKED_FEE_PACKED_SET_SLOT = 2;

    uint256 constant LAST_BLOCK_CONFIG_STORE_SLOT = 3;
    uint256 constant LAST_BLOCK_BIT_OFFSET = 0;
    uint256 constant STORE_BIT_OFFSET = 64;

    uint256 constant BALANCES_SLOT = 5;

    function controller(IAngstromAuth self) internal view returns (address) {
        return address(uint160(self.extsload(CONTROLLER_SLOT)));
    }

    function lastBlockUpdated(IAngstromAuth self) internal view returns (uint64) {
        return uint64(self.extsload(LAST_BLOCK_CONFIG_STORE_SLOT) >> LAST_BLOCK_BIT_OFFSET);
    }

    function configStore(IAngstromAuth self) internal view returns (PoolConfigStore addr) {
        uint256 value = self.extsload(LAST_BLOCK_CONFIG_STORE_SLOT);
        return PoolConfigStore.wrap(address(uint160(value >> STORE_BIT_OFFSET)));
    }

    function unlockedFee(IAngstromAuth self, StoreKey key)
        internal
        view
        returns (bool isSet, uint24 fee)
    {
        uint256 slot = uint256(keccak256(abi.encode(key, UNLOCKED_FEE_PACKED_SET_SLOT)));
        uint256 unlockedPackedIsSet = self.extsload(slot);
        isSet = unlockedPackedIsSet & 1 == 1;
        fee = uint24(unlockedPackedIsSet >> 1);
    }

    function balanceOf(IAngstromAuth self, address asset, address owner)
        internal
        view
        returns (uint256)
    {
        bytes32 assetMapSlot = keccak256(abi.encode(asset, BALANCES_SLOT));
        bytes32 ownerBalanceSlot = keccak256(abi.encode(owner, assetMapSlot));
        return self.extsload(uint256(ownerBalanceSlot));
    }
}
