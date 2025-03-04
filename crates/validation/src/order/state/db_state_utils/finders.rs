// Given that the gas spec when fuzzing balances runs off the assumption
// that all tokens are ERC-20 which all are written in solidity and don't
// pack the balance / approval slots. we align to the same assumptions here.
use std::fmt::Debug;

use alloy::{
    primitives::{Address, U256, keccak256},
    sol_types::*
};
use angstrom_types::contract_bindings::mintable_mock_erc_20::MintableMockERC20::{
    allowanceCall, balanceOfCall
};
use revm::{
    db::CacheDB,
    primitives::{EnvWithHandlerCfg, TxKind}
};

/// panics if we cannot find the slot for the given token
pub fn find_slot_offset_for_balance<DB: revm::DatabaseRef>(
    db: &DB,
    token_address: Address
) -> eyre::Result<u64>
where
    <DB as revm::DatabaseRef>::Error: Debug
{
    let probe_address = Address::random();

    let mut db = CacheDB::new(db);
    let evm_handler = EnvWithHandlerCfg::default();

    // check the first 100 offsets
    for offset in 0..100 {
        // set balance
        let balance_slot = keccak256((probe_address, offset as u64).abi_encode());
        db.insert_account_storage(token_address, balance_slot.into(), U256::from(123456789))
            .map_err(|e| eyre::eyre!("{e:?}"))?;
        // execute revm to see if we hit the slot
        let mut evm = revm::Evm::builder()
            .with_ref_db(db.clone())
            .with_env_with_handler_cfg(evm_handler.clone())
            .modify_env(|env| {
                env.cfg.disable_balance_check = true;
            })
            .modify_tx_env(|tx| {
                tx.caller = probe_address;
                tx.transact_to = TxKind::Call(token_address);
                tx.data = balanceOfCall::new((probe_address,)).abi_encode().into();
                tx.value = U256::from(0);
                tx.nonce = None;
            })
            .build();

        let output = evm
            .transact()
            .map_err(|e| eyre::eyre!("{e:?}"))?
            .result
            .output()
            .unwrap()
            .to_vec();
        let return_data = balanceOfCall::abi_decode_returns(&output, false)?;
        if return_data._0 == U256::from(123456789) {
            return Ok(offset as u64);
        }
    }

    Err(eyre::eyre!("was not able to find balance offset"))
}

/// panics if we cannot prove the slot for the given token
pub fn find_slot_offset_for_approval<DB: revm::DatabaseRef>(
    db: &DB,
    token_address: Address
) -> eyre::Result<u64>
where
    <DB as revm::DatabaseRef>::Error: Debug
{
    let probe_user_address = Address::random();
    let probe_contract_address = Address::random();

    let mut db = CacheDB::new(db);
    let evm_handler = EnvWithHandlerCfg::default();

    // check the first 100 offsets
    for offset in 0..100 {
        // set approval
        let approval_slot = keccak256(
            (probe_contract_address, keccak256((probe_user_address, offset as u64).abi_encode()))
                .abi_encode()
        );

        db.insert_account_storage(token_address, approval_slot.into(), U256::from(123456789))
            .unwrap();
        // execute revm to see if we hit the slot
        let mut evm = revm::Evm::builder()
            .with_ref_db(db.clone())
            .with_env_with_handler_cfg(evm_handler.clone())
            .modify_env(|env| {
                env.cfg.disable_balance_check = true;
            })
            .modify_tx_env(|tx| {
                tx.caller = probe_user_address;
                tx.transact_to = TxKind::Call(token_address);
                tx.data = allowanceCall::new((probe_user_address, probe_contract_address))
                    .abi_encode()
                    .into();
                tx.value = U256::from(0);
                tx.nonce = None;
            })
            .build();

        let output = evm
            .transact()
            .map_err(|e| eyre::eyre!("{e:?}"))?
            .result
            .output()
            .unwrap()
            .to_vec();
        let return_data = allowanceCall::abi_decode_returns(&output, false)?;
        if return_data._0 == U256::from(123456789) {
            return Ok(offset as u64);
        }
    }

    Err(eyre::eyre!("was not able to find approval offset"))
}
