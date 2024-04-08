use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{Address, Bytes, U256};
use angstrom_types::{
    primitive::{Angstrom::Bundle, *},
    rpc::CallerInfo
};
use ethers_core::types::{transaction::eip2718::TypedTransaction, I256, U256 as EU256};
use eyre::Result;
use reth_primitives::revm_primitives::{Bytecode, ExecutionResult, TransactTo, TxEnv};
use reth_provider::StateProviderFactory;
use reth_revm::{DatabaseRef, EvmBuilder};

use crate::{
    bundle::{
        errors::{SimError, SimResult},
        BundleOrTransactionResult
    },
    common::lru_db::RevmLRU
};

pub trait RevmBackend {
    fn update_evm_state(&self, slot_changes: &AddressSlots) -> Result<(), SimError>;
}

pub type AddressSlots = HashMap<Address, HashMap<U256, U256>>;

/// struct used to share the mutable state across threads
pub struct RevmState<DB> {
    /// holds state to sim on
    db: RevmLRU<DB>
}

impl<DB> RevmState<DB>
where
    DB: StateProviderFactory + Send + Sync + Clone + 'static
{
    pub fn new(db: RevmLRU<DB>) -> Self {
        Self { db }
    }

    // updates the evm state on a new block
    pub fn update_evm_state(this: Arc<Self>, slot_changes: &AddressSlots) -> Result<(), SimError> {
        RevmLRU::update_evm_state(&this.db, slot_changes)
    }

    pub fn simulate_external_state(
        &self,
        txes: ExternalStateSim,
        _caller_info: CallerInfo,
        overrides: HashMap<Address, U256>
    ) -> Result<(SimResult, AddressSlots), SimError> {
        let mut prehook_env = TxEnv::default();
        let (prehook_addr, pre_hook_calldata) = txes.pre_hook();
        prehook_env.data = pre_hook_calldata;
        prehook_env.transact_to = TransactTo::Call(prehook_addr);

        let (res, mut pre_slots) = self.simulate_single_tx(prehook_env, HashMap::default())?;
        let pre_hook_gas = match res {
            ExecutionResult::Success { gas_used, .. } => gas_used.into(),
            _ => return Err(SimError::HookFailed)
        };

        let mut post_env = TxEnv::default();
        let (posthook_addr, posthook_calldata) = txes.post_hook();
        post_env.data = posthook_calldata;
        post_env.transact_to = TransactTo::Call(posthook_addr);

        let out_token_override = overrides.get(&txes.amount_out_token).unwrap();

        let db = self.db.clone();
        let current_bal = db.storage_ref(posthook_addr, *out_token_override).unwrap();

        let mut overrides: HashMap<Address, HashMap<U256, U256>> = HashMap::default();

        let mut slot_map = HashMap::default();
        slot_map.insert(*out_token_override, current_bal + U256::from(txes.amount_out_lim));
        overrides.insert(posthook_addr, slot_map);

        let (res, post_slots) = self.simulate_single_tx(post_env, overrides)?;

        let post_hook_gas = match res {
            ExecutionResult::Success { gas_used, .. } => gas_used.into(),
            _ => return Err(SimError::HookFailed)
        };
        pre_slots.extend(post_slots);

        Ok((
            SimResult::ExecutionResult(BundleOrTransactionResult::HookSimResult {
                tx: txes.tx,
                pre_hook_gas,
                post_hook_gas
            }),
            pre_slots
        ))
    }

    pub fn simulate_v4_tx(
        &self,
        tx: TypedTransaction,
        contract_overrides: HashMap<Address, Bytecode>
    ) -> Result<SimResult, SimError> {
        let mut env = TxEnv::default();
        env.data = Bytes::copy_from_slice(&tx.data().cloned().unwrap());

        env.transact_to = TransactTo::Call((tx.to_addr().unwrap().0).into());
        let mut db = self.db.clone();
        db.set_bytecode_overrides(contract_overrides);

        let mut evm = EvmBuilder::default()
            .with_db(db)
            .with_tx_env(env.clone())
            .build();

        let res = evm
            .transact()
            .map_err(|_| SimError::RevmEVMTransactionError(env.clone()))?;

        let (delta, gas) = match res.result {
            ExecutionResult::Success { gas_used, output, .. } => {
                let delta = I256::from_raw(EU256::from(unsafe {
                    *(output.into_data().to_vec().as_slice() as *const _ as *mut [u8; 32])
                }));

                (delta, gas_used.into())
            }
            _ => return Err(SimError::HookFailed)
        };

        Ok(SimResult::ExecutionResult(BundleOrTransactionResult::UniswapV4Results { delta, gas }))
    }

    /// simulates a bundle of transactions
    pub fn simulate_vanilla_bundle(
        &self,
        bundle: Bundle,
        caller_info: CallerInfo
    ) -> Result<SimResult, SimError> {
        let mut evm_db = self.db.clone();
        evm_db.set_state_overrides(caller_info.overrides);

        let mut tx_env: TxEnv = bundle.clone().into();
        tx_env.caller = caller_info.address;
        tx_env.nonce = Some(caller_info.nonce);

        let mut evm = EvmBuilder::default()
            .with_db(evm_db)
            .with_tx_env(tx_env.clone())
            .build();

        let _state_change = evm
            .transact()
            .map_err(|_| SimError::RevmEVMTransactionError(tx_env.clone()))?;

        let result = SimResult::ExecutionResult(BundleOrTransactionResult::Bundle(bundle));

        Ok(result)
    }

    /// simulates a bundle of transactions
    pub fn simulate_composable_bundle(
        &self,
        bundle: Bundle,
        caller_info: CallerInfo
    ) -> Result<SimResult, SimError> {
        let mut evm_db = self.db.clone();
        evm_db.set_state_overrides(caller_info.overrides);

        let mut tx_env: TxEnv = bundle.clone().into();
        tx_env.caller = caller_info.address;
        tx_env.nonce = Some(caller_info.nonce);

        let mut evm = EvmBuilder::default()
            .with_db(evm_db)
            .with_tx_env(tx_env.clone())
            .build();

        let _state_change = evm
            .transact()
            .map_err(|_| SimError::RevmEVMTransactionError(tx_env.clone()))?;

        let result = SimResult::ExecutionResult(BundleOrTransactionResult::MevBundle(bundle));

        Ok(result)
    }

    /// simulates a single tx
    fn simulate_single_tx(
        &self,
        tx_env: TxEnv,
        overrides: HashMap<Address, HashMap<U256, U256>>
    ) -> Result<(ExecutionResult, HashMap<Address, HashMap<U256, U256>>), SimError> {
        let mut evm_db = self.db.clone();
        evm_db.set_state_overrides(overrides);

        let mut evm = EvmBuilder::default()
            .with_db(evm_db)
            .with_tx_env(tx_env)
            .build();

        let tx = evm.tx().clone();
        let _ = match tx.transact_to {
            TransactTo::Call(a) => a,
            TransactTo::Create(_) => return Err(SimError::CallInsteadOfCreateError(tx.clone()))
        };

        let result = evm
            .transact()
            .map_err(|_| SimError::RevmEVMTransactionError(tx.clone()))?;

        let slots = result.state;

        let slots = slots
            .into_iter()
            .map(|(addr, mut account)| {
                account.storage.retain(|_, slot| slot.is_changed());
                let account = account
                    .storage
                    .into_iter()
                    .map(|(k, acc)| (k, acc.present_value()))
                    .collect::<HashMap<_, _>>();
                (addr, account)
            })
            .collect();

        Ok((result.result, slots))
    }
}
/*

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    //use ethers::types::{AccessList, Address, Bytes, NameOrAddress, U256, U64};
    use ethers_core::types::{
        transaction::{eip2718::TypedTransaction, eip2930::AccessList, eip712::TypedData},
        Eip1559TransactionRequest, NameOrAddress, H160
    };
    use hex_literal::hex;
    use serde_json::Value;
    use serial_test::serial;

    use super::*;

    fn init_revm_state() -> RevmState {
        let db = Arc::new(
            reth_db::mdbx::Env::<reth_db::mdbx::WriteMap>::open(
                "/home/data/reth/db".as_ref(),
                reth_db::mdbx::EnvKind::RO,
                None
            )
            .unwrap()
        );

        RevmState::new(db, 5000000000)
    }

    #[test]
    #[serial]
    fn test_decode_eip712() {
        let json = json_eip712();

        let typed_data: TypedData = serde_json::from_value(json).unwrap();
        let eip712 = Eip712(typed_data);

        let decoded_txs = convert_eip712(eip712).unwrap();
        assert_eq!(decoded_txs.len(), 2);

        let tx1 = TxEnv {
            caller:           H160(hex!("0b1e9cd778e3b2aa09beab0887650b8889cdf04b")).into(),
            gas_limit:        1605411621,
            gas_price:        U256::ZERO,
            gas_priority_fee: None,
            transact_to:      TransactTo::Call(
                H160(hex!("CcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC")).into()
            ),
            value:            U256::ZERO,
            data:             revm_primitives::Bytes::default(),
            chain_id:         Some(1),
            nonce:            None,
            access_list:      vec![]
        };

        assert_eq!(
            serde_json::to_value(tx1).unwrap(),
            serde_json::to_value(decoded_txs[0].clone()).unwrap()
        );

        let tx2 = TxEnv {
            caller:           H160(hex!("0b1e9cd778e3b2aa09beab0887650b8889cdf04b")).into(),
            gas_limit:        1605411621,
            gas_price:        U256::ZERO,
            gas_priority_fee: None,
            transact_to:      TransactTo::Call(
                H160(hex!("CcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC")).into()
            ),
            value:            U256::ZERO,
            data:             revm_primitives::Bytes::default(),
            chain_id:         Some(1),
            nonce:            None,
            access_list:      vec![]
        };

        assert_eq!(
            serde_json::to_value(tx2).unwrap(),
            serde_json::to_value(decoded_txs[1].clone()).unwrap()
        );
    }

    #[test]
    #[serial]
    fn test_simulate_single_tx() {
        let state = Arc::new(RwLock::new(init_revm_state()));

        let decoded_tx = convert_typed_tx(&contract_tx());
        let mut state = state.write();
        state.evm.env.tx = decoded_tx.clone();
        state.evm.env.cfg.spec_id = SpecId::SHANGHAI;

        let tx = state.evm.env.tx.clone();

        let contract_address = match tx.transact_to {
            TransactTo::Call(a) => a,
            _ => return
        };

        println!("contract address: {:?}", contract_address);

        assert_eq!(state.slot_changes.get(&contract_address), None);

        /*
                let contract_slots = state
                    .slot_changes
                    .entry(contract_address)
                    .or_insert(HashMap::new());
        */
        //assert_eq!(state.slot_changes.get(&contract_address).unwrap(),
        // &HashMap::new());

        let result = state.evm.transact().unwrap();
        println!("\nRESULT: {:?}\n", result);

        let slots = &result.state.get(&contract_address).unwrap().storage;
        println!("SLOTS: {:?}\n", slots);
    }

    fn convert_typed_tx(tx: &TypedTransaction) -> TxEnv {
        let transact_to = match tx.to_addr() {
            Some(to) => TransactTo::Call(Address::from(*to)),
            None => TransactTo::Create(CreateScheme::Create)
        };

        let tx_env = TxEnv {
            caller: Into::<Address>::into(*tx.from().unwrap()),
            gas_limit: u64::MAX,
            gas_price: U256::ZERO,
            gas_priority_fee: None,
            transact_to,
            value: Into::<U256>::into(*tx.value().unwrap()),
            data: tx
                .data()
                .unwrap_or(&ethers_core::types::Bytes::default())
                .to_vec()
                .into(),
            chain_id: Some(tx.chain_id().unwrap().as_u64()),
            nonce: Some(tx.nonce().unwrap().as_u64()),
            access_list: Vec::new()
        };

        println!("{:?}", tx_env);

        tx_env
    }

    fn contract_tx() -> TypedTransaction {
        let tx_request = Eip1559TransactionRequest {
            from: Some(H160(hex!("4e44b8436bc94c889fe16ecfe1d92036e1b7669b"))),
            to: Some(NameOrAddress::Address(H160(hex!("e592427a0aece92de3edee1f18e0157c05861564")))),
            gas: Some(U256::from_str("0x927c0").unwrap().into()),
            value: Some(U256::from_str("0x38d7ea4c68000").unwrap().into()),
            data: Some(ethers_core::types::Bytes(Bytes::from("0x414bf389000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000ebb82c932759b515b2efc1cfbb6bf2f6dbace40400000000000000000000000000000000000000000000000000000000000027100000000000000000000000004e44b8436bc94c889fe16ecfe1d92036e1b7669b0000000000000000000000000000000000000000000000000000000064FF617300000000000000000000000000000000000000000000000000038d7ea4c6800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"))),
            nonce: Some(U256::from_str("0x69").unwrap().into()),
            access_list: AccessList::default(),
            max_priority_fee_per_gas: Some(U256::from_str("0x2fd0013e7").unwrap().into()),
            max_fee_per_gas: Some(U256::from_str("0x2fd0013e7").unwrap().into()),
            chain_id: Some(ethers_core::types::U64::from_str("0x1").unwrap()),
        };
        tx_request.into()
    }

    fn json_eip712() -> Value {
        serde_json::json!({
        "domain": {
            "name": "ExampleDApp",
            "version": "1",
            "chainId": 1,
            "verifyingContract": "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC"
        },
        "types": {
            "EIP712Domain": [
                { "name": "name", "type": "string" },
                { "name": "version", "type": "string" },
                { "name": "chainId", "type": "uint256" },
                { "name": "verifyingContract", "type": "address" }
            ],
            "UserOrder": [
                { "name": "token_out", "type": "address" },
                { "name": "token_in", "type": "address" },
                { "name": "amount_in", "type": "uint128" },
                { "name": "amount_out_min", "type": "uint128" },
                { "name": "deadline", "type": "uint256" },
                { "name": "gas_cap", "type": "uint256" },
                { "name": "pre_hook", "type": "bytes" },
                { "name": "post_hook", "type": "bytes" }
            ],
            "UserSettlement": [
                { "name": "order", "type": "UserOrder" },
                { "name": "signature", "type": "bytes" },
                { "name": "amount_out", "type": "uint256" },
                { "name": "gas_actual", "type": "uint256" }
            ],
            "SearcherOrder": [
                { "name": "pool", "type": "bytes32" },
                { "name": "token_in", "type": "address" },
                { "name": "token_out", "type": "address" },
                { "name": "amount_in", "type": "uint128" },
                { "name": "amount_out_min", "type": "uint128" },
                { "name": "deadline", "type": "uint256" },
                { "name": "gas_cap", "type": "uint256" },
                { "name": "bribe", "type": "uint256" },
                { "name": "pre_hook", "type": "bytes" },
                { "name": "post_hock", "type": "bytes" }
            ],
            "PoolSettlement": [
                { "name": "pool", "type": "PoolKey" },
                { "name": "token_0_in", "type": "uint256" },
                { "name": "token_1_in", "type": "uint256" }
            ],
            "PoolKey": [
                { "name": "currency_0", "type": "address" },
                { "name": "currency_1", "type": "address" },
                { "name": "fee", "type": "uint32" },
                { "name": "tick_spacing", "type": "uint32" },
                { "name": "hooks", "type": "address" }
            ],
            "PoolFees": [
                { "name": "pool", "type": "PoolKey" },
                { "name": "fees_0", "type": "uint256" },
                { "name": "fees_1", "type": "uint256" }
            ],
            "CurrencySettlement": [
                { "name": "currency", "type": "address" },
                { "name": "amount_net", "type": "int256" }
            ],
            "Bundle": [
                { "name": "swaps", "type": "PoolSwap[]" },
                { "name": "lvr", "type": "LvrSettlement[]" },
                { "name": "users", "type": "UserSettlement[]" },
                { "name": "currencies", "type": "CurrencySettlement[]" },
                { "name": "pools", "type": "PoolFees[]" }
            ],
            "PoolSwap": [
                { "name": "pool", "type": "PoolKey" },
                { "name": "currency_in", "type": "address" },
                { "name": "amount_in", "type": "uint256" }
            ],
            "LvrSettlement": [
                { "name": "order", "type": "SearcherOrder" },
                { "name": "signature", "type": "bytes" },
                { "name": "gas_actual", "type": "uint256" }
            ]
        },
        "primaryType": "Bundle",
            "message": {
                "swaps": [],
                "lvr": [],
                "users": [
                    {
                        "order": {
                            "token_out": "0xaaadF7A763BB3706119671043526A52c3869e42F",
                            "token_in": "0xabadF7A763BB3706119671043526A52c3869e42F",
                            "amount_in": 100,
                            "amount_out_min": 50,
                            "deadline": "0x5FB0A325",
                            "gas_cap": "0x5FB0A325",
                            "pre_hook": "0x5FB0A325",
                            "post_hook": "0x5FB0A325"
                        },
                        "signature": "a8cf1db738b96b728ae230dc8df4c4c1c288beedf969fa8a3a54c142b48208c027b974e765557556e96dc80d6d63ddb44cac551c145de5b23df6e667875b4f7d1c",
                        "amount_out": "0x5FB0A325",
                        "gas_actual": "0x5FB0A325"
                    },
                    {
                        "order": {
                            "token_out": "0xaaadF7A763BB3706119671043526A52c3869e42F",
                            "token_in": "0xabadF7A763BB3706119671043526A52c3869e42F",
                            "amount_in": 10,
                            "amount_out_min": 50,
                            "deadline": "0x5FB0A325",
                            "gas_cap": "0x5FB0A325",
                            "pre_hook": "0x5FB0A325",
                            "post_hook": "0x5FB0A325"
                        },
                        "signature": "a8cf1db738b96b728ae230dc8df4c4c1c288beedf969fa8a3a54c142b48208c027b974e765557556e96dc80d6d63ddb44cac551c145de5b23df6e667875b4f7d1c",
                        "amount_out": "0x5FB0A325",
                        "gas_actual": "0x5FB0A325"
                    }
                ],
                "currencies": [],
                "pools": []
            }
        })
    }
}
*/
