use std::{future::Future, pin::Pin};

use super::events::HookEvents;
use crate::testnet_controllers::AngstromTestnet;

// pub struct StateMachineHook {
//     /// the `expected` event starts being searched for
//     pub trigger:    HookEvents,
//     /// event expected to happen
//     pub expected:   HookEvents,
//     /// if `expected` doesn't happen by this event
//     /// then failure
//     pub kill_event: HookEvents
// }

pub enum StateMachineHook<C> {
    Action(StateMachineActionHookFn<C>),
    Check(StateMachineCheckHookFn<C>)
}

pub type StateMachineActionHookFn<C> =
    Box<dyn Fn(&mut AngstromTestnet<C>) -> Pin<Box<dyn Future<Output = eyre::Result<()>>>>>;
pub type StateMachineCheckHookFn<C> = Box<dyn Fn(&AngstromTestnet<C>) -> bool>;
