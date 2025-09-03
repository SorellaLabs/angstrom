mod devnet;
mod op_testnet;
mod state_machine;

use alloy::{node_bindings::AnvilInstance, providers::Provider};
pub use state_machine::*;

use crate::{
    controllers::strom::OpTestnetNode,
    providers::{AnvilProvider, TestnetBlockProvider, utils::async_to_sync},
    types::{GlobalTestingConfig, WithWalletProvider}
};

pub struct OpAngstromTestnet<G, P> {
    block_provider: TestnetBlockProvider,
    config:         G,
    node:           OpTestnetNode<P, G>,
    anvil:          Option<AnvilInstance>
}

impl<G, P> OpAngstromTestnet<G, P>
where
    G: GlobalTestingConfig,
    P: WithWalletProvider
{
    pub fn node(&self) -> &OpTestnetNode<P, G> {
        &self.node
    }

    pub fn node_provider(&self) -> &AnvilProvider<P> {
        self.node.state_provider()
    }

    /// checks the current block number on all peers matches the expected
    pub(crate) fn check_block_numbers(&self, expected_block_num: u64) -> eyre::Result<bool> {
        let latest = async_to_sync(self.node_provider().rpc_provider().get_block_number())?;
        Ok(latest == expected_block_num)
    }

    pub(crate) async fn update_state(&self) -> eyre::Result<()> {
        let (updated_state, block) = self
            .node
            .state_provider()
            .execute_and_return_state()
            .await?;
        self.block_provider.broadcast_block(block);

        self.node
            .state_provider()
            .set_state(updated_state.clone())
            .await?;

        Ok(())
    }
}
