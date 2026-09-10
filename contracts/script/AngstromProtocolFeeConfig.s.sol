// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseScript} from "./BaseScript.sol";
import {IAngstromAuth} from "src/interfaces/IAngstromAuth.sol";
import {AngstromView} from "src/periphery/AngstromView.sol";
import {ControllerV1} from "src/periphery/ControllerV1.sol";
import {AngstromProtocolFeeConfig} from "src/periphery/AngstromProtocolFeeConfig.sol";
import {console} from "forge-std/console.sol";

/// @notice Deploys `AngstromProtocolFeeConfig` standalone against an already-deployed Angstrom.
/// @dev Nothing else is deployed or modified: Angstrom, `ControllerV1` and the governance
/// contracts are untouched, and Angstrom never learns this address. The initial values reproduce
/// today's economics exactly, so activation changes where the rate comes from, not what it is.
/// Set `ANGSTROM_ADDRESS` to override the per-chain default.
///
/// `verify` is a public entry point of its own, so an already-deployed config can be re-checked
/// against live state without deploying anything:
/// `forge script AngstromProtocolFeeConfigScript --sig "verify(address,address)" <config> <angstrom>`
/// @author jnoorchashm37 <https://github.com/jnoorchashm37>
contract AngstromProtocolFeeConfigScript is BaseScript {
    using AngstromView for IAngstromAuth;

    uint32 internal constant MAX_SHARE_E6 = 1_000_000;

    /// @dev 75% to LPs, matching the `LP_DONATION_SPLIT` this replaces.
    uint32 internal constant INITIAL_USER_LP_SHARE_E6 = 750_000;

    /// @dev 100% to LPs, so the protocol's ToB share starts at zero. Raising it is a separate,
    /// later governance call, gated on fee accounting being in place.
    uint32 internal constant INITIAL_TOB_LP_SHARE_E6 = 1_000_000;

    bytes32 internal constant SPLITS_SLOT = bytes32(uint256(0));

    uint256 internal constant MAINNET_CHAIN_ID = 1;
    uint256 internal constant SEPOLIA_CHAIN_ID = 11155111;

    function run() public {
        address angstromAddress = angstromOnCurrentChain();
        require(angstromAddress.code.length != 0, "No code at the Angstrom address");

        vm.startBroadcast();
        AngstromProtocolFeeConfig config = new AngstromProtocolFeeConfig(
            IAngstromAuth(angstromAddress), INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6
        );
        vm.stopBroadcast();

        verify(config, angstromAddress);
    }

    /// @notice Re-checks everything the deployment claims, straight from chain state.
    /// @dev Reverts on the first disagreement, so a broken deployment cannot pass quietly. Public
    /// so it can be run on its own against an existing deployment, not only right after one.
    function verify(AngstromProtocolFeeConfig config, address angstromAddress) public view {
        console.log("AngstromProtocolFeeConfig: %s", address(config));

        uint256 codeLength = address(config).code.length;
        require(codeLength != 0, "No runtime code at the deployed config");
        console.log("  runtime code: %s bytes", codeLength);
        console.log("  code hash: %s", vm.toString(address(config).codehash));

        require(config.angstrom() == angstromAddress, "angstrom() is not the intended deployment");
        console.log("  angstrom(): %s", config.angstrom());

        // `controller()` resolves from Angstrom's live state on every call, so this is what the
        // setter itself will see -- not a copy taken at construction.
        address resolvedController = config.controller();
        require(
            resolvedController == IAngstromAuth(angstromAddress).controller(),
            "controller() disagrees with Angstrom's controller slot"
        );
        require(resolvedController.code.length != 0, "Resolved controller has no code");
        require(
            address(ControllerV1(resolvedController).ANGSTROM()) == angstromAddress,
            "Resolved controller is bound to a different Angstrom"
        );
        console.log("  controller(): %s", resolvedController);

        // The two addresses that may call `setLpDonationSplits`. On mainnet these are the timelock
        // and the multisig respectively; neither gains withdrawal authority from this deployment.
        address owner = ControllerV1(resolvedController).owner();
        address fastOwner = ControllerV1(resolvedController).fastOwner();
        require(owner != address(0), "Controller owner is the zero address");
        require(fastOwner != address(0), "Controller fast owner is the zero address");
        console.log("    owner: %s", owner);
        console.log("    fastOwner: %s", fastOwner);

        (uint32 userLpShareE6, uint32 tobLpShareE6) = config.getLpDonationSplits();
        require(userLpShareE6 == INITIAL_USER_LP_SHARE_E6, "userLpShareE6 is not the initial value");
        require(tobLpShareE6 == INITIAL_TOB_LP_SHARE_E6, "tobLpShareE6 is not the initial value");
        console.log(
            "  userLpShareE6: %s -> protocol %s", userLpShareE6, MAX_SHARE_E6 - userLpShareE6
        );
        console.log("  tobLpShareE6: %s -> protocol %s", tobLpShareE6, MAX_SHARE_E6 - tobLpShareE6);

        // Nodes decode slot 0 directly, so prove it agrees with the getter at this one state.
        uint256 word = uint256(vm.load(address(config), SPLITS_SLOT));
        (uint32 decodedUserLpShareE6, uint32 decodedTobLpShareE6) = decodeSlot0(word);
        require(decodedUserLpShareE6 == userLpShareE6, "slot 0 user share disagrees with getter");
        require(decodedTobLpShareE6 == tobLpShareE6, "slot 0 tob share disagrees with getter");
        require(word >> 64 == 0, "slot 0 has nonzero padding above bit 63");
        console.log("  slot 0: %s", vm.toString(bytes32(word)));
        console.log("  slot 0 agrees with getLpDonationSplits()");
    }

    /// @dev The decode nodes use off chain: user share in bytes 0..4, tob share in bytes 4..8.
    function decodeSlot0(uint256 word)
        internal
        pure
        returns (uint32 userLpShareE6, uint32 tobLpShareE6)
    {
        // casting to 'uint32' is safe because each field is masked to 32 bits first
        // forge-lint: disable-next-line(unsafe-typecast)
        userLpShareE6 = uint32(word & 0xffffffff);
        // forge-lint: disable-next-line(unsafe-typecast)
        tobLpShareE6 = uint32((word >> 32) & 0xffffffff);
    }

    /// @dev Matched on chain id rather than through `getChain`, which the sibling scripts use:
    /// that needs an RPC endpoint configured for every chain it names, so on an unsupported chain
    /// it fails with a missing-RPC error instead of saying the chain is unsupported.
    function angstromOnCurrentChain() internal view returns (address) {
        address fromEnv = vm.envOr("ANGSTROM_ADDRESS", address(0));
        if (fromEnv != address(0)) return fromEnv;
        if (block.chainid == MAINNET_CHAIN_ID) {
            return 0x0000000aa232009084Bd71A5797d089AA4Edfad4;
        }
        if (block.chainid == SEPOLIA_CHAIN_ID) {
            return 0x9051085355BA7e36177e0a1c4082cb88C270ba90;
        }
        revert(
            string.concat(
                "No Angstrom address known for chain ",
                vm.toString(block.chainid),
                "; set ANGSTROM_ADDRESS"
            )
        );
    }
}
