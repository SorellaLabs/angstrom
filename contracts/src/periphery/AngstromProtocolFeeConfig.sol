// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {AngstromView} from "./AngstromView.sol";
import {ControllerV1} from "./ControllerV1.sol";

/// @notice Configuration for the LP/protocol split of user fees and of top-of-block auction
/// payments.
/// @dev Angstrom does not read this contract and does not enforce these ratios. Nodes read both
/// rates from canonical state at the parent block of the bundle they are building, and the split
/// lands in that bundle as ordinary donation and `save` amounts. This is a configuration source,
/// not a settlement component: it holds no funds and has no withdrawal path. Withdrawal
/// authority stays with `ControllerV1.distributeFees`, which is owner-only.
/// @author jnoorchashm37 <https://github.com/jnoorchashm37>
contract AngstromProtocolFeeConfig {
    using AngstromView for IAngstromAuth;

    /// @dev 100% in E6, and the denominator both shares are taken over.
    uint32 internal constant MAX_SHARE_E6 = 1_000_000;

    /// @dev Authorization resolves through Angstrom's live controller on every call, so authority
    /// follows a controller replacement without any action here.
    IAngstromAuth private immutable ANGSTROM;

    /// @dev Both shares pack into slot 0, which readers decode from a single storage read:
    /// `userLpShareE6 = word & 0xffffffff`, `tobLpShareE6 = (word >> 32) & 0xffffffff`. Bits 64
    /// and above are unused and always zero. Both fields stay private on purpose: one read must
    /// return a consistent pair, and per-field getters would invite composing a pair from two
    /// reads at different states. This layout is part of the interface -- changing the
    /// declaration order or the widths breaks off-chain decoders.
    uint32 private _userLpShareE6;
    uint32 private _tobLpShareE6;

    error NotAuthorized();
    error InvalidConfig();

    event LpDonationSplitsSet(
        uint32 oldUserLpShareE6,
        uint32 newUserLpShareE6,
        uint32 oldTobLpShareE6,
        uint32 newTobLpShareE6
    );

    constructor(IAngstromAuth angstrom, uint32 initialUserLpShareE6, uint32 initialTobLpShareE6) {
        if (
            address(angstrom) == address(0) || initialUserLpShareE6 > MAX_SHARE_E6
                || initialTobLpShareE6 > MAX_SHARE_E6
        ) {
            revert InvalidConfig();
        }

        ANGSTROM = angstrom;
        _userLpShareE6 = initialUserLpShareE6;
        _tobLpShareE6 = initialTobLpShareE6;

        emit LpDonationSplitsSet(0, initialUserLpShareE6, 0, initialTobLpShareE6);
    }

    /// @notice Sets both LP shares, in E6 out of `1_000_000`. Callable by the controller's owner
    /// or its fast owner.
    /// @dev Writes the full pair, so a call meaning to change one share still overwrites the
    /// other with whatever it was given. A queued timelock call will therefore overwrite an
    /// intervening fast-owner change; governance tooling must show both values and re-check the
    /// other one immediately before execution.
    function setLpDonationSplits(uint32 newUserLpShareE6, uint32 newTobLpShareE6) external {
        ControllerV1 angstromController = ControllerV1(controller());
        // `owner()` is only reached when the caller is not the fast owner, so a fast-owner call
        // does not depend on the owner lookup. Either lookup reverting fails closed.
        if (
            msg.sender != angstromController.fastOwner() && msg.sender != angstromController.owner()
        ) {
            revert NotAuthorized();
        }
        if (newUserLpShareE6 > MAX_SHARE_E6 || newTobLpShareE6 > MAX_SHARE_E6) {
            revert InvalidConfig();
        }

        (uint32 oldUserLpShareE6, uint32 oldTobLpShareE6) = (_userLpShareE6, _tobLpShareE6);
        (_userLpShareE6, _tobLpShareE6) = (newUserLpShareE6, newTobLpShareE6);

        emit LpDonationSplitsSet(
            oldUserLpShareE6, newUserLpShareE6, oldTobLpShareE6, newTobLpShareE6
        );
    }

    /// @notice Returns both LP shares, in E6 out of `1_000_000`. The protocol share of each is
    /// the complement.
    /// @dev Read the pair from one call pinned to a single block; never compose a pair from two
    /// reads.
    function getLpDonationSplits()
        external
        view
        returns (uint32 userLpShareE6, uint32 tobLpShareE6)
    {
        return (_userLpShareE6, _tobLpShareE6);
    }

    /// @notice Returns the Angstrom deployment this config is bound to.
    /// @dev Immutable, fixed at construction. Both the controller lookup and any off-chain check
    /// that this config belongs to a given deployment resolve through it.
    function angstrom() public view returns (address) {
        return address(ANGSTROM);
    }

    /// @notice Returns the controller whose owner and fast owner may call
    /// `setLpDonationSplits`.
    /// @dev Not stored here. It is resolved from Angstrom's live state on every call, so if the
    /// controller is replaced this follows the replacement, and so does the authority to
    /// configure the splits.
    function controller() public view returns (address) {
        return ANGSTROM.controller();
    }
}
