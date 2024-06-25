// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
abstract contract DumbDispatch {
    error NotThis();

    modifier dispatched() {
        if (msg.sender != address(this)) revert NotThis();
        _;
    }
}
