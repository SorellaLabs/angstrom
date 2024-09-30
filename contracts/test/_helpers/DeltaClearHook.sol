// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IUniV4, IPoolManager} from "src/interfaces/IUniV4.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {
    IAngstromComposable, EXPECTED_HOOK_RETURN_MAGIC
} from "src/interfaces/IAngstromComposable.sol";
import {CommonBase} from "forge-std/Base.sol";

/// @author philogy <https://github.com/philogy>
contract DeltaClearHook is IAngstromComposable, CommonBase {
    using IUniV4 for IPoolManager;

    IPoolManager immutable UNI_V4;

    constructor(IPoolManager uni) {
        UNI_V4 = uni;
        require(
            address(vm).code.length > 0,
            "This contract uses vm.prank but vm not found, probably in prod-like environment"
        );
    }

    function compose(address, bytes calldata payload) external returns (uint32) {
        (address assetOut) = abi.decode(payload, (address));
        int256 amount = UNI_V4.getDelta(msg.sender, assetOut);
        vm.prank(msg.sender);
        UNI_V4.clear(Currency.wrap(assetOut), uint256(amount));
        return EXPECTED_HOOK_RETURN_MAGIC;
    }
}
