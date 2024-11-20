use std::{
    collections::VecDeque,
    fmt::Debug,
    ops::RangeInclusive,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, RwLock
    },
    task::Waker
};

use dashmap::DashMap;

/// Producer to block syncing events
pub trait BlockSyncProducer: Debug + Clone + Send + Sync + Unpin + 'static {
    /// when a new block is produced. starts new block process
    /// always assume truthful values are passed
    fn new_block(&self, block_number: u64);

    /// when a reorg happens, starts syncing process
    /// always assume truthful values are passed
    fn reorg(&self, reorg_range: RangeInclusive<u64>);

    /// On the first call. disables the ability to add modules to the BlockSync.
    /// this ensures that we can't have race conditions caused by the sign off
    /// set changing.
    fn finalize_modules(&self);
}

/// Consumer to block sync producer
pub trait BlockSyncConsumer: Debug + Clone + Send + Sync + Unpin + 'static {
    fn sign_off_reorg(
        &self,
        module: &'static str,
        block_range: RangeInclusive<u64>,
        waker: Option<Waker>
    );
    fn sign_off_on_block(&self, module: &'static str, block_number: u64, waker: Option<Waker>);
    fn current_block_number(&self) -> u64;
    fn has_proposal(&self) -> bool;
    fn fetch_current_proposal(&self) -> Option<GlobalBlockState>;
    fn register(&self, module_name: &'static str);

    fn can_operate(&self) -> bool {
        !self.has_proposal()
    }
}

/// The global block sync is a global syncing mechanism.
///
/// every module that interacts with blocks and or other modules that are block
/// sensitive are registered with the global block sync. The global block number
/// will only increase when all modules that are registered mark the global sync
/// as ready to increment. Unlike progressing. Any one module is able to abort
/// the process (aborts due to reorgs)
#[derive(Debug, Clone)]
pub struct GlobalBlockSync {
    /// state that we are waiting on all sign offs for
    pending_state:          Arc<RwLock<VecDeque<GlobalBlockState>>>,
    /// the block number
    block_number:           Arc<AtomicU64>,
    /// the modules with there current sign off state for the transition of
    /// pending state -> cur state
    registered_modules:     DashMap<&'static str, VecDeque<SignOffState>>,
    /// Avoids having a module join the set while running. This is to ensure
    /// no race conditions.
    all_modules_registered: Arc<AtomicBool>
}

impl GlobalBlockSync {
    pub fn new(block_number: u64) -> Self {
        Self {
            block_number:           Arc::new(AtomicU64::new(block_number)),
            pending_state:          Arc::new(RwLock::new(VecDeque::with_capacity(2))),
            registered_modules:     DashMap::default(),
            all_modules_registered: AtomicBool::new(false).into()
        }
    }

    fn proper_proposal(&self, proposal: &GlobalBlockState) -> bool {
        self.pending_state.read().unwrap().contains(proposal)
    }
}
impl BlockSyncProducer for GlobalBlockSync {
    fn new_block(&self, block_number: u64) {
        // add to pending state. this will trigger everyone to stop and start dealing
        // with new blocks
        if self.block_number.load(Ordering::SeqCst) + 1 != block_number {
            panic!("already have this block_number");
        }

        self.pending_state
            .write()
            .unwrap()
            .push_back(GlobalBlockState::PendingProgression(block_number));
    }

    fn reorg(&self, reorg_range: RangeInclusive<u64>) {
        self.pending_state
            .write()
            .unwrap()
            .push_back(GlobalBlockState::PendingReorg(reorg_range));
    }

    fn finalize_modules(&self) {
        self.all_modules_registered.store(true, Ordering::SeqCst);
    }
}

impl BlockSyncConsumer for GlobalBlockSync {
    fn sign_off_reorg(
        &self,
        module: &'static str,
        block_range: RangeInclusive<u64>,
        waker: Option<Waker>
    ) {
        // check to see if there is pending state
        if self.pending_state.read().unwrap().is_empty() {
            panic!("{} tried to sign off on a proposal that didn't exist", module);
        }
        // ensure we are signing over equivalent proposals
        if !self.proper_proposal(&GlobalBlockState::PendingReorg(block_range.clone())) {
            panic!("{} tried to sign off on a incorrect proposal", module);
        }

        let check = SignOffState::HandledReorg(waker);

        self.registered_modules
            .entry(module)
            .and_modify(|sign_off_state| {
                sign_off_state.push_back(check.clone());
            });

        let mut transition = true;
        self.registered_modules.iter().for_each(|v| {
            transition &= v.value().front() == Some(&check);
        });

        if transition {
            // was last sign off, pending state -> cur state
            let mut lock = self.pending_state.write().unwrap();
            let new_state = lock
                .pop_front()
                .expect("everyone signed off on a transition proposal that didn't exist");
            drop(lock);

            tracing::info!(handled_state=?new_state, "detected reorg has been handled successfully");

            // reset sign off state
            self.registered_modules.iter_mut().for_each(|mut v| {
                let val = v.value_mut();
                val.pop_front().unwrap().try_wake_task();
            });
        }
    }

    fn sign_off_on_block(&self, module: &'static str, block_number: u64, waker: Option<Waker>) {
        // check to see if there is pending state
        if self.pending_state.read().unwrap().is_empty() {
            panic!("{} tried to sign off on a proposal that didn't exist", module);
        }
        // ensure the block number is cur_block + 1
        if !self.proper_proposal(&GlobalBlockState::PendingProgression(block_number)) {
            panic!("{} tried to sign off on a incorrect proposal", module);
        }

        let check = SignOffState::ReadyForNextBlock(waker);

        self.registered_modules
            .entry(module)
            .and_modify(|sign_off_state| {
                sign_off_state.push_back(check.clone());
            });

        let mut transition = true;
        self.registered_modules.iter().for_each(|v| {
            transition &= v.value().front() == Some(&check);
        });

        if transition {
            let mut lock = self.pending_state.write().unwrap();
            let new_state = lock
                .pop_front()
                .expect("everyone signed off on a transition proposal that didn't exist");
            drop(lock);

            tracing::info!(handled_state=?new_state, "new block has been handled successfully");
            self.block_number.fetch_add(1, Ordering::SeqCst);

            // reset sign off state
            self.registered_modules.iter_mut().for_each(|mut v| {
                let val = v.value_mut();
                val.pop_front().unwrap().try_wake_task();
            });
        }
    }

    #[inline(always)]
    fn can_operate(&self) -> bool {
        !self.has_proposal()
    }

    #[inline(always)]
    fn current_block_number(&self) -> u64 {
        self.block_number.load(Ordering::SeqCst)
    }

    #[inline(always)]
    fn has_proposal(&self) -> bool {
        !self.pending_state.read().unwrap().is_empty()
    }

    #[inline(always)]
    fn fetch_current_proposal(&self) -> Option<GlobalBlockState> {
        self.pending_state.read().unwrap().front().cloned()
    }

    fn register(&self, module_name: &'static str) {
        if self.all_modules_registered.load(Ordering::SeqCst) {
            tracing::warn!(%module_name, "tried to register a module after setting no more modules to true. This module won't be added");
            return
        }

        if self
            .registered_modules
            .insert(module_name, VecDeque::new())
            .is_some()
        {
            panic!("module registered twice with global block sync");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalBlockState {
    /// current block number processing
    Processing(u64),
    /// a block that we need to deal with the reorg for
    PendingReorg(RangeInclusive<u64>),
    /// a new block that all modules need to index
    PendingProgression(u64)
}

#[derive(Debug, Clone)]
pub enum SignOffState {
    /// module has registered that there is a new block and made sure it is up
    /// to date
    ReadyForNextBlock(Option<Waker>),
    /// module has registered that there was a reorg and has appropriately
    /// handled it and is ready to continue processing
    HandledReorg(Option<Waker>)
}

impl SignOffState {
    pub fn try_wake_task(&self) {
        match self {
            Self::ReadyForNextBlock(waker) | Self::HandledReorg(waker) => {
                waker.as_ref().inspect(|w| w.wake_by_ref());
            }
        }
    }
}

impl PartialEq for SignOffState {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::ReadyForNextBlock(_), Self::ReadyForNextBlock(_))
                | (Self::HandledReorg(_), Self::HandledReorg(_))
        )
    }
}

impl Eq for SignOffState {}

#[cfg(test)]
pub mod test {
    use crate::block_sync::{BlockSyncConsumer, BlockSyncProducer, GlobalBlockSync};

    const MOD1: &str = "Sick Module";
    const MOD2: &str = "Sick Module Two";

    #[test]
    fn test_block_progression() {
        let global_sync = GlobalBlockSync::new(10);

        assert!(global_sync.can_operate());
        assert!(!global_sync.has_proposal());

        // register module
        global_sync.register(MOD1);
        global_sync.register(MOD2);

        global_sync.new_block(11);

        assert!(!global_sync.can_operate());
        assert!(global_sync.has_proposal());

        global_sync.sign_off_on_block(MOD1, 11, None);

        assert!(!global_sync.can_operate());
        assert!(global_sync.has_proposal());
        assert!(global_sync.current_block_number() == 10);

        global_sync.sign_off_on_block(MOD2, 11, None);

        assert!(global_sync.can_operate());
        assert!(!global_sync.has_proposal());
        assert!(global_sync.current_block_number() == 11);
    }

    #[test]
    fn test_reorg_progression() {
        let global_sync = GlobalBlockSync::new(10);

        assert!(global_sync.can_operate());
        assert!(!global_sync.has_proposal());

        // register module
        global_sync.register(MOD1);
        global_sync.register(MOD2);
        let reorg_range = 8..=10u64;

        // trigger reorg
        global_sync.reorg(reorg_range.clone());

        assert!(!global_sync.can_operate());
        assert!(global_sync.has_proposal());

        global_sync.sign_off_reorg(MOD1, reorg_range.clone(), None);

        assert!(!global_sync.can_operate());
        assert!(global_sync.has_proposal());
        assert!(global_sync.current_block_number() == 10);

        global_sync.sign_off_reorg(MOD2, reorg_range.clone(), None);

        assert!(global_sync.can_operate());
        assert!(!global_sync.has_proposal());
        assert!(global_sync.current_block_number() == 10);
    }

    #[test]
    fn test_double_proposal_sign_offs() {
        let global_sync = GlobalBlockSync::new(10);

        assert!(global_sync.can_operate());
        assert!(!global_sync.has_proposal());

        // register module
        global_sync.register(MOD1);
        global_sync.register(MOD2);

        global_sync.new_block(11);
        global_sync.reorg(9..=11);

        assert!(!global_sync.can_operate());
        assert!(global_sync.has_proposal());

        global_sync.sign_off_on_block(MOD1, 11, None);
        global_sync.sign_off_reorg(MOD1, 9..=11, None);

        assert!(!global_sync.can_operate());
        assert!(global_sync.has_proposal());
        assert!(global_sync.current_block_number() == 10);

        global_sync.sign_off_on_block(MOD2, 11, None);

        assert!(!global_sync.can_operate());
        assert!(global_sync.has_proposal());
        assert!(global_sync.current_block_number() == 11);

        global_sync.sign_off_reorg(MOD2, 9..=11, None);

        assert!(global_sync.can_operate());
        assert!(!global_sync.has_proposal());
        assert!(global_sync.current_block_number() == 11);
    }

    #[test]
    #[should_panic]
    fn test_block_progression_error() {
        let global_sync = GlobalBlockSync::new(10);

        assert!(global_sync.can_operate());
        assert!(!global_sync.has_proposal());

        // register module
        global_sync.register(MOD1);
        global_sync.register(MOD2);

        global_sync.new_block(11);

        assert!(!global_sync.can_operate());
        assert!(global_sync.has_proposal());

        global_sync.sign_off_on_block(MOD1, 12, None);
    }

    #[test]
    #[should_panic]
    fn test_reorg_progression_errors() {
        let global_sync = GlobalBlockSync::new(10);

        assert!(global_sync.can_operate());
        assert!(!global_sync.has_proposal());

        // register module
        global_sync.register(MOD1);
        global_sync.register(MOD2);

        global_sync.reorg(8..=10u64);

        assert!(!global_sync.can_operate());
        assert!(global_sync.has_proposal());

        global_sync.sign_off_reorg(MOD1, 10..=12u64, None);
    }
}
