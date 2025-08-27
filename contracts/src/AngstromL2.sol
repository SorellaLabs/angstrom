// SPDX-License-Identifier: BUSL-1.1
pragma solidity =0.8.29;

import {UniConsumer} from "./modules/UniConsumer.sol";
import {IUniV4, IPoolManager, PoolId} from "./interfaces/IUniV4.sol";
import {PoolKey, BalanceDelta, IBeforeSwapHook, IAfterSwapHook} from "./interfaces/IHooks.sol";
import {BeforeSwapDelta, toBeforeSwapDelta} from "v4-core/src/types/BeforeSwapDelta.sol";
import {Hooks, IHooks} from "v4-core/src/libraries/Hooks.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";

import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

/// @author philogy <https://github.com/philogy>
contract AngstromL2 is UniConsumer, IBeforeSwapHook, IAfterSwapHook {
    using IUniV4 for IPoolManager;
    using Hooks for IHooks;

    error NegationOverflow();

    /// @dev The `SWAP_TAXED_GAS` is the abstract estimated gas cost for a swap. We want it to be a constant so that competing searchers have a bid cost independent of how much gas their swap actually uses, the overall tax just needs to scale proportional to `priority_fee * swap_fixed_cost`.
    uint256 internal constant SWAP_TAXED_GAS = 100_000;
    /// @dev MEV tax charged is `priority_fee * SWAP_MEV_TAX_FACTOR` meaning the tax rate is `SWAP_MEV_TAX_FACTOR / (SWAP_MEV_TAX_FACTOR + 1)`
    uint256 internal constant SWAP_MEV_TAX_FACTOR = 49;

    uint64 internal blockOfLastTopOfBlock;

    uint128 internal transient liquidityBeforeSwap;
    Slot0 internal transient slot0BeforeSwap;

    constructor(IPoolManager uniV4) UniConsumer(uniV4) {
        Hooks.validateHookPermissions(
            IHooks(address(this)),
            Hooks.Permissions({
                beforeInitialize: true, // To constrain that this is an ETH pool
                afterInitialize: false,
                beforeAddLiquidity: false,
                afterAddLiquidity: true, // To tax liquidity additions that may be JIT
                beforeRemoveLiquidity: false,
                afterRemoveLiquidity: true, // To tax liquidity removals that may be JIT
                beforeSwap: true, // To tax ToB
                afterSwap: true, // Also to tax with ToB (after swap contains reward dist. calculations)
                beforeDonate: false,
                afterDonate: false,
                beforeSwapReturnDelta: true, // To charge the ToB MEV tax.
                afterSwapReturnDelta: false,
                afterAddLiquidityReturnDelta: true, // To charge the JIT liquidity MEV tax.
                afterRemoveLiquidityReturnDelta: true // To charge the JIT liquidity MEV tax.
            })
        );
    }

    // TODO: Dynamic LP fee
    function beforeSwap(
        address,
        PoolKey calldata key,
        IPoolManager.SwapParams calldata params,
        bytes calldata
    ) external override returns (bytes4, BeforeSwapDelta, uint24) {
        _onlyUniV4();

        if (_getBlock() == blockOfLastTopOfBlock) {
            return (this.beforeSwap.selector, BeforeSwapDelta.wrap(0), 0);
        }

        int128 etherDelta = _negate(_getSwapTaxAmount());
        bool ethWasSpecified = params.zeroForOne == params.amountSpecified < 0; // ETH aka asset 0 was specified.

        PoolId id = _toId(key);
        liquidityBeforeSwap = UNI_V4.getPoolLiquidity(id);
        slot0BeforeSwap = UNI_V4.getSlot0(id);

        return (
            this.beforeSwap.selector,
            ethWasSpecified ? toBeforeSwapDelta(etherDelta, 0) : toBeforeSwapDelta(0, etherDelta),
            0
        );
    }

    function afterSwap(
        address,
        PoolKey calldata key,
        IPoolManager.SwapParams calldata params,
        BalanceDelta,
        bytes calldata
    ) external override returns (bytes4, int128) {
        uint64 blockNumber = _getBlock();
        if (blockNumber == blockOfLastTopOfBlock) {
            return (this.afterSwap.selector, 0);
        }
        blockOfLastTopOfBlock = blockNumber;

        params.zeroForOne ? _zeroForOneDistributeTax() : _oneForZeroDistributeTax();

        return (this.afterSwap.selector, 0);
    }

    function _zeroForOneDistributeTax() internal view {}

    function _oneForZeroDistributeTax() internal view {}

    function _getBlock() internal pure returns (uint64) {
        // TODO
        return 0;
    }

    function _getSwapTaxAmount() internal view returns (uint256) {
        uint256 priorityFee = tx.gasprice - block.basefee;
        return SWAP_MEV_TAX_FACTOR * SWAP_TAXED_GAS * priorityFee;
    }

    function _negate(uint256 x) internal pure returns (int128 y) {
        require(x <= 1 << 128, NegationOverflow());
        unchecked {
            return -int128(int256(x));
        }
    }
}
