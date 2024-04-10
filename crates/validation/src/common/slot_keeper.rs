use std::{
    collections::HashMap,
    pin::Pin,
    task::{Context, Poll}
};

use alloy_primitives::keccak256;
use alloy_sol_types::SolCall;
use angstrom_types::primitive::ERC20;
use futures::Future;
use futures_util::FutureExt;
use reth_primitives::revm_primitives::{Address, ExecutionResult, TransactTo, TxEnv, U256};
use reth_provider::StateProviderFactory;
use reth_revm::{revm::Evm, Database, DatabaseRef, EvmBuilder};
use tokio::{runtime::Handle, task::JoinHandle};

use crate::common::lru_db::RevmLRU;

pub struct SlotKeeper<DB> {
    addresses: Vec<Address>,
    slots:     HashMap<Address, U256>,
    db:        RevmLRU<DB>,
    handle:    Handle,
    fut:       Option<JoinHandle<HashMap<Address, U256>>>
}

impl<DB> SlotKeeper<DB>
where
    DB: StateProviderFactory + Send + Sync + Clone + 'static
{
    pub fn new(db: RevmLRU<DB>, addresses: Vec<Address>, handle: Handle) -> Self {
        let slots = SlotKeeper::get_slots(addresses.clone(), db.clone());

        Self { addresses, db, slots, handle, fut: None }
    }

    pub fn get_current_slots(&self) -> &HashMap<Address, U256> {
        &self.slots
    }

    pub fn new_addresses(&mut self, address: Vec<Address>) {
        let clone_addr = address.clone();
        let db = self.db.clone();

        self.fut = Some(
            self.handle
                .spawn(async move { Self::get_slots(clone_addr, db) })
        );

        self.addresses.extend(address);
    }

    pub fn get_slots(addresses: Vec<Address>, db: RevmLRU<DB>) -> HashMap<Address, U256> {
        // this is pretty big. would be nice to breakup
        addresses
            .iter()
            .map(|token_addr| {
                let mut call = ERC20::balanceOfCall::default();
                call._owner = Address::ZERO;

                let mut tx_env = TxEnv::default();
                tx_env.data = call.abi_encode().into();
                tx_env.transact_to = TransactTo::Call(*token_addr);

                let prob_value = U256::from_limbs([u64::MAX, u64::MAX, u64::MAX, u64::MAX]);
                let search_addr = Address::ZERO;

                for i in 0..100 {
                    let mut user_addr_encoded = search_addr.to_vec();
                    user_addr_encoded.extend(U256::from(i).to_be_bytes::<32>().to_vec());

                    let user_balance_slot = U256::from_be_bytes(*keccak256(user_addr_encoded));

                    let mut slot = HashMap::new();
                    slot.insert(user_balance_slot, prob_value);
                    let mut overrides = HashMap::new();
                    overrides.insert(*token_addr, slot);

                    let mut db = db.clone();
                    db.set_state_overrides(overrides);
                    let mut evm = EvmBuilder::default()
                        .with_db(db)
                        .with_tx_env(tx_env.clone())
                        .build();

                    // this is just a balance_of call. should never fail
                    let output = match evm.transact().unwrap().result {
                        ExecutionResult::Success { output, .. } => output.into_data(),
                        _ => unreachable!()
                    };

                    // bad but needed to convert directly. ideally we can remove this
                    let result = U256::from_be_bytes(unsafe {
                        *(output.to_vec().as_slice() as *const _ as *mut [u8; 32])
                    });

                    if U256::MAX == result {
                        return ((*token_addr), U256::from(i))
                    }
                }
                unreachable!()
            })
            .collect()
    }
}

impl<DB: Send + Sync + Unpin + 'static> Future for SlotKeeper<DB> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(mut fut) = self.fut.take() {
            if let Poll::Ready(res) = fut.poll_unpin(cx) {
                let Ok(res) = res else { return Poll::Ready(()) };
                self.slots.extend(res);
            } else {
                self.fut = Some(fut)
            }
        }

        Poll::Pending
    }
}
