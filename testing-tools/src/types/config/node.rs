use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    providers::{ext::AnvilApi, IpcConnect},
    signers::local::PrivateKeySigner
};
use alloy_primitives::{Address, U256};
use angstrom_types::primitive::AngstromSigner;
use consensus::AngstromValidator;
use reth_network_peers::pk2id;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

use super::TestingConfigKind;
use crate::{providers::WalletProvider, types::GlobalTestingConfig};

#[derive(Debug, Clone)]
pub struct TestingNodeConfig<C> {
    pub node_id:       u64,
    pub global_config: C,
    pub pub_key:       PublicKey,
    pub secret_key:    SecretKey,
    pub voting_power:  u64
}

impl<C: GlobalTestingConfig> TestingNodeConfig<C> {
    pub fn new(node_id: u64, global_config: C, secret_key: SecretKey, voting_power: u64) -> Self {
        Self {
            node_id,
            global_config,
            pub_key: secret_key.public_key(&Secp256k1::default()),
            voting_power,
            secret_key
        }
    }

    pub fn strom_rpc_port(&self) -> u64 {
        4200 + self.node_id
    }

    pub fn signing_key(&self) -> PrivateKeySigner {
        PrivateKeySigner::from_bytes(&self.secret_key.secret_bytes().into()).unwrap()
    }

    pub fn angstrom_signer(&self) -> AngstromSigner {
        AngstromSigner::new(self.signing_key())
    }

    pub fn angstrom_validator(&self) -> AngstromValidator {
        AngstromValidator::new(pk2id(&self.pub_key), self.voting_power)
    }

    pub fn address(&self) -> Address {
        self.signing_key().address()
    }

    pub fn configure_anvil(&self) -> Anvil {
        if matches!(self.global_config.config_type(), TestingConfigKind::Testnet) {
            self.configure_testnet_leader_anvil()
        } else {
            self.configure_devnet_anvil()
        }
    }

    fn configure_testnet_leader_anvil(&self) -> Anvil {
        if !self.global_config.is_leader(self.node_id) {
            panic!("only the leader can call this!")
        }

        Anvil::new()
            .chain_id(1)
            .fork(self.global_config.eth_ws_url())
            .arg("--ipc")
            .arg(self.global_config.anvil_rpc_endpoint(self.node_id))
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit")
            .arg("--preserve-historical-states")
            .arg("--max-persisted-states")
            .arg("500")
            .block_time(12)
    }

    fn configure_devnet_anvil(&self) -> Anvil {
        let mut anvil_builder = Anvil::new()
            .chain_id(1)
            .arg("--ipc")
            .arg(self.global_config.anvil_rpc_endpoint(self.node_id))
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--preserve-historical-states")
            .arg("--max-persisted-states")
            .arg("500")
            .arg("--disable-block-gas-limit");

        if let Some((start_block, fork_url)) = self.global_config.fork_config() {
            anvil_builder = anvil_builder
                .fork(fork_url)
                .arg("--fork-block-number")
                .arg(format!("{}", start_block));
        }

        anvil_builder
    }

    pub async fn spawn_anvil_rpc(&self) -> eyre::Result<(WalletProvider, Option<AnvilInstance>)> {
        if matches!(self.global_config.config_type(), TestingConfigKind::Testnet) {
            self.spawn_testnet_anvil_rpc().await
        } else {
            self.spawn_devnet_anvil_rpc().await
        }
    }

    async fn spawn_testnet_anvil_rpc(
        &self
    ) -> eyre::Result<(WalletProvider, Option<AnvilInstance>)> {
        let sk = self.signing_key();
        let wallet = EthereumWallet::new(sk.clone());

        let anvil = self
            .global_config
            .is_leader(self.node_id)
            .then_some(self.configure_testnet_leader_anvil().try_spawn())
            .transpose()?;

        let endpoint = "/tmp/testnet_anvil.ipc".to_string();
        tracing::info!(?endpoint);

        let rpc = alloy::providers::builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_ipc(IpcConnect::new(endpoint))
            .await?;

        tracing::info!("connected to anvil");

        rpc.anvil_set_balance(sk.address(), U256::from(1000000000000000000_u64))
            .await?;

        Ok((WalletProvider::new_with_provider(rpc, sk), anvil))
    }

    async fn spawn_devnet_anvil_rpc(
        &self
    ) -> eyre::Result<(WalletProvider, Option<AnvilInstance>)> {
        let anvil = self.configure_devnet_anvil().try_spawn()?;

        let endpoint = self.global_config.anvil_rpc_endpoint(self.node_id);
        tracing::info!(?endpoint);
        let ipc = alloy::providers::IpcConnect::new(endpoint);

        let sk = self.signing_key();

        let wallet = EthereumWallet::new(sk.clone());
        let rpc = alloy::providers::builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_ipc(ipc)
            .await?;

        tracing::info!("connected to anvil");

        rpc.anvil_set_balance(sk.address(), U256::from(1000000000000000000_u64))
            .await?;

        Ok((WalletProvider::new_with_provider(rpc, sk), Some(anvil)))
    }
}
