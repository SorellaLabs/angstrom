use std::{future::Future, pin::Pin};

use super::AngstromTestnet;
use crate::types::{StateMachineActionHookFn, StateMachineCheckHookFn, StateMachineHook};

pub struct StateMachineTestnet<C> {
    testnet: AngstromTestnet<C>,
    hooks:   Vec<(&'static str, StateMachineHook<C>)>
}

impl<C> StateMachineTestnet<C> {
    pub(crate) fn new(testnet: AngstromTestnet<C>) -> Self {
        Self { testnet, hooks: Vec::new() }
    }

    pub async fn run(mut self) {
        let hooks = std::mem::take(&mut self.hooks);
        let num_hooks = hooks.len();
        for (i, (name, hook)) in hooks.into_iter().enumerate() {
            self.run_hook(i, name, hook).await;
        }
    }

    async fn run_hook(&mut self, i: usize, name: &'static str, hook: StateMachineHook<C>) {
        match hook {
            StateMachineHook::Action(action) => self.run_action(action).await.fmt_result(i, name),
            StateMachineHook::Check(check) => self.run_check(check).await.fmt_result(i, name)
        }
    }

    pub fn add_action<F>(&mut self, action_name: &'static str, action: F)
    where
        F: Fn(&mut AngstromTestnet<C>) -> Pin<Box<dyn Future<Output = eyre::Result<()>>>> + 'static
    {
        self.hooks
            .push((action_name, StateMachineHook::Action(Box::new(action))))
    }

    pub fn add_check<F>(&mut self, check_name: &'static str, check: F)
    where
        F: Fn(&AngstromTestnet<C>) -> bool + 'static
    {
        self.hooks
            .push((check_name, StateMachineHook::Check(Box::new(check))))
    }

    async fn run_action(&mut self, action: StateMachineActionHookFn<C>) -> eyre::Result<()> {
        Ok(())
    }

    async fn run_check(&mut self, check: StateMachineCheckHookFn<C>) -> eyre::Result<bool> {
        Ok(false)
    }
}

trait HookResult: Sized {
    fn error(&self) -> Option<&eyre::ErrReport>;

    fn is_pass(&self) -> bool;

    fn fmt_result(self, i: usize, name: &'static str) {
        if let Some(e) = self.error() {
            tracing::error!(target: "testnet::state-machine", hook = i, name, "{:?}", e);
            panic!();
        }

        if self.is_pass() {
            tracing::info!(target: "testnet::state-machine", hook = i, name, "hook PASSED");
        } else {
            tracing::warn!(target: "testnet::state-machine", hook = i, name, "hook FAILED");
        }
    }
}

impl HookResult for eyre::Result<()> {
    fn is_pass(&self) -> bool {
        self.is_ok()
    }

    fn error(&self) -> Option<&eyre::ErrReport> {
        self.as_ref().err()
    }
}

impl HookResult for eyre::Result<bool> {
    fn is_pass(&self) -> bool {
        match self.as_ref() {
            Ok(true) => true,
            _ => false
        }
    }

    fn error(&self) -> Option<&eyre::ErrReport> {
        self.as_ref().err()
    }
}
