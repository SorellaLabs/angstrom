use std::{fmt::Display, future::Future};

use alloy::node_bindings::{Anvil, AnvilInstance};

use crate::anvil_state_provider::WalletProvider;

pub trait TestingConfig: Clone {
    fn configure_anvil(&self, node_id: impl Display) -> Anvil;

    fn spawn_rpc(
        &self,
        node_id: impl Display + Clone
    ) -> impl Future<Output = eyre::Result<(WalletProvider, Option<AnvilInstance>)>>;

    fn anvil_endpoint(&self, id: impl Display) -> String;

    fn rpc_port(&self, node_id: Option<u64>) -> u16;
}
