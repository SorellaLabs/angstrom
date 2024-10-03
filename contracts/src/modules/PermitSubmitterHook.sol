// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {
    IAngstromComposable, EXPECTED_HOOK_RETURN_MAGIC
} from "../interfaces/IAngstromComposable.sol";
import {IERC2612} from "../interfaces/IERC2612.sol";
import {IDaiPermit} from "../interfaces/IDaiPermit.sol";
import {CalldataReader, CalldataReaderLib} from "../types/CalldataReader.sol";

/// @author philogy <https://github.com/philogy>
abstract contract PermitSubmitterHook is IAngstromComposable {
    enum PermitPayloadType {
        ERC2612_INFINITE,
        ERC2612_SPECIFIC,
        DAI_INFINITE
    }

    function compose(address from, bytes calldata payload) external override returns (uint32) {
        CalldataReader reader = CalldataReaderLib.from(payload);
        PermitPayloadType payloadType;
        {
            uint8 typeByte;
            (reader, typeByte) = reader.readU8();
            payloadType = PermitPayloadType(typeByte);
        }

        if (payloadType == PermitPayloadType.ERC2612_INFINITE) {
            address token;
            (reader, token) = reader.readAddr();
            uint40 deadline;
            (reader, deadline) = reader.readU40();
            uint8 v;
            (reader, v) = reader.readU8();
            uint256 r;
            (reader, r) = reader.readU256();
            uint256 s;
            (reader, s) = reader.readU256();
            IERC2612(token).permit(
                from, msg.sender, type(uint256).max, deadline, v, bytes32(r), bytes32(s)
            );
        } else if (payloadType == PermitPayloadType.ERC2612_SPECIFIC) {
            address token;
            (reader, token) = reader.readAddr();
            uint128 value;
            (reader, value) = reader.readU128();
            uint40 deadline;
            (reader, deadline) = reader.readU40();
            uint8 v;
            (reader, v) = reader.readU8();
            uint256 r;
            (reader, r) = reader.readU256();
            uint256 s;
            (reader, s) = reader.readU256();
            IERC2612(token).permit(from, msg.sender, value, deadline, v, bytes32(r), bytes32(s));
        } else if (payloadType == PermitPayloadType.DAI_INFINITE) {
            address token;
            (reader, token) = reader.readAddr();
            uint32 nonce;
            (reader, nonce) = reader.readU32();
            uint40 deadline;
            (reader, deadline) = reader.readU40();
            uint8 v;
            (reader, v) = reader.readU8();
            uint256 r;
            (reader, r) = reader.readU256();
            uint256 s;
            (reader, s) = reader.readU256();
            IDaiPermit(token).permit(
                from, msg.sender, nonce, deadline, true, v, bytes32(r), bytes32(s)
            );
        }

        return EXPECTED_HOOK_RETURN_MAGIC;
    }
}
