// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Ownable} from "solady/src/auth/Ownable.sol";

/// @notice Simple contract which allows the contract owner to emit events.
contract EventEmitter is Ownable {
    enum FeeClaimType {
        GAS,
        PROTOCOL,
        BOTH
    }

    event FeeClaimQueued(
        address indexed asset,
        FeeClaimType indexed claimType,
        uint256 amount,
        uint256 startBlock,
        uint256 endBlock
    );

    event FeeClaimExecuted(
        address indexed asset,
        FeeClaimType indexed claimType,
        uint256 amount,
        uint256 startBlock,
        uint256 endBlock
    );

    error InputLengthMismatch();
    error TopicsExceedMax();

    constructor(address initialOwner) {
        _initializeOwner(initialOwner);
    }

    function emitFeeClaimQueued(
        address[] calldata assets,
        uint256[] calldata amounts,
        FeeClaimType claimType,
        uint256 startBlock,
        uint256 endBlock
    ) external onlyOwner {
        if (assets.length != amounts.length) {
            revert InputLengthMismatch();
        }
        for (uint256 i = 0; i < assets.length; ++i) {
            emit FeeClaimQueued(assets[i], claimType, amounts[i], startBlock, endBlock);
        }
    }

    function emitFeeClaimExecuted(
        address[] calldata assets,
        uint256[] calldata amounts,
        FeeClaimType claimType,
        uint256 startBlock,
        uint256 endBlock
    ) external onlyOwner {
        if (assets.length != amounts.length) {
            revert InputLengthMismatch();
        }
        for (uint256 i = 0; i < assets.length; ++i) {
            emit FeeClaimExecuted(assets[i], claimType, amounts[i], startBlock, endBlock);
        }
    }

    /// @notice Emits an event defined entirely by the calldata.
    /// @param topics defines the indexed topics and can have from 0 to 4 fields.
    /// @param logData defines the unindexed data of the log.
    function emitGenericEvent(bytes32[] calldata topics, bytes calldata logData)
        external
        onlyOwner
    {
        if (topics.length > 4) {
            revert TopicsExceedMax();
        }
        assembly {
            // Allocate memory for the logData
            let dataPtr := mload(0x40)
            let logDataLength := logData.length
            calldatacopy(dataPtr, logData.offset, logDataLength)

            // Update the free memory pointer
            mstore(0x40, add(dataPtr, logDataLength))

            let topicsLength := topics.length
            switch topicsLength
            case 0 {
                log0(dataPtr, logDataLength)
            }
            case 1 {
                log1(dataPtr, logDataLength, calldataload(topics.offset))
            }
            case 2 {
                log2(
                    dataPtr,
                    logDataLength,
                    calldataload(topics.offset),
                    calldataload(add(topics.offset, 32))
                )
            }
            case 3 {
                log3(
                    dataPtr,
                    logDataLength,
                    calldataload(topics.offset),
                    calldataload(add(topics.offset, 32)),
                    calldataload(add(topics.offset, 64))
                )
            }
            case 4 {
                log4(
                    dataPtr,
                    logDataLength,
                    calldataload(topics.offset),
                    calldataload(add(topics.offset, 32)),
                    calldataload(add(topics.offset, 64)),
                    calldataload(add(topics.offset, 96))
                )
            }
        }
    }
}
