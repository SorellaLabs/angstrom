// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {RayMathLib} from "../libraries/RayMathLib.sol";

type PriceAB is uint256;

type AmountA is uint256;

type AmountB is uint256;

using PriceLib for PriceAB global;
using PriceLib for AmountA global;
using {addA as +, subA as -, gtA as >, ltA as <} for AmountA global;
using PriceLib for AmountB global;
using {addB as +, subB as -, gtB as >, ltB as <} for AmountB global;

function addA(AmountA x, AmountA y) pure returns (AmountA) {
    return AmountA.wrap(x.into() + y.into());
}

function subA(AmountA x, AmountA y) pure returns (AmountA) {
    return AmountA.wrap(x.into() - y.into());
}

function gtA(AmountA x, AmountA y) pure returns (bool) {
    return x.into() > y.into();
}

function ltA(AmountA x, AmountA y) pure returns (bool) {
    return x.into() < y.into();
}

function addB(AmountB x, AmountB y) pure returns (AmountB) {
    return AmountB.wrap(x.into() + y.into());
}

function subB(AmountB x, AmountB y) pure returns (AmountB) {
    return AmountB.wrap(x.into() - y.into());
}

function gtB(AmountB x, AmountB y) pure returns (bool) {
    return x.into() > y.into();
}

function ltB(AmountB x, AmountB y) pure returns (bool) {
    return x.into() < y.into();
}

/// @author philogy <https://github.com/philogy>
library PriceLib {
    using RayMathLib for *;

    function into(PriceAB price) internal pure returns (uint256) {
        return PriceAB.unwrap(price);
    }

    function into(AmountA amount) internal pure returns (uint256) {
        return AmountA.unwrap(amount);
    }

    function into(AmountB amount) internal pure returns (uint256) {
        return AmountB.unwrap(amount);
    }

    function convert(PriceAB priceAB, AmountA amountA) internal pure returns (AmountB) {
        return AmountB.wrap(amountA.into().divRay(priceAB.into()));
    }

    function convert(PriceAB priceAB, AmountB amountB) internal pure returns (AmountA) {
        return AmountA.wrap(amountB.into().mulRay(priceAB.into()));
    }

    function mulRayScalar(AmountA x, uint256 ray) internal pure returns (AmountA) {
        return AmountA.wrap(x.into().mulRay(ray));
    }

    function mulRayScalar(AmountB x, uint256 ray) internal pure returns (AmountB) {
        return AmountB.wrap(x.into().mulRay(ray));
    }

    function mulRayScalar(PriceAB price, uint256 ray) internal pure returns (PriceAB) {
        return PriceAB.wrap(price.into().mulRay(ray));
    }
}
