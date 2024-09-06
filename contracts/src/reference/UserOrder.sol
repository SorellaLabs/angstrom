// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {
    PartialStandingOrder,
    ExactStandingOrder,
    PartialFlashOrder,
    ExactFlashOrder,
    OrderMeta,
    OrdersLib
} from "./OrderTypes.sol";
import {TypedDataHasher} from "src/types/TypedDataHasher.sol";
import {Pair} from "./Pair.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

import {console} from "forge-std/console.sol";

type UserOrder is uint256;

enum UserOrderVariant {
    PartialStandingOrder,
    ExactStandingOrder,
    PartialFlashOrder,
    ExactFlashOrder
}

using UserOrderLib for UserOrder global;
using UserOrderLib for UserOrderVariant global;

/// @author philogy <https://github.com/philogy>
library UserOrderLib {
    using SafeCastLib for *;

    function from(PartialStandingOrder memory spOrder) internal pure returns (UserOrder order) {
        UserOrderVariant variant = UserOrderVariant.PartialStandingOrder;
        assembly ("memory-safe") {
            order := or(shl(8, spOrder), variant)
        }
    }

    function from(ExactStandingOrder memory spOrder) internal pure returns (UserOrder order) {
        UserOrderVariant variant = UserOrderVariant.ExactStandingOrder;
        assembly ("memory-safe") {
            order := or(shl(8, spOrder), variant)
        }
    }

    function from(PartialFlashOrder memory spOrder) internal pure returns (UserOrder order) {
        UserOrderVariant variant = UserOrderVariant.PartialFlashOrder;
        assembly ("memory-safe") {
            order := or(shl(8, spOrder), variant)
        }
    }

    function from(ExactFlashOrder memory spOrder) internal pure returns (UserOrder order) {
        UserOrderVariant variant = UserOrderVariant.ExactFlashOrder;
        assembly ("memory-safe") {
            order := or(shl(8, spOrder), variant)
        }
    }

    function getVariant(UserOrder order) internal pure returns (UserOrderVariant variant) {
        assembly {
            variant := and(order, 0xff)
        }
    }

    function setMeta(UserOrder order, OrderMeta memory meta) internal pure {
        UserOrderVariant variant = order.getVariant();
        if (variant == UserOrderVariant.PartialStandingOrder) {
            _toPartialStandingFn(_toMemPtr)(order).meta = meta;
        } else if (variant == UserOrderVariant.ExactStandingOrder) {
            _toExactStandingFn(_toMemPtr)(order).meta = meta;
        } else if (variant == UserOrderVariant.PartialFlashOrder) {
            _toPartialFlashFn(_toMemPtr)(order).meta = meta;
        } else if (variant == UserOrderVariant.ExactFlashOrder) {
            _toExactFlashFn(_toMemPtr)(order).meta = meta;
        } else {
            revert("Unimplemented variant");
        }
    }

    function hash712(UserOrder order, TypedDataHasher hasher) internal pure returns (bytes32) {
        return hasher.hashTypedData(order.hash());
    }

    function hash(UserOrder order) internal pure returns (bytes32) {
        UserOrderVariant variant = order.getVariant();
        if (variant == UserOrderVariant.PartialStandingOrder) {
            return _toPartialStandingFn(_toMemPtr)(order).hash();
        } else if (variant == UserOrderVariant.ExactStandingOrder) {
            return _toExactStandingFn(_toMemPtr)(order).hash();
        } else if (variant == UserOrderVariant.PartialFlashOrder) {
            return _toPartialFlashFn(_toMemPtr)(order).hash();
        } else if (variant == UserOrderVariant.ExactFlashOrder) {
            return _toExactFlashFn(_toMemPtr)(order).hash();
        } else {
            revert("Unimplemented variant");
        }
    }

    function encode(UserOrder[] memory orders, Pair[] memory pairs) internal pure returns (bytes memory b) {
        for (uint256 i = 0; i < orders.length; i++) {
            b = bytes.concat(b, _logB(orders[i].encode(pairs)));
        }
        b = bytes.concat(bytes3(b.length.toUint24()), b);
    }

    function encode(UserOrder order, Pair[] memory pairs) internal pure returns (bytes memory) {
        UserOrderVariant variant = order.getVariant();
        if (variant == UserOrderVariant.PartialStandingOrder) {
            return _toPartialStandingFn(_toMemPtr)(order).encode(pairs);
        } else if (variant == UserOrderVariant.ExactStandingOrder) {
            return _toExactStandingFn(_toMemPtr)(order).encode(pairs);
        } else if (variant == UserOrderVariant.PartialFlashOrder) {
            return _toPartialFlashFn(_toMemPtr)(order).encode(pairs);
        } else if (variant == UserOrderVariant.ExactFlashOrder) {
            return _toExactFlashFn(_toMemPtr)(order).encode(pairs);
        } else {
            revert("Unimplemented variant");
        }
    }

    function toStr(UserOrder order) internal pure returns (string memory) {
        UserOrderVariant variant = order.getVariant();
        if (variant == UserOrderVariant.PartialStandingOrder) {
            return _toPartialStandingFn(_toMemPtr)(order).toStr();
        } else if (variant == UserOrderVariant.ExactStandingOrder) {
            return _toExactStandingFn(_toMemPtr)(order).toStr();
        } else if (variant == UserOrderVariant.PartialFlashOrder) {
            return _toPartialFlashFn(_toMemPtr)(order).toStr();
        } else if (variant == UserOrderVariant.ExactFlashOrder) {
            return _toExactFlashFn(_toMemPtr)(order).toStr();
        } else {
            revert("Unimplemented variant");
        }
    }

    function _toPartialStandingFn(function(UserOrder) internal pure returns (bytes32) fnIn)
        private
        pure
        returns (function(UserOrder) internal pure returns (PartialStandingOrder memory) fnOut)
    {
        assembly {
            fnOut := fnIn
        }
    }

    function _toExactStandingFn(function(UserOrder) internal pure returns (bytes32) fnIn)
        private
        pure
        returns (function(UserOrder) internal pure returns (ExactStandingOrder memory) fnOut)
    {
        assembly {
            fnOut := fnIn
        }
    }

    function _toPartialFlashFn(function(UserOrder) internal pure returns (bytes32) fnIn)
        private
        pure
        returns (function(UserOrder) internal pure returns (PartialFlashOrder memory) fnOut)
    {
        assembly {
            fnOut := fnIn
        }
    }

    function _toExactFlashFn(function(UserOrder) internal pure returns (bytes32) fnIn)
        private
        pure
        returns (function(UserOrder) internal pure returns (ExactFlashOrder memory) fnOut)
    {
        assembly {
            fnOut := fnIn
        }
    }

    function _toMemPtr(UserOrder order) private pure returns (bytes32 ptr) {
        assembly ("memory-safe") {
            ptr := shr(8, order)
        }
    }

    function _logB(bytes memory b) internal pure returns (bytes memory) {
        return b;
    }

    function toStr(UserOrderVariant variant) internal pure returns (string memory) {
        if (variant == UserOrderVariant.PartialStandingOrder) {
            return "UserOrderVariant::PartialStandingOrder";
        } else if (variant == UserOrderVariant.ExactStandingOrder) {
            return "UserOrderVariant::ExactStandingOrder";
        } else if (variant == UserOrderVariant.PartialFlashOrder) {
            return "UserOrderVariant::PartialFlashOrder";
        } else if (variant == UserOrderVariant.ExactFlashOrder) {
            return "UserOrderVariant::ExactFlashOrder";
        } else {
            revert("Unimplemented variant");
        }
    }
}
