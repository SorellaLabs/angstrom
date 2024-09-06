// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {CalldataReader} from "../types/CalldataReader.sol";
import {SignatureCheckerLib} from "solady/src/utils/SignatureCheckerLib.sol";

/// @author philogy <https://github.com/philogy>
library SignatureLib {
    error InvalidSignature();

    uint256 internal constant ECRECOVER_ADDR = 1;

    function readAndCheckEcdsa(CalldataReader reader, bytes32 hash)
        internal
        view
        returns (CalldataReader, address from)
    {
        assembly ("memory-safe") {
            let free := mload(0x40)
            mstore(free, hash)
            // Ensure next word is clear
            mstore(add(free, 0x20), 0)
            // Read signature.
            calldatacopy(add(free, 0x3f), reader, 65)
            reader := add(reader, 65)
            // Call ec-Recover pre-compile (addr: 0x01).
            // Credit to Vectorized's ECDSA in Solady: https://github.com/Vectorized/solady/blob/a95f6714868cfe5d590145f936d0661bddff40d2/src/utils/ECDSA.sol#L108-L123
            from := mload(staticcall(gas(), ECRECOVER_ADDR, free, 0x80, 0x01, 0x20))
            if iszero(returndatasize()) {
                mstore(0x00, 0x8baa579f /* InvalidSignature() */ )
                revert(0x1c, 0x04)
            }
        }

        return (reader, from);
    }

    function readAndCheckERC1271(CalldataReader reader, bytes32 hash)
        internal
        view
        returns (CalldataReader, address from)
    {
        (reader, from) = reader.readAddr();
        bytes calldata signature;
        (reader, signature) = reader.readBytes();
        if (!SignatureCheckerLib.isValidERC1271SignatureNowCalldata(from, hash, signature)) {
            revert InvalidSignature();
        }
        return (reader, from);
    }
}
