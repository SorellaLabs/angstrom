use std::{collections::HashMap, sync::Arc, task::Poll};

use common::PollExt;
use ethers_core::abi::Bytes;
use futures_util::{stream::FuturesUnordered, Future, FutureExt, StreamExt};
use reth_provider::StateProviderFactory;
use revm_primitives::{Address, Bytecode};
use tokio::{runtime::Handle, sync::mpsc::UnboundedReceiver, task::JoinHandle};

use crate::{
    bundle::{
        errors::{SimError, SimResult},
        BundleSimRequest
    },
    common::{
        executor::{TaskKind, ThreadPool},
        lru_db::RevmLRU,
        slot_keeper::SlotKeeper,
        state::{AddressSlots, RevmState}
    }
};

///TODO: replace once settled
const V4_BYTE_CODE: Bytes = vec![];
///TODO: replace once settled
const ANGSTROM_ADDRESS: Address = Address::ZERO;

/// revm state handler
pub struct Revm<DB> {
    transaction_rx: UnboundedReceiver<BundleSimRequest>,
    threadpool:     ThreadPool,
    state:          Arc<RevmState<DB>>,
    slot_changes:   AddressSlots,
    slot_keeper:    SlotKeeper<DB>,
    futures:        FuturesUnordered<JoinHandle<Option<AddressSlots>>>
}

impl<DB> Revm<DB>
where
    DB: StateProviderFactory + Send + Sync + Clone + Unpin + 'static
{
    pub fn new(
        transaction_rx: UnboundedReceiver<BundleSimRequest>,
        db: RevmLRU<DB>
    ) -> Result<Self, SimError> {
        let threadpool = ThreadPool::new()?;
        Ok(Self {
            slot_keeper: SlotKeeper::new(db.clone(), vec![], threadpool.runtime.handle().clone()),
            transaction_rx,
            threadpool,
            state: Arc::new(RevmState::new(db)),
            slot_changes: HashMap::new(),
            futures: FuturesUnordered::new()
        })
    }

    pub fn get_threadpool_handle(&self) -> Handle {
        self.threadpool.runtime.handle().clone()
    }

    fn update_slots(&mut self, touched_slots: AddressSlots) {
        for (addr, t_slots) in touched_slots.into_iter() {
            let slot = self
                .slot_changes
                .entry(addr)
                .or_insert_with(|| HashMap::new());
            for (key, val) in t_slots.into_iter() {
                slot.insert(key, val);
            }
        }
    }

    /// handles incoming transactions from clients
    fn handle_incoming_event(&mut self, tx_type: BundleSimRequest) {
        let state = self.state.clone();

        match tx_type {
            BundleSimRequest::Hook(data, overrides, sender) => {
                let slots = self.slot_keeper.get_current_slots().clone();
                let fut = async move {
                    let res = state.simulate_external_state(data, overrides, slots);

                    match res {
                        Ok((sim_res, slots)) => {
                            let _ = sender.send(sim_res);
                            Some(slots)
                        }
                        Err(e) => {
                            let _ = sender.send(SimResult::SimError(e));
                            None
                        }
                    }
                };

                self.futures.push(self.threadpool.spawn_return_task_as(fut));
            }
            BundleSimRequest::UniswapV4(tx, sender) => {
                let fut = async move {
                    let mut map = HashMap::new();

                    let bytecode = Bytecode {
                        bytecode: Bytes::from(V4_BYTE_CODE).into(),
                        ..Default::default()
                    };

                    map.insert(ANGSTROM_ADDRESS, bytecode);
                    let _ = match state.simulate_v4_tx(tx, map) {
                        Ok(res) => sender.send(res),
                        Err(err) => sender.send(SimResult::SimError(err))
                    };
                };

                let _ = self.threadpool.spawn_task_as(fut, TaskKind::Blocking);
            }
            BundleSimRequest::Bundle(tx, caller_info, sender) => {
                let fut = async move {
                    let res = state.simulate_vanilla_bundle(tx, caller_info);
                    let _ = if let Err(e) = res {
                        sender.send(SimResult::SimError(e))
                    } else {
                        sender.send(res.unwrap())
                    };
                };
                let _ = self.threadpool.spawn_task_as(fut, TaskKind::Blocking);
            }
            BundleSimRequest::MevBundle(tx, caller_info, sender) => {
                let fut = async move {
                    let res = state.simulate_composable_bundle(tx, caller_info);
                    let _ = if let Err(e) = res {
                        sender.send(SimResult::SimError(e))
                    } else {
                        sender.send(res.unwrap())
                    };
                };
                let _ = self.threadpool.spawn_task_as(fut, TaskKind::Blocking);
            }
            BundleSimRequest::NewBlock(sender) => {
                let slot_changes = self.slot_changes.clone();
                let fut = async move {
                    let res = RevmState::update_evm_state(state, &slot_changes);
                    let _ = if let Err(e) = res {
                        sender.send(SimResult::SimError(e))
                    } else {
                        sender.send(SimResult::SuccessfulRevmBlockUpdate)
                    };
                };
                let _ = self.threadpool.block_on_rt(fut);
                self.slot_changes.clear();
            }
        };
    }

    // this will be wired into new block
    #[allow(unused)]
    fn handle_new_pools(&mut self, pools: Vec<Address>) {
        self.slot_keeper.new_addresses(pools);
    }
}

impl<DB> Future for Revm<DB>
where
    DB: StateProviderFactory + Send + Sync + Clone + Unpin + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while let Poll::Ready(poll_tx) = self
            .transaction_rx
            .poll_recv(cx)
            .map(|t| t.map(|tx| self.handle_incoming_event(tx)))
        {
            if poll_tx.is_none() {
                return Poll::Ready(())
            }
        }

        while self
            .futures
            .poll_next_unpin(cx)
            .filter_map(|f| f.transpose().ok().flatten().flatten())
            .apply(|slot| self.update_slots(slot))
        {}

        self.slot_keeper.poll_unpin(cx)
    }
}
