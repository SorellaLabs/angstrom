// Given that the gas spec when fuzzing balances runs off the assumption
// that all tokens are ERC-20 which all are written in solidity and don't
// pack the balance / approval slots. we align to the same assumptions here.
use std::sync::Arc;

use alloy::primitives::Address;


alloy::sol!(
function transfer(address _to, uint256 _value) public returns (bool success);
function transferFrom(address _from, address _to, uint256 _value) public returns (bool success);
function approve(address _spender, uint256 _value) public returns (bool success);
);

/// panics if we cannot find the slot for the given token
// fn find_slot_offset_for_balance<DB: BlockStateProviderFactory>(db: Arc<DB>) -> u64 {
//     let probe_address = Address::random();
//
//     todo!()
// }

/// panics if we cannot prove the slot for the given token
// fn find_slot_offset_for_approval<DB: BlockStateProviderFactory>(
//     db: Arc<DB>,
//     token_address: Address
// ) -> u64 {
//     todo!()
// }

// fn set_balances_and_approvals<DB: DatabaseRef + Unpin>(
//         cache_db: &mut CacheDB<Arc<DB>>,
//         calle_address: Address,
//         user_address: Address,
//         token: Address,
//         amount: U256
//     ) {
//         for i in 0..10 {
//             let balance_amount_out_slot = keccak256((user_address,
// i).abi_encode());             let approval_slot =
//                 keccak256((calle_address, keccak256((user_address,
// i).abi_encode())).abi_encode());
//
//             cache_db
//                 .insert_account_storage(token,
// balance_amount_out_slot.into(), amount)                 .map_err(|_|
// eyre!("failed to insert account into storage"))                 .unwrap();
//
//             cache_db
//                 .insert_account_storage(token, approval_slot.into(), amount)
//                 .map_err(|_| eyre!("failed to insert account into storage"))
//                 .unwrap();
//         }
//     }
