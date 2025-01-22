// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IPositionManager} from "v4-periphery/src/interfaces/IPositionManager.sol";
import {PositionInfo} from "v4-periphery/src/libraries/PositionInfoLibrary.sol";

struct Position {
    uint256 tokenId;
    int24 tickLower;
    int24 tickUpper;
    bytes25 poolId;
}

/// @author philogy <https://github.com/philogy>
contract PositionFetcher {
    IPositionManager internal immutable _MANAGER;
    address internal immutable _ANGSTROM;

    uint256 internal constant ID_UNCHECKED = 0;
    uint256 internal constant ID_BAD = 1;
    uint256 internal constant ID_GOOD = 2;

    constructor(IPositionManager manager, address angstrom) {
        _MANAGER = manager;
        _ANGSTROM = angstrom;
    }

    uint256 internal constant _POSITION_STRUCT_SIZE = 0x80;

    function getPositions(address owner, uint256 tokenId, uint256 lastTokenId, uint256 maxResults)
        public
        returns (uint256, uint256, Position[] memory)
    {
        if (lastTokenId == 0) lastTokenId = _MANAGER.nextTokenId();

        if (maxResults == 0) return (tokenId, lastTokenId, new Position[](0));

        uint256 returnData_ptr;
        assembly ("memory-safe") {
            returnData_ptr := mload(0x40)
            mstore(0x40, add(add(returnData_ptr, 0x80), mul(maxResults, _POSITION_STRUCT_SIZE)))
            mstore(add(returnData_ptr, 0x20), lastTokenId)
            mstore(add(returnData_ptr, 0x40), 0x60)
            mstore(add(returnData_ptr, 0x60), 0)
        }

        Position memory pos;

        address m = address(_MANAGER);
        uint256 noError = 1;
        for (; tokenId < lastTokenId; tokenId++) {
            address tokenOwner;
            assembly ("memory-safe") {
                mstore(0x00, 0x6352211e /* ownerOf(uint256) */ )
                mstore(0x20, tokenId)
                noError := and(noError, staticcall(gas(), m, 0x1c, 0x24, 0x00, 0x20))
                tokenOwner := mload(0x00)
            }
            if (tokenOwner != owner) continue;

            PositionInfo info;
            assembly ("memory-safe") {
                mstore(0x00, 0x89097a6a /* positionInfo(uint256) */ )
                noError := and(noError, staticcall(gas(), m, 0x1c, 0x24, 0x00, 0x20))
                info := mload(0x00)
            }

            bytes25 poolId = info.poolId();
            uint256 idStatus;
            assembly ("memory-safe") {
                idStatus := tload(poolId)
            }

            if (idStatus == ID_BAD) continue;

            if (idStatus == ID_UNCHECKED) {
                address a = _ANGSTROM;
                address hook;
                assembly ("memory-safe") {
                    mstore(0x00, 0x86b6be7d /* poolKeys(bytes25) */ )
                    mstore(0x20, poolId)
                    noError := and(noError, staticcall(gas(), m, 0x1c, 0x24, 0x00, 0x00))
                    returndatacopy(0x00, 0x80, 0x20)
                    hook := mload(0x00)
                    idStatus := add(eq(hook, a), 1)
                    tstore(poolId, idStatus)
                }
                if (idStatus == ID_BAD) continue;
            }

            uint256 length;
            assembly ("memory-safe") {
                length := mload(add(returnData_ptr, 0x60))
                pos := add(add(returnData_ptr, 0x80), mul(length, _POSITION_STRUCT_SIZE))
            }
            pos.tokenId = tokenId;
            pos.tickLower = info.tickLower();
            pos.tickUpper = info.tickUpper();
            pos.poolId = poolId;

            assembly ("memory-safe") {
                length := add(length, 1)
                mstore(add(returnData_ptr, 0x60), length)
            }

            if (length >= maxResults) {
                tokenId++;
                break;
            }
        }

        require(noError == 1);

        assembly ("memory-safe") {
            let length := mload(add(returnData_ptr, 0x60))
            mstore(returnData_ptr, tokenId)
            return(
                returnData_ptr, add(add(returnData_ptr, 0x80), mul(length, _POSITION_STRUCT_SIZE))
            )
        }
    }
}
