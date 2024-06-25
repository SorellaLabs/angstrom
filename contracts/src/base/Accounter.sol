// SPDX-License-Identifier: BUSL-1.1
pragma solidity 0.8.24;

import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {SilentERC6909} from "./SilentERC6909.sol";
import {tuint256, tint256} from "transient-goodies/TransientPrimitives.sol";
import {AssetForm} from "../interfaces/OrderTypes.sol";
import {DumbDispatch} from "./DumbDispatch.sol";
import {ConversionLib} from "../libraries/ConversionLib.sol";
import {Globals} from "../libraries/Globals.sol";

import {IPoolManager, IUniV4} from "../interfaces/IUniV4.sol";
import {AssetIndex} from "../libraries/PriceGraph.sol";
import {BitmapLib} from "../libraries/BitmapLib.sol";
import {MixedSignLib} from "../libraries/MixedSignLib.sol";
import {RayMathLib} from "../libraries/RayMathLib.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";

// TODO: Remove debug.
import {console2 as console} from "forge-std/console2.sol";
import {FormatLib} from "../libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract Accounter is SilentERC6909, DumbDispatch {
    // TODO: Remove debug.
    using FormatLib for *;

    using SafeCastLib for uint256;
    using SafeTransferLib for address;
    using ConversionLib for *;
    using RayMathLib for uint256;
    using PoolIdLibrary for PoolKey;
    using BitmapLib for uint256;
    using MixedSignLib for uint256;
    using IUniV4 for IPoolManager;

    error NotUniswap();

    IPoolManager internal immutable UNI_V4;

    mapping(address => uint256) internal _savedFees;
    mapping(PoolId id => mapping(int24 tick => uint256 cumulativeFeeGrowth)) internal feeGrowthOutside;
    mapping(PoolId id => uint256 cumulativeFeeGrowth) internal feeGrowthGlobal;

    mapping(address => mapping(address => tint256)) internal netLiquid;
    mapping(address => mapping(address => tint256)) internal netV4Claims;
    tuint256 internal unresolvedChanges;
    /// @dev Amount of available liquid assets within the angstrom contract.
    mapping(address => tuint256) internal freeBalance;

    constructor(address uniV4) {
        UNI_V4 = IPoolManager(uniV4);
    }

    modifier onlyUniV4() {
        if (msg.sender != address(UNI_V4)) revert NotUniswap();
        _;
    }

    ////////////////////////////////////////////////////////////////
    //                    EXECUTION ACCOUNTING                    //
    ////////////////////////////////////////////////////////////////

    function _accountIn(address from, AssetForm atype, address asset, uint256 amount) internal {
        if (atype == AssetForm.AngstromClaim) {
            _burn(from, asset.into(), amount);
            // Don't need to update unresolved as it doesn't support negatives and is overflow
            // checking, worst case unused balance is lost.
            freeBalance[asset].inc(amount);
            return;
        }
        if (atype == AssetForm.UniV4Claim) {
            _accountChange(netV4Claims[from][asset], amount.neg());
            return;
        }
        if (atype == AssetForm.Liquid) {
            _accountChange(netLiquid[from][asset], amount.neg());
            return;
        }
        // Should be unreachable (<3 to Solidity for not having match statements).
        assert(false);
    }

    function _accountOut(address to, AssetForm atype, address asset, uint256 amount) internal {
        if (atype == AssetForm.AngstromClaim) {
            _mint(to, asset.into(), amount);
            // Don't need to update unresolved as it doesn't support negatives and is overflow
            // checking, worst case unused balance is lost.
            // TODO: Allow negative and do "is resolved checking"?
            freeBalance[asset].dec(amount);
            return;
        }
        if (atype == AssetForm.UniV4Claim) {
            _accountChange(netV4Claims[to][asset], amount.pos());
            return;
        }
        if (atype == AssetForm.Liquid) {
            _accountChange(netLiquid[to][asset], amount.pos());
            return;
        }
        // Should be unreachable (<3 to Solidity for not having match statements).
        assert(false);
    }

    struct Donate {
        AssetIndex asset0Index;
        AssetIndex asset1Index;
        int24 startTick;
        uint256 totalTicks;
        uint128 startLiquidity;
        uint256[] amounts0;
    }

    function _donate(Globals memory g, Donate memory donate) internal {
        address asset0 = g.get(donate.asset0Index);
        address asset1 = g.get(donate.asset1Index);
        PoolId poolid = ConversionLib.toPoolKey(address(this), asset0, asset1).toId();
        int24 currentTick = UNI_V4.getSlot0(poolid).tick();

        uint256 totalDonated;
        uint256 cumulativeFeeGrowth;
        uint256 liquidity = donate.startLiquidity;

        int24 tick = donate.startTick;

        if (tick <= currentTick) {
            bool initialized = false;
            uint256 index = 0;
            while (true) {
                if (initialized && index < donate.amounts0.length) {
                    uint256 amount = donate.amounts0[index++];
                    totalDonated += amount;
                    cumulativeFeeGrowth += amount.rayDiv(liquidity);
                }

                {
                    int24 compressed = BitmapLib.compress(tick - 1, ConversionLib.TICK_SPACING);
                    (int16 wordPos, uint8 bitPos) = BitmapLib.position(compressed);
                    (initialized, bitPos) = UNI_V4.getPoolBitmapInfo(poolid, wordPos).nextBitPosLte(bitPos);
                    tick = ConversionLib.toTick(wordPos, bitPos);
                }

                if (tick > currentTick) break;

                if (initialized) {
                    feeGrowthOutside[poolid][tick] += cumulativeFeeGrowth;
                    (, int256 netLiquidity) = UNI_V4.getTickLiquidity(poolid, tick);
                    liquidity = liquidity.sub(netLiquidity);
                }
            }
        } else {
            bool initialized = false;
            uint256 index = 0;
            while (true) {
                {
                    int24 compressed = BitmapLib.compress(tick + 1, ConversionLib.TICK_SPACING);
                    (int16 wordPos, uint8 bitPos) = BitmapLib.position(compressed);
                    (initialized, bitPos) = UNI_V4.getPoolBitmapInfo(poolid, wordPos).nextBitPosLte(bitPos);
                    tick = ConversionLib.toTick(wordPos, bitPos);
                }

                if (tick <= currentTick) break;

                if (initialized && index < donate.amounts0.length) {
                    uint256 amount = donate.amounts0[index++];
                    totalDonated += amount;
                    cumulativeFeeGrowth += amount.rayDiv(liquidity);
                }

                if (initialized) {
                    feeGrowthOutside[poolid][tick] += cumulativeFeeGrowth;
                    (, int256 netLiquidity) = UNI_V4.getTickLiquidity(poolid, tick);
                    liquidity = liquidity.add(netLiquidity);
                }
            }
        }

        require(liquidity == UNI_V4.getPoolLiquidity(poolid), "WRONG_LIQUIDITY");

        feeGrowthGlobal[poolid] += cumulativeFeeGrowth;
        freeBalance[asset0].dec(totalDonated);
    }

    ////////////////////////////////////////////////////////////////
    //                        USER-TO-USER                        //
    ////////////////////////////////////////////////////////////////

    function userToUserLiquidTransfer(address from, address to, address asset, uint256 amount) public dispatched {
        asset.safeTransferFrom(from, to, amount);
        _accountChange(netLiquid[from][asset], amount.pos());
        _accountChange(netLiquid[to][asset], amount.neg());
    }

    function userToUserV4ClaimTransfer(address from, address to, address asset, uint256 amount) public dispatched {
        UNI_V4.transferFrom(from, to, asset.into(), amount);
        _accountChange(netV4Claims[from][asset], amount.pos());
        _accountChange(netV4Claims[to][asset], amount.neg());
    }

    ////////////////////////////////////////////////////////////////
    //                       USER-TO-SYSTEM                       //
    ////////////////////////////////////////////////////////////////

    function pullLiquid(address from, address asset, uint256 amount) public dispatched {
        asset.safeTransferFrom(from, address(this), amount);
        _accountChange(netLiquid[from][asset], amount.pos());
        freeBalance[asset].inc(amount);
    }

    // 3000.0 USDC (6) / WETH (18)
    // 3000 * 10**6 / 10**18 * 10**27

    function pushLiquid(address to, address asset, uint256 amount) public dispatched {
        asset.safeTransfer(to, amount);
        _accountChange(netLiquid[to][asset], amount.neg());
        freeBalance[asset].dec(amount);
    }

    function pullToV4(address from, address asset, uint256 amount) public dispatched {
        asset.safeTransferFrom(from, address(UNI_V4), amount);
        _accountChange(netLiquid[from][asset], amount.pos());
    }

    function pushFromV4(address to, address asset, uint256 amount) public dispatched {
        UNI_V4.take(Currency.wrap(asset), to, amount);
        _accountChange(netLiquid[to][asset], amount.neg());
    }

    function burnV4(address from, address asset, uint256 amount) public dispatched {
        UNI_V4.burn(from, asset.into(), amount);
        _accountChange(netV4Claims[from][asset], amount.pos());
    }

    function mintV4(address to, address asset, uint256 amount) public dispatched {
        UNI_V4.mint(to, asset.into(), amount);
        _accountChange(netV4Claims[to][asset], amount.neg());
    }

    ////////////////////////////////////////////////////////////////
    //                   SYSTEM TRANSFORMATIONS                   //
    ////////////////////////////////////////////////////////////////

    function saveNodeFee(address asset, uint256 amount) public dispatched {
        freeBalance[asset].dec(amount);
        _savedFees[asset] += amount;
    }

    function uniV4DeltaToFree(address asset, uint256 amount) public dispatched {
        UNI_V4.take(Currency.wrap(asset), address(this), amount);
        freeBalance[asset].inc(amount);
    }

    function freeToUniV4Free(address asset, uint256 amount) public dispatched {
        asset.safeTransfer(address(UNI_V4), amount);
        freeBalance[asset].dec(amount);
    }

    function allUniV4FreeToUniV4Delta(address asset) public dispatched {
        UNI_V4.settle(Currency.wrap(asset));
    }

    /**
     * @dev Positive change indicates assets the end-user is owed vs. negative change indicates
     * assets the users owes to the protocol.
     */
    function _accountChange(tint256 storage balance, int256 change) internal returns (uint256 countChange) {
        int256 preBal = balance.get();
        int256 newBal = preBal + change;
        balance.set(newBal);
        unchecked {
            countChange = (newBal != 0).into() - (preBal != 0).into();
            if (countChange != 0) {
                uint256 prevCount = unresolvedChanges.get();
                unresolvedChanges.set(prevCount + countChange);
            }
        }
    }
}
