use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};

use crate::{
    controllers::enviroments::{AngstromTestnet, DevnetStateMachine},
    providers::WalletProvider,
    types::{config::DevnetConfig, StateMachineCheckHookFn}
};

pub trait WithCheck<C>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static
{
    type FunctionOutput = StateMachineCheckHookFn<C>;

    fn check_block(&mut self, block_number: u64);
}

impl<'a, C> WithCheck<C> for DevnetStateMachine<'a, C>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static
{
    fn check_block(&mut self, block_number: u64) {
        let f = move |testnet: &mut AngstromTestnet<C, DevnetConfig, WalletProvider>| {
            testnet.check_block_numbers(block_number)
        };
        self.add_check("check block", f);
    }
}
