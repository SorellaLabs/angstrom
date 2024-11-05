use std::{future::Future, pin::Pin};

use crate::testnet_controllers::AngstromTestnet;

pub enum StateMachineHook<C> {
    Action(StateMachineActionHookFn<C>),
    Check(StateMachineCheckHookFn<C>)
}

pub type StateMachineActionHookFn<C> =
    Box<dyn Fn(&mut AngstromTestnet<C>) -> Pin<Box<dyn Future<Output = eyre::Result<()>>>>>;
pub type StateMachineCheckHookFn<C> = Box<dyn Fn(&AngstromTestnet<C>) -> bool>;
