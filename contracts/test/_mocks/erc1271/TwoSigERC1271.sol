// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IERC1271} from "./IERC1271.sol";
import {ECDSA} from "solady/src/utils/ECDSA.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract TwoSigERC1271 is IERC1271 {
    address[2] signers;

    constructor(address signer1, address signer2) {
        signers[0] = signer1;
        signers[1] = signer2;
    }

    function isValidSignature(bytes32 hash, bytes calldata sigs) external view returns (bytes4) {
        uint256 validated = 0;
        for (uint256 i = 0; i < sigs.length; i += 65) {
            address signer = signers[validated];
            address recovered = ECDSA.recoverCalldata(hash, sigs[i:i + 65]);
            console.log("recovered: %s", recovered);
            console.log("signer: %s", signer);
            require(recovered == signer, "Wrong Signer Recovered");
            validated += 1;
        }

        require(validated == signers.length, "Not All Validated");

        return IERC1271.isValidSignature.selector;
    }
}
