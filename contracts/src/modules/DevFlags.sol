// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @dev Whether to add extra logs, if used in top-level IF should always get optimized out
/// completely, whether  viaIR is used or not.
bool constant DEBUG_LOGS = true;

bool constant MOCK_LOGS = true;
