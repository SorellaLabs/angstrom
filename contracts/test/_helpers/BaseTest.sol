// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Angstrom} from "src/Angstrom.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {Test} from "forge-std/Test.sol";
import {Trader} from "./types/Trader.sol";
import {console2 as console} from "forge-std/console2.sol";
import {HookDeployer} from "./HookDeployer.sol";
import {stdError} from "forge-std/StdError.sol";
import {OrderMeta, TopOfBlockOrder} from "test/_reference/OrderTypes.sol";
import {TickLib} from "src/libraries/TickLib.sol";
import {HookDeployer} from "./HookDeployer.sol";
import {ANGSTROM_HOOK_FLAGS} from "src/Constants.sol";
import {TypedDataHasherLib} from "src/types/TypedDataHasher.sol";
import {PoolConfigStore, PoolConfigStoreLib} from "src/libraries/PoolConfigStore.sol";
import {PairLib} from "test/_reference/Pair.sol";

import {MockERC20} from "super-sol/mocks/MockERC20.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
contract BaseTest is Test, HookDeployer {
    using FormatLib for *;

    uint256 internal constant REAL_TIMESTAMP = 1721652639;

    bytes32 internal constant ANG_BALANCES_SLOT = bytes32(uint256(0x2));
    bytes32 internal constant ANG_CONFIG_STORE_SLOT = bytes32(uint256(0x4));

    function pm(address addr) internal pure returns (IPoolManager) {
        return IPoolManager(addr);
    }

    function deployAngstrom(bytes memory initcode, IPoolManager uni, address controller)
        internal
        returns (address)
    {
        return deployAngstrom(initcode, uni, controller, address(0));
    }

    function deployAngstrom(
        bytes memory initcode,
        IPoolManager uni,
        address controller,
        address feeMaster
    ) internal returns (address addr) {
        bool success;
        (success, addr,) = deployHook(
            bytes.concat(initcode, abi.encode(uni, controller, feeMaster)),
            ANGSTROM_HOOK_FLAGS,
            CREATE2_FACTORY
        );
        assertTrue(success);
    }

    function rawGetConfigStore(address angstrom) internal view returns (address) {
        return address(bytes20(vm.load(angstrom, ANG_CONFIG_STORE_SLOT) << 32));
    }

    function rawGetBalance(address angstrom, address asset, address owner)
        internal
        view
        returns (uint256)
    {
        return uint256(
            vm.load(
                angstrom,
                keccak256(abi.encode(owner, keccak256(abi.encode(asset, ANG_BALANCES_SLOT))))
            )
        );
    }

    function poolKey(Angstrom angstrom, address asset0, address asset1, int24 tickSpacing)
        internal
        pure
        returns (PoolKey memory pk)
    {
        pk.hooks = IHooks(address(angstrom));
        pk.currency0 = Currency.wrap(asset0);
        pk.currency1 = Currency.wrap(asset1);
        pk.tickSpacing = tickSpacing;
    }

    function poolKey(address asset0, address asset1, int24 tickSpacing)
        internal
        pure
        returns (PoolKey memory pk)
    {
        pk.currency0 = Currency.wrap(asset0);
        pk.currency1 = Currency.wrap(asset1);
        pk.tickSpacing = tickSpacing;
    }

    function computeDomainSeparator(address angstrom) internal view returns (bytes32) {
        return keccak256(
            abi.encode(
                keccak256(
                    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
                ),
                keccak256("Angstrom"),
                keccak256("v1"),
                block.chainid,
                address(angstrom)
            )
        );
    }

    function pythonRunCmd() internal pure returns (string[] memory args) {
        args = new string[](1);
        args[0] = ".venv/bin/python3.12";
    }

    function ffiPython(string[] memory args) internal returns (bytes memory) {
        string[] memory runArgs = pythonRunCmd();
        string[] memory all = new string[](runArgs.length + args.length);
        for (uint256 i = 0; i < runArgs.length; i++) {
            all[i] = runArgs[i];
        }

        for (uint256 i = 0; i < args.length; i++) {
            all[runArgs.length + i] = args[i];
        }

        return vm.ffi(all);
    }

    function i24(uint256 x) internal pure returns (int24 y) {
        assertLe(x, uint24(type(int24).max), "Unsafe cast to int24");
        y = int24(int256(x));
    }

    function u128(uint256 x) internal pure returns (uint128 y) {
        assertLe(x, type(uint128).max, "Unsafe cast to uint128");
        y = uint128(x);
    }

    function u16(uint256 x) internal pure returns (uint16 y) {
        assertLe(x, type(uint16).max, "Unsafe cast to uint16");
        y = uint16(x);
    }

    function u64(uint256 x) internal pure returns (uint64 y) {
        assertLe(x, type(uint64).max, "Unsafe cast to uint64");
        y = uint64(x);
    }

    function u40(uint256 x) internal pure returns (uint40 y) {
        assertLe(x, type(uint40).max, "Unsafe cast to uint40");
        y = uint40(x);
    }

    function makeTrader(string memory name) internal returns (Trader memory trader) {
        (trader.addr, trader.key) = makeAddrAndKey(name);
    }

    function makeTraders(uint256 n) internal returns (Trader[] memory traders) {
        traders = new Trader[](n);
        for (uint256 i = 0; i < n; i++) {
            traders[i] = makeTrader(string.concat("trader_", (i + 1).toStr()));
        }
    }

    function tryAdd(uint256 x, uint256 y) internal view returns (bool, bytes memory, uint256) {
        return tryFn(this.__safeAdd, x, y);
    }

    function trySub(uint256 x, uint256 y) internal view returns (bool, bytes memory, uint256) {
        return tryFn(this.__safeSub, x, y);
    }

    function tryMul(uint256 x, uint256 y) internal view returns (bool, bytes memory, uint256) {
        return tryFn(this.__safeMul, x, y);
    }

    function tryDiv(uint256 x, uint256 y) internal view returns (bool, bytes memory, uint256) {
        return tryFn(this.__safeDiv, x, y);
    }

    function tryMod(uint256 x, uint256 y) internal view returns (bool, bytes memory, uint256) {
        return tryFn(this.__safeMod, x, y);
    }

    function tryFn(function(uint, uint) external pure returns (uint) op, uint256 x, uint256 y)
        internal
        pure
        returns (bool hasErr, bytes memory err, uint256 z)
    {
        try op(x, y) returns (uint256 result) {
            hasErr = false;
            z = result;
        } catch (bytes memory errorData) {
            err = errorData;
            assertEq(err, stdError.arithmeticError);
            hasErr = true;
            z = 0;
        }
    }

    function __safeAdd(uint256 x, uint256 y) external pure returns (uint256) {
        return x + y;
    }

    function __safeSub(uint256 x, uint256 y) external pure returns (uint256) {
        return x - y;
    }

    function __safeMul(uint256 x, uint256 y) external pure returns (uint256) {
        return x * y;
    }

    function __safeDiv(uint256 x, uint256 y) external pure returns (uint256) {
        return x / y;
    }

    function __safeMod(uint256 x, uint256 y) external pure returns (uint256) {
        return x / y;
    }

    function freePtr() internal pure returns (uint256 ptr) {
        assembly ("memory-safe") {
            ptr := mload(0x40)
        }
    }

    function _brutalize(uint256 seed, uint256 freeWordsToBrutalize) internal pure {
        assembly ("memory-safe") {
            mstore(0x00, seed)
            let free := mload(0x40)
            for { let i := 0 } lt(i, freeWordsToBrutalize) { i := add(i, 1) } {
                let newGarbage := keccak256(0x00, 0x20)
                mstore(add(free, mul(i, 0x20)), newGarbage)
                mstore(0x01, newGarbage)
            }
            mstore(0x20, keccak256(0x00, 0x20))
            mstore(0x00, keccak256(0x10, 0x20))
        }
    }

    function sign(Account memory account, TopOfBlockOrder memory order, bytes32 domainSeparator)
        internal
        pure
    {
        sign(account, order.meta, erc712Hash(domainSeparator, order.hash()));
    }

    function sign(Account memory account, OrderMeta memory targetMeta, bytes32 hash)
        internal
        pure
    {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(account.key, hash);
        targetMeta.isEcdsa = true;
        targetMeta.from = account.addr;
        targetMeta.signature = abi.encodePacked(v, r, s);
    }

    function sign(Trader memory account, OrderMeta memory targetMeta, bytes32 hash) internal pure {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(account.key, hash);
        targetMeta.isEcdsa = true;
        targetMeta.from = account.addr;
        targetMeta.signature = abi.encodePacked(v, r, s);
    }

    function erc712Hash(bytes32 domainSeparator, bytes32 structHash)
        internal
        pure
        returns (bytes32)
    {
        return TypedDataHasherLib.init(domainSeparator).hashTypedData(structHash);
    }

    function bumpBlock() internal {
        vm.roll(block.number + 1);
    }

    function deployTokensSorted() internal returns (address, address) {
        address asset0 = address(new MockERC20());
        address asset1 = address(new MockERC20());
        return asset0 < asset1 ? (asset0, asset1) : (asset1, asset0);
    }

    function addrs(bytes memory encoded) internal pure returns (address[] memory) {
        return abi.decode(
            bytes.concat(bytes32(uint256(0x20)), bytes32(encoded.length / 0x20), encoded),
            (address[])
        );
    }

    function min(uint256 x, uint256 y) internal pure returns (uint256) {
        return x < y ? x : y;
    }

    function max(uint256 x, uint256 y) internal pure returns (uint256) {
        return x > y ? x : y;
    }

    function poolId(Angstrom angstrom, address asset0, address asset1)
        internal
        view
        returns (PoolId)
    {
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        address store = rawGetConfigStore(address(angstrom));
        uint256 storeIndex = PairLib.getStoreIndex(store, asset0, asset1);
        (int24 tickSpacing,) = PoolConfigStore.wrap(store).get(
            PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1), storeIndex
        );
        return poolKey(angstrom, asset0, asset1, tickSpacing).toId();
    }
}
