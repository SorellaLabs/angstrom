// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
library X128MathLib {
    error FullMulX128Failed();

    /// @dev Computes `(numerator * 2**128) / denominator` returning `0` if `denominator = 0`
    /// instead of reverting.
    function flatDivX128(uint128 numerator, uint256 denominator)
        internal
        pure
        returns (uint256 result)
    {
        assembly ("memory-safe") {
            result := div(shl(128, numerator), denominator)
        }
    }

    /// @dev Compute `floor((x * y) / 2**128)` with full precision. Revert if result is larger than / 256 bits.
    function fullMulX128(uint256 x, uint256 y) internal pure returns (uint256 z) {
        // Credit to solady under MIT license (https://github.com/Vectorized/solady/blob/7deab021af0426307ae79d091c4d1e26e9e89cf0/src/utils/FixedPointMathLib.sol).
        assembly ("memory-safe") {
            // Reuse `z` to store lower 256 bits of `x * y`.
            z := mul(x, y)
            for {} 1 {} {
                // We can skip the fancy 512-bit stuff if the 256-bit mul didn't overflow.
                if iszero(or(iszero(x), eq(div(z, x), y))) {
                    // Use mathemagic to compute the upper 256 bits of `x * y`.
                    // Credit to Remco Bloeman under MIT License: https://xn--2-umb.com/17/chinese-remainder-theorem/.
                    let mm := mulmod(x, y, not(0))
                    let p1 := sub(mm, add(z, lt(mm, z))) // Upper 256 bits of `x * y`.

                    // We now have the 512-bit numerator (`x * y`) in (p1, z):
                    // numerator:            |        p1        |       z       |
                    // In 128-bit chunks:    |  p1_0  ¦   p1_1  |  z_0  ¦  z_1  |
                    // Right shifted result: |    0   ¦   p1_0  |  p1_1 ¦  z_0  |
                    // The lower 128 bits of `z` (z_1) are part of the fraction which `floor` discards.

                    // We check the final result doesn't overflow by checking that p1_0 = 0.
                    if iszero(lt(p1, shl(128, 1))) {
                        mstore(0x00, 0xc56a0159 /* FullMulX128Failed() */ )
                        revert(0x1c, 0x04)
                    }

                    // We now know that our result doesn't overflow.
                    // Non-overflowing result: |    0   ¦    0    |  p1_1  ¦   z_0   |
                    // We compute p1_1 and z_0 and slice together.
                    z := add(shl(128, p1), shr(128, z))
                    break
                }

                z := shr(128, z)
                break
            }
        }
    }
}
