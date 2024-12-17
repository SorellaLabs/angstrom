use std::{future::Future, pin::Pin};

use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};

use crate::{
    controllers::enviroments::{AngstromTestnet, DevnetStateMachine},
    providers::WalletProvider,
    types::{config::DevnetConfig, StateMachineActionHookFn}
};

pub trait WithAction<'a, C>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static
{
    type FunctionOutput = StateMachineActionHookFn<'a, C>;

    fn advance_block(&mut self);
}

impl<'a, C> WithAction<'a, C> for DevnetStateMachine<'a, C>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static
{
    fn advance_block(&mut self) {
        let f = |testnet: &'a mut AngstromTestnet<C, DevnetConfig, WalletProvider>| {
            pin_action(testnet.all_peers_update_state(0))
        };
        self.add_action("advance block", f);
    }
}

fn pin_action<'a, F>(fut: F) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
where
    F: Future<Output = eyre::Result<()>> + Send + 'a
{
    Box::pin(fut)
}
