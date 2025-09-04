use std::{future::Future, pin::Pin};

use super::OpAngstromTestnet;
use crate::{
    providers::WalletProvider,
    types::{HookResult, StateMachineHook, config::DevnetConfig}
};

pub struct DevnetStateMachine<'a> {
    pub testnet:      OpAngstromTestnet<DevnetConfig, WalletProvider>,
    pub(crate) hooks: Vec<(&'static str, StateMachineHook<'a>)>
}

impl<'a> DevnetStateMachine<'a> {
    pub(crate) fn new(testnet: OpAngstromTestnet<DevnetConfig, WalletProvider>) -> Self {
        Self { testnet, hooks: Vec::new() }
    }

    pub async fn run(mut self) {
        let hooks = std::mem::take(&mut self.hooks);

        for (i, (name, hook)) in hooks.into_iter().enumerate() {
            Self::run_hook(
                unsafe {
                    std::mem::transmute::<
                        &mut OpAngstromTestnet<DevnetConfig, WalletProvider>,
                        &mut OpAngstromTestnet<DevnetConfig, WalletProvider>
                    >(&mut self.testnet)
                },
                i,
                name,
                hook
            )
            .await;
        }
    }

    async fn run_hook(
        testnet: &'a mut OpAngstromTestnet<DevnetConfig, WalletProvider>,
        i: usize,
        name: &'static str,
        hook: StateMachineHook<'a>
    ) {
        match hook {
            StateMachineHook::Action(action) => action(testnet).await.fmt_result(i, name),
            StateMachineHook::Check(check) => check(testnet).fmt_result(i, name),
            StateMachineHook::CheckedAction(checked_action) => {
                checked_action(testnet).await.fmt_result(i, name)
            }
        };
    }

    pub(crate) fn add_check<F>(&mut self, check_name: &'static str, check: F)
    where
        F: Fn(&mut OpAngstromTestnet<DevnetConfig, WalletProvider>) -> eyre::Result<bool> + 'static
    {
        self.hooks
            .push((check_name, StateMachineHook::Check(Box::new(check))))
    }

    pub(crate) fn add_action<F>(&mut self, action_name: &'static str, action: F)
    where
        F: FnOnce(
                &'a mut OpAngstromTestnet<DevnetConfig, WalletProvider>
            ) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
            + 'static
    {
        self.hooks
            .push((action_name, StateMachineHook::Action(Box::new(action))))
    }

    pub(crate) fn add_checked_action<F>(
        &mut self,
        checked_action_name: &'static str,
        checked_action: F
    ) where
        F: FnOnce(
                &'a mut OpAngstromTestnet<DevnetConfig, WalletProvider>
            )
                -> Pin<Box<dyn Future<Output = eyre::Result<bool>> + Send + Sync + 'a>>
            + 'static
    {
        self.hooks
            .push((checked_action_name, StateMachineHook::CheckedAction(Box::new(checked_action))))
    }
}
