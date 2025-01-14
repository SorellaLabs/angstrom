use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    providers::ext::AnvilApi,
    signers::local::PrivateKeySigner
};
use alloy_primitives::{Address, U256};
use angstrom_types::primitive::AngstromSigner;
use consensus::AngstromValidator;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

use super::TestingConfigKind;
use crate::{
    providers::WalletProvider,
    types::{initial_state::PartialConfigPoolKey, GlobalTestingConfig}
};

#[derive(Debug, Clone)]
pub struct TestingNodeConfig<C> {
    pub node_id:       u64,
    pub global_config: C,
    pub pub_key:       PublicKey,
    pub secret_key:    SecretKey,
    pub voting_power:  u64,
    base_port_rand:    u8
}

impl<C: GlobalTestingConfig> TestingNodeConfig<C> {
    pub fn new(node_id: u64, global_config: C, voting_power: u64) -> Self {
        let secret_key = SecretKey::new(&mut rand::thread_rng());
        Self {
            node_id,
            global_config,
            pub_key: secret_key.public_key(&Secp256k1::default()),
            voting_power,
            secret_key,
            base_port_rand: rand::random()
        }
    }

    pub fn is_devnet(&self) -> bool {
        matches!(self.global_config.config_type(), TestingConfigKind::Devnet)
    }

    pub fn strom_rpc_port(&self) -> u64 {
        self.base_port_rand as u64 + self.node_id
    }

    pub fn signing_key(&self) -> PrivateKeySigner {
        PrivateKeySigner::from_bytes(&self.secret_key.secret_bytes().into()).unwrap()
    }

    pub fn angstrom_signer(&self) -> AngstromSigner {
        AngstromSigner::new(self.signing_key())
    }

    pub fn angstrom_validator(&self) -> AngstromValidator {
        let id = self.angstrom_signer().id();
        AngstromValidator::new(id, self.voting_power)
    }

    pub fn address(&self) -> Address {
        self.signing_key().address()
    }

    pub fn pool_keys(&self) -> Vec<PartialConfigPoolKey> {
        self.global_config.pool_keys()
    }

    fn configure_testnet_leader_anvil(&self) -> Anvil {
        if !self.global_config.is_leader(self.node_id) {
            panic!("only the leader can call this!")
        }

        Anvil::new()
            .chain_id(1)
            .arg("--host")
            .arg("0.0.0.0")
            .port((9545 + self.node_id) as u16)
            .fork(self.global_config.eth_ws_url())
            .arg("--ipc")
            .arg(self.global_config.anvil_rpc_endpoint(self.node_id))
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit")
            .block_time(12)
    }

    fn configure_devnet_anvil(&self) -> Anvil {
        let mut anvil_builder = Anvil::new()
            .chain_id(1)
            .arg("--host")
            .arg("0.0.0.0")
            .port((9545 + self.node_id) as u16)
            .arg("--ipc")
            .arg(self.global_config.anvil_rpc_endpoint(self.node_id))
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit");

        if let Some((fork_block_number, fork_url)) = self.global_config.fork_config() {
            anvil_builder = anvil_builder
                .fork(fork_url)
                .fork_block_number(fork_block_number)
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
        let anvil = self
            .global_config
            .is_leader(self.node_id)
            .then(|| self.configure_testnet_leader_anvil().try_spawn())
            .transpose()?;

        let sk = self.signing_key();
        let wallet = EthereumWallet::new(sk.clone());

        let endpoint = self.global_config.anvil_rpc_endpoint(self.node_id);
        tracing::info!(?endpoint);

        let rpc = alloy::providers::builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_builtin(&endpoint)
            .await?;

        tracing::info!("connected to anvil");

        rpc.anvil_set_balance(sk.address(), U256::MAX).await?;

        Ok((WalletProvider::new_with_provider(rpc, sk), anvil))
    }

    async fn spawn_devnet_anvil_rpc(
        &self
    ) -> eyre::Result<(WalletProvider, Option<AnvilInstance>)> {
        let anvil = self.configure_devnet_anvil().try_spawn()?;

        let sk = self.signing_key();
        let wallet = EthereumWallet::new(sk.clone());

        let endpoint = self.global_config.anvil_rpc_endpoint(self.node_id);
        tracing::info!(?endpoint);

        let rpc = alloy::providers::builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_builtin(&endpoint)
            .await?;

        tracing::info!("connected to anvil");

        rpc.anvil_set_balance(sk.address(), U256::MAX).await?;

        Ok((WalletProvider::new_with_provider(rpc, sk), Some(anvil)))
    }
}
