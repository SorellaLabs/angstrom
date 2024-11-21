use std::fmt::Display;

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    providers::builder,
    signers::local::PrivateKeySigner
};
use alloy_primitives::Address;

use crate::{anvil_state_provider::WalletProvider, types::TestingConfig};

#[derive(Debug, Clone)]
pub struct DevnetConfig {
    pub anvil_key:         usize,
    pub intial_node_count: u64,
    pub initial_rpc_port:  u16,
    pub start_block:       Option<u64>,
    pub fork_url:          Option<String>
}

impl DevnetConfig {
    pub fn new(
        anvil_key: usize,
        intial_node_count: u64,
        initial_rpc_port: u16,
        start_block: Option<u64>,
        fork_url: Option<String>
    ) -> Self {
        Self { anvil_key, intial_node_count, initial_rpc_port, start_block, fork_url }
    }

    pub fn rpc_port_with_node_id(&self, node_id: Option<u64>) -> u64 {
        if let Some(id) = node_id {
            self.initial_rpc_port as u64 + id
        } else {
            self.initial_rpc_port as u64
        }
    }

    pub fn fork_block_number(&self) -> Option<u64> {
        self.start_block
    }

    fn fork_config(&self) -> Option<(u64, String)> {
        self.start_block.zip(self.fork_url.clone())
    }
}

impl Default for DevnetConfig {
    fn default() -> Self {
        Self {
            anvil_key:         7,
            intial_node_count: 2,
            initial_rpc_port:  4200,
            start_block:       None,
            fork_url:          None
        }
    }
}

impl TestingConfig for DevnetConfig {
    fn configure_anvil(&self, id: impl Display) -> Anvil {
        let mut anvil_builder = Anvil::new()
            .chain_id(1)
            .arg("--ipc")
            .arg(self.anvil_endpoint(id))
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit");

        if let Some((start_block, fork_url)) = self.fork_config() {
            anvil_builder = anvil_builder
                .fork(fork_url)
                .arg("--fork-block-number")
                .arg(format!("{}", start_block));
        }

        anvil_builder
    }

    async fn spawn_rpc(
        &self,
        id: impl Display + Clone
    ) -> eyre::Result<(WalletProvider, Option<AnvilInstance>)> {
        let anvil = self.configure_anvil(id.clone()).try_spawn()?;

        let endpoint = self.anvil_endpoint(id);
        tracing::info!(?endpoint);
        let ipc = alloy::providers::IpcConnect::new(endpoint);
        // TODO: Revert after things are working
        // let sk: PrivateKeySigner =
        // "4bbbf85ce3377467afe5d46f804f221813b2bb87f24d81f60f1fcdbf7cbf4356".parse().
        // unwrap(); let controller_address =
        // "0x14dc79964da2c08b23698b3d3cc7ca32193d9955".parse().unwrap();
        let sk: PrivateKeySigner = anvil.keys()[self.anvil_key].clone().into();
        let controller_address = anvil.addresses()[self.anvil_key];

        let wallet = EthereumWallet::new(sk.clone());
        let rpc = builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_ipc(ipc)
            .await?;

        tracing::info!("connected to anvil");

        Ok((WalletProvider::new(rpc, controller_address, sk), Some(anvil)))
    }

    fn rpc_port(&self, node_id: Option<u64>) -> u64 {
        self.initial_rpc_port as u64 + node_id.expect("node id must be set")
    }

    fn anvil_endpoint(&self, id: impl Display) -> String {
        format!("/tmp/anvil_{}.ipc", id)
    }
}
