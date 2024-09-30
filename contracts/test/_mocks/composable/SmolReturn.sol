// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {
    IAngstromComposable,
    EXPECTED_HOOK_RETURN_MAGIC
} from "../../../src/interfaces/IAngstromComposable.sol";

/// @author philogy <https://github.com/philogy>
contract SmolReturn is IAngstromComposable {
    bytes32 public retWord;
    uint8 public size;

    function setReturn(bytes32 retWord_, uint8 size_) external {
        require(size_ <= 32, "Size above 32");
        size = size_;
        retWord = retWord_;
    }

    function compose(address, bytes calldata) external view returns (uint32) {
        uint8 z = size;
        assembly {
            mstore(0x00, sload(retWord.slot))
            return(0x00, z)
        }
    }
}
