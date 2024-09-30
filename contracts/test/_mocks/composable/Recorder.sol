// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {
    IAngstromComposable,
    EXPECTED_HOOK_RETURN_MAGIC
} from "../../../src/interfaces/IAngstromComposable.sol";

/// @author philogy <https://github.com/philogy>
contract Recorder is IAngstromComposable {
    uint256 public callCount;
    address public lastFrom;
    bytes public lastPayload;

    function compose(address from, bytes calldata payload) external returns (uint32) {
        callCount++;
        lastFrom = from;
        lastPayload = payload;
        return EXPECTED_HOOK_RETURN_MAGIC;
    }
}
