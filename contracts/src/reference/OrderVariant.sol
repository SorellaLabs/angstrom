// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {OrderVariantMap as Variant, OrderVariantMapLib as VariantLib} from "../types/OrderVariantMap.sol";
import {BitPackLib} from "./BitPackLib.sol";

struct OrderVariant {
    bool isExact;
    bool isFlash;
    bool isOut;
    bool noHook;
    bool useInternal;
    bool hasRecipient;
    bool isEcdsa;
    bool aToB;
}

using OrderVariantLib for OrderVariant global;

/// @author philogy <https://github.com/philogy>
library OrderVariantLib {
    using BitPackLib for uint256;

    function encode(OrderVariant memory variant) internal pure returns (Variant) {
        // forgefmt: disable-next-item
        return Variant.wrap(
            uint8(
                (!variant.isExact ? VariantLib.QTY_PARTIAL_BIT : 0)
                    .bitOverlay(!variant.isFlash ? VariantLib.IS_STANDING_BIT : 0)
                    .bitOverlay(!variant.isOut ? VariantLib.IS_EXACT_IN_BIT : 0)
                    .bitOverlay(!variant.noHook ? VariantLib.HAS_HOOK_BIT : 0)
                    .bitOverlay(variant.useInternal ? VariantLib.USE_INTERNAL_BIT : 0)
                    .bitOverlay(variant.hasRecipient ? VariantLib.HAS_RECIPIENT_BIT : 0)
                    .bitOverlay(variant.isEcdsa ? VariantLib.IS_ECDSA_BIT : 0)
                    .bitOverlay(variant.aToB ? VariantLib.A_TO_B_BIT : 0)
            )
        );
    }
}
