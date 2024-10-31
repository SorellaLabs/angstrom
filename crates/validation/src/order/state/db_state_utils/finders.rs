// Given that the gas spec when fuzzing balances runs off the assumption
// that all tokens are ERC-20 which all are written in solidity and don't
// pack the balance / approval slots. we align to the same assumptions here.
use std::{fmt::Debug, sync::Arc};

use alloy::{
    primitives::{keccak256, Address, U256},
    sol_types::{SolCall, SolStruct, SolValue}
};
use revm::{
    db::CacheDB,
    primitives::{EnvWithHandlerCfg, ResultAndState, TxEnv, TxKind},
    DatabaseRef
};

alloy::sol!(
    function balanceOf(address _owner) public view returns (uint256 balance);
    function allowance(address _owner, address _spender) public view returns (uint256 remaining);
);

/// panics if we cannot find the slot for the given token
fn find_slot_offset_for_balance<DB: revm::DatabaseRef>(db: Arc<DB>, token_address: Address) -> u64
where
    <DB as revm::DatabaseRef>::Error: Debug
{
    let probe_address = Address::random();

    let mut db = CacheDB::new(db.clone());
    let evm_handler = EnvWithHandlerCfg::default();
    let mut evm = revm::Evm::builder()
        .with_ref_db(db.clone())
        .with_env_with_handler_cfg(evm_handler)
        .modify_env(|env| {
            env.cfg.disable_balance_check = true;
        })
        .build();

    // check the first 100 offsets
    for offset in 0..100 {
        // set balance
        let balance_slot = keccak256((probe_address, offset).abi_encode());
        db.insert_account_storage(token_address, balance_slot.into(), U256::from(123456789))
            .unwrap();
        // execute revm to see if we hit the slot
        evm = evm
            .modify()
            .modify_tx_env(|tx| {
                tx.caller = probe_address;
                tx.transact_to = TxKind::Call(token_address);
                tx.data = balanceOfCall::new((probe_address,)).abi_encode().into();
                tx.value = U256::from(0);
                tx.nonce = None;
            })
            .build();

        let output = evm.transact().unwrap().result.output().unwrap();
        let output_balance = balanceOfReturn::abi_decode(output, false);
        if output_balance.balance == U256::from(123456789) {
            return offset as u64
        }
    }

    panic!("was not able to find balance offset");
}

/// panics if we cannot prove the slot for the given token
fn find_slot_offset_for_approval<DB: revm::DatabaseRef>(
    db: Arc<DB>,
    token_address: Address
) -> u64 {
    todo!()
}

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
