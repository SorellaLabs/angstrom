use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker}
};

use alloy::{
    primitives::{BlockNumber, Bytes},
    providers::Provider
};
use angstrom_eth::manager::EthEvent;
use angstrom_metrics::ConsensusMetricsWrapper;
use angstrom_network::{StromMessage, StromNetworkHandle};
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    consensus::{
        ConsensusRoundName, ConsensusRoundOrderHashes, StromConsensusEvent, SystemTimeSlotClock
    },
    contract_payloads::angstrom::UniswapAngstromRegistry,
    primitive::{AngstromMetaSigner, AngstromSigner},
    sol_bindings::rpc_orders::AttestAngstromBlockEmpty,
    submission::SubmissionHandler
};
use futures::StreamExt;
use matching_engine::MatchingEngineHandle;
use order_pool::order_storage::OrderStorage;
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_tasks::shutdown::GracefulShutdown;
use telemetry_recorder::telemetry_event;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::{
    AngstromValidator, ConsensusDataWithBlock, ConsensusRequest, ConsensusSubscriptionData,
    ConsensusSubscriptionRequestKind, ConsensusTimingConfig,
    leader_selection::WeightedRoundRobin,
    rounds::{ConsensusMessage, RoundStateMachine, SharedRoundState}
};

const MODULE_NAME: &str = "Consensus";

pub struct ConsensusManager<P, Matching, BlockSync, S: AngstromMetaSigner>
where
    P: Provider + Unpin + 'static
{
    current_height:         BlockNumber,
    leader_selection:       WeightedRoundRobin,
    consensus_round_state:  RoundStateMachine<P, Matching, S>,
    canonical_block_stream: UnboundedReceiverStream<EthEvent>,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>,
    network:                StromNetworkHandle,
    block_sync:             BlockSync,
    rpc_rx:                 mpsc::UnboundedReceiver<ConsensusRequest>,
    state_updates:          Option<mpsc::UnboundedSender<ConsensusRoundName>>,
    subscribers:            ConsensusSubscriptionManager,

    /// Track broadcasted messages to avoid rebroadcasting
    broadcasted_messages: HashSet<StromConsensusEvent>
}

impl<P, Matching, BlockSync, S> ConsensusManager<P, Matching, BlockSync, S>
where
    P: Provider + Unpin + 'static,
    BlockSync: BlockSyncConsumer,
    Matching: MatchingEngineHandle,
    S: AngstromMetaSigner
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        netdeps: ManagerNetworkDeps,
        signer: AngstromSigner<S>,
        validators: Vec<AngstromValidator>,
        order_storage: Arc<OrderStorage>,
        deploy_block: BlockNumber,
        current_height: BlockNumber,
        pool_registry: UniswapAngstromRegistry,
        uniswap_pools: SyncedUniswapPools,
        provider: SubmissionHandler<P>,
        matching_engine: Matching,
        block_sync: BlockSync,
        rpc_rx: mpsc::UnboundedReceiver<ConsensusRequest>,
        state_updates: Option<mpsc::UnboundedSender<ConsensusRoundName>>,
        timing_config: ConsensusTimingConfig,
        slot_clock: SystemTimeSlotClock
    ) -> Self {
        let ManagerNetworkDeps { network, canonical_block_stream, strom_consensus_event } = netdeps;
        tracing::info!(?validators, "setting up with validators");
        let mut leader_selection = WeightedRoundRobin::new(validators.clone(), deploy_block);
        let leader = leader_selection.choose_proposer(current_height).unwrap();
        block_sync.register(MODULE_NAME);

        let metrics = ConsensusMetricsWrapper::new();
        metrics.set_block_height(current_height);

        Self {
            strom_consensus_event,
            current_height,
            leader_selection,
            consensus_round_state: RoundStateMachine::new(
                SharedRoundState::<_, _, S>::new(
                    current_height,
                    order_storage,
                    signer,
                    leader,
                    validators.clone(),
                    metrics,
                    pool_registry,
                    uniswap_pools,
                    provider,
                    matching_engine,
                    timing_config,
                    slot_clock.clone()
                ),
                slot_clock
            ),
            rpc_rx,
            state_updates,
            block_sync,
            network,
            canonical_block_stream,
            broadcasted_messages: HashSet::new(),
            subscribers: ConsensusSubscriptionManager::default()
        }
    }

    fn on_blockchain_state(&mut self, event: EthEvent, waker: Waker) {
        tracing::info!("got new block_chain state");

        match event {
            EthEvent::NewBlock(block) => {
                self.current_height = block;

                self.block_sync
                    .sign_off_on_block(MODULE_NAME, self.current_height, Some(waker));
            }
            EthEvent::ReorgedOrders(_, reorg) => {
                self.current_height = *reorg.end();
                self.block_sync
                    .sign_off_reorg(MODULE_NAME, reorg, Some(waker));
            }
            // If this isn't a new block event. we don't wanna reset.
            EthEvent::AddedNode(node) => {
                self.leader_selection.add_validator(node, 100);
                return;
            }
            EthEvent::RemovedNode(node) => {
                self.leader_selection.remove_validator(&node);
                return;
            }
            _ => return
        }

        ConsensusMetricsWrapper::new().set_block_height(self.current_height);
        let round_leader = self
            .leader_selection
            .choose_proposer(self.current_height)
            .unwrap();
        tracing::info!(?round_leader, "selected new round leader");

        self.consensus_round_state
            .reset_round(self.current_height, round_leader);

        // We just reset to BidAggregation so let's make sure to send our listener.
        if let Some(su) = self.state_updates.as_ref() {
            let _ = su.send(ConsensusRoundName::BidAggregation);
        }
        self.broadcasted_messages.clear();
    }

    fn handle_request(&mut self, request: ConsensusRequest) {
        match request {
            ConsensusRequest::CurrentLeader(tx) => {
                let block = self.current_height;
                let _ = tx.send(ConsensusDataWithBlock {
                    data: self.consensus_round_state.current_leader(),
                    block
                });
            }
            ConsensusRequest::CurrentConsensusState(tx) => {
                let block = self.current_height;
                let data = self.leader_selection.get_validator_state();
                let _ = tx.send(ConsensusDataWithBlock { data, block });
            }
            ConsensusRequest::SubscribeAttestations(tx) => {
                self.subscribers
                    .add_subscription(ConsensusSubscriptionRequestKind::Attestations, tx);
            }
            ConsensusRequest::SubscribeRoundEventOrders(tx) => {
                self.subscribers
                    .add_subscription(ConsensusSubscriptionRequestKind::RoundEventOrders, tx);
            }
            ConsensusRequest::Timing(tx) => {
                let block = self.current_height;
                let _ = tx.send(ConsensusDataWithBlock {
                    data: self.consensus_round_state.timing(),
                    block
                });
            }
            ConsensusRequest::IsRoundClosed(tx) => {
                let block = self.current_height;
                let _ = tx.send(ConsensusDataWithBlock {
                    data: self.consensus_round_state.is_auction_closed(),
                    block
                });
            }
        }
    }

    fn on_network_event(&mut self, event: StromConsensusEvent) {
        if self.current_height != event.block_height() {
            tracing::warn!(
                event_block_height=%event.block_height(),
                msg_sender=%event.sender(),
                current_height=%self.current_height,
                "ignoring event for wrong block",
            );
            return;
        }

        if let StromConsensusEvent::BundleUnlockAttestation(_, block, bytes) = &event {
            // verify is correct
            if AttestAngstromBlockEmpty::is_valid_attestation(block + 1, bytes) {
                let data =
                    ConsensusDataWithBlock { data: bytes.clone(), block: self.current_height };
                self.subscribers.subscription_send_attestations(data);
            }
        }

        let block = event.block_height();
        telemetry_event!(block, event.clone());

        self.consensus_round_state.handle_message(event);
    }

    fn on_round_event(&mut self, event: ConsensusMessage) {
        match event.clone() {
            ConsensusMessage::StateChange(state) => {
                // If we have telemetry, record the state change.
                telemetry_event!(self.current_height, state);

                // If we have a state update listener, report the new state.
                if let Some(su) = self.state_updates.as_ref() {
                    let _ = su.send(state);
                }
            }
            ConsensusMessage::PropagateProposal(p) => {
                self.network.broadcast_message(StromMessage::Propose(p))
            }
            ConsensusMessage::PropagatePreProposal(p) => {
                self.network.broadcast_message(StromMessage::PrePropose(p))
            }
            ConsensusMessage::PropagatePreProposalAgg(p) => self
                .network
                .broadcast_message(StromMessage::PreProposeAgg(p)),
            ConsensusMessage::PropagateEmptyBlockAttestation(p) => {
                let data = ConsensusDataWithBlock { data: p.clone(), block: self.current_height };
                self.subscribers.subscription_send_attestations(data);

                self.network
                    .broadcast_message(StromMessage::BundleUnlockAttestation(
                        self.current_height,
                        p
                    ));
            }
        };
        self.subscribers.subscription_send_round_event(event);
    }

    pub async fn run_till_shutdown(mut self, sig: GracefulShutdown) {
        let mut g = None;
        tokio::select! {
            _ = &mut self => {
            }
            cancel = sig => {
                g = Some(cancel);
            }
        }

        // ensure we shutdown properly.
        if g.is_some() {
            self.cleanup().await;
        }

        drop(g);
    }

    /// Currently this doesn't do much. However,
    /// once we start adding slashing. this will be critical in storing
    /// all of our evidence.
    #[allow(unused)]
    async fn cleanup(mut self) {}
}

impl<P, Matching, BlockSync, S> Future for ConsensusManager<P, Matching, BlockSync, S>
where
    P: Provider + Unpin + 'static,
    Matching: MatchingEngineHandle,
    BlockSync: BlockSyncConsumer,
    S: AngstromMetaSigner
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        while let Poll::Ready(Some(msg)) = this.canonical_block_stream.poll_next_unpin(cx) {
            this.on_blockchain_state(msg, cx.waker().clone());
        }

        while let Poll::Ready(Some(data)) = this.rpc_rx.poll_recv(cx) {
            this.handle_request(data);
        }

        if this.block_sync.can_operate() {
            while let Poll::Ready(Some(msg)) = this.strom_consensus_event.poll_next_unpin(cx) {
                this.on_network_event(msg);
            }

            while let Poll::Ready(Some(msg)) = this.consensus_round_state.poll_next_unpin(cx) {
                this.on_round_event(msg);
            }
        }

        Poll::Pending
    }
}

pub struct ManagerNetworkDeps {
    network:                StromNetworkHandle,
    canonical_block_stream: UnboundedReceiverStream<EthEvent>,
    strom_consensus_event:  UnboundedMeteredReceiver<StromConsensusEvent>
}

impl ManagerNetworkDeps {
    pub fn new(
        network: StromNetworkHandle,
        canonical_block_stream: UnboundedReceiverStream<EthEvent>,
        strom_consensus_event: UnboundedMeteredReceiver<StromConsensusEvent>
    ) -> Self {
        Self { network, canonical_block_stream, strom_consensus_event }
    }
}

#[derive(Debug, Default)]
struct ConsensusSubscriptionManager {
    subscribers:
        HashMap<ConsensusSubscriptionRequestKind, Vec<mpsc::Sender<ConsensusSubscriptionData>>>
}

impl ConsensusSubscriptionManager {
    fn add_subscription(
        &mut self,
        kind: ConsensusSubscriptionRequestKind,
        tx: mpsc::Sender<ConsensusSubscriptionData>
    ) {
        self.subscribers.entry(kind).or_default().push(tx);
    }

    fn subscription_send_attestations(&mut self, data: ConsensusDataWithBlock<Bytes>) {
        if let Some(subs) = self
            .subscribers
            .get_mut(&ConsensusSubscriptionRequestKind::Attestations)
        {
            subs.retain(|tx| {
                tx.try_send(ConsensusSubscriptionData::Attestations(data.clone()))
                    .is_ok()
            });
        }
    }

    fn subscription_send_round_event(&mut self, data: ConsensusMessage) {
        if let Some(subs) = self
            .subscribers
            .get_mut(&ConsensusSubscriptionRequestKind::RoundEventOrders)
        {
            let order_hashes = ConsensusRoundOrderHashes {
                limit:    HashSet::from_iter(data.limit_order_hashes()),
                searcher: HashSet::from_iter(data.searcher_order_hashes()),
                round:    data.round_event()
            };
            subs.retain(|tx| {
                tx.try_send(ConsensusSubscriptionData::RoundEventOrders(order_hashes.clone()))
                    .is_ok()
            });
        }
    }
}
