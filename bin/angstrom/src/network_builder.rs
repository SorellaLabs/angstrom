use reth::{chainspec::ChainSpec, transaction_pool::TransactionPool};
use reth_network::{protocol::IntoRlpxSubProtocol, NetworkHandle, NetworkManager};
use reth_node_builder::{
    components::NetworkBuilder, node::FullNodeTypes, BuilderContext, NodeTypes
};

/// A basic ethereum payload service.
pub struct AngstromNetworkBuilder<I: IntoRlpxSubProtocol + Send> {
    custom_protocol: I
}

impl<I: IntoRlpxSubProtocol + Send> AngstromNetworkBuilder<I> {
    pub fn new(protocol: I) -> Self {
        Self { custom_protocol: protocol }
    }
}

impl<Node, Pool, I: IntoRlpxSubProtocol + Send> NetworkBuilder<Node, Pool>
    for AngstromNetworkBuilder<I>
where
    Node: FullNodeTypes<Types: NodeTypes<ChainSpec = ChainSpec>>,
    Pool: TransactionPool + Unpin + 'static
{
    async fn build_network(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool
    ) -> eyre::Result<NetworkHandle> {
        let mut network_config = ctx.network_config()?;
        network_config.extra_protocols.push(self.custom_protocol);

        let network = NetworkManager::builder(network_config).await?;
        let handle = ctx.start_network(network, pool);

        Ok(handle)
    }
}
