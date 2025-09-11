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
import {console} from "forge-std/console.sol";

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
            -int24(uint24(tickSpacing)) * 100,  // Lower tick
            int24(uint24(tickSpacing)) * 100,    // Upper tick  
            100_000e18,           // Liquidity amount
            bytes32(0)            // Salt
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
    function generateAttestation(uint64 blockNumber) internal view returns (IAngstromAdapter.Attestation memory) {
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
        
        return IAngstromAdapter.Attestation({
            blockNumber: blockNumber,
            unlockData: unlockData
        });
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
        assertEq(MockERC20(asset0).balanceOf(user1.addr), user1Asset0Before - amountIn, "User1 should have spent input tokens");
        assertEq(MockERC20(asset1).balanceOf(recipient), recipientAsset1Before + amountOut, "Recipient should have received output tokens");
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
            assertEq(MockERC20(asset0).balanceOf(user1.addr), user1Asset0Before - amountIn, "Asset0 balance should decrease");
            assertEq(MockERC20(asset1).balanceOf(user1.addr), user1Asset1Before + amountOut, "Asset1 balance should increase");
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
            assertEq(MockERC20(asset1).balanceOf(user2.addr), user2Asset1Before - amountIn, "Asset1 balance should decrease");
            assertEq(MockERC20(asset0).balanceOf(user2.addr), user2Asset0Before + amountOut, "Asset0 balance should increase");
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
            adapter.swap(
                pool,
                true,
                amountIn,
                minAmountOut,
                bundle,
                recipient,
                pastDeadline
            );
        }
        
        // Test 2: Current timestamp as deadline should work (inclusive)
        {
            uint256 currentDeadline = block.timestamp;
            
            uint256 beforeBalance = MockERC20(asset1).balanceOf(recipient);
            
            vm.prank(user1.addr);
            uint256 amountOut = adapter.swap(
                pool,
                true,
                amountIn,
                minAmountOut,
                bundle,
                recipient,
                currentDeadline
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
            adapter.swap(
                pool,
                true,
                amountIn,
                minAmountOut,
                bundle,
                recipient,
                deadline
            );
        }
    }
}