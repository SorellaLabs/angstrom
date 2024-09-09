use std::{
    pin::Pin,
    sync::{Arc, Mutex}
};

use angstrom_metrics::ConsensusMetricsWrapper;
use angstrom_network::{manager::StromConsensusEvent, StromMessage, StromNetworkHandle};
use angstrom_types::{
    consensus::{PreProposal, Proposal},
    orders::PoolSolution
};
use futures::{FutureExt, Stream, StreamExt};
use matching_engine::MatchingManager;
use order_pool::{order_storage::OrderStorage, timer::async_time_fn};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_provider::{CanonStateNotification, CanonStateNotifications};
use reth_tasks::TaskSpawner;
use tokio::{
    select,
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinSet
};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tracing::{error, warn};

use crate::{
    cache::ProposalCache,
    global::GlobalConsensusState,
    round::{Leader, RoundState, RoundStateTimings},
    round_robin_algo::RoundRobinAlgo,
    signer::Signer,
    ConsensusListener, ConsensusMessage, ConsensusState, ConsensusUpdater
};

enum ConsensusTaskResult {
    BuiltProposal(Proposal),
    ValidationSolutions { height: u64, solutions: Vec<PoolSolution> }
}

pub async fn build_proposal_task(
    preproposals: Vec<PreProposal>
) -> Result<Vec<PoolSolution>, String> {
    let matcher = MatchingManager {};
    matcher.build_proposal(preproposals).await
}

#[allow(unused)]
pub struct ConsensusManager {
    // core: ConsensusCore,
    /// keeps track of the current round state
    roundstate:             RoundState,
    globalstate:            Arc<Mutex<GlobalConsensusState>>,
    command:                ReceiverStream<ConsensusCommand>,
    subscribers:            Vec<Sender<ConsensusMessage>>,
    /// Used to trigger new consensus rounds
    canonical_block_stream: BroadcastStream<CanonStateNotification>,
    /// events from the network,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>,
    network:                StromNetworkHandle,

    roundrobin:    RoundRobinAlgo,
    signer:        Signer,
    order_storage: Arc<OrderStorage>,
    cache:         ProposalCache,
    tasks:         JoinSet<ConsensusTaskResult>,
    metrics:       ConsensusMetricsWrapper
}

pub struct ManagerNetworkDeps {
    network:                StromNetworkHandle,
    canonical_block_stream: CanonStateNotifications,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>,
    tx:                     Sender<ConsensusCommand>,
    rx:                     Receiver<ConsensusCommand>
}

impl ManagerNetworkDeps {
    pub fn new(
        network: StromNetworkHandle,
        canonical_block_stream: CanonStateNotifications,
        strom_consensus_event: UnboundedMeteredReceiver<StromConsensusEvent>,
        tx: Sender<ConsensusCommand>,
        rx: Receiver<ConsensusCommand>
    ) -> Self {
        Self { network, canonical_block_stream, strom_consensus_event, tx, rx }
    }
}

impl ConsensusManager {
    pub fn new(
        globalstate: Arc<Mutex<GlobalConsensusState>>,
        netdeps: ManagerNetworkDeps,
        signer: Signer,
        order_storage: Arc<OrderStorage>,
        timings: Option<RoundStateTimings>
    ) -> Self {
        let ManagerNetworkDeps {
            network, canonical_block_stream, strom_consensus_event, rx, ..
        } = netdeps;
        let stream = ReceiverStream::new(rx);

        // This is still a lot of stuff to track that we don't necessarily have to worry
        // about
        let timings = timings.unwrap_or_else(|| RoundStateTimings::new(6, 2, 6));
        let roundstate = RoundState::new(0, 1, Leader::default(), Some(timings));
        let wrapped_broadcast_stream = BroadcastStream::new(canonical_block_stream);
        // Use our default round robin algo
        let (roundrobin, _cacheheight) = RoundRobinAlgo::new();
        Self {
            strom_consensus_event,
            roundstate,
            globalstate,
            subscribers: Vec::new(),
            command: stream,
            network,
            roundrobin,
            signer,
            order_storage,
            tasks: JoinSet::new(),
            cache: ProposalCache::new(),
            canonical_block_stream: wrapped_broadcast_stream,
            metrics: ConsensusMetricsWrapper::new()
        }
    }

    pub fn spawn<TP: TaskSpawner>(
        tp: TP,
        globalstate: Arc<Mutex<GlobalConsensusState>>,
        netdeps: ManagerNetworkDeps,
        signer: Signer,
        order_storage: Arc<OrderStorage>,
        timings: Option<RoundStateTimings>
    ) -> ConsensusHandle {
        let tx = netdeps.tx.clone();
        let manager = ConsensusManager::new(globalstate, netdeps, signer, order_storage, timings);
        let fut = manager.message_loop().boxed();
        // let fut = manager_thread(globalstate, netdeps, signer, order_storage,
        // timings).boxed();
        tp.spawn_critical("consensus", fut);

        ConsensusHandle { sender: tx }
    }

    fn send_preproposal(&mut self) {
        let orders = self.order_storage.get_all_orders();
        let preproposal = PreProposal::new(
            self.roundstate.current_height(),
            &self.signer.key,
            alloy_primitives::FixedBytes::default(),
            orders
        );
        tracing::info!("Sending out preproposal");
        self.network
            .broadcast_tx(StromMessage::PrePropose(preproposal.clone()));
        self.broadcast(ConsensusMessage::PrePropose(preproposal));
    }

    /// Start the Build Proposal job to produce a signed Proposal out of this
    /// round's preproposals
    fn start_build_proposal(&mut self) {
        let preproposals = self.roundstate.get_preproposals();
        let signer = self.signer.clone();
        let ethereum_block = self.roundstate.current_height();
        let metrics = self.metrics.clone();
        let build_handle = self.tasks.spawn(async move {
            let (res, timer) = async_time_fn(|| async move {
                let solutions = build_proposal_task(preproposals.clone()).await.unwrap();
                let proposal = signer.sign_proposal(ethereum_block, preproposals, solutions);
                ConsensusTaskResult::BuiltProposal(proposal)
            })
            .await;
            metrics.set_proposal_build_time(ethereum_block, timer);
            res
        });
        self.roundstate.on_proposal(build_handle);
    }

    /// Start to verify our proposal.  This method will return `false` if the
    /// proposal fails signature verification.  Presuming signature verification
    /// is passed, this proposal will be stored in our Proposal Store and a task
    /// will be launched to verify the solutions produced within the proposal
    fn start_verify_proposal(&mut self, proposal: &Proposal) -> bool {
        if proposal.validate() {
            let preproposals = proposal.preproposals().clone();
            let height = proposal.ethereum_height;
            let metrics = self.metrics.clone();
            self.tasks.spawn(async move {
                let (res, timer) = async_time_fn(|| async move {
                    let solutions = build_proposal_task(preproposals).await.unwrap();
                    ConsensusTaskResult::ValidationSolutions { height, solutions }
                })
                .await;
                metrics.set_proposal_verification_time(height, timer);
                res
            });
            // We've passed basic validation so we can return true here
            return true
        }
        false
    }

    /// Send a built proposal out over the network
    fn broadcast_proposal(&mut self, proposal: Proposal) {
        self.network
            .broadcast_tx(StromMessage::Propose(proposal.clone()));
        self.broadcast(ConsensusMessage::Proposal(proposal));
    }

    fn on_command(&mut self, command: ConsensusCommand) {
        match command {
            ConsensusCommand::Subscribe(sender) => self.subscribers.push(sender)
        }
    }

    /// Respond to the messages our spawned tasks pass back when complete
    fn on_task_complete(&mut self, task_result: ConsensusTaskResult) {
        match task_result {
            ConsensusTaskResult::ValidationSolutions { height, solutions } => {
                // Get the proposal for that height
                let Some(proposal) = self.cache.get(height) else {
                    error!(
                        "Built solutions for a proposal that we don't have in our cache: {}",
                        height
                    );
                    return;
                };

                // Match the solutions
                if proposal.solutions != solutions {
                    warn!("Proposal for {} failed validation with non-matching signatures", height);
                    return
                }
                // Prepare our commit messge
                let commit = self.signer.sign_commit(proposal);

                // Then, broadcast our Commit message to all local subscribers and the network
                self.network
                    .broadcast_tx(StromMessage::Commit(Box::new(commit.clone())));
                self.broadcast(ConsensusMessage::Commit(Box::new(commit)));
            }
            ConsensusTaskResult::BuiltProposal(p) => {
                self.broadcast_proposal(p);
            }
        }
    }

    fn on_new_globalstate(&mut self, new_state: ConsensusState) {
        // First let's update our global state
        match self.globalstate.lock() {
            Ok(mut lock) => {
                lock.set_state(new_state);
            }
            Err(_) => {
                tracing::error!("Global state mutex poisoned!");
            }
        }
        // Depending on what state we moved into, we might have some work to do
        match new_state {
            // Send out our preproposal if we entered PreProposal state
            ConsensusState::PreProposal => self.send_preproposal(),
            // Send out our Proposal if we're the leader and we entered Proposal state
            ConsensusState::Proposal => {
                if self.roundstate.is_leader() {
                    // This now sits and waits on a pretty long computational task - do we want the
                    // Consensus Manager thread to be effectively blocked on awaiting this or do we
                    // want to spawn it and figure out how to sync it back up here presuming it
                    // completes in time?
                    self.start_build_proposal()
                }
            }
            _ => ()
        }
    }

    fn on_blockchain_state(&mut self, notification: CanonStateNotification) {
        // Get the newest block height
        let new_block = notification.tip();
        let new_block_height = new_block.block.number;

        if self.roundstate.current_height() + 1 == new_block_height {
            // We should immediately start a new round and drop our current round
            let new_leader = self.roundrobin.on_new_block(&new_block.block);
            // TODO:  Figure out the best way to get our node count
            let new_round_state =
                RoundState::new(new_block_height, 1, new_leader, Some(self.roundstate.timings()));
            // Update our round state to the new round state
            self.roundstate = new_round_state;
            // Update the global state to show that we're back in OrderAccumulation
            self.on_new_globalstate(ConsensusState::OrderAccumulation);
            // Broadcast that we have a new block
            self.broadcast(ConsensusMessage::NewBlock(new_block_height));
        } else {
            // We should figure out what to do here....while we're testing let's make this a
            // panic
            tracing::error!("Block height is not sequential, this breaks round robin!");
            panic!("Unrecoverable consensus error - Block height not sequential");
        }
    }

    fn on_network_event(&mut self, event: StromConsensusEvent) {
        match event {
            // A PrePropose message is a set of orders from a node, signed by that node.  If
            // we're the leader, we should be holding on to it because we need to collect at
            // least 2/3rds of these in order to make a Proposal.  If we're not the leader,
            // do we need to hold onto this?
            StromConsensusEvent::PrePropose(peer, pre_proposal) => {
                // When I get a prepropose from another peer we should do a few things
                // Validate the signature for the prepropose
                // Validate that the prepropose is for the current block and we're in prepropose
                // state Store the prepropose as part of the current round state
                // Rebroadcast the prepropose to all other hosts?  Maybe...very chatty.
                // We should now check to see if we have gotten enough pre-proposals to step
                // forward
                let _ = self.roundstate.on_pre_propose(peer, pre_proposal.clone());

                // Alert all subscribers to our preproposal received
                self.broadcast(ConsensusMessage::PrePropose(pre_proposal))
            }
            // A Proposal is a backwards-looking data packet that contains all the information
            // needed to validate the actions of the leader in a previous round.  We should
            // validate this information and send out a commit ASAP when we can.
            StromConsensusEvent::Propose(_peer, proposal) => {
                // Put the proposal in the cache
                self.cache.set(proposal.ethereum_height, proposal.clone());

                // Start a verification task
                if !self.start_verify_proposal(&proposal) {
                    warn!("Proposal failed verification with invalid signatures");
                }
            }
            // A Commit message is the validation of a previous proposal by another host.  Right
            // now I'm not sure what we should do with this but overall SOMEONE has to collect
            // these for slashing purposes, no?
            StromConsensusEvent::Commit(peer, mut commit) => {
                let block_height = commit.block_height;
                // Validate the commit itself - not currently checked
                commit.validate(&[]);
                if commit.signed_by(self.signer.validator_id) {
                    // Do nothing I suppose, we're good.  Maybe we want to
                    // rebroadcast?
                } else {
                    // Add our signature to the commit and broadcast
                    commit.add_signature(self.signer.validator_id, &self.signer.bls_key);
                    // Broadcast to our local subscribers
                    self.broadcast(ConsensusMessage::Commit(commit.clone()));
                    // Also broadcast to the Strom network
                    self.network
                        .broadcast_tx(angstrom_network::StromMessage::Commit(commit.clone()));
                    // Store it locally as a commit we've seen
                    let _ = self.roundstate.on_commit(peer, *commit.clone());
                    // Broadcast to all our internal subscribers
                    self.broadcast(ConsensusMessage::Commit(commit));
                }
                self.metrics.set_commit_time(block_height);
            }
        }
    }

    fn broadcast(&mut self, msg: ConsensusMessage) {
        self.subscribers
            .retain_mut(|sub| sub.try_send(msg.clone()).is_ok());
    }

    pub async fn message_loop(mut self) {
        // Start message loop
        loop {
            select! {
                Some(msg) = self.tasks.join_next() => {
                    match msg {
                        Ok(task_result) => self.on_task_complete(task_result),
                        Err(e) => {
                            // Don't log an error if we cancelled the task, that error is expected
                            if !e.is_cancelled() {
                                tracing::error!("Task error: {}", e)
                            }
                        }
                    }
                },
                Some(msg) = self.command.next() => {
                    self.on_command(msg);
                },
                Some(msg) = self.canonical_block_stream.next() => {
                    match msg {
                        Ok(notification) => self.on_blockchain_state(notification),
                        Err(e) => tracing::error!("Error receiving chain state notification: {}", e)
                    };
                    self.metrics.set_block_height(self.roundstate.current_height());
                },
                Some(msg) = self.strom_consensus_event.next() => {
                    self.on_network_event(msg);
                },
                Some(msg) = self.roundstate.next() => {
                    self.on_new_globalstate(msg);
                }
            }
        }
    }
}

pub enum ConsensusCommand {
    Subscribe(Sender<ConsensusMessage>)
}

#[derive(Debug, Clone)]
pub struct ConsensusHandle {
    pub sender: Sender<ConsensusCommand>
}

impl ConsensusUpdater for ConsensusHandle {
    fn new_block(&self, _block: ()) {
        todo!()
    }
}

impl ConsensusListener for ConsensusHandle {
    fn subscribe_messages(&self) -> Pin<Box<dyn Stream<Item = ConsensusMessage>>> {
        let (tx, rx) = channel(10);
        let _ = self.sender.try_send(ConsensusCommand::Subscribe(tx));

        Box::pin(ReceiverStream::new(rx))
    }
}

#[cfg(test)]
mod tests {

    use std::{
        collections::HashSet,
        sync::{Arc, Mutex}
    };

    use alloy_primitives::FixedBytes;
    use angstrom_types::{consensus::Proposal, sol_bindings::grouped_orders::GroupedUserOrder};
    use order_pool::{order_storage::OrderStorage, PoolConfig};
    use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
    use testing_tools::{
        mocks::network_events::MockNetworkHandle,
        type_generator::consensus::{generate_limit_order_distribution, generate_random_proposal}
    };
    use tokio::sync::mpsc::{channel, unbounded_channel};
    use tokio_stream::StreamExt;

    use crate::{
        manager::ConsensusTaskResult, round::RoundStateTimings, ConsensusManager, ConsensusMessage,
        ConsensusState, GlobalConsensusState, ManagerNetworkDeps, Signer
    };

    fn mock_net_deps() -> ManagerNetworkDeps {
        let (_, network, ..) = MockNetworkHandle::new();
        let (_, strom_consensus_receiver) = unbounded_channel();
        let strom_consensus_event =
            UnboundedMeteredReceiver::new(strom_consensus_receiver, "mock strom handle");
        let (tx, rx) = channel(100);
        let (_, canon_state_rx) = tokio::sync::broadcast::channel(100);
        ManagerNetworkDeps::new(network, canon_state_rx, strom_consensus_event, tx, rx)
    }

    #[tokio::test]
    async fn can_be_spawned() {
        let globalstate = Arc::new(Mutex::new(GlobalConsensusState::default()));
        let netdeps = mock_net_deps();
        let order_storage = Arc::new(OrderStorage::default());
        let manager =
            ConsensusManager::new(globalstate, netdeps, Signer::default(), order_storage, None);
        let thread = tokio::spawn(manager.message_loop());
        thread.abort();
    }

    #[tokio::test]
    async fn builds_preproposal() {
        let globalstate = Arc::new(Mutex::new(GlobalConsensusState::default()));
        let netdeps = mock_net_deps();
        let poolconfig = PoolConfig { ids: vec![10], ..Default::default() };
        let order_storage = Arc::new(OrderStorage::new(&poolconfig));
        // Let's make some orders to go in our order storage
        let orders = generate_limit_order_distribution(100, 10, 0);
        let in_ord: HashSet<FixedBytes<32>> = orders.iter().map(|i| i.hash()).collect();
        for o in orders {
            let lim = o
                .try_map_inner(|inner| Ok(GroupedUserOrder::Vanilla(inner)))
                .unwrap();
            order_storage.add_new_limit_order(lim).unwrap();
        }
        let timings = RoundStateTimings::default();
        let mut manager = ConsensusManager::new(
            globalstate,
            netdeps,
            Signer::default(),
            order_storage,
            Some(timings)
        );
        let (tx, mut rx) = channel(100);
        manager.on_command(crate::ConsensusCommand::Subscribe(tx));
        let new_state = manager.roundstate.next().await.unwrap();
        // We shift to PreProposal state and should have sent out a PreProposal
        assert!(matches!(new_state, ConsensusState::PreProposal));
        manager.on_new_globalstate(new_state);
        let preproposal = rx.recv().await.unwrap();
        let ConsensusMessage::PrePropose(p) = preproposal else {
            panic!("Didn't get preproposal message");
        };
        let out_ord: HashSet<FixedBytes<32>> = p.limit.iter().map(|i| i.hash()).collect();
        assert!(in_ord == out_ord, "Orders missing from preproposal");
    }

    #[tokio::test]
    async fn verifies_proposal() {
        let globalstate = Arc::new(Mutex::new(GlobalConsensusState::default()));
        let netdeps = mock_net_deps();
        let poolconfig = PoolConfig { ids: vec![10], ..Default::default() };
        let order_storage = Arc::new(OrderStorage::new(&poolconfig));
        let timings = RoundStateTimings::default();
        let mut manager = ConsensusManager::new(
            globalstate,
            netdeps,
            Signer::default(),
            order_storage,
            Some(timings)
        );
        let (tx, mut rx) = channel(100);
        manager.on_command(crate::ConsensusCommand::Subscribe(tx));
        let proposal: Proposal = generate_random_proposal(100, 1, 10);
        manager
            .cache
            .set(proposal.ethereum_height, proposal.clone());
        manager.start_verify_proposal(&proposal);
        assert!(!manager.tasks.is_empty(), "Verify proposal task not started!");
        let res = manager.tasks.join_next().await.unwrap().unwrap();
        let ConsensusTaskResult::ValidationSolutions { ref solutions, .. } = res else {
            panic!("Got some other task result");
        };
        assert!(*solutions == proposal.solutions, "Solutions don't match!");
        manager.on_task_complete(res);
        let commit = rx.recv().await.unwrap();
        let ConsensusMessage::Commit(c) = commit else {
            panic!("Didn't get commit message");
        };
        assert!(
            c.validate(&[manager.signer.bls_key.public_key()]),
            "Unable to validate generated commit"
        );
    }
}
