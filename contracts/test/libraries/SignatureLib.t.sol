// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "../_helpers/BaseTest.sol";
import {SignatureLib} from "../../src/libraries/SignatureLib.sol";
import {CalldataReaderLib, CalldataReader} from "../../src/types/CalldataReader.sol";
import {IERC1271} from "../_mocks/erc1271/IERC1271.sol";
import {TwoSigERC1271} from "../_mocks/erc1271/TwoSigERC1271.sol";
import {ReturnerERC1271} from "../_mocks/erc1271/ReturnerERC1271.sol";
import {RevertingERC1271} from "../_mocks/erc1271/RevertingERC1271.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract SignatureLibTest is BaseTest {
    Account a1;
    Account a2;

    TwoSigERC1271 twoSig;
    ReturnerERC1271 returner;
    RevertingERC1271 reverting;

    function setUp() public {
        a1 = makeAccount("dude_1");
        a2 = makeAccount("dude_2");
        twoSig = new TwoSigERC1271(a1.addr, a2.addr);
        returner = new ReturnerERC1271();
        reverting = new RevertingERC1271();
    }

    function test_fuzzing_readAndCheckEcdsa_ecrecoverEquivalence(bytes32 hash, uint8 v, bytes32 r, bytes32 s) public {
        address recovered = ecrecover(hash, v, r, s);

        bytes memory data = abi.encodePacked(v, r, s);

        if (recovered == address(0)) {
            vm.expectRevert(SignatureLib.InvalidSignature.selector);
            this._readAndCheckEcdsa(data, hash);
        } else {
            (CalldataReader startReader, CalldataReader endReader, address libRecovered) =
                this._readAndCheckEcdsa(data, hash);
            assertEq(libRecovered, recovered);
            assertEq(startReader.offset() + 65, endReader.offset());
        }
    }

    function test_fuzzing_readAndCheckEcdsa_recoversSigner(bytes32 hash, uint256 privateKey) public view {
        privateKey = boundPrivateKey(privateKey);
        address signer = vm.addr(privateKey);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(privateKey, hash);
        (CalldataReader startReader, CalldataReader endReader, address recovered) =
            this._readAndCheckEcdsa(abi.encodePacked(v, r, s), hash);
        assertEq(signer, recovered);
        assertEq(startReader.offset() + 65, endReader.offset());
    }

    function test_fuzzing_readAndCheckERC1271_revertWithExpectedIsInvalid(bytes32 hash, bytes calldata sig) public {
        vm.assume(sig.length <= type(uint24).max);
        reverting.setRevertData(abi.encode(IERC1271.isValidSignature.selector));
        vm.expectRevert(SignatureLib.InvalidSignature.selector);
        this._readAndCheckERC1271(abi.encodePacked(address(reverting), uint24(sig.length), sig), hash);
    }

    function test_fuzzing_readAndCheckERC1271_revertAlwaysInvalid(
        bytes32 hash,
        bytes calldata sig,
        bytes calldata revertData
    ) public {
        vm.assume(sig.length <= type(uint24).max);
        reverting.setRevertData(revertData);
        vm.expectRevert(SignatureLib.InvalidSignature.selector);
        this._readAndCheckERC1271(abi.encodePacked(address(reverting), uint24(sig.length), sig), hash);
    }

    function test_fuzzing_readAndCheckERC1271_revertsInvalidReturn(bytes32 hash, bytes calldata sig, bytes4 returnValue)
        public
    {
        vm.assume(sig.length <= type(uint24).max);
        vm.assume(returnValue != IERC1271.isValidSignature.selector);
        returner.setReturnValue(returnValue);
        vm.expectRevert(SignatureLib.InvalidSignature.selector);
        this._readAndCheckERC1271(abi.encodePacked(address(returner), uint24(sig.length), sig), hash);
    }

    function test_fuzzing_readAndCheckERC1271_acceptsValidReturn(bytes32 hash, bytes calldata sig) public {
        vm.assume(sig.length <= type(uint24).max);
        returner.setReturnValue(IERC1271.isValidSignature.selector);
        (CalldataReader startReader, CalldataReader endReader, address recovered) =
            this._readAndCheckERC1271(abi.encodePacked(address(returner), uint24(sig.length), sig), hash);
        assertEq(recovered, address(returner));
        assertEq(startReader.offset() + 20 + 3 + sig.length, endReader.offset());
    }

    function test_fuzzing_readAndCheckERC1271_acceptsValidSignature(bytes32 hash) public view {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(a1.key, hash);
        bytes memory sig1 = abi.encodePacked(r, s, v);
        (v, r, s) = vm.sign(a2.key, hash);
        bytes memory sig2 = abi.encodePacked(r, s, v);

        bytes memory totalSig = bytes.concat(sig1, sig2);

        (CalldataReader startReader, CalldataReader endReader, address recovered) =
            this._readAndCheckERC1271(abi.encodePacked(address(twoSig), uint24(totalSig.length), totalSig), hash);
        assertEq(recovered, address(twoSig));
        assertEq(startReader.offset() + 20 + 3 + totalSig.length, endReader.offset());
    }

    function _readAndCheckEcdsa(bytes calldata data, bytes32 hash)
        external
        view
        returns (CalldataReader, CalldataReader newReader, address from)
    {
        CalldataReader reader = CalldataReaderLib.from(data);
        (newReader, from) = SignatureLib.readAndCheckEcdsa(reader, hash);
        return (reader, newReader, from);
    }

    function _readAndCheckERC1271(bytes calldata data, bytes32 hash)
        external
        view
        returns (CalldataReader, CalldataReader newReader, address from)
    {
        CalldataReader reader = CalldataReaderLib.from(data);
        (newReader, from) = SignatureLib.readAndCheckERC1271(reader, hash);
        return (reader, newReader, from);
    }
}
