// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @dev Whether to add extra logs, if used in top-level IF should always get optimized out
/// completely, whether  viaIR is used or not.
bool constant DEBUG_LOGS = true;

bool constant MOCK_LOGS = true;
bool constant TRACE_LOGS = false;

uint256 constant TRACING_LEVEL = 0;
uint256 constant DEBUG_LEVEL = 1;
uint256 constant MED_LEVEL = 2;
uint256 constant LOG_LEVEL = TRACING_LEVEL;

bool constant TEST_LOGS = true;
