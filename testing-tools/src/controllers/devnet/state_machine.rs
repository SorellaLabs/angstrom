use std::{future::Future, pin::Pin};

use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider};

use super::AngstromDevnet;
use crate::types::{HookResult, StateMachineHook};

pub struct DevnetStateMachine<'a, C> {
    pub(crate) testnet: AngstromDevnet<C>,
    pub(crate) hooks:   Vec<(&'static str, StateMachineHook<'a, C>)>
}

impl<'a, C> DevnetStateMachine<'a, C>
where
    C: BlockReader
        + HeaderProvider
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static
{
    pub(crate) fn new(testnet: AngstromDevnet<C>) -> Self {
        Self { testnet, hooks: Vec::new() }
    }

    pub async fn run(mut self) {
        let hooks = std::mem::take(&mut self.hooks);

        for (i, (name, hook)) in hooks.into_iter().enumerate() {
            Self::run_hook(
                unsafe {
                    std::mem::transmute::<&mut AngstromDevnet<C>, &mut AngstromDevnet<C>>(
                        &mut self.testnet
                    )
                },
                i,
                name,
                hook
            )
            .await;
        }
    }

    async fn run_hook(
        testnet: &'a mut AngstromDevnet<C>,
        i: usize,
        name: &'static str,
        hook: StateMachineHook<'a, C>
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
        F: Fn(&mut AngstromDevnet<C>) -> eyre::Result<bool> + 'static
    {
        self.hooks
            .push((check_name, StateMachineHook::Check(Box::new(check))))
    }

    pub(crate) fn add_action<F>(&mut self, action_name: &'static str, action: F)
    where
        F: FnOnce(
                &'a mut AngstromDevnet<C>
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
                &'a mut AngstromDevnet<C>
            )
                -> Pin<Box<dyn Future<Output = eyre::Result<bool>> + Send + Sync + 'a>>
            + 'static
    {
        self.hooks
            .push((checked_action_name, StateMachineHook::CheckedAction(Box::new(checked_action))))
    }
}