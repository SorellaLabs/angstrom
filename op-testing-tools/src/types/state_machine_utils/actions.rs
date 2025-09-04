use std::{future::Future, pin::Pin};

use alloy_primitives::U256;

use crate::{
    controllers::enviroments::{DevnetStateMachine, OpAngstromTestnet},
    providers::WalletProvider,
    types::{
        StateMachineActionHookFn,
        config::DevnetConfig,
        initial_state::{Erc20ToDeploy, PartialConfigPoolKey}
    }
};

pub trait WithAction<'a> {
    type FunctionOutput;

    fn advance_block(&mut self);

    fn deploy_new_pool(
        &mut self,
        pool_key: PartialConfigPoolKey,
        token0: Erc20ToDeploy,
        token1: Erc20ToDeploy,
        store_index: U256
    );
}

impl<'a> WithAction<'a> for DevnetStateMachine<'a> {
    type FunctionOutput = StateMachineActionHookFn<'a>;

    fn advance_block(&mut self) {
        let f = |testnet: &'a mut OpAngstromTestnet<DevnetConfig, WalletProvider>| {
            pin_action(testnet.update_state())
        };
        self.add_action("advance block", f);
    }

    fn deploy_new_pool(
        &mut self,
        pool_key: PartialConfigPoolKey,
        token0: Erc20ToDeploy,
        token1: Erc20ToDeploy,
        store_index: U256
    ) {
        let f = move |testnet: &'a mut OpAngstromTestnet<DevnetConfig, WalletProvider>| {
            pin_action(testnet.deploy_new_pool(pool_key, token0, token1, store_index))
        };
        self.add_action("deploy new pool", f);
    }
}

fn pin_action<'a, F>(fut: F) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
where
    F: Future<Output = eyre::Result<()>> + Send + 'a
{
    Box::pin(fut)
}
