// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {CalldataReader, CalldataReaderLib} from "src/types/CalldataReader.sol";
import {HookBuffer, HookBufferLib} from "src/types/HookBuffer.sol";
import {IAngstromComposable, EXPECTED_HOOK_RETURN_MAGIC} from "../../src/interfaces/IAngstromComposable.sol";
import {Recorder} from "../_mocks/composable/Recorder.sol";
import {SmolReturn} from "../_mocks/composable/SmolReturn.sol";
import {PRNG} from "super-sol/collections/PRNG.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract HookBufferTest is BaseTest {
    Recorder recorder;
    SmolReturn smol;

    function setUp() public {
        recorder = new Recorder();
        smol = new SmolReturn();
    }

    function test_emptyBytesHash() public pure {
        assertEq(bytes32(HookBufferLib.EMPTY_BYTES_HASH), keccak256(""));
    }

    function test_fuzzing_readFrom_noHookIsEmpty(bytes calldata data, address from) public {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader outReader, HookBuffer hook, bytes32 hash) = HookBufferLib.readFrom(reader, true);
        assertEq(hash, keccak256(""));
        assertEq(HookBuffer.unwrap(hook), 0);
        assertEq(reader.offset(), outReader.offset());
        assertTrue(reader == outReader);
        // Should do nothing.
        hook.tryTrigger(from);
    }

    function test_fuzzing_readFrom_executeNormalCall(
        address from,
        bytes calldata hookPayload,
        bytes calldata garbage,
        uint256 brutalizeSeed
    ) public {
        vm.assume(hookPayload.length <= type(uint24).max - 20);
        this._test_fuzzing_readFrom_executeNormalCall(
            abi.encodePacked(uint24(hookPayload.length + 20), address(recorder), hookPayload, garbage),
            from,
            hookPayload,
            brutalizeSeed
        );
    }

    function _test_fuzzing_readFrom_executeNormalCall(
        bytes calldata data,
        address from,
        bytes calldata hookPayload,
        uint256 brutalizeSeed
    ) external {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader outReader, HookBuffer hookBuffer, bytes32 hash) = HookBufferLib.readFrom(reader, false);
        _brutalize(brutalizeSeed, 20);
        assertEq(hash, keccak256(abi.encodePacked(address(recorder), hookPayload)), "wrong hash");
        assertEq(reader.offset() + 23 + hookPayload.length, outReader.offset());

        uint256 callCount = recorder.callCount();

        vm.expectCall(
            address(recorder),
            abi.encodePacked(
                IAngstromComposable.compose.selector, abi.encode(from), uint256(0x40), hookPayload.length, hookPayload
            )
        );
        hookBuffer.tryTrigger(from);

        assertEq(recorder.callCount(), callCount + 1);
        assertEq(recorder.lastFrom(), from);
        assertEq(recorder.lastPayload(), hookPayload);
    }

    function test_fuzzing_readFrom_revertsWhenInsufficientReturnData(
        address from,
        bytes calldata hookPayload,
        bytes calldata garbage,
        uint256 brutalizeSeed,
        bytes32 retVal,
        uint256 retLength
    ) public {
        retLength = bound(retLength, 0, 31);
        vm.assume(hookPayload.length <= type(uint24).max - 20);

        smol.setReturn(retVal, uint8(retLength));

        vm.expectRevert(HookBufferLib.InvalidHookReturn.selector);
        this._test_fuzzing_readFrom_smolReturn(
            abi.encodePacked(uint24(hookPayload.length + 20), address(smol), hookPayload, garbage),
            from,
            hookPayload,
            brutalizeSeed
        );
    }

    function test_fuzzing_readFrom_revertsWhenMagicNotReturned(
        address from,
        bytes calldata hookPayload,
        bytes calldata garbage,
        uint256 brutalizeSeed,
        bytes32 retVal
    ) public {
        vm.assume(hookPayload.length <= type(uint24).max - 20);
        vm.assume(retVal != bytes32(uint256(EXPECTED_HOOK_RETURN_MAGIC)));

        smol.setReturn(retVal, 32);

        vm.expectRevert(HookBufferLib.InvalidHookReturn.selector);
        this._test_fuzzing_readFrom_smolReturn(
            abi.encodePacked(uint24(hookPayload.length + 20), address(smol), hookPayload, garbage),
            from,
            hookPayload,
            brutalizeSeed
        );
    }

    function _test_fuzzing_readFrom_smolReturn(
        bytes calldata data,
        address from,
        bytes calldata hookPayload,
        uint256 brutalizeSeed
    ) external {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader outReader, HookBuffer hookBuffer, bytes32 hash) = HookBufferLib.readFrom(reader, false);
        assertEq(hash, keccak256(abi.encodePacked(address(smol), hookPayload)), "wrong hash");
        assertEq(reader.offset() + 23 + hookPayload.length, outReader.offset());

        _brutalize(brutalizeSeed, 20);
        assembly ("memory-safe") {
            mstore(0x00, EXPECTED_HOOK_RETURN_MAGIC)
        }

        vm.expectCall(
            address(smol),
            abi.encodePacked(
                IAngstromComposable.compose.selector, abi.encode(from), uint256(0x40), hookPayload.length, hookPayload
            )
        );
        hookBuffer.tryTrigger(from);
    }
}
