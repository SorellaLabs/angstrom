use alloy::{
    consensus::{SignableTransaction, Signed},
    network::{Ethereum, Network},
    primitives::Signature
};
use op_alloy_network::Optimism;
use reth_optimism_primitives::OpPrimitives;
use reth_primitives::EthPrimitives;
use reth_provider::NodePrimitivesProvider;

/// A trait that contains all type templates for supporting a different network.
pub trait NetworkProvider: NodePrimitivesProvider + Send + Sync + Clone + Unpin {
    /// The [`Network`] used in any providers.
    type Network: Network<
            UnsignedTx: SignableTransaction<Signature>,
            TxEnvelope: From<Signed<<Self::Network as Network>::UnsignedTx>>
        >;
}

#[derive(Clone)]
pub struct OpNetworkProvider;

impl NodePrimitivesProvider for OpNetworkProvider {
    type Primitives = OpPrimitives;
}

impl NetworkProvider for OpNetworkProvider {
    type Network = Optimism;
}

#[derive(Clone)]
pub struct EthNetworkProvider;

impl NodePrimitivesProvider for EthNetworkProvider {
    type Primitives = EthPrimitives;
}

impl NetworkProvider for EthNetworkProvider {
    type Network = Ethereum;
}
