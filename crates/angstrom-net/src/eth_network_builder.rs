use std::time::Duration;

use reth::{chainspec::ChainSpec, transaction_pool::TransactionPool};
use reth_eth_wire::BasicNetworkPrimitives;
use reth_network::{NetworkHandle, NetworkManager, PeersInfo, protocol::IntoRlpxSubProtocol};
use reth_node_builder::{
    BuilderContext, NodeTypes, PrimitivesTy, TxTy, components::NetworkBuilder, node::FullNodeTypes
};
use reth::primitives::EthPrimitives;
use reth_transaction_pool::{PoolPooledTx, PoolTransaction};

/// A basic ethereum payload service.
pub struct AngstromNetworkBuilder<I: IntoRlpxSubProtocol + Send> {
    custom_protocol: I
}

impl<I: IntoRlpxSubProtocol + Send> AngstromNetworkBuilder<I> {
    pub fn new(protocol: I) -> Self {
        Self { custom_protocol: protocol }
    }
}

impl<Node, Pool, I> NetworkBuilder<Node, Pool> for AngstromNetworkBuilder<I>
where
    I: IntoRlpxSubProtocol + Send,
    Node: FullNodeTypes<Types: NodeTypes<ChainSpec = ChainSpec, Primitives = EthPrimitives>>,
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus = TxTy<Node::Types>>>
        + Unpin
        + 'static
{
    type Network =
        NetworkHandle<BasicNetworkPrimitives<PrimitivesTy<Node::Types>, PoolPooledTx<Pool>>>;

    async fn build_network(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool
    ) -> eyre::Result<Self::Network> {
        let mut network_config = ctx.network_config()?;
        network_config.extra_protocols.push(self.custom_protocol);
        // try to dial outbound 5x quicker
        network_config.peers_config = network_config
            .peers_config
            .with_refill_slots_interval(Duration::from_secs(1));

        let network = NetworkManager::builder(network_config).await?;
        let handle = ctx.start_network(network, pool);
        tracing::info!(enode=%handle.local_node_record(), "P2P networking initialized");
        Ok(handle)
    }
}
