// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {StdCheatsSafe} from "forge-std/StdCheats.sol";
import {Vm} from "forge-std/Vm.sol";
import {UserOrder} from "../../src/reference/UserOrder.sol";
import {TypedDataHasher} from "../../src/types/TypedDataHasher.sol";

/// @author philogy <https://github.com/philogy>
library AccountPlusLib {
    using AccountPlusLib for StdCheatsSafe.Account;

    Vm constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function sign(StdCheatsSafe.Account memory account, bytes32 hash)
        internal
        pure
        returns (uint8 v, bytes32 r, bytes32 s)
    {
        (v, r, s) = vm.sign(account.key, hash);
    }

    function sign2098(StdCheatsSafe.Account memory account, bytes32 hash)
        internal
        pure
        returns (bytes32 vs, bytes32 r)
    {
        uint8 v;
        bytes32 s;
        (v, r, s) = account.sign(hash);
        vs = bytes32((uint256(v - 27) << 255) | uint256(s));
    }
}
