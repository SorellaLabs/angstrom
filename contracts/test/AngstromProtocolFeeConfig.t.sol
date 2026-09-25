// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {Angstrom} from "src/Angstrom.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {IAngstromAuth} from "src/interfaces/IAngstromAuth.sol";
import {ControllerV1} from "src/periphery/ControllerV1.sol";
import {AngstromProtocolFeeConfig} from "src/periphery/AngstromProtocolFeeConfig.sol";

/// @dev Mirrors Angstrom's `extsload`, which is the only thing `AngstromView.controller` needs.
/// Deliberately declares no state, so slot 0 -- the controller slot -- is written only through
/// `vm.store`, exactly as `AngstromView` reads it.
contract MockAngstromAuth {
    function extsload(uint256 slot) external view returns (uint256 value) {
        assembly ("memory-safe") {
            value := sload(slot)
        }
    }
}

contract MockController {
    address public immutable owner;
    address public immutable fastOwner;

    constructor(address owner_, address fastOwner_) {
        owner = owner_;
        fastOwner = fastOwner_;
    }
}

/// @dev `fastOwner()` is checked first, so a revert here must block *every* caller.
contract RevertingFastOwnerController {
    address public immutable owner;

    constructor(address owner_) {
        owner = owner_;
    }

    function fastOwner() external pure returns (address) {
        revert("fastOwner reverted");
    }
}

/// @dev `owner()` is only reached when the caller is not the fast owner, so a revert here must
/// block everyone *except* the fast owner.
contract RevertingOwnerController {
    address public immutable fastOwner;

    constructor(address fastOwner_) {
        fastOwner = fastOwner_;
    }

    function owner() external pure returns (address) {
        revert("owner reverted");
    }
}

/// @author jnoorchashm37 <https://github.com/jnoorchashm37>
contract AngstromProtocolFeeConfigTest is BaseTest {
    uint32 internal constant MAX_SHARE_E6 = 1_000_000;
    uint32 internal constant INITIAL_USER_LP_SHARE_E6 = 750_000;
    uint32 internal constant INITIAL_TOB_LP_SHARE_E6 = 1_000_000;

    bytes32 internal constant SPLITS_SLOT = bytes32(uint256(0));

    string internal constant ARTIFACT_PATH =
        "/out/AngstromProtocolFeeConfig.sol/AngstromProtocolFeeConfig.json";

    /// @dev Loop guard for the ABI walk; the ABI is 8 entries and must stay far below this.
    uint256 internal constant _ABI_ENTRY_CAP = 64;

    MockAngstromAuth angstrom;
    ControllerV1 controller;
    AngstromProtocolFeeConfig config;

    address controller_owner = makeAddr("controller_owner");
    address controller_fast_owner = makeAddr("controller_fast_owner");

    function setUp() public {
        angstrom = new MockAngstromAuth();
        controller = new ControllerV1(
            IAngstromAuth(address(angstrom)), controller_owner, controller_fast_owner
        );
        _pointAngstromAt(address(controller));
        config = new AngstromProtocolFeeConfig(
            IAngstromAuth(address(angstrom)), INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6
        );
    }

    ////////////////////////////////////////////////////////////////
    //                        CONSTRUCTOR                         //
    ////////////////////////////////////////////////////////////////

    function test_constructor_setsInitialValues() public view {
        (uint32 userLpShareE6, uint32 tobLpShareE6) = config.getLpDonationSplits();
        assertEq(userLpShareE6, INITIAL_USER_LP_SHARE_E6);
        assertEq(tobLpShareE6, INITIAL_TOB_LP_SHARE_E6);
        assertEq(config.angstrom(), address(angstrom));
    }

    function test_constructor_emitsFromZero() public {
        vm.expectEmit(true, true, true, true);
        emit AngstromProtocolFeeConfig.LpDonationSplitsSet(
            0, INITIAL_USER_LP_SHARE_E6, 0, INITIAL_TOB_LP_SHARE_E6
        );
        new AngstromProtocolFeeConfig(
            IAngstromAuth(address(angstrom)), INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6
        );
    }

    function test_constructor_rejectsZeroAngstrom() public {
        vm.expectRevert(AngstromProtocolFeeConfig.InvalidConfig.selector);
        new AngstromProtocolFeeConfig(IAngstromAuth(address(0)), 0, 0);
    }

    function test_constructor_rejectsUserShareAboveMax() public {
        vm.expectRevert(AngstromProtocolFeeConfig.InvalidConfig.selector);
        new AngstromProtocolFeeConfig(IAngstromAuth(address(angstrom)), MAX_SHARE_E6 + 1, 0);
    }

    function test_constructor_rejectsTobShareAboveMax() public {
        vm.expectRevert(AngstromProtocolFeeConfig.InvalidConfig.selector);
        new AngstromProtocolFeeConfig(IAngstromAuth(address(angstrom)), 0, MAX_SHARE_E6 + 1);
    }

    function test_constructor_acceptsBounds() public {
        _assertSplits(new AngstromProtocolFeeConfig(IAngstromAuth(address(angstrom)), 0, 0), 0, 0);
        _assertSplits(
            new AngstromProtocolFeeConfig(
                IAngstromAuth(address(angstrom)), MAX_SHARE_E6, MAX_SHARE_E6
            ),
            MAX_SHARE_E6,
            MAX_SHARE_E6
        );
    }

    ////////////////////////////////////////////////////////////////
    //                      AUTHORIZATION                         //
    ////////////////////////////////////////////////////////////////

    function test_auth_ownerCanSet() public {
        vm.prank(controller_owner);
        config.setLpDonationSplits(500_000, 900_000);
        _assertSplits(config, 500_000, 900_000);
    }

    function test_auth_fastOwnerCanSet() public {
        vm.prank(controller_fast_owner);
        config.setLpDonationSplits(500_000, 900_000);
        _assertSplits(config, 500_000, 900_000);
    }

    function test_fuzzing_auth_rejectsEveryoneElse(address caller) public {
        vm.assume(caller != controller_owner && caller != controller_fast_owner);

        vm.expectRevert(AngstromProtocolFeeConfig.NotAuthorized.selector);
        vm.prank(caller);
        config.setLpDonationSplits(500_000, 900_000);

        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);
    }

    /// @dev Neither the controller itself nor this contract (the deployer) has standing.
    function test_auth_rejectsControllerAndDeployer() public {
        vm.expectRevert(AngstromProtocolFeeConfig.NotAuthorized.selector);
        vm.prank(address(controller));
        config.setLpDonationSplits(500_000, 900_000);

        vm.expectRevert(AngstromProtocolFeeConfig.NotAuthorized.selector);
        config.setLpDonationSplits(500_000, 900_000);

        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);
    }

    function test_auth_identicalOwnerAndFastOwner() public {
        address sole = makeAddr("sole_owner");
        _pointAngstromAt(address(new MockController(sole, sole)));

        vm.prank(sole);
        config.setLpDonationSplits(400_000, 800_000);
        _assertSplits(config, 400_000, 800_000);

        vm.expectRevert(AngstromProtocolFeeConfig.NotAuthorized.selector);
        vm.prank(controller_owner);
        config.setLpDonationSplits(1, 1);
    }

    /// @dev `fastOwner()` is consulted first, so its revert must fail closed for everyone --
    /// including the owner, who would otherwise be authorized.
    function test_auth_revertingFastOwnerLookupFailsClosed() public {
        _pointAngstromAt(address(new RevertingFastOwnerController(controller_owner)));

        vm.expectRevert();
        vm.prank(controller_owner);
        config.setLpDonationSplits(400_000, 800_000);

        vm.expectRevert();
        vm.prank(controller_fast_owner);
        config.setLpDonationSplits(400_000, 800_000);

        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);
    }

    /// @dev The fast owner short-circuits before `owner()` is reached, so its revert must not
    /// block a fast-owner call -- but must fail closed for everybody else.
    function test_auth_revertingOwnerLookupBlocksAllButFastOwner() public {
        _pointAngstromAt(address(new RevertingOwnerController(controller_fast_owner)));

        vm.expectRevert();
        vm.prank(controller_owner);
        config.setLpDonationSplits(400_000, 800_000);
        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);

        vm.prank(controller_fast_owner);
        config.setLpDonationSplits(400_000, 800_000);
        _assertSplits(config, 400_000, 800_000);
    }

    function test_auth_controllerWithoutCodeFailsClosed() public {
        _pointAngstromAt(makeAddr("not_a_contract"));

        vm.expectRevert();
        vm.prank(controller_owner);
        config.setLpDonationSplits(400_000, 800_000);

        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);
    }

    function test_auth_zeroControllerFailsClosed() public {
        _pointAngstromAt(address(0));
        assertEq(config.controller(), address(0));

        vm.expectRevert();
        vm.prank(controller_owner);
        config.setLpDonationSplits(400_000, 800_000);

        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);
    }

    /// @dev `controller()` reads Angstrom's live state, so replacing the controller moves
    /// configuration authority with it, without touching this contract.
    function test_auth_followsControllerReplacement() public {
        assertEq(config.controller(), address(controller));

        address next_owner = makeAddr("next_owner");
        address next_fast_owner = makeAddr("next_fast_owner");
        MockController next = new MockController(next_owner, next_fast_owner);
        _pointAngstromAt(address(next));

        assertEq(config.controller(), address(next));
        assertEq(config.angstrom(), address(angstrom), "binding must not move");

        // The old controller's authorities lose standing.
        vm.expectRevert(AngstromProtocolFeeConfig.NotAuthorized.selector);
        vm.prank(controller_owner);
        config.setLpDonationSplits(400_000, 800_000);

        vm.expectRevert(AngstromProtocolFeeConfig.NotAuthorized.selector);
        vm.prank(controller_fast_owner);
        config.setLpDonationSplits(400_000, 800_000);

        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);

        // The replacement's do not.
        vm.prank(next_owner);
        config.setLpDonationSplits(400_000, 800_000);
        _assertSplits(config, 400_000, 800_000);

        vm.prank(next_fast_owner);
        config.setLpDonationSplits(300_000, 700_000);
        _assertSplits(config, 300_000, 700_000);
    }

    /// @dev The mock only stands in for `extsload`; prove the same resolution against a real
    /// Angstrom deployment and a real `ControllerV1`.
    function test_auth_resolvesThroughRealAngstrom() public {
        PoolManager uni = new PoolManager(makeAddr("pm_owner"));
        address temp_controller = makeAddr("temp_controller");
        Angstrom realAngstrom =
            Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, temp_controller));
        ControllerV1 realController =
            new ControllerV1(realAngstrom, controller_owner, controller_fast_owner);
        vm.prank(temp_controller);
        realAngstrom.setController(address(realController));

        AngstromProtocolFeeConfig realConfig = new AngstromProtocolFeeConfig(
            IAngstromAuth(address(realAngstrom)), INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6
        );

        assertEq(realConfig.angstrom(), address(realAngstrom));
        assertEq(realConfig.controller(), address(realController));
        assertEq(realConfig.controller(), rawGetController(address(realAngstrom)));

        vm.prank(controller_fast_owner);
        realConfig.setLpDonationSplits(400_000, 800_000);
        _assertSplits(realConfig, 400_000, 800_000);

        vm.expectRevert(AngstromProtocolFeeConfig.NotAuthorized.selector);
        vm.prank(makeAddr("nobody"));
        realConfig.setLpDonationSplits(1, 1);
    }

    ////////////////////////////////////////////////////////////////
    //                          BOUNDS                            //
    ////////////////////////////////////////////////////////////////

    function test_bounds_acceptsInclusiveRange() public {
        uint32[4][4] memory cases = [
            [uint32(0), 0, 0, 0],
            [uint32(0), MAX_SHARE_E6, 0, MAX_SHARE_E6],
            [MAX_SHARE_E6, 0, MAX_SHARE_E6, 0],
            [MAX_SHARE_E6, MAX_SHARE_E6, MAX_SHARE_E6, MAX_SHARE_E6]
        ];
        for (uint256 i = 0; i < cases.length; i++) {
            vm.prank(controller_owner);
            config.setLpDonationSplits(cases[i][0], cases[i][1]);
            _assertSplits(config, cases[i][2], cases[i][3]);
        }
    }

    function test_bounds_rejectsUserShareAboveMax() public {
        vm.expectRevert(AngstromProtocolFeeConfig.InvalidConfig.selector);
        vm.prank(controller_owner);
        config.setLpDonationSplits(MAX_SHARE_E6 + 1, 0);

        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);
    }

    function test_bounds_rejectsTobShareAboveMax() public {
        vm.expectRevert(AngstromProtocolFeeConfig.InvalidConfig.selector);
        vm.prank(controller_owner);
        config.setLpDonationSplits(0, MAX_SHARE_E6 + 1);

        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);
    }

    /// @dev The pair is written atomically: an out-of-range half must not let the valid half land.
    function test_fuzzing_bounds_rejectionWritesNothing(
        uint32 newUserLpShareE6,
        uint32 newTobLpShareE6
    ) public {
        vm.assume(newUserLpShareE6 > MAX_SHARE_E6 || newTobLpShareE6 > MAX_SHARE_E6);

        vm.expectRevert(AngstromProtocolFeeConfig.InvalidConfig.selector);
        vm.prank(controller_owner);
        config.setLpDonationSplits(newUserLpShareE6, newTobLpShareE6);

        _assertSplits(config, INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6);
        assertEq(
            vm.load(address(config), SPLITS_SLOT),
            _packed(INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6),
            "slot 0 written on rejection"
        );
    }

    /// @dev An unauthorized caller must not write either, even with in-range values.
    function test_auth_rejectionWritesNothing() public {
        vm.expectRevert(AngstromProtocolFeeConfig.NotAuthorized.selector);
        vm.prank(makeAddr("nobody"));
        config.setLpDonationSplits(0, 0);

        assertEq(
            vm.load(address(config), SPLITS_SLOT),
            _packed(INITIAL_USER_LP_SHARE_E6, INITIAL_TOB_LP_SHARE_E6)
        );
    }

    ////////////////////////////////////////////////////////////////
    //                    GETTER / SLOT 0                         //
    ////////////////////////////////////////////////////////////////

    function test_slot0_matchesDocumentedLayout() public view {
        uint256 word = uint256(vm.load(address(config), SPLITS_SLOT));
        (uint32 userLpShareE6, uint32 tobLpShareE6) = _decodeSlot0(word);
        assertEq(userLpShareE6, INITIAL_USER_LP_SHARE_E6, "user share at bytes 0..4");
        assertEq(tobLpShareE6, INITIAL_TOB_LP_SHARE_E6, "tob share at bytes 4..8");
        assertEq(word >> 64, 0, "bits 64 and above must stay zero");
    }

    /// @dev Off-chain readers decode slot 0 directly; it must never disagree with the getter.
    function test_fuzzing_getterAndSlot0Agree(uint32 newUserLpShareE6, uint32 newTobLpShareE6)
        public
    {
        newUserLpShareE6 = uint32(bound(newUserLpShareE6, 0, MAX_SHARE_E6));
        newTobLpShareE6 = uint32(bound(newTobLpShareE6, 0, MAX_SHARE_E6));

        vm.prank(controller_owner);
        config.setLpDonationSplits(newUserLpShareE6, newTobLpShareE6);

        (uint32 userLpShareE6, uint32 tobLpShareE6) = config.getLpDonationSplits();
        assertEq(userLpShareE6, newUserLpShareE6);
        assertEq(tobLpShareE6, newTobLpShareE6);

        uint256 word = uint256(vm.load(address(config), SPLITS_SLOT));
        (uint32 decodedUserLpShareE6, uint32 decodedTobLpShareE6) = _decodeSlot0(word);
        assertEq(decodedUserLpShareE6, userLpShareE6, "user share disagrees with slot 0");
        assertEq(decodedTobLpShareE6, tobLpShareE6, "tob share disagrees with slot 0");
        assertEq(word >> 64, 0, "bits 64 and above must stay zero");
        assertEq(bytes32(word), _packed(newUserLpShareE6, newTobLpShareE6));
    }

    function test_setLpDonationSplits_emitsOldAndNew() public {
        vm.expectEmit(true, true, true, true);
        emit AngstromProtocolFeeConfig.LpDonationSplitsSet(
            INITIAL_USER_LP_SHARE_E6, 400_000, INITIAL_TOB_LP_SHARE_E6, 800_000
        );
        vm.prank(controller_owner);
        config.setLpDonationSplits(400_000, 800_000);
    }

    ////////////////////////////////////////////////////////////////
    //                        ABI SHAPE                           //
    ////////////////////////////////////////////////////////////////

    /// @dev Exactly one state-changing function and three views, with no fallback, no receive and
    /// nothing payable -- so there is no path that moves value and no withdrawal path.
    function test_abiShape() public view {
        string memory artifact = vm.readFile(string.concat(vm.projectRoot(), ARTIFACT_PATH));

        string[] memory signatures = vm.parseJsonKeys(artifact, "$.methodIdentifiers");
        assertEq(signatures.length, 4, "unexpected number of external functions");
        string[4] memory expected = [
            "angstrom()",
            "controller()",
            "getLpDonationSplits()",
            "setLpDonationSplits(uint32,uint32)"
        ];
        for (uint256 i = 0; i < expected.length; i++) {
            bool found = false;
            for (uint256 j = 0; j < signatures.length; j++) {
                if (_eq(signatures[j], expected[i])) {
                    found = true;
                    break;
                }
            }
            assertTrue(found, string.concat("missing from ABI: ", expected[i]));
        }

        uint256 functions;
        uint256 views;
        uint256 payables;
        uint256 fallbacks;
        uint256 receives;
        uint256 entries;
        for (; entries < _ABI_ENTRY_CAP; entries++) {
            string memory entry = string.concat("$.abi[", vm.toString(entries), "]");
            if (!vm.keyExistsJson(artifact, entry)) break;

            string memory entryType = vm.parseJsonString(artifact, string.concat(entry, ".type"));
            if (_eq(entryType, "function")) functions++;
            if (_eq(entryType, "fallback")) fallbacks++;
            if (_eq(entryType, "receive")) receives++;

            string memory mutabilityKey = string.concat(entry, ".stateMutability");
            if (!vm.keyExistsJson(artifact, mutabilityKey)) continue;
            string memory mutability = vm.parseJsonString(artifact, mutabilityKey);
            if (_eq(mutability, "payable")) payables++;
            if (_eq(mutability, "view") && _eq(entryType, "function")) views++;
        }
        assertLt(entries, _ABI_ENTRY_CAP, "ABI walk hit its cap without finishing");

        assertEq(functions, 4, "expected exactly four functions");
        assertEq(views, 3, "expected exactly three views");
        assertEq(functions - views, 1, "expected exactly one state-changing function");
        assertEq(fallbacks, 0, "must have no fallback");
        assertEq(receives, 0, "must have no receive");
        assertEq(payables, 0, "nothing may be payable");
    }

    /// @dev The ABI's `view` claim, enforced by the EVM: the three views survive a staticcall and
    /// the setter -- authorized, so only the state write can fail it -- does not.
    function test_abiShape_onlySetterMutatesState() public {
        (bool ok,) = address(config)
            .staticcall(abi.encodeCall(AngstromProtocolFeeConfig.getLpDonationSplits, ()));
        assertTrue(ok, "getLpDonationSplits must be static-callable");
        (ok,) = address(config).staticcall(abi.encodeCall(AngstromProtocolFeeConfig.angstrom, ()));
        assertTrue(ok, "angstrom must be static-callable");
        (ok,) = address(config).staticcall(abi.encodeCall(AngstromProtocolFeeConfig.controller, ()));
        assertTrue(ok, "controller must be static-callable");

        bytes memory setCall =
            abi.encodeCall(AngstromProtocolFeeConfig.setLpDonationSplits, (400_000, 800_000));

        vm.prank(controller_owner);
        (ok,) = address(config).staticcall(setCall);
        assertFalse(ok, "setLpDonationSplits must not be static-callable");

        vm.prank(controller_owner);
        (ok,) = address(config).call(setCall);
        assertTrue(ok, "the same call must succeed outside a staticcall");
        _assertSplits(config, 400_000, 800_000);
    }

    function test_abiShape_noFallback() public {
        (bool ok,) = address(config).call(abi.encodeWithSelector(bytes4(0xdeadbeef)));
        assertFalse(ok, "unknown selector must revert");

        (ok,) = address(config).call("");
        assertFalse(ok, "empty calldata must revert");
    }

    function test_abiShape_noReceive() public {
        deal(address(this), 3 ether);

        (bool ok,) = address(config).call{value: 1 ether}("");
        assertFalse(ok, "plain value transfer must revert");

        (ok,) = address(config).call{value: 1 ether}(abi.encodeWithSelector(bytes4(0xdeadbeef)));
        assertFalse(ok, "value with unknown selector must revert");

        vm.prank(controller_owner);
        (ok,) = address(config).call{value: 1 ether}(
            abi.encodeCall(AngstromProtocolFeeConfig.setLpDonationSplits, (400_000, 800_000))
        );
        assertFalse(ok, "the setter must not be payable");

        assertEq(address(config).balance, 0);
    }

    ////////////////////////////////////////////////////////////////
    //                         HELPERS                            //
    ////////////////////////////////////////////////////////////////

    function _pointAngstromAt(address newController) internal {
        vm.store(address(angstrom), ANG_CONTROLLER_SLOT, bytes32(uint256(uint160(newController))));
    }

    function _assertSplits(
        AngstromProtocolFeeConfig target,
        uint32 expectedUserLpShareE6,
        uint32 expectedTobLpShareE6
    ) internal view {
        (uint32 userLpShareE6, uint32 tobLpShareE6) = target.getLpDonationSplits();
        assertEq(userLpShareE6, expectedUserLpShareE6, "userLpShareE6");
        assertEq(tobLpShareE6, expectedTobLpShareE6, "tobLpShareE6");
    }

    /// @dev The decode PLAN.md hands to off-chain readers, run against the real slot.
    function _decodeSlot0(uint256 word)
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

    function _packed(uint32 userLpShareE6, uint32 tobLpShareE6) internal pure returns (bytes32) {
        return bytes32(uint256(userLpShareE6) | (uint256(tobLpShareE6) << 32));
    }

    function _eq(string memory a, string memory b) internal pure returns (bool) {
        return keccak256(bytes(a)) == keccak256(bytes(b));
    }
}
