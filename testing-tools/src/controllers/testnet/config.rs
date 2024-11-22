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
    pub rpc_port:             u16,
    pub signing_key:          PrivateKeySigner,
    pub secret_key:           SecretKey,
    pub pool_keys:            Vec<PoolKey>,
    pub angstrom_address:     Address,
    pub pool_manager_address: Address,
    /// only the leader can have this
    pub eth_ws_url:           Option<String>,
    pub is_leader:            bool
}

impl TestnetConfig {
    pub fn new(
        anvil_key: usize,
        node_count: u64,
        rpc_port: u16,
        leader_ws_url: impl ToString,
        controller_address: Address,
        pk: PublicKey,
        signing_key: PrivateKeySigner,
        secret_key: SecretKey,
        pool_keys: Vec<PoolKey>,
        angstrom_address: Address,
        pool_manager_address: Address,
        eth_ws_url: Option<impl ToString>,
        is_leader: bool
    ) -> Self {
        Self {
            anvil_key,
            controller_address,
            pk,
            signing_key,
            secret_key,
            node_count,
            rpc_port,
            leader_ws_url: leader_ws_url.to_string(),
            angstrom_address,
            pool_manager_address,
            pool_keys,
            eth_ws_url: eth_ws_url.map(|w| w.to_string()),
            is_leader
        }
    }
}

impl TestingConfig for TestnetConfig {
    fn configure_anvil(&self, id: impl Display) -> Anvil {
        if !self.is_leader {
            panic!("only the leader can call this!")
        }

        Anvil::new()
            .chain_id(1)
            .port(8545u16)
            // TODO: revert when testing is done
            // .fork(self.eth_ws_url.as_ref().unwrap())
            .arg("--ipc")
            .arg(self.anvil_endpoint(id))
            .arg("--code-size-limit")
            .arg("393216")
            .block_time(12)
    }

    async fn spawn_rpc(
        &self,
        id: impl Display + Clone
    ) -> eyre::Result<(WalletProvider, Option<AnvilInstance>)> {
        let sk = self.signing_key.clone();
        let wallet = EthereumWallet::new(sk.clone());

        if self.is_leader {
            // TODO: revert after the test is done
            // let anvil = self.configure_anvil(id.clone()).try_spawn()?;

            let endpoint = self.anvil_endpoint(id);
            tracing::info!(?endpoint);

            let rpc = builder::<Ethereum>()
                .with_recommended_fillers()
                .wallet(wallet)
                .on_ipc(IpcConnect::new(endpoint))
                .await?;

            tracing::info!("connected to anvil");

            // TODO: revert after testing
            // Ok((WalletProvider::new(rpc, self.controller_address, sk), Some(anvil)))
            Ok((WalletProvider::new(rpc, self.controller_address, sk), None))
        } else {
            let rpc = builder::<Ethereum>()
                .with_recommended_fillers()
                .wallet(wallet)
                .on_ws(WsConnect::new(self.leader_ws_url.clone()))
                .await?;

            Ok((WalletProvider::new(rpc, self.controller_address, sk), None))
        }
    }

    fn rpc_port(&self, _: Option<u64>) -> u16 {
        self.rpc_port
    }

    fn anvil_endpoint(&self, _: impl Display) -> String {
        format!("/tmp/anvil.ipc")
    }
}
