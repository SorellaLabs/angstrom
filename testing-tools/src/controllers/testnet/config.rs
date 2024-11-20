use std::fmt::Display;

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    providers::{builder, IpcConnect, WsConnect},
    signers::local::PrivateKeySigner
};
use alloy_primitives::Address;
use angstrom_types::contract_bindings::angstrom::Angstrom::PoolKey;
use secp256k1::{PublicKey, SecretKey};

use crate::{anvil_state_provider::WalletProvider, types::TestingConfig};

#[derive(Debug, Clone)]
pub struct TestnetConfig {
    pub anvil_key:            usize,
    pub node_count:           u64,
    pub leader_ws_url:        String,
    pub controller_address:   Address,
    pub pk:                   PublicKey,
    pub signing_key:          PrivateKeySigner,
    pub secret_key:           SecretKey,
    pub pool_keys:            Vec<PoolKey>,
    pub angstrom_address:     Address,
    pub pool_manager_address: Address,
    pub leader_config:        Option<TestnetLeaderConfig>
}

impl TestnetConfig {
    pub fn new(
        anvil_key: usize,
        node_count: u64,
        leader_ws_url: impl ToString,
        controller_address: Address,
        pool_manager_address: Address,
        pk: PublicKey,
        signing_key: PrivateKeySigner,
        secret_key: SecretKey,
        pool_keys: Vec<PoolKey>,
        angstrom_address: Address,
        leader_config: Option<TestnetLeaderConfig>
    ) -> Self {
        Self {
            anvil_key,
            controller_address,
            pool_manager_address,
            pk,
            signing_key,
            secret_key,
            node_count,
            leader_ws_url: leader_ws_url.to_string(),
            angstrom_address,
            pool_keys,
            leader_config
        }
    }

    pub fn is_leader(&self) -> bool {
        self.leader_config.is_some()
    }
}

impl TestingConfig for TestnetConfig {
    fn configure_anvil(&self, id: impl Display) -> Anvil {
        let Some(config) = self.leader_config.clone() else {
            panic!("only the leader can call this!")
        };

        Anvil::new()
            .chain_id(1)
            .fork(config.eth_fork_url)
            .fork_block_number(config.fork_block)
            .arg("--ipc")
            .arg(self.anvil_endpoint(id))
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit")
    }

    async fn spawn_rpc(
        &self,
        id: impl Display + Clone
    ) -> eyre::Result<(WalletProvider, Option<AnvilInstance>)> {
        let sk = self.signing_key.clone();
        let wallet = EthereumWallet::new(sk.clone());

        if self.is_leader() {
            let anvil = self.configure_anvil(id.clone()).try_spawn()?;

            let endpoint = self.anvil_endpoint(id);
            tracing::info!(?endpoint);

            let rpc = builder::<Ethereum>()
                .with_recommended_fillers()
                .wallet(wallet)
                .on_ipc(IpcConnect::new(endpoint))
                .await?;

            tracing::info!("connected to anvil");

            Ok((WalletProvider::new(rpc, self.controller_address, sk), Some(anvil)))
        } else {
            let rpc = builder::<Ethereum>()
                .with_recommended_fillers()
                .wallet(wallet)
                .on_ws(WsConnect::new(self.leader_ws_url.clone()))
                .await?;

            Ok((WalletProvider::new(rpc, self.controller_address, sk), None))
        }
    }

    fn rpc_port(&self, _: Option<u64>) -> u64 {
        4200
    }

    fn anvil_endpoint(&self, _: impl Display) -> String {
        format!("/tmp/anvil.ipc")
    }
}

#[derive(Debug, Clone)]
pub struct TestnetLeaderConfig {
    pub fork_block:   u64,
    pub eth_fork_url: String
}

impl TestnetLeaderConfig {
    pub fn new(fork_block: u64, eth_fork_url: impl ToString) -> Self {
        Self { fork_block, eth_fork_url: eth_fork_url.to_string() }
    }
}
