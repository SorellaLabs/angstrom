// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
library Ops {
    ////////////////////////////////////////////////////////////////
    //                        USER-TO-USER                        //
    ////////////////////////////////////////////////////////////////

    /// @dev Intra-user Liquid asset transfer.
    uint256 internal constant UU_LIQUID = 0x00;

    /// @dev Intra-user V4 claim transfer.
    uint256 internal constant UU_V4_CLAIM = 0x01;

    ////////////////////////////////////////////////////////////////
    //                       USER-TO-SYSTEM                       //
    ////////////////////////////////////////////////////////////////

    /// @dev User to Angstrom liquid asset transfer.
    uint256 internal constant US_PULL_LIQUID = 0x10;

    /// @dev Angstrom to user liquid asset transfer.
    uint256 internal constant US_PUSH_LIQUID = 0x11;

    /// @dev User to Uniswap V4 (free balance) liquid asset transfer.
    uint256 internal constant US_PULL_TO_V4 = 0x12;

    /// @dev Uniswap V4 (free balance) to User liquid asset transfer.
    uint256 internal constant US_PUSH_FROM_V4 = 0x13;

    /// @dev Burn V4 claim for V4 Delta
    uint256 internal constant US_BURN_V4 = 0x14;

    /// @dev Mint V4 claim from V4 Delta
    uint256 internal constant US_MINT_V4 = 0x15;

    ////////////////////////////////////////////////////////////////
    //                     SYSTEM MANAGEMENT                      //
    ////////////////////////////////////////////////////////////////

    /// @dev Calls `take` on V4, accruing V4 delta in exchange for Angstrom free balance.
    uint256 internal constant SYS_TAKE_FROM_V4 = 0x20;

    /// @dev Sends liquid tokens to V4, exchaning Angstrom free balance for V4 free balance.
    uint256 internal constant SYS_SEND_TO_V4 = 0x21;

    /// @dev Calls `settle` on V4, erasing debt in-exchange for reducing V4 free balance.
    uint256 internal constant SYS_SETTLE_V4 = 0x22;
}
