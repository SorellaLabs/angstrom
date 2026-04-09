// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {Angstrom} from "src/Angstrom.sol";
import {AngstromAdapter} from "src/periphery/AngstromAdapter.sol";
import {IAngstromAdapter} from "src/interfaces/IAngstromAdapter.sol";
import {MockERC20} from "test/_mocks/MintableMockERC20.sol";
import {RouterActor} from "test/_mocks/RouterActor.sol";

/// @title AngstromAdapter Test
/// @notice Tests for the Angstrom adapter that handles attestation selection
/// @author Jet Jadeja <jjadeja@usc.edu>
contract AngstromAdapterTest is BaseTest {
    PoolManager uni;
    Angstrom angstrom;
    AngstromAdapter adapter;

    address controller = makeAddr("controller");
    Account node = makeAccount("node");

    address asset0;
    address asset1;

    RouterActor actor;
    PoolKey pool;

    // Test users
    Account user1 = makeAccount("user_1");
    Account user2 = makeAccount("user_2");

    function setUp() public {
        // Deploy Uniswap v4 PoolManager
        uni = new PoolManager(address(0));

        // Deploy Angstrom hook
        angstrom = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, controller));

        // Deploy the adapter pointing to the PoolManager
        adapter = new AngstromAdapter(uni);

        // Enable the node for attestations
        vm.prank(controller);
        angstrom.toggleNodes(addressArray(abi.encode(node.addr)));

        // Deploy and sort test tokens
        (asset0, asset1) = deployTokensSorted();

        // Configure the pool with fees
        uint16 tickSpacing = 60; // Standard tick spacing
        uint24 unlockedFee = 3000; // 0.3% fee
        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, tickSpacing, 0, unlockedFee, 0);

        // Initialize the pool at price 1:1 (tick 0)
        angstrom.initializePool(asset0, asset1, 0, TickMath.getSqrtPriceAtTick(0));

        // Create the pool key
        pool = poolKey(angstrom, asset0, asset1, int24(uint24(tickSpacing)));

        // Deploy router actor for liquidity management
        actor = new RouterActor(uni);

        // Mint tokens to actor for liquidity provision
        MockERC20(asset0).mint(address(actor), 1_000_000e18);
        MockERC20(asset1).mint(address(actor), 1_000_000e18);

        // Add liquidity to the pool (wide range around tick 0)
        actor.modifyLiquidity(
            pool,
            -int24(uint24(tickSpacing)) * 100, // Lower tick
            int24(uint24(tickSpacing)) * 100, // Upper tick
            100_000e18, // Liquidity amount
            bytes32(0) // Salt
        );

        // Set up test users with token balances
        MockERC20(asset0).mint(user1.addr, 10_000e18);
        MockERC20(asset1).mint(user1.addr, 10_000e18);
        MockERC20(asset0).mint(user2.addr, 10_000e18);
        MockERC20(asset1).mint(user2.addr, 10_000e18);

        // Approve adapter to spend user tokens
        vm.prank(user1.addr);
        MockERC20(asset0).approve(address(adapter), type(uint256).max);
        vm.prank(user1.addr);
        MockERC20(asset1).approve(address(adapter), type(uint256).max);

        vm.prank(user2.addr);
        MockERC20(asset0).approve(address(adapter), type(uint256).max);
        vm.prank(user2.addr);
        MockERC20(asset1).approve(address(adapter), type(uint256).max);
    }

    // Helper function to generate attestation for a specific block
    function generateAttestation(uint64 blockNumber)
        internal
        view
        returns (IAngstromAdapter.Attestation memory)
    {
        // Generate EIP-712 digest for the attestation
        bytes32 digest = erc712Hash(
            computeDomainSeparator(address(angstrom)),
            keccak256(
                abi.encode(keccak256("AttestAngstromBlockEmpty(uint64 block_number)"), blockNumber)
            )
        );

        // Sign the digest with the node's private key
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(node.key, digest);

        // Pack the attestation data: 20 bytes node address + signature
        bytes memory unlockData = abi.encodePacked(node.addr, r, s, v);

        return IAngstromAdapter.Attestation({blockNumber: blockNumber, unlockData: unlockData});
    }

    function test_selectCorrectAttestation() public {
        // Create attestations for different blocks
        uint64 currentBlock = uint64(block.number);
        IAngstromAdapter.Attestation[] memory bundle = new IAngstromAdapter.Attestation[](3);

        // Generate attestations for previous, current, and next block
        bundle[0] = generateAttestation(currentBlock - 1);
        bundle[1] = generateAttestation(currentBlock);
        bundle[2] = generateAttestation(currentBlock + 1);

        // Set up swap parameters
        uint128 amountIn = 100e18;
        uint128 minAmountOut = 95e18; // Allow 5% slippage
        address recipient = user2.addr;
        uint256 deadline = block.timestamp + 1 hours;

        // Record balances before swap
        uint256 user1Asset0Before = MockERC20(asset0).balanceOf(user1.addr);
        uint256 recipientAsset1Before = MockERC20(asset1).balanceOf(recipient);

        // Execute swap from user1
        vm.prank(user1.addr);
        uint256 amountOut = adapter.swap(
            pool,
            true, // zeroForOne (swap asset0 for asset1)
            amountIn,
            minAmountOut,
            bundle,
            recipient,
            deadline
        );

        // Verify the swap executed successfully
        assertGt(amountOut, 0, "Should have received output tokens");
        assertGe(amountOut, minAmountOut, "Should meet minimum output requirement");

        // Verify tokens were transferred correctly
        assertEq(
            MockERC20(asset0).balanceOf(user1.addr),
            user1Asset0Before - amountIn,
            "User1 should have spent input tokens"
        );
        assertEq(
            MockERC20(asset1).balanceOf(recipient),
            recipientAsset1Before + amountOut,
            "Recipient should have received output tokens"
        );
    }

    function test_revertOnMissingAttestation() public {
        // Create attestations for different blocks, but NOT for the current block
        uint64 currentBlock = uint64(block.number);
        IAngstromAdapter.Attestation[] memory bundle = new IAngstromAdapter.Attestation[](2);

        // Generate attestations for previous and next block, but NOT current
        bundle[0] = generateAttestation(currentBlock - 1);
        bundle[1] = generateAttestation(currentBlock + 1);

        // Set up swap parameters
        uint128 amountIn = 100e18;
        uint128 minAmountOut = 95e18;
        address recipient = user2.addr;
        uint256 deadline = block.timestamp + 1 hours;

        // Expect the swap to revert with missing attestation error
        vm.prank(user1.addr);
        vm.expectRevert("MISSING_ATTESTATION_FOR_CURRENT_BLOCK");
        adapter.swap(
            pool,
            true, // zeroForOne
            amountIn,
            minAmountOut,
            bundle,
            recipient,
            deadline
        );
    }

    function test_swapWithValidAttestation() public {
        // This test verifies that a swap works correctly with just a single valid attestation
        uint64 currentBlock = uint64(block.number);
        IAngstromAdapter.Attestation[] memory bundle = new IAngstromAdapter.Attestation[](1);

        // Generate only one attestation for the current block
        bundle[0] = generateAttestation(currentBlock);

        // Test both swap directions
        // First: asset0 -> asset1 (zeroForOne = true)
        {
            uint128 amountIn = 50e18;
            uint128 minAmountOut = 45e18; // Allow some slippage

            uint256 user1Asset0Before = MockERC20(asset0).balanceOf(user1.addr);
            uint256 user1Asset1Before = MockERC20(asset1).balanceOf(user1.addr);

            vm.prank(user1.addr);
            uint256 amountOut = adapter.swap(
                pool,
                true, // zeroForOne
                amountIn,
                minAmountOut,
                bundle,
                user1.addr, // recipient is same as sender
                block.timestamp + 1 hours
            );

            // Verify the swap executed
            assertGt(amountOut, 0, "Should receive output tokens");
            assertGe(amountOut, minAmountOut, "Should meet minimum output");

            // Verify balances changed correctly
            assertEq(
                MockERC20(asset0).balanceOf(user1.addr),
                user1Asset0Before - amountIn,
                "Asset0 balance should decrease"
            );
            assertEq(
                MockERC20(asset1).balanceOf(user1.addr),
                user1Asset1Before + amountOut,
                "Asset1 balance should increase"
            );
        }

        // Second: asset1 -> asset0 (zeroForOne = false)
        {
            // Move to next block to test with fresh attestation
            vm.roll(currentBlock + 1);
            bundle[0] = generateAttestation(uint64(block.number));

            uint128 amountIn = 30e18;
            uint128 minAmountOut = 25e18;

            uint256 user2Asset1Before = MockERC20(asset1).balanceOf(user2.addr);
            uint256 user2Asset0Before = MockERC20(asset0).balanceOf(user2.addr);

            vm.prank(user2.addr);
            uint256 amountOut = adapter.swap(
                pool,
                false, // zeroForOne = false (swap asset1 for asset0)
                amountIn,
                minAmountOut,
                bundle,
                user2.addr,
                block.timestamp + 1 hours
            );

            // Verify the reverse swap executed
            assertGt(amountOut, 0, "Should receive output tokens");
            assertGe(amountOut, minAmountOut, "Should meet minimum output");

            // Verify balances changed correctly for reverse direction
            assertEq(
                MockERC20(asset1).balanceOf(user2.addr),
                user2Asset1Before - amountIn,
                "Asset1 balance should decrease"
            );
            assertEq(
                MockERC20(asset0).balanceOf(user2.addr),
                user2Asset0Before + amountOut,
                "Asset0 balance should increase"
            );
        }
    }

    function test_deadlineProtection() public {
        // Create valid attestation for current block
        uint64 currentBlock = uint64(block.number);
        IAngstromAdapter.Attestation[] memory bundle = new IAngstromAdapter.Attestation[](1);
        bundle[0] = generateAttestation(currentBlock);

        uint128 amountIn = 50e18;
        uint128 minAmountOut = 45e18;
        address recipient = user2.addr;

        // Test 1: Deadline in the past should revert
        {
            uint256 pastDeadline = block.timestamp - 1;

            vm.prank(user1.addr);
            vm.expectRevert("DEADLINE_EXPIRED");
            adapter.swap(pool, true, amountIn, minAmountOut, bundle, recipient, pastDeadline);
        }

        // Test 2: Current timestamp as deadline should work (inclusive)
        {
            uint256 currentDeadline = block.timestamp;

            uint256 beforeBalance = MockERC20(asset1).balanceOf(recipient);

            vm.prank(user1.addr);
            uint256 amountOut = adapter.swap(
                pool, true, amountIn, minAmountOut, bundle, recipient, currentDeadline
            );

            // Verify swap executed successfully at exact deadline
            assertGt(amountOut, 0, "Should work at exact deadline");
            assertGt(
                MockERC20(asset1).balanceOf(recipient),
                beforeBalance,
                "Should receive tokens at exact deadline"
            );
        }

        // Test 3: Future deadline should work
        {
            uint256 futureDeadline = block.timestamp + 100;

            // Use user2 for this test since user1 already spent tokens
            uint256 beforeBalance = MockERC20(asset0).balanceOf(user1.addr);

            vm.prank(user2.addr);
            uint256 amountOut = adapter.swap(
                pool,
                false, // swap asset1 for asset0
                amountIn,
                minAmountOut,
                bundle,
                user1.addr, // different recipient
                futureDeadline
            );

            // Verify swap executed successfully
            assertGt(amountOut, 0, "Should receive output tokens");
            assertEq(
                MockERC20(asset0).balanceOf(user1.addr),
                beforeBalance + amountOut,
                "Recipient should receive tokens"
            );
        }

        // Test 4: Advancing time past deadline should fail
        {
            uint256 deadline = block.timestamp + 100;

            // Advance time past the deadline
            vm.warp(block.timestamp + 101);

            // Need fresh attestation for new block
            vm.roll(currentBlock + 1);
            bundle[0] = generateAttestation(uint64(block.number));

            vm.prank(user1.addr);
            vm.expectRevert("DEADLINE_EXPIRED");
            adapter.swap(pool, true, amountIn, minAmountOut, bundle, recipient, deadline);
        }
    }

    function test_protectedSwapUsesLowerFee() public {
        MockERC20 tokenA = new MockERC20();
        MockERC20 tokenB = new MockERC20();

        (address testAsset0, address testAsset1) = address(tokenA) < address(tokenB)
            ? (address(tokenA), address(tokenB))
            : (address(tokenB), address(tokenA));

        uint16 tickSpacing = 60;
        uint24 protectedFee = 500; // 0.05% - LOWER fee with valid attestation
        uint24 unlockedFee = 3000; // 0.30% - HIGHER fee without attestation
        uint24 surplusFee = 0; // No surplus fee for simplicity

        vm.prank(controller);
        angstrom.configurePool(
            testAsset0, testAsset1, tickSpacing, protectedFee, unlockedFee, surplusFee
        );

        angstrom.initializePool(testAsset0, testAsset1, 1, TickMath.getSqrtPriceAtTick(0));

        PoolKey memory testPool =
            poolKey(angstrom, testAsset0, testAsset1, int24(uint24(tickSpacing)));

        MockERC20(testAsset0).mint(address(actor), 1_000_000e18);
        MockERC20(testAsset1).mint(address(actor), 1_000_000e18);

        actor.modifyLiquidity(
            testPool,
            -int24(uint24(tickSpacing)) * 100,
            int24(uint24(tickSpacing)) * 100,
            100_000e18,
            bytes32(0)
        );

        Account memory testUser = makeAccount("test_user");
        MockERC20(testAsset0).mint(testUser.addr, 10_000e18);
        MockERC20(testAsset1).mint(testUser.addr, 10_000e18);

        vm.prank(testUser.addr);
        MockERC20(testAsset0).approve(address(adapter), type(uint256).max);
        vm.prank(testUser.addr);
        MockERC20(testAsset1).approve(address(adapter), type(uint256).max);

        uint64 currentBlock = uint64(block.number);
        IAngstromAdapter.Attestation[] memory bundle = new IAngstromAdapter.Attestation[](1);
        bundle[0] = generateAttestation(currentBlock);

        uint128 amountIn = 1000e18;
        uint128 minAmountOut = 0;

        uint256 balanceBefore = MockERC20(testAsset1).balanceOf(testUser.addr);

        vm.prank(testUser.addr);
        uint256 amountOut = adapter.swap(
            testPool, true, amountIn, minAmountOut, bundle, testUser.addr, block.timestamp + 1 hours
        );

        uint256 balanceAfter = MockERC20(testAsset1).balanceOf(testUser.addr);
        uint256 actualOutput = balanceAfter - balanceBefore;

        assertEq(amountOut, actualOutput, "Output amount mismatch");
        assertGt(amountOut, 0, "Should receive output tokens");

        uint256 expectedWithProtectedFee = (amountIn * (10000 - 50)) / 10000;
        uint256 expectedWithUnlockedFee = (amountIn * (10000 - 300)) / 10000;

        uint256 diffFromProtected = amountOut > expectedWithProtectedFee
            ? amountOut - expectedWithProtectedFee
            : expectedWithProtectedFee - amountOut;

        uint256 diffFromUnlocked = amountOut > expectedWithUnlockedFee
            ? amountOut - expectedWithUnlockedFee
            : expectedWithUnlockedFee - amountOut;

        assertLt(
            diffFromProtected,
            diffFromUnlocked,
            "Output should be closer to protected fee amount, indicating lower fee was used"
        );

        assertGt(
            amountOut,
            expectedWithUnlockedFee + 2e18, // At least 2 tokens better than unlocked
            "Protected swap should yield notably more output than unlocked would"
        );
    }

    function test_compareProtectedVsUnlockedSwapOutput() public {
        (address testAsset0, address testAsset1, PoolKey memory testPool) = _setupComparisonPool();

        Account memory protectedUser = makeAccount("protected_user");
        Account memory unlockedUser = makeAccount("unlocked_user");

        uint128 swapAmount = 1000e18;
        _prepareUsers(testAsset0, protectedUser, unlockedUser, swapAmount);

        uint64 currentBlock = uint64(block.number);
        IAngstromAdapter.Attestation[] memory bundle = new IAngstromAdapter.Attestation[](1);
        bundle[0] = generateAttestation(currentBlock);

        uint256 protectedOutput;
        {
            uint256 balanceBefore = MockERC20(testAsset1).balanceOf(protectedUser.addr);

            vm.prank(node.addr);
            angstrom.execute("");

            vm.prank(protectedUser.addr);
            protectedOutput = adapter.swap(
                testPool, true, swapAmount, 0, bundle, protectedUser.addr, block.timestamp + 1 hours
            );

            uint256 balanceAfter = MockERC20(testAsset1).balanceOf(protectedUser.addr);
            assertEq(protectedOutput, balanceAfter - balanceBefore, "Protected output mismatch");
        }

        uint256 unlockedOutput;
        {
            vm.prank(unlockedUser.addr);
            MockERC20(testAsset0).transfer(address(actor), swapAmount);

            uint256 actorBalanceBefore = MockERC20(testAsset1).balanceOf(address(actor));

            actor.swap(
                testPool,
                true,
                -int256(uint256(swapAmount)), // Negative for exact input
                TickMath.MIN_SQRT_PRICE + 1 // Price limit
            );

            uint256 actorBalanceAfter = MockERC20(testAsset1).balanceOf(address(actor));
            unlockedOutput = actorBalanceAfter - actorBalanceBefore;

            vm.prank(address(actor));
            MockERC20(testAsset1).transfer(unlockedUser.addr, unlockedOutput);
        }

        assertGt(
            protectedOutput,
            unlockedOutput,
            "Protected swap should yield more output than unlocked swap"
        );

        uint256 actualDifference = protectedOutput - unlockedOutput;

        uint256 minExpectedDifference = (unlockedOutput * 15) / 10000; // 0.15% minimum improvement

        assertGt(
            actualDifference,
            minExpectedDifference,
            "Protected swap should yield significantly better output (at least 0.15% better)"
        );

        uint256 maxExpectedDifference = (unlockedOutput * 50) / 10000; // 0.5% max
        assertLt(
            actualDifference,
            maxExpectedDifference,
            "Difference should be reasonable (less than 0.5%)"
        );

        assertGt(protectedOutput, 0, "Protected swap should produce output");
        assertGt(unlockedOutput, 0, "Unlocked swap should produce output");

        assertEq(
            MockERC20(testAsset0).balanceOf(protectedUser.addr),
            0,
            "Protected user should have spent all input"
        );
        assertEq(
            MockERC20(testAsset0).balanceOf(unlockedUser.addr),
            0,
            "Unlocked user should have spent all input"
        );
    }

    function test_minAmountOutProtection() public {
        uint64 currentBlock = uint64(block.number);
        IAngstromAdapter.Attestation[] memory bundle = new IAngstromAdapter.Attestation[](1);
        bundle[0] = generateAttestation(currentBlock);

        uint128 swapAmount = 100e18;
        address recipient = user2.addr;
        uint256 deadline = block.timestamp + 1 hours;

        // First: Execute a successful swap to determine actual output
        uint256 actualOutput;
        {
            uint256 balanceBefore = MockERC20(asset1).balanceOf(recipient);

            vm.prank(user1.addr);
            actualOutput = adapter.swap(
                pool,
                true, // zeroForOne
                swapAmount,
                0, // No minimum for this test swap
                bundle,
                recipient,
                deadline
            );

            uint256 balanceAfter = MockERC20(asset1).balanceOf(recipient);
            assertEq(actualOutput, balanceAfter - balanceBefore, "Output mismatch");
            assertGt(actualOutput, 0, "Should receive output");
        }

        // Second: Test that swap succeeds when minAmountOut is slightly less than actual
        // (We use 99% of actual to account for any rounding)
        {
            uint128 safeMinimum = uint128((actualOutput * 99) / 100);
            uint256 balanceBefore = MockERC20(asset1).balanceOf(recipient);

            vm.prank(user1.addr);
            uint256 output = adapter.swap(
                pool,
                true, // zeroForOne
                swapAmount,
                safeMinimum, // Slightly less than actual to ensure it passes
                bundle,
                recipient,
                deadline
            );

            assertGe(output, safeMinimum, "Output should meet minimum");
            uint256 balanceAfter = MockERC20(asset1).balanceOf(recipient);
            assertGt(balanceAfter, balanceBefore, "Should receive tokens");
        }

        // Third: Test that swap reverts when minAmountOut is unrealistically high
        // We use 110% of the first swap's output as an unrealistic expectation
        {
            uint128 unrealisticMinimum = uint128((actualOutput * 110) / 100);

            vm.prank(user1.addr);
            vm.expectRevert("MIN_OUT_NOT_SATISFIED");
            adapter.swap(
                pool,
                true, // zeroForOne
                swapAmount,
                unrealisticMinimum,
                bundle,
                recipient,
                deadline
            );
        }

        // Fourth: Test with a more reasonable but still too high minimum (105%)
        {
            uint128 tooHighMinimum = uint128((actualOutput * 105) / 100);

            vm.prank(user1.addr);
            vm.expectRevert("MIN_OUT_NOT_SATISFIED");
            adapter.swap(pool, true, swapAmount, tooHighMinimum, bundle, recipient, deadline);
        }

        // Fifth: Test with reasonable slippage tolerance (90% of actual)
        {
            uint128 reasonableMinimum = uint128((actualOutput * 90) / 100);
            uint256 balanceBefore = MockERC20(asset1).balanceOf(recipient);

            vm.prank(user1.addr);
            uint256 output = adapter.swap(
                pool, true, swapAmount, reasonableMinimum, bundle, recipient, deadline
            );

            assertGe(output, reasonableMinimum, "Output should meet minimum");
            assertGt(output, 0, "Should receive output");
            uint256 balanceAfter = MockERC20(asset1).balanceOf(recipient);
            assertEq(output, balanceAfter - balanceBefore, "Balance change should match output");
        }

        // Sixth: Test reverse direction (zeroForOne = false) with slippage protection
        {
            uint256 reverseActualOutput;
            {
                uint256 balanceBefore = MockERC20(asset0).balanceOf(recipient);

                vm.prank(user2.addr);
                reverseActualOutput =
                    adapter.swap(pool, false, swapAmount, 0, bundle, recipient, deadline);

                uint256 balanceAfter = MockERC20(asset0).balanceOf(recipient);
                assertEq(
                    reverseActualOutput, balanceAfter - balanceBefore, "Reverse output mismatch"
                );
            }

            uint128 tooHighReverseMin = uint128(reverseActualOutput + 1);

            vm.prank(user2.addr);
            vm.expectRevert("MIN_OUT_NOT_SATISFIED");
            adapter.swap(pool, false, swapAmount, tooHighReverseMin, bundle, recipient, deadline);

            uint128 acceptableReverseMin = uint128((reverseActualOutput * 99) / 100);

            vm.prank(user2.addr);
            uint256 output = adapter.swap(
                pool, false, swapAmount, acceptableReverseMin, bundle, recipient, deadline
            );

            assertGe(output, acceptableReverseMin, "Reverse output should meet minimum");
        }
    }

    function test_invalidSignatureRevertsToUnlocked() public {
        // Simple test: invalid attestations should revert or use unlocked fee
        uint64 currentBlock = uint64(block.number);
        uint128 swapAmount = 100e18;

        // Create invalid attestation with wrong signer
        Account memory wrongSigner = makeAccount("wrong_signer");
        bytes32 digest = erc712Hash(
            computeDomainSeparator(address(angstrom)),
            keccak256(
                abi.encode(keccak256("AttestAngstromBlockEmpty(uint64 block_number)"), currentBlock)
            )
        );

        (uint8 v, bytes32 r, bytes32 s) = vm.sign(wrongSigner.key, digest);
        bytes memory invalidUnlockData = abi.encodePacked(wrongSigner.addr, r, s, v);

        IAngstromAdapter.Attestation[] memory invalidBundle = new IAngstromAdapter.Attestation[](1);
        invalidBundle[0] = IAngstromAdapter.Attestation({
            blockNumber: currentBlock, unlockData: invalidUnlockData
        });

        // First unlock the pool so it's in unlocked state
        vm.prank(node.addr);
        angstrom.execute("");

        // Try swap with invalid attestation - should work but use unlocked fee
        vm.prank(user1.addr);
        uint256 output = adapter.swap(
            pool, true, swapAmount, 0, invalidBundle, user2.addr, block.timestamp + 1 hours
        );

        // Swap succeeds because pool is unlocked, even with bad attestation
        assertGt(output, 0, "Swap should succeed when pool is unlocked despite invalid attestation");
    }

    function _setupComparisonPool() internal returns (address, address, PoolKey memory) {
        MockERC20 tokenA = new MockERC20();
        MockERC20 tokenB = new MockERC20();

        (address testAsset0, address testAsset1) = address(tokenA) < address(tokenB)
            ? (address(tokenA), address(tokenB))
            : (address(tokenB), address(tokenA));

        vm.prank(controller);
        angstrom.configurePool(testAsset0, testAsset1, 60, 500, 3000, 100);

        // Initialize pool
        angstrom.initializePool(testAsset0, testAsset1, 1, TickMath.getSqrtPriceAtTick(0));

        PoolKey memory testPool = poolKey(angstrom, testAsset0, testAsset1, 60);

        // Add liquidity
        MockERC20(testAsset0).mint(address(actor), 10_000_000e18);
        MockERC20(testAsset1).mint(address(actor), 10_000_000e18);
        actor.modifyLiquidity(testPool, -60000, 60000, 1_000_000e18, bytes32(0));

        return (testAsset0, testAsset1, testPool);
    }

    function _prepareUsers(
        address _asset0,
        Account memory _user1,
        Account memory _user2,
        uint128 amount
    ) internal {
        MockERC20(_asset0).mint(_user1.addr, amount);
        MockERC20(_asset0).mint(_user2.addr, amount);

        vm.prank(_user1.addr);
        MockERC20(_asset0).approve(address(adapter), type(uint256).max);

        vm.prank(_user2.addr);
        MockERC20(_asset0).approve(address(actor), type(uint256).max);
    }
}
