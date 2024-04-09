use angstrom_network::{
    NetworkOrderEvent, StromNetworkEvent, StromNetworkHandle, StromNetworkHandleMsg
};
use angstrom_types::orders::PooledOrder;
use reth_metrics::common::mpsc::{
    metered_unbounded_channel, UnboundedMeteredReceiver, UnboundedMeteredSender
};
use reth_rpc_types::PeerId;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

pub struct MockNetworkHandle {
    /// recieves from the strom network handle
    pub from_handle_rx: UnboundedReceiver<StromNetworkHandleMsg>,
    /// sender for network event
    pub network_event:  UnboundedSender<StromNetworkEvent>,
    /// sender for orders
    pub order_sender:   UnboundedMeteredSender<NetworkOrderEvent>
}
impl MockNetworkHandle {
    pub fn new() -> (
        Self,
        StromNetworkHandle,
        UnboundedReceiverStream<StromNetworkEvent>,
        UnboundedMeteredReceiver<NetworkOrderEvent>
    ) {
        let (network_tx, network_rx) = unbounded_channel();
        let (order_tx, order_rx) = metered_unbounded_channel("orders");
        let (handle_tx, handle_rx) = unbounded_channel();

        let network = StromNetworkHandle::new(
            Default::default(),
            UnboundedMeteredSender::new(handle_tx, "mock strom handle")
        );

        (
            Self {
                network_event:  network_tx,
                order_sender:   order_tx,
                from_handle_rx: handle_rx
            },
            network,
            network_rx.into(),
            order_rx
        )
    }

    pub fn connect_peer(&self, peer_id: PeerId) {
        self.network_event
            .send(StromNetworkEvent::PeerAdded(peer_id))
            .expect("failed to add peer");
    }

    pub fn send_orders_from_peers(&self, peer_id: PeerId, orders: Vec<PooledOrder>) {
        self.order_sender
            .send(NetworkOrderEvent::IncomingOrders { peer_id, orders })
            .expect("failed to send orders");
    }

    pub fn respsond_to_strom_message_handle(&mut self) {
        let mut messages = Vec::new();
        while let Ok(next) = self.from_handle_rx.try_recv() {
            messages.push(next);
        }

        for message in messages {
            match message {
                StromNetworkHandleMsg::DisconnectPeer(id, _) => {
                    self.network_event
                        .send(StromNetworkEvent::PeerRemoved(id))
                        .unwrap();
                }
                _ => {
                    tracing::debug!("not handling varient");
                }
            }
        }
    }
}
