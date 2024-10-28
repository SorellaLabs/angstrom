use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    providers::{builder, ext::AnvilApi, Provider},
    rpc::types::{anvil::MineOptions, Block},
    signers::local::PrivateKeySigner
};
use alloy_primitives::{BlockNumber, Bytes};
use reth_provider::{
    CanonStateNotification, CanonStateNotifications, CanonStateSubscriptions, ProviderError,
    ProviderResult
};
use tokio::sync::broadcast;
use validation::common::lru_db::BlockStateProviderFactory;

use super::{utils::AnvilWalletRpc, RpcStateProvider};
use crate::{
    anvil_state_provider::utils::async_to_sync,
    mocks::canon_state::AnvilConsensusCanonStateNotification,
    testnet_controllers::AngstromTestnetConfig
};

#[derive(Debug)]
pub struct RpcStateProviderFactoryWrapper {
    provider:  RpcStateProviderFactory,
    _instance: AnvilInstance
}
impl RpcStateProviderFactoryWrapper {
    pub async fn spawn_new(config: AngstromTestnetConfig, id: u64) -> eyre::Result<Self> {
        let mut anvil_builder = Anvil::new()
            .block_time(config.testnet_block_time_secs)
            .chain_id(1)
            .arg("--ipc")
            .arg(format!("/tmp/anvil_{id}.ipc"))
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit");

        if let Some(config) = config.state_machine_config() {
            anvil_builder = anvil_builder
                .arg("--fork-block-number")
                .arg(format!("{}", config.start_block));
        }

        let anvil = anvil_builder.try_spawn()?;

        let endpoint = format!("/tmp/anvil_{id}.ipc");
        tracing::info!(?endpoint);
        let ipc = alloy::providers::IpcConnect::new(endpoint.to_string());
        let sk: PrivateKeySigner = anvil.keys()[0].clone().into();

        let wallet = EthereumWallet::new(sk);
        let rpc = builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_ipc(ipc)
            .await?;

        tracing::info!("connected to anvil");

        let (tx, _) = broadcast::channel(1000);

        Ok(Self {
            provider:  RpcStateProviderFactory {
                provider:       rpc,
                canon_state_tx: tx,
                canon_state:    AnvilConsensusCanonStateNotification::new()
            },
            _instance: anvil
        })
    }

    pub fn provider(&self) -> RpcStateProviderFactory {
        self.provider.clone()
    }

    pub async fn execute_and_return_state(&self) -> eyre::Result<(Bytes, Block)> {
        let block = self.mine_block().await?;

        Ok((self.provider.provider.anvil_dump_state().await?, block))
    }

    pub async fn return_state(&self) -> eyre::Result<Bytes> {
        Ok(self.provider.provider.anvil_dump_state().await?)
    }

    pub async fn set_state(&self, state: Bytes) -> eyre::Result<()> {
        self.provider.provider.anvil_load_state(state).await?;

        Ok(())
    }

    pub async fn mine_block(&self) -> eyre::Result<Block> {
        let mined = self
            .provider
            .provider
            .anvil_mine_detailed(Some(MineOptions::Options { timestamp: None, blocks: Some(1) }))
            .await?
            .first()
            .cloned()
            .unwrap();

        self.provider.update_canon_chain(&mined)?;

        Ok(mined)
    }
}

#[derive(Debug, Clone)]
pub struct RpcStateProviderFactory {
    provider:       AnvilWalletRpc,
    canon_state:    AnvilConsensusCanonStateNotification,
    canon_state_tx: broadcast::Sender<CanonStateNotification>
}

impl RpcStateProviderFactory {
    pub fn provider(&self) -> AnvilWalletRpc {
        self.provider.clone()
    }

    fn update_canon_chain(&self, new_block: &Block) -> eyre::Result<()> {
        let state = self.canon_state.new_block(new_block);
        let _ = self
            .canon_state_tx
            .send(CanonStateNotification::Commit { new: state })?;

        Ok(())
    }
}

impl BlockStateProviderFactory for RpcStateProviderFactory {
    type Provider = RpcStateProvider;

    fn state_by_block(&self, block: u64) -> ProviderResult<Self::Provider> {
        Ok(RpcStateProvider::new(block, self.provider.clone()))
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        async_to_sync(self.provider.get_block_number())
            .map_err(|_| ProviderError::BestBlockNotFound)
    }
}

impl CanonStateSubscriptions for RpcStateProviderFactory {
    fn subscribe_to_canonical_state(&self) -> CanonStateNotifications {
        self.canon_state_tx.subscribe()
    }
}
